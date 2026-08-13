//! Source-control core: detect, connect, disconnect and reconcile agent CLIs.
//!
//! ONE home for "which agent CLIs exist, their connection state, and how to
//! change it." The scriptable CLI (`sources|connect|disconnect|setup`) sits on
//! top. The mutating ops here are the
//! PERSISTED half — they write the `[sources]` flag + install/uninstall hooks,
//! but do not touch a running window's live `ConnectedSources`; changes appear
//! on the next launch.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use pixtuoid_core::source::registry;

use crate::config;
use crate::install::{
    self,
    target::{by_source, is_present, Target},
    InstallReport, UninstallReport,
};

/// The wire-facing outcome token used by JSON CLI consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireOutcome {
    Connected,
    Disconnected,
    NoOp,
    Failed,
}

impl WireOutcome {
    /// The serialized token — the ONE string authority (serde's snake_case
    /// rename and this table are pinned equal by `wire_outcome_serializes_as_its_token`).
    pub fn token(self) -> &'static str {
        match self {
            WireOutcome::Connected => "connected",
            WireOutcome::Disconnected => "disconnected",
            WireOutcome::NoOp => "no_op",
            WireOutcome::Failed => "failed",
        }
    }
}

impl std::fmt::Display for WireOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.token())
    }
}

/// Outcome of a single connect/disconnect, so a batch (`reconcile_to`) can
/// report per-source without aborting the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeOutcome {
    Connected,
    Disconnected,
    /// Already in the desired state — nothing written.
    NoOp,
    /// The change failed (e.g. a hook install error); the message is human-readable.
    Failed(String),
}

impl ChangeOutcome {
    /// Stable BARE wire token for `--json` / scripting — a machine-matchable
    /// value, never carrying human text (the detail rides in [`Self::message`]).
    /// Kept separate from the enum's `Debug` so the JSON contract can't drift
    /// if a variant is renamed.
    pub fn wire_outcome(&self) -> WireOutcome {
        match self {
            ChangeOutcome::Connected => WireOutcome::Connected,
            ChangeOutcome::Disconnected => WireOutcome::Disconnected,
            ChangeOutcome::NoOp => WireOutcome::NoOp,
            ChangeOutcome::Failed(_) => WireOutcome::Failed,
        }
    }

    /// The serialized token for this outcome (via [`WireOutcome::token`]).
    pub fn wire_token(&self) -> &'static str {
        self.wire_outcome().token()
    }

    /// The human-readable detail alongside the token — `Some` exactly for
    /// `Failed` (the only variant that carries any).
    pub fn message(&self) -> Option<&str> {
        match self {
            ChangeOutcome::Failed(msg) => Some(msg),
            _ => None,
        }
    }
}

/// One row of the stable `--json` batch envelope emitted by
/// `connect`, `disconnect` and `sources set`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OutcomeRow {
    /// The registry source id the outcome applies to (e.g. `codex`).
    pub id: String,
    /// The BARE outcome token: `connected` | `disconnected` | `no_op` |
    /// `failed` — a schema ENUM, so the generated TS side is a string-literal
    /// union (machine-matchable with `===`); human text rides in `message`.
    pub outcome: WireOutcome,
    /// Human-readable detail for the row — present exactly when the outcome
    /// carries any (`failed`), and OMITTED (not `null`) otherwise, so a
    /// success row stays the minimal `{id, outcome}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl OutcomeRow {
    /// Map one applied outcome to its wire row: the bare token plus the
    /// optional human message — the ONE outcome→row authority, so the two
    /// emitting surfaces (`run_change` / `run_sources_set`) can't drift.
    ///
    /// The message is control-char-stripped HERE, where the untrusted value
    /// enters the row (the `verify::display_safe` discipline): it folds another
    /// CLI's config content verbatim — a failed `connect codex` carries
    /// `toml::de::Error`'s Display, which embeds the RAW offending source line —
    /// and `sources_cli::text_line` prints it to a real terminal (R0615-06).
    pub fn new(id: String, outcome: &ChangeOutcome) -> Self {
        OutcomeRow {
            id,
            outcome: outcome.wire_outcome(),
            message: outcome.message().map(crate::strip_control_chars),
        }
    }
}

/// A serializable status row for `maple-agent-market sources --json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SourceStatus {
    pub id: String,
    pub display_name: String,
    pub connected: bool,
    pub cli_present: bool,
    /// A health/issue summary (install-broken / decode-drift), or `null` when n/a.
    pub health: Option<String>,
}

