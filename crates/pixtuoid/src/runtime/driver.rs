//! Source construction and reducer task used by the floating Maple runtime.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use pixtuoid_core::source::antigravity::AntigravitySource;
use pixtuoid_core::source::claude_code::ClaudeCodeSource;
use pixtuoid_core::source::codex::CodexSource;
use pixtuoid_core::source::copilot::CopilotSource;
use pixtuoid_core::source::daemon::{self, PresenceMsg};
use pixtuoid_core::source::grok::GrokSource;
use pixtuoid_core::source::hook::HookRouter;
use pixtuoid_core::source::jsonl::ChildEndUnclaims;
use pixtuoid_core::source::omp::OmpSource;
use pixtuoid_core::source::DynSource;
use pixtuoid_core::state::MAX_FLOORS;
use pixtuoid_core::{Reducer, SceneState, TaggedReceiver};
use tokio::sync::watch;

use super::gate;
use super::ConnectedSources;

/// Build the runtime source set `run_async` spawns — the ONE place that set is
/// constructed: the `HookRouter` (shared-socket owner) + the transcript-bearing
/// watchers. A transcript source registered in the core registry but missing here
/// (a silent "never spawns" no-op) is caught by
/// `build_source_set_wires_every_transcript_bearing_source_plus_the_hook_router`.
/// Each transcript source carries different typed config (CC's projects root,
/// Codex's sessions root), so this stays imperative rather than a registry-driven
/// loop — invariant #3's per-source-typed seam. Hook-only sources + the daemon
/// (OpenClaw) are absent by design — they ride the router's shared socket.
pub(crate) fn build_source_set(
    socket_path: PathBuf,
    projects_root: Option<PathBuf>,
    codex_sessions_root: Option<PathBuf>,
    presence_tx: Option<daemon::PresenceSender>,
) -> Vec<Box<dyn DynSource>> {
    let mut cc_src = ClaudeCodeSource::default_paths();
    if let Some(p) = projects_root {
        cc_src.projects_root = p;
    }
    let ag_src = AntigravitySource::default_paths();
    let copilot_src = CopilotSource::default_paths();
    let omp_src = OmpSource::default_paths();

    let mut codex_src = CodexSource::default_paths();
    if let Some(p) = codex_sessions_root {
        codex_src.sessions_root = p;
    }

    // #246: ONE shared child-end un-claim handle. The HookRouter's hook tee is the
    // PRODUCER (every source's SubagentStop rides the one shared socket it owns);
    // both watchers CONSUME — each drains only the ids whose transcripts it claims
    // (AgentId is source-namespaced), so a Codex child's id waits for the Codex
    // watcher even though the router decoded its hook.
    let child_end_unclaims = ChildEndUnclaims::new();
    cc_src.child_end_unclaims = Some(child_end_unclaims.clone());
    codex_src.child_end_unclaims = Some(child_end_unclaims.clone());

    // grok consumes too: its subagent_stop/subagent_end hooks decode to
    // Hook-transport `SessionEnd{as_child:true}` (the tee's trigger), and the
    // grok watcher releases the ended child's flat-sibling transcript claim.
    let mut grok_src = GrokSource::default_paths();
    grok_src.child_end_unclaims = Some(child_end_unclaims.clone());

    // The HookRouter owns the ONE shared hook socket every source's hooks ride;
    // it is the tee producer + the daemon-presence demux. CC/Codex are now pure
    // transcript watchers (consumers of the un-claim handle).
    let hook_router = HookRouter::new(socket_path)
        .with_child_end_unclaims(Some(child_end_unclaims))
        .with_presence_tx(presence_tx);

    vec![
        Box::new(hook_router) as Box<dyn DynSource>,
        Box::new(cc_src),
        Box::new(ag_src),
        Box::new(codex_src),
        Box::new(copilot_src),
        Box::new(omp_src),
        Box::new(grok_src),
    ]
}