/// Resolve a user-supplied id to the `'static` registry id, or a clear error
/// (the CLI surface takes arbitrary input; `config::save_source_connected`
/// needs `&'static str`). Mirrors how the panel only ever feeds registry ids.
pub fn registered_id(id: &str) -> Result<&'static str> {
    registry::registered_source_names()
        .find(|s| *s == id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unknown source '{id}' (known: {})",
                registry::registered_source_names()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// Result of a successful single connect — carries the `InstallReport` for a
/// target-bearing source so the panel can render its rich notes (backup / PATH
/// warning); `FlagOnly` for a no-target (JSONL-only) source.
#[derive(Debug)]
pub enum ConnectOutcome {
    FlagOnly,
    Installed(InstallReport),
}

/// Result of a single disconnect whose FLAG was persisted false (the user IS
/// disconnected). `Err` from `disconnect` is reserved for the persist-failure
/// abort; a failed hook removal is folded in here (harmless stale hooks remain
/// behind the now-closed gate), so the gate still closes — mirroring the original
/// panel asymmetry (connect rolls back on install failure; disconnect does not).
#[derive(Debug)]
pub enum DisconnectOutcome {
    FlagOnly,
    Uninstalled(UninstallReport),
    /// Flag persisted false, but removing the hooks errored. Carries the message.
    HookRemovalFailed(String),
}

/// The step (if any) a user must still take after a successful `connect` of `id`,
/// read from that target's ONE `post_install_hint` — exposed here because the
/// scriptable CLI presenter lives in the bin crate and `install::target` is
/// `pub(crate)`. `None` for a source with no target, and for every target whose
/// hooks take effect on the CLI's next run.
pub fn post_install_hint(id: &str) -> Option<&'static str> {
    crate::install::target::by_source(id).and_then(|t| t.post_install_hint)
}

/// Connect a source: PERSIST the `[sources]` flag FIRST (so it survives restart),
/// then — only for a target-bearing source — install its hooks, rolling the flag
/// back if the install fails (a persisted "connected" with no integration behind
/// it would show connected yet never produce an agent). A separate CLI process
/// has no live set, so a running window observes the change on its next launch.
///
/// **Honors the explicit id — it does NOT gate on CLI presence.**
/// `connect` / `reconcile_to` install for any registered id even if that CLI isn't
/// installed yet — pre-provisioning for automation/onboarding where the caller
/// stated intent. (`detect()` returns only PRESENT CLIs, so onboarding offers
/// only installed ones; a `connect <absent-cli>` is a deliberate user/script
/// choice that materializes that CLI's config dir.)
pub fn connect(cfg: &Path, id: &str) -> Result<ConnectOutcome> {
    let sid = registered_id(id)?;
    connect_target(cfg, sid, by_source(sid))
}

/// The persist + install + rollback core, with the `target` passed EXPLICITLY so
/// tests can inject a deterministic-fail fake (`connect` resolves it from the
/// registry). Hooks install at the target's default config path — the
/// per-target config override is `install_target`'s own seam (exercised there),
/// and the sources layer only ever wants the default, so it isn't re-threaded.
fn connect_target(
    cfg: &Path,
    sid: &'static str,
    target: Option<&Target>,
) -> Result<ConnectOutcome> {
    // Capture the PRIOR flag before the optimistic save, so a failed install
    // restores the exact pre-attempt state: a re-connect of an ALREADY-connected
    // source (`connect` re-run, `setup --yes`) can fail while the old, working
    // hooks stay on disk — forcing `false` there would silently disconnect a
    // healthy source on the next launch.
    let prior = config::load(cfg, &mut Vec::new()).sources.get(sid).copied();
    config::save_source_connected(cfg, sid, true)?;
    match target {
        Some(t) => match install::install_target(t, None, None) {
            Ok(r) => Ok(ConnectOutcome::Installed(r)),
            Err(e) => {
                // Roll the flag back to the prior state so the next launch
                // doesn't honor a "connected" with no hooks behind it — and an
                // absent flag rolls back to ABSENT (preserving the
                // unconfigured empty-table state), not an explicit `false`.
                let restore = match prior {
                    Some(v) => config::save_source_connected(cfg, sid, v),
                    None => config::remove_source_connected(cfg, sid),
                };
                if let Err(re) = restore {
                    // The write path just succeeded, so this is rare — but a
                    // silently-failed restore leaves flag=true with no hooks
                    // (the shown-but-broken class), so it must leave a trace.
                    // The chain can carry a `toml_edit` parse failure — raw config
                    // content — and `connect` writes tracing to RAW stderr.
                    tracing::warn!(
                        source = sid,
                        error = %crate::strip_control_chars(&format!("{re:#}")),
                        "connect rollback failed to restore the prior [sources] flag"
                    );
                }
                Err(e)
            }
        },
        None => Ok(ConnectOutcome::FlagOnly),
    }
}

/// Disconnect a source: persist the flag false FIRST, then remove its hooks
/// (target-bearing only). No rollback — a failed uninstall still leaves the user
/// disconnected (the safer direction).
pub fn disconnect(cfg: &Path, id: &str) -> Result<DisconnectOutcome> {
    let sid = registered_id(id)?;
    disconnect_target(cfg, sid, by_source(sid))
}

fn disconnect_target(
    cfg: &Path,
    sid: &'static str,
    target: Option<&Target>,
) -> Result<DisconnectOutcome> {
    // `?` here = the persist-failure abort (flip nothing). Past it, the flag is
    // false, so a hook-removal error folds into the outcome rather than erroring.
    config::save_source_connected(cfg, sid, false)?;
    Ok(match target {
        Some(t) => match install::uninstall_target(t, None) {
            Ok(r) => DisconnectOutcome::Uninstalled(r),
            Err(e) => DisconnectOutcome::HookRemovalFailed(format!("{e:#}")),
        },
        None => DisconnectOutcome::FlagOnly,
    })
}

/// What `reconcile_to` should do to one source. Pure — see `plan_reconcile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Connect,
    Disconnect,
    NoOp,
}

/// PURE diff: given the CURRENT connected-set and the DESIRED set, decide each
/// registered source's action. The declarative "connected set = exactly these"
/// semantics a declarative `sources set` consumer needs: a source in
/// `desired` but not `current` → Connect; in `current` but not `desired` →
/// Disconnect; otherwise NoOp. Ids outside the source registry are ignored
/// here (the I/O wrapper validates them up front so an unknown id is a loud
/// error, not a silent drop).
pub(crate) fn plan_reconcile(
    current: &HashSet<String>,
    desired: &HashSet<String>,
) -> Vec<(&'static str, Action)> {
    registry::registered_source_names()
        .map(|sid| {
            let want = desired.contains(sid);
            let have = current.contains(sid);
            let action = match (want, have) {
                (true, false) => Action::Connect,
                (false, true) => Action::Disconnect,
                _ => Action::NoOp,
            };
            (sid, action)
        })
        .collect()
}

/// Declarative apply: make the connected set exactly `desired` (the automation
/// checkbox-form / `sources set` semantics). For each registered source: connect
/// the newly-desired, disconnect the no-longer-desired, NoOp the rest — reporting
/// each (a failed item doesn't abort the batch). The CURRENT set is computed the
/// same way the boot seed is (`config::resolve_connected` — explicit `true`
/// flags only, pure config read since the 0.12.0 migrate-inference removal).
pub fn reconcile_to(cfg: &Path, desired: &HashSet<String>) -> Vec<(String, ChangeOutcome)> {
    let app = config::load(cfg, &mut Vec::new());
    let current = config::resolve_connected(&app);
    plan_reconcile(&current, desired)
        .into_iter()
        .map(|(sid, action)| (sid.to_string(), apply_one(cfg, sid, action)))
        .collect()
}

/// Apply ONE planned action and map it to a reportable `ChangeOutcome`. The single
/// connect/disconnect→outcome mapping shared by `reconcile_to` (declarative) and
/// `apply_choices` (the explicit onboarding list) so the folded-hook-removal-
/// failure surfacing can't drift between them.
fn apply_one(cfg: &Path, sid: &'static str, action: Action) -> ChangeOutcome {
    match action {
        Action::Connect => match connect(cfg, sid) {
            Ok(_) => ChangeOutcome::Connected,
            Err(e) => ChangeOutcome::Failed(format!("{e:#}")),
        },
        Action::Disconnect => match disconnect(cfg, sid) {
            Ok(o) => map_disconnect_outcome(o),
            Err(e) => ChangeOutcome::Failed(format!("{e:#}")),
        },
        Action::NoOp => ChangeOutcome::NoOp,
    }
}

/// The marker a folded hook-removal failure carries into [`ChangeOutcome::Failed`]
/// — the `sources set` wire token AND the tag a presenter reads back to tell the
/// fold apart from a real failure (the disconnect itself SUCCEEDED; only the hook
/// removal didn't). One definition keeps all CLI presenters aligned.
pub(crate) const HOOK_REMOVAL_FAILED_PREFIX: &str = "hooks not removed: ";

/// Human wording for the disconnect-landed / hook-removal-failed fold. This is
/// `pub` because the binary's `main.rs` is a separate crate and consumes it.
pub const HOOK_REMOVAL_FAILED_PHRASE: &str = "disconnected, but hook removal failed";

/// Map a SUCCESSFUL `disconnect`'s [`DisconnectOutcome`] to the CLI
/// [`ChangeOutcome`]. Split out of `apply_one` so the load-bearing fold is
/// teeth-testable apart from the real install FS path: a folded hook-removal
/// failure MUST surface as `Failed` (with the reason), NEVER a clean
/// `Disconnected` — else a caller hides stale hooks behind a clean "disconnected".
fn map_disconnect_outcome(o: DisconnectOutcome) -> ChangeOutcome {
    match o {
        DisconnectOutcome::HookRemovalFailed(e) => {
            ChangeOutcome::Failed(format!("{HOOK_REMOVAL_FAILED_PREFIX}{e}"))
        }
        DisconnectOutcome::FlagOnly | DisconnectOutcome::Uninstalled(_) => {
            ChangeOutcome::Disconnected
        }
    }
}