/// The reducer event loop: gate + apply incoming `AgentEvent`s, merge daemon
/// presence, and run the 1-Hz reconcile sweep — the codecov/mutants-excluded
/// async shell over [`super::gate`] (#103), which owns the gate/reconcile
/// decision so it stays covered and mutation-tested.
pub(crate) async fn reducer_task(
    mut rx: TaggedReceiver,
    scene_tx: watch::Sender<Arc<SceneState>>,
    floor_caps: Arc<[AtomicUsize; MAX_FLOORS]>,
    connected: ConnectedSources,
    mut presence_rx: tokio::sync::mpsc::UnboundedReceiver<PresenceMsg>,
    presence_exit_watch: Option<daemon::PresenceExitWatch>,
) {
    let mut reducer = Reducer::new();
    // Disabled once the presence channel closes (all senders dropped) so its
    // `recv() -> None` branch can't busy-loop the select.
    let mut presence_open = true;
    // Registered sources already announced as gated (see the connection gate).
    // Bounded by the registry, NOT by the wire: `_pixtuoid_source` arrives
    // verbatim from socket JSON with no registry check and no length cap, and an
    // unknown name is a supported, tested decode path — so keying this on the
    // raw string would let a long-lived `run` accumulate one entry per distinct
    // name seen. An unregistered source is a DRIFT story, not a connection-gate
    // one, and `source/drift.rs` already owns it.
    let mut gate_logged: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    let initial_caps: [usize; MAX_FLOORS] =
        std::array::from_fn(|i| floor_caps[i].load(Ordering::Relaxed));
    let mut scene = SceneState::new(initial_caps);
    // 1-Hz tick so exit-grace sweeps run even when no new events arrive.
    const SWEEP_TICK_INTERVAL_SECS: u64 = 1;
    let mut sweep_interval = tokio::time::interval(Duration::from_secs(SWEEP_TICK_INTERVAL_SECS));
    sweep_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        // Sync per-floor capacities from the shared atomics so the
        // auto-computed layout capacity propagates to next_free_desk().
        for (i, a) in floor_caps.iter().enumerate() {
            scene.floor_capacities[i] = a.load(Ordering::Relaxed);
        }
        tokio::select! {
            event = rx.recv() => {
                let Some((transport, ev)) = event else { break };
                // Connection gate + apply, in the shared `gate` core (covered +
                // mutation-tested; the async shell here is neither). A gated event
                // mutates nothing, so publish only when it actually applied.
                if gate::apply_gated_event(
                    &mut reducer,
                    &mut scene,
                    ev,
                    transport,
                    &connected,
                    SystemTime::now(),
                    &mut gate_logged,
                ) && scene_tx.send(Arc::new(scene.clone())).is_err()
                {
                    tracing::warn!("scene channel closed — renderer dropped");
                    break;
                }
            }
            // Daemon-presence deltas — instance-tagged `(DaemonInstanceKey, delta)`
            // (hook-derived + `PidExited` from the shared exit watch) — merged into
            // SceneState::daemons, NEVER through Reducer::apply (which is
            // AgentId-pure). Invariant #2. N daemons AND N instances of one daemon
            // (two OpenClaw gateways) route by that key, so nothing here is
            // per-source special-cased.
            update = presence_rx.recv(), if presence_open => {
                match update {
                    Some(PresenceMsg { key, delta }) => {
                        // Connection gate + armable-pid selection + apply, in the
                        // shared `gate` core (covered + mutation-tested) — the
                        // presence twin of `apply_gated_event`. A disconnected
                        // daemon's delta is dropped (walked out by the sweep-tick
                        // reconcile); only `ew.watch` (IO) + the publish stay here.
                        if let gate::PresenceGate::Applied { arm_pid } = gate::apply_gated_presence(
                            &mut scene,
                            &key,
                            delta,
                            &connected,
                            SystemTime::now(),
                        ) {
                            // Arm the instant abrupt-down watch on the gateway pid
                            // (GatewayUp/PidSeen #318). Arming AFTER apply_presence is
                            // safe: the two touch DISJOINT state (the exit-watch pid
                            // map vs scene.daemons), and a dead pid's synthesized
                            // PidExited (ESRCH at registration) re-enters only on a
                            // LATER select iteration — it can never observe a
                            // half-applied arm within this synchronous arm.
                            if let (Some(ew), Some(pid)) = (presence_exit_watch.as_ref(), arm_pid) {
                                ew.watch(&key, pid);
                            }
                            if scene_tx.send(Arc::new(scene.clone())).is_err() {
                                tracing::warn!("scene channel closed — renderer dropped");
                                break;
                            }
                        }
                    }
                    None => presence_open = false,
                }
            }
            _ = sweep_interval.tick() => {
                // Walk out disconnected sources, advance exit-grace, decay daemon
                // presence — the shared `gate` core (covered + mutation-tested; a
                // 2nd daemon needs no edit, it is registry-DRIVEN).
                gate::reconcile_sweep_tick(&mut reducer, &mut scene, &connected, SystemTime::now());
                if scene_tx.send(Arc::new(scene.clone())).is_err() {
                    tracing::warn!("scene channel closed — renderer dropped");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The documented "a source registered in the core registry but NOT wired
    // into run_async passes every conformance/manifest test yet never spawns"
    // gap — closed. `build_source_set` constructs the shared-
    // socket `HookRouter` PLUS every transcript-bearing registered source
    // (`line_decoder().is_some()`); it reads names off the real boxes, so it
    // can't drift from a hand-maintained second list. Hook-only sources + the
    // daemon (OpenClaw) are absent by design (they ride the router's socket).
    #[test]
    fn build_source_set_wires_every_transcript_bearing_source_plus_the_hook_router() {
        use pixtuoid_core::source::registry::{self, descriptor_for};
        use std::collections::BTreeSet;

        let sources = build_source_set(PathBuf::from("/tmp/pixtuoid-test.sock"), None, None, None);
        let built: BTreeSet<&str> = sources.iter().map(|s| s.name()).collect();

        // The HookRouter (infrastructure — owns the shared socket, NOT a
        // registered CLI) must be in the set so its fatal-bind death surfaces via
        // `spawn_with_health` (#157); it has no descriptor, so it's excluded from
        // the transcript-coverage check below.
        assert!(
            built.contains("hook-router"),
            "the shared-socket HookRouter must be spawned (else hook signals never decode)"
        );

        let transcript_built: BTreeSet<&str> = built
            .iter()
            .copied()
            .filter(|&n| n != "hook-router")
            .collect();
        let expected: BTreeSet<&str> = registry::registered_source_names()
            .filter(|&name| descriptor_for(name).is_some_and(|d| d.line_decoder().is_some()))
            .collect();
        assert_eq!(
            transcript_built, expected,
            "run_async's transcript-source wiring diverged from the registry: a \
             transcript-bearing source is registered but not built (it would never \
             spawn), or a built source isn't registered"
        );
    }
}