/// Apply an EXPLICIT per-source decision list (the first-run onboarding apply):
/// connect each `true` id, disconnect each `false` id. Unlike `reconcile_to` (which
/// is declarative over EVERY registered source and would disconnect the complement),
/// this touches ONLY the ids passed — a source absent from the list (e.g.
/// `antigravity`, which never appears in `detect()`) keeps its existing flag —
/// or, absent one, the plain disconnected default — never a surprise write.
/// Each write makes `[sources]` explicit. Idempotent: connect / disconnect are
/// no-ops at the install layer when already in state.
pub fn apply_choices(cfg: &Path, choices: &[(&'static str, bool)]) -> Vec<(String, ChangeOutcome)> {
    choices
        .iter()
        .map(|&(sid, want)| {
            let action = if want {
                Action::Connect
            } else {
                Action::Disconnect
            };
            (sid.to_string(), apply_one(cfg, sid, action))
        })
        .collect()
}

// ---- Source status MODEL (moved here from `tui::connection`, which re-exports
//      it so the panel/harness are unchanged). Pure: no ratatui, no SceneState. ----

/// Connection state for one CLI row — the single facet the toggle acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// Bound: this source's events flow and its characters show. Toggle disconnects.
    Connected,
    /// Unbound: the gate is closed; no characters. Toggle connects.
    Disconnected,
    /// A target-bearing CLI that isn't installed on this machine — nothing to bind
    /// to. Carries the persisted `[sources]` intent (`connected`) because `NoCli`
    /// overrides the `Connected`/`Disconnected` display (an absent CLI is worth
    /// surfacing), yet a connected-but-absent source is still disconnectable — its
    /// hooks live in the config, not the missing binary — so the toggle needs the
    /// bit the display hides.
    NoCli { connected: bool },
}

impl ConnState {
    /// Whether this source is in the live connected-set (the persisted `[sources]`
    /// intent): `true` for `Connected`, `false` for `Disconnected`, and the carried
    /// bit for `NoCli` (a connected-but-absent CLI is still disconnectable). The ONE
    /// derivation now that `ConnectionRow` no longer stores the bit separately.
    pub fn connected(self) -> bool {
        match self {
            ConnState::Connected => true,
            ConnState::Disconnected => false,
            ConnState::NoCli { connected } => connected,
        }
    }
}

/// One row = one agent CLI (the union of registry sources + install targets).
#[derive(Debug, Clone)]
pub struct ConnectionRow {
    /// The core source id (registry `SourceDescriptor.name`, e.g. "claude-code")
    /// — the unifying key; joined to an install target via `Target.core_source`.
    pub source_id: &'static str,
    /// 2-char badge id (`cc`/`cx`/…), from the source descriptor.
    pub label_prefix: &'static str,
    pub display_name: &'static str,
    /// The connection facet — and, for `NoCli`, the persisted-intent bit the
    /// display hides (read it via `ConnState::connected`). The row no longer
    /// stores `connected` separately: `state` is the single source of truth.
    pub state: ConnState,
    /// The config the hooks live in; `None` for no-target (JSONL-only) rows.
    pub config_path: Option<PathBuf>,
    /// The install target backing this row; `None` ⇒ connect/disconnect is a
    /// flag-only flip (Antigravity — no hooks to write).
    pub target: Option<&'static Target>,
    /// Cached HEALTH summary — a one-line `⚠ …` verdict from
    /// `doctor::diagnose(..).summary()`, computed for CONNECTED rows only.
    pub health: Option<String>,
}

/// Per-target filesystem facts, injected so `build_rows_from` is pure (the FS
/// reads happen in `build_rows`). `Some` exactly when the row has an install target.
#[derive(Debug, Clone)]
pub struct RowFacts {
    pub present: bool,
    pub config_path: Option<PathBuf>,
}

/// One pure input row for `build_rows_from`.
#[derive(Debug, Clone)]
pub struct RowInput {
    pub source_id: &'static str,
    pub label_prefix: &'static str,
    pub target: Option<&'static Target>,
    pub facts: Option<RowFacts>,
    /// Whether this source is in the live connected-set (the persisted intent).
    pub connected: bool,
    /// Cached health summary (injected so `build_rows_from` stays pure).
    pub health: Option<String>,
}

/// Title-case the no-target sources (the registry omits their display names).
fn display_name_for(source_id: &'static str) -> &'static str {
    match source_id {
        "antigravity" => "Antigravity",
        "copilot" => "Copilot CLI",
        "omp" => "Oh My Pi",
        other => other,
    }
}

/// Pure row builder over injected facts — the testable core of `build_rows`.
/// A target-bearing CLI that isn't present is `NoCli`; otherwise the connected-set
/// is authoritative.
pub fn build_rows_from(inputs: Vec<RowInput>) -> Vec<ConnectionRow> {
    inputs
        .into_iter()
        .map(|input| {
            let absent_cli = matches!(
                (&input.target, &input.facts),
                (Some(_), Some(f)) if !f.present
            );
            let state = if absent_cli {
                ConnState::NoCli {
                    connected: input.connected,
                }
            } else if input.connected {
                ConnState::Connected
            } else {
                ConnState::Disconnected
            };
            ConnectionRow {
                source_id: input.source_id,
                label_prefix: input.label_prefix,
                display_name: input
                    .target
                    .map_or_else(|| display_name_for(input.source_id), |t| t.display_name),
                state,
                config_path: input.facts.and_then(|f| f.config_path),
                target: input.target,
                health: input.health,
            }
        })
        .collect()
}

/// Build the status rows from the registry + install targets + the connected-set.
/// Performs FS reads (`is_present`/`default_config_path`) AND, for connected rows,
/// the health rollup (`doctor::diagnose`). `log` is the warn-floor log text.
pub fn build_rows(connected: &HashSet<String>, log: &str) -> Vec<ConnectionRow> {
    let inputs = pixtuoid_core::source::registry::REGISTRY
        .iter()
        .map(|d| {
            // Join on the SOURCE id via `core_source`, NOT `by_name`: Claude's
            // target is "claude" but its source is "claude-code".
            let target = by_source(d.name);
            let facts = target.map(|t| RowFacts {
                present: is_present(t),
                config_path: (t.default_config_path)().ok(),
            });
            let connected = connected.contains(d.name);
            RowInput {
                source_id: d.name,
                label_prefix: d.label_prefix,
                target,
                facts,
                connected,
                health: connected
                    .then(|| crate::doctor::diagnose(d.name, log, None).summary())
                    .flatten(),
            }
        })
        .collect();
    build_rows_from(inputs)
}

/// Map a status row to the serializable `SourceStatus` DTO used by the JSON CLI.
///
/// NOTE: the wire `connected` here is deliberately PRESENT-AND-BOUND
/// (`state == Connected`), NOT the persisted `[sources]` intent bit
/// (`ConnState::connected` — which stays `true` for a connected-but-absent `NoCli`
/// source). The two answer different questions; the wire keeps the
/// present-and-bound meaning it always had (changing it is a `--json` contract
/// change needing `gen-contract`).
fn status_from_row(r: &ConnectionRow) -> SourceStatus {
    SourceStatus {
        id: r.source_id.to_string(),
        display_name: r.display_name.to_string(),
        connected: matches!(r.state, ConnState::Connected),
        // A no-target (JSONL-only) source is always "present"; a target-bearing
        // one is present unless probed absent (`NoCli`).
        cli_present: !matches!(r.state, ConnState::NoCli { .. }),
        health: r.health.clone(),
    }
}

/// The status of every registered source — what
/// `maple-agent-market sources [--json]` and onboarding read. Resolves the
/// connected-set the same way the boot seed does.
pub fn status(cfg: &Path, log: &str) -> Vec<SourceStatus> {
    let app = config::load(cfg, &mut Vec::new());
    let connected = config::resolve_connected(&app);
    build_rows(&connected, log)
        .iter()
        .map(status_from_row)
        .collect()
}

/// Which agent CLIs are installed on this machine (target-bearing + probed present)
/// — the "offer to connect these" set for first-run onboarding.
pub fn detect() -> Vec<&'static str> {
    registry::registered_source_names()
        .filter(|sid| by_source(sid).is_some_and(is_present))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// The lookup behind every presenter's post-install step. Mutation testing found
    /// it wholly untested: returning `None`, `Some("")` or `Some("xyzzy")` from here
    /// all passed the suite, so the feature could have gone silent — or started
    /// printing nonsense — without one red test.
    #[test]
    fn post_install_hint_names_a_real_step_only_for_targets_that_need_one() {
        // OpenClaw is the one target whose install does NOT take effect on the CLI's
        // next run: a RUNNING gateway loads plugins at boot, so it must be restarted.
        let hint = post_install_hint("openclaw").expect("openclaw needs a restart step");
        assert!(
            hint.contains("restart") && hint.contains("gateway"),
            "the step must actually say to restart the gateway — got {hint:?}"
        );
        assert!(
            hint.contains("openclaw gateway restart"),
            "and name the runnable command, so the user need not guess — got {hint:?}"
        );

        // Every other registered source's hooks apply on its next run, so a hint there
        // would be a nag with no action. Ranged over the REGISTRY, not a hand list, so
        // a new source must consciously opt in.
        for id in pixtuoid_core::source::registry::registered_source_names() {
            if id == "openclaw" {
                continue;
            }
            assert!(
                post_install_hint(id).is_none(),
                "{id} declares a post-install step — if that is intended, assert it here"
            );
        }
        // An id with no target at all resolves to no step rather than panicking.
        assert!(post_install_hint("not-a-source").is_none());
    }

    #[test]
    fn status_from_row_connected_is_present_and_bound_not_persisted_intent() {
        // The wire `connected` is PRESENT-AND-BOUND (state == Connected), NOT the
        // persisted `[sources]` intent bit. A NoCli{connected:true} (absent CLI whose
        // stored intent is "connected") must serialize connected:false AND
        // cli_present:false — the divergence status_from_row's doc warns about. The
        // empty-HOME golden only ever produces NoCli{connected:false}+connected:false
        // rows, so it can't distinguish `matches!(Connected)` from `state.connected()`.
        let row = |state| ConnectionRow {
            source_id: "claude-code",
            label_prefix: "cc",
            display_name: "Claude Code",
            state,
            config_path: None,
            target: None,
            health: None,
        };
        let connected = status_from_row(&row(ConnState::Connected));
        assert!(connected.connected, "Connected → wire connected:true");
        assert!(connected.cli_present, "Connected → present");

        let nocli_intent_on = status_from_row(&row(ConnState::NoCli { connected: true }));
        assert!(
            !nocli_intent_on.connected,
            "NoCli persisted-intent true must NOT leak as wire connected (present-and-bound is false)"
        );
        assert!(!nocli_intent_on.cli_present, "an absent CLI is not present");
    }

    #[test]
    fn registered_id_accepts_known_rejects_unknown() {
        assert_eq!(registered_id("antigravity").unwrap(), "antigravity");
        let err = registered_id("not-a-source").unwrap_err().to_string();
        assert!(err.contains("unknown source 'not-a-source'"), "{err}");
        assert!(err.contains("antigravity"), "lists known sources: {err}");
    }

    #[test]
    fn map_disconnect_outcome_surfaces_a_folded_hook_removal_failure() {
        // The safety fold: a disconnect whose flag flipped but whose hooks did NOT
        // get removed must surface as Failed (with the reason), never a clean
        // Disconnected — else a caller hides stale hooks behind "disconnected".
        match map_disconnect_outcome(DisconnectOutcome::HookRemovalFailed("boom".into())) {
            ChangeOutcome::Failed(m) => assert_eq!(m, "hooks not removed: boom"),
            other => panic!("expected Failed, got {other:?}"),
        }
        // A clean disconnect (flag-only) maps to a plain Disconnected.
        assert!(matches!(
            map_disconnect_outcome(DisconnectOutcome::FlagOnly),
            ChangeOutcome::Disconnected
        ));
    }

    #[test]
    fn connect_then_disconnect_a_no_target_source_persists_the_flag() {
        // Antigravity has no install target → connect/disconnect is a pure flag
        // flip (no agent-config I/O), so we can exercise the persist round in a
        // tempdir without touching any real ~/.claude-style file. No env mutation
        // (the cfg path is explicit), so no TEST_ENV_LOCK needed.
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");

        assert!(matches!(
            connect(&cfg, "antigravity").unwrap(),
            ConnectOutcome::FlagOnly
        ));
        let app = config::load(&cfg, &mut Vec::new());
        assert_eq!(
            app.sources.get("antigravity"),
            Some(&true),
            "flag persisted true"
        );

        assert!(matches!(
            disconnect(&cfg, "antigravity").unwrap(),
            DisconnectOutcome::FlagOnly
        ));
        let app = config::load(&cfg, &mut Vec::new());
        assert_eq!(
            app.sources.get("antigravity"),
            Some(&false),
            "flag persisted false"
        );
    }

    // A target whose install ALWAYS fails (its `default_config_path` errs, so
    // `install_target` bails before any FS) — exercises `connect_target`'s
    // install-failure rollback deterministically + cross-platform.
    static FAIL_TARGET: Target = Target {
        name: "rollbacktest",
        core_source: "rollbacktest",
        display_name: "RollbackTest",
        default_config_path: || Err(anyhow::anyhow!("forced install failure")),
        hook_command: |_, _| Ok(String::new()),
        merge_install: |c, _| {
            Ok(crate::install::target::MergeOutcome {
                content: c.to_string(),
                changed: false,
            })
        },
        merge_uninstall: |c| {
            Ok(crate::install::target::MergeOutcome {
                content: c.to_string(),
                changed: false,
            })
        },
        verify_schema: |_| crate::install::verify::SchemaParse::broken("test fake"),
        binary_strategy: crate::install::target::BinaryStrategy::EmbedAbsolute,
        presence_probe: None,
        extra_artifacts: None,
        post_install_hint: None,
    };

    #[test]
    fn connect_target_rolls_the_flag_back_when_install_fails() {
        // Persist succeeds (writable cfg), THEN install_target fails → the flag
        // must roll back to its PRIOR state. From a fresh config that state is
        // ABSENT (keeping the is_first_run signal), not a forced `false`.
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let err = connect_target(&cfg, "rollbacktest", Some(&FAIL_TARGET)).unwrap_err();
        assert!(err.to_string().contains("forced install failure"), "{err}");
        let app = config::load(&cfg, &mut Vec::new());
        assert_eq!(
            app.sources.get("rollbacktest"),
            None,
            "a previously-absent flag rolls back to ABSENT, not false"
        );
    }

    #[test]
    fn connect_target_rollback_restores_a_previously_connected_flag() {
        // The already-connected re-connect case (`pixtuoid connect` re-run,
        // `setup --yes` over a working source): a failed re-install must RESTORE
        // the prior `true`, never force `false` — the old hooks are still on
        // disk and working, so persisting false silently disconnects a healthy
        // source on next launch.
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        config::save_source_connected(&cfg, "rollbacktest", true).unwrap();

        let err = connect_target(&cfg, "rollbacktest", Some(&FAIL_TARGET)).unwrap_err();
        assert!(err.to_string().contains("forced install failure"), "{err}");
        let app = config::load(&cfg, &mut Vec::new());
        assert_eq!(
            app.sources.get("rollbacktest"),
            Some(&true),
            "a previously-connected flag must survive a failed re-install"
        );
    }

    #[test]
    fn connect_target_rollback_restores_a_previously_disconnected_flag() {
        // Explicit prior `false` is restored as `false` (not removed — the
        // rollback restores the exact pre-attempt state, and removal would
        // re-open the is_first_run signal for an onboarded user).
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        config::save_source_connected(&cfg, "rollbacktest", false).unwrap();

        connect_target(&cfg, "rollbacktest", Some(&FAIL_TARGET)).unwrap_err();
        let app = config::load(&cfg, &mut Vec::new());
        assert_eq!(app.sources.get("rollbacktest"), Some(&false));
    }

    #[test]
    fn disconnect_target_folds_a_hook_removal_failure_into_the_outcome() {
        // FAIL_TARGET's uninstall errs (default_config_path errs) → the flag is
        // STILL persisted false (disconnect's primary semantics hold), and the
        // error is FOLDED into HookRemovalFailed (Err is reserved for the
        // persist-abort) so the gate still closes + the CLI/panel can surface it.
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let outcome = disconnect_target(&cfg, "rollbacktest", Some(&FAIL_TARGET)).unwrap();
        assert!(matches!(outcome, DisconnectOutcome::HookRemovalFailed(_)));
        let app = config::load(&cfg, &mut Vec::new());
        assert_eq!(
            app.sources.get("rollbacktest"),
            Some(&false),
            "the flag is persisted false even though hook removal failed"
        );
    }

    #[test]
    fn connect_rejects_an_unknown_source_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        assert!(connect(&cfg, "bogus").is_err());
        assert!(
            !cfg.exists(),
            "a rejected id must not create/write the config"
        );
    }

    #[test]
    fn plan_reconcile_is_declarative_and_idempotent() {
        let current = set(&["claude-code", "codex"]);
        let desired = set(&["claude-code", "cursor"]);
        let plan: std::collections::HashMap<_, _> =
            plan_reconcile(&current, &desired).into_iter().collect();
        assert_eq!(plan["codex"], Action::Disconnect, "in current, not desired");
        assert_eq!(plan["cursor"], Action::Connect, "in desired, not current");
        assert_eq!(plan["claude-code"], Action::NoOp, "in both");
        // A source in neither is a NoOp (not touched).
        assert_eq!(plan["antigravity"], Action::NoOp);

        // Idempotent: reconciling an already-matching state is all NoOp.
        let steady = plan_reconcile(&desired, &desired);
        assert!(
            steady.iter().all(|(_, a)| *a == Action::NoOp),
            "matching state ⇒ no changes"
        );
    }

    #[test]
    fn wire_outcome_serializes_as_its_token() {
        // serde's snake_case rename and the token() table are two spellings of
        // one contract — pin them equal for every variant.
        for w in [
            WireOutcome::Connected,
            WireOutcome::Disconnected,
            WireOutcome::NoOp,
            WireOutcome::Failed,
        ] {
            assert_eq!(
                serde_json::to_value(w).unwrap(),
                serde_json::Value::String(w.token().to_string())
            );
        }
    }

    #[test]
    fn change_outcome_wire_tokens_are_stable() {
        assert_eq!(ChangeOutcome::Connected.wire_token(), "connected");
        assert_eq!(ChangeOutcome::Disconnected.wire_token(), "disconnected");
        assert_eq!(ChangeOutcome::NoOp.wire_token(), "no_op");
        assert_eq!(ChangeOutcome::Failed("boom".into()).wire_token(), "failed");
        // The human detail rides SEPARATELY (the `message` field) — never
        // folded into the token.
        assert_eq!(ChangeOutcome::Failed("boom".into()).message(), Some("boom"));
        assert_eq!(ChangeOutcome::Connected.message(), None);
    }

    #[test]
    fn source_status_json_shape_is_stable() {
        let s = SourceStatus {
            id: "codex".into(),
            display_name: "Codex".into(),
            connected: true,
            cli_present: true,
            health: None,
        };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"id":"codex","display_name":"Codex","connected":true,"cli_present":true,"health":null}"#
        );
    }

    #[test]
    fn outcome_row_json_shape_is_stable() {
        // Pins the exact `{id, outcome, message?}` JSON row `connect`/
        // `disconnect`/`sources set --json` emit per source: a bare machine
        // token in `outcome`, the human detail in `message` — present exactly
        // on failure, OMITTED (not null) on success.
        let ok = OutcomeRow::new("codex".into(), &ChangeOutcome::Connected);
        let failed = OutcomeRow::new("cursor".into(), &ChangeOutcome::Failed("boom".into()));
        assert_eq!(
            serde_json::to_string(&ok).unwrap(),
            r#"{"id":"codex","outcome":"connected"}"#
        );
        assert_eq!(
            serde_json::to_string(&failed).unwrap(),
            r#"{"id":"cursor","outcome":"failed","message":"boom"}"#
        );
    }

    #[test]
    fn outcome_row_message_is_control_char_stripped_at_the_authority() {
        // `message` carries another CLI's config content verbatim: a failed
        // `connect codex` folds `toml::de::Error`'s Display, which embeds the RAW
        // offending source line, and `sources_cli::text_line` prints that row to a
        // real terminal. Sanitize where the untrusted value ENTERS the row (the
        // `verify::display_safe` discipline) so no present or future presenter can
        // reopen the hole (R0615-06).
        let row = OutcomeRow::new(
            "codex".into(),
            &ChangeOutcome::Failed("bad\u{1b}]0;PWNED\u{7}key\u{202e}txet".into()),
        );
        assert_eq!(row.message.as_deref(), Some("bad]0;PWNEDkeytxet"));

        // Stays silent on ordinary text: an untouched message is byte-identical.
        let clean = "processing /home/u/.codex/config.toml: not valid TOML";
        assert_eq!(
            OutcomeRow::new("codex".into(), &ChangeOutcome::Failed(clean.into()))
                .message
                .as_deref(),
            Some(clean)
        );
    }

    #[test]
    fn reconcile_to_disconnects_the_complement_and_noops_the_rest() {
        // Drive only the no-target source (antigravity) to avoid agent-config I/O;
        // every other source has no flag ⇒ resolves "not connected" (NoOp under an
        // empty desired — resolve_connected reads only explicit flags since the
        // 0.12.0 migrate-inference removal, so no install-state injection needed).
        // Pre-set antigravity connected; desired={} ⇒ antigravity disconnects,
        // all targets NoOp. Deterministic, no real hooks.
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        connect(&cfg, "antigravity").unwrap(); // flag → true

        let outcomes: std::collections::HashMap<_, _> =
            reconcile_to(&cfg, &HashSet::new()).into_iter().collect();

        assert_eq!(outcomes["antigravity"], ChangeOutcome::Disconnected);
        assert_eq!(
            outcomes["codex"],
            ChangeOutcome::NoOp,
            "not connected → no change"
        );
        // The flag was actually written.
        let app = config::load(&cfg, &mut Vec::new());
        assert_eq!(app.sources.get("antigravity"), Some(&false));
    }

    #[test]
    fn apply_choices_writes_only_the_listed_sources() {
        // The onboarding apply is SCOPED to the ids passed — a source absent from
        // the list is never touched (the "an unlisted source's flag is never
        // written" property that a declarative reconcile_to would break). Drive
        // only the no-target source so there's no agent-config I/O.
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");

        let outcomes: std::collections::HashMap<_, _> =
            apply_choices(&cfg, &[("antigravity", true)])
                .into_iter()
                .collect();
        assert_eq!(outcomes["antigravity"], ChangeOutcome::Connected);

        let app = config::load(&cfg, &mut Vec::new());
        assert_eq!(
            app.sources.get("antigravity"),
            Some(&true),
            "listed → written"
        );
        assert_eq!(app.sources.get("codex"), None, "unlisted → untouched");

        // Unchecked (the uncheck / skip-freeze path) persists false.
        apply_choices(&cfg, &[("antigravity", false)]);
        let app = config::load(&cfg, &mut Vec::new());
        assert_eq!(app.sources.get("antigravity"), Some(&false));
    }
}
