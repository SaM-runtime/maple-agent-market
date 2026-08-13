//! The `native`-only runtime half of the Codex source: the liveness probe
//! (open-rollout FD binding) + `CodexSource` and its `JsonlWatcher` wiring.
//! The pure decoder stays in the always-compiled parent module; this whole
//! file sits behind the parent's ONE `#[cfg(feature = "native")] mod native;`
//! gate and is re-exported there, so public paths don't move.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use super::{codex_home, codex_id_from_path, decode_codex_line, SOURCE_NAME};
use crate::source::decoder::{derive_prefixed_label, display_safe, label_prefix_for};
use crate::source::jsonl::{ChildEndUnclaims, JsonlWatcher, ProbeSnapshot};
use crate::source::{Source, TaggedSender};

const CODEX_SESSION_INDEX_ENV: &str = "PIXTUOID_CODEX_SESSION_INDEX";
const TASK_TITLE_MAX_CHARS: usize = 11;

/// Codex writes no session-end marker; the reducer's stale-sweep reaps dead
/// sessions. Always false (defer to mtime window + stale-sweep).
fn codex_session_ended(_tail: &[u8]) -> bool {
    false
}

/// A structural Codex new-turn marker. The exact outer/inner pair prevents a
/// user message merely containing `task_started` from releasing the watcher's
/// first-sight claim.
fn codex_session_reentry(line: &[u8]) -> bool {
    const START_MARKERS: [&[u8]; 2] = [b"task_started", b"turn_started"];
    if !START_MARKERS
        .iter()
        .any(|marker| line.windows(marker.len()).any(|window| window == *marker))
    {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return false;
    };
    value.get("type").and_then(Value::as_str) == Some("event_msg")
        && matches!(
            value
                .get("payload")
                .and_then(|payload| payload.get("type"))
                .and_then(Value::as_str),
            Some("task_started" | "turn_started")
        )
}

/// Codex's liveness probe: the rollout UUIDs (in `codex_id_from_path`
/// id-space, so they join the watcher's first-sight gate directly) of every
/// rollout under `sessions_root` held OPEN by a running `codex` process, plus
/// the owning pid per id.
///
/// Codex has no session registry (unlike CC's `sessions/<pid>.json`), but a
/// live `codex` process holds its rollout file open in append mode for the
/// whole session (upstream `RolloutRecorder` owns the handle), so an open
/// rollout fd IS the first-party liveness signal: pid → open fd → rollout
/// path → UUID. The mechanics (canonicalize, under-root filter, #223 failure
/// semantics, #252 pid bind, the id-space fold via `walk::id_path`) live in
/// `ProbeSnapshot::from_open_fds`; only the RECOGNIZER (`is_rollout_filename`)
/// and the id deriver (`codex_id_from_path`) are ours (invariant #3).
pub fn live_codex_rollout_ids(sessions_root: &Path) -> Option<ProbeSnapshot> {
    // A held-open path vouches iff it is a `rollout-*.jsonl`; its id is the
    // rollout UUID via `codex_id_from_path` — the SAME fn `with_id_deriver`
    // installs, so probe ids and gate ids can't drift (UUIDs are fold-invariant).
    ProbeSnapshot::from_open_fds(
        sessions_root,
        &["codex"],
        is_rollout_filename,
        codex_id_from_path,
    )
}

fn is_rollout_filename(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("jsonl")
        && path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("rollout-"))
}

fn task_title_from_index(index: &str, session_id: &str) -> Option<String> {
    let mut latest = None;
    for line in index.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if entry.get("id").and_then(Value::as_str) != Some(session_id) {
            continue;
        }
        let Some(raw_title) = entry.get("thread_name").and_then(Value::as_str) else {
            continue;
        };
        let collapsed = raw_title.split_whitespace().collect::<Vec<_>>().join(" ");
        let safe = display_safe(&collapsed);
        let title: String = safe.chars().take(TASK_TITLE_MAX_CHARS).collect();
        if !title.is_empty() {
            latest = Some(title);
        }
    }
    latest
}

fn codex_task_label_from_index(
    rollout_path: &Path,
    source: &str,
    cwd: &Path,
    index: &str,
) -> String {
    let fallback = || derive_prefixed_label(source, cwd);
    let session_id = codex_id_from_path(rollout_path);
    let Some(title) = task_title_from_index(index, &session_id) else {
        return fallback();
    };
    format!("{}·{title}", label_prefix_for(source))
}

fn codex_task_label(rollout_path: &Path, source: &str, cwd: &Path) -> String {
    let fallback = || derive_prefixed_label(source, cwd);
    let Some(index_path) = std::env::var_os(CODEX_SESSION_INDEX_ENV) else {
        return fallback();
    };
    let Ok(index) = fs::read_to_string(index_path) else {
        return fallback();
    };
    codex_task_label_from_index(rollout_path, source, cwd, &index)
}

/// Attach the probe ONLY for codex's first-party layout: the standard
/// `~/.codex/sessions` shape (the root's file_name is literally `sessions`
/// AND its parent's is `.codex`) or the resolved `codex_home()/sessions` for
/// THIS environment (a `CODEX_HOME` user's real rollout root — codex itself
/// writes there, and rejecting it would silently drop the whole liveness
/// ladder for a supported config). Mirrors `cc_sessions_dir`'s gating: a
/// `--codex-sessions-root /tmp/fixture` replay points at an arbitrary dir,
/// and those runs must keep the pure-mtime first-sight gate (the probe is
/// additive-only; a replayed rollout vouched for by a coincidentally-running
/// codex would resurrect as live).
fn codex_probe_root(sessions_root: &Path) -> Option<PathBuf> {
    codex_probe_root_resolved(sessions_root, &codex_home())
}

/// The injectable core of [`codex_probe_root`] (mirrors
/// `platform::resolve_codex_home`'s testable split): `home` is the resolved
/// codex home for this environment.
fn codex_probe_root_resolved(sessions_root: &Path, home: &Path) -> Option<PathBuf> {
    if sessions_root.file_name().and_then(|n| n.to_str()) != Some("sessions") {
        return None;
    }
    let parent = sessions_root.parent();
    let parent_is_codex =
        parent.and_then(|p| p.file_name()).and_then(|n| n.to_str()) == Some(".codex");
    // A parent that IS the resolved codex home is first-party even when not
    // named `.codex` — the CODEX_HOME case (`codex_home()` honors the env
    // var the same way `default_paths` does, one resolution for both).
    let parent_is_resolved_home = parent.is_some_and(|p| p == home);
    if !parent_is_codex && !parent_is_resolved_home {
        return None;
    }
    // Not canonicalized here: the dir may not exist yet at wiring time
    // (codex never run); `live_codex_rollout_ids` canonicalizes per probe
    // call, which also picks up a root created after startup.
    Some(sessions_root.to_path_buf())
}

/// Source that watches the Codex session transcript directory.
pub struct CodexSource {
    /// The watched Codex `sessions` rollout root (`~/.codex/sessions`).
    pub sessions_root: PathBuf,
    /// The #246 child-end un-claim side-channel — Codex is consumer-only:
    /// its `SubagentStop` hooks ride the shared socket the `HookRouter`
    /// owns (whose tee is the producer), and THIS watcher releases the ended
    /// child's rollout claim so a multi-turn child's turn-N+1 append
    /// re-registers (the motivating #246 case). The runtime shares ONE
    /// handle across the router + the CC and Codex watchers; `None` disables
    /// it (bare test construction).
    pub child_end_unclaims: Option<ChildEndUnclaims>,
}

impl CodexSource {
    /// Construct pointed at the default Codex `sessions` rollout root.
    pub fn default_paths() -> Self {
        Self {
            sessions_root: codex_home().join("sessions"),
            child_end_unclaims: None,
        }
    }
}

impl Source for CodexSource {
    fn name(&self) -> &str {
        SOURCE_NAME
    }

    async fn run(self: Box<Self>, tx: TaggedSender) -> Result<()> {
        let mut watcher = JsonlWatcher::new(
            self.sessions_root.clone(),
            SOURCE_NAME.to_string(),
            decode_codex_line,
            codex_session_ended,
        )
        .with_id_deriver(codex_id_from_path)
        .with_label_deriver(codex_task_label)
        .with_session_reentry_detector(codex_session_reentry);
        if let Some(root) = codex_probe_root(&self.sessions_root) {
            watcher = watcher
                .with_liveness_probe(std::sync::Arc::new(move || live_codex_rollout_ids(&root)));
        }
        if let Some(unclaims) = &self.child_end_unclaims {
            watcher = watcher.with_child_end_unclaims(unclaims.clone());
        }
        watcher.run(tx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_reentry_accepts_only_structural_codex_turn_starts() {
        for event in ["task_started", "turn_started"] {
            let line = serde_json::json!({
                "type": "event_msg",
                "payload": { "type": event, "turn_id": "t" }
            });
            assert!(
                codex_session_reentry(line.to_string().as_bytes()),
                "{event}"
            );
        }

        let user_message = serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "user_message",
                "message": "the literal task_started is not a lifecycle marker"
            }
        });
        assert!(!codex_session_reentry(user_message.to_string().as_bytes()));
        assert!(!codex_session_reentry(b"not json"));
    }

    #[test]
    fn task_label_uses_the_latest_index_title_and_keeps_the_codex_prefix() {
        const TASK_ID: &str = "019fb5c7-4701-7862-bfa9-f3f026cd7f90";
        let rollout = PathBuf::from(format!(
            "/home/u/.codex/sessions/2026/07/31/rollout-2026-07-31T09-26-03-{TASK_ID}.jsonl"
        ));
        let index = concat!(
            r#"{"id":"019fb5c7-4701-7862-bfa9-f3f026cd7f90","thread_name":"舊名稱","updated_at":"2026-07-31T01:20:00Z"}"#,
            "\n",
            r#"{"id":"019fb5c7-4701-7862-bfa9-f3f026cd7f90","thread_name":"尋找 Agents Orchestration 視覺化 Repo","updated_at":"2026-07-31T01:26:41Z"}"#,
            "\n",
        );

        assert_eq!(
            codex_task_label_from_index(
                &rollout,
                SOURCE_NAME,
                Path::new("/work/new-chat-2"),
                index
            ),
            "cx·尋找 Agents O"
        );
    }

    #[test]
    fn task_label_falls_back_to_the_original_cwd_label_without_an_index_match() {
        let rollout = Path::new(
            "/home/u/.codex/sessions/2026/07/31/rollout-2026-07-31T09-26-03-019fb5c7-4701-7862-bfa9-f3f026cd7f90.jsonl",
        );
        let index = concat!(
            "not json\n",
            r#"{"id":"01900000-0000-0000-0000-000000000000","thread_name":"another task"}"#,
            "\n",
        );

        assert_eq!(
            codex_task_label_from_index(rollout, SOURCE_NAME, Path::new("/work/new-chat-2"), index),
            "cx·new-chat-2"
        );
    }

    #[test]
    fn codex_session_ended_is_always_false() {
        // Codex writes no end marker — the checker always defers to the
        // mtime window + stale-sweep.
        assert!(!codex_session_ended(b"anything"));
        assert!(!codex_session_ended(b""));
    }

    // ---- liveness probe (open-rollout FD binding) ----

    const UUID: &str = "019e7762-9ded-7e33-be41-946ecf105bf4";

    fn snap_of(root: &Path, paths: Vec<PathBuf>) -> ProbeSnapshot {
        ProbeSnapshot::from_open_fd_pairs(
            root,
            paths.into_iter().map(|p| (42, p)),
            is_rollout_filename,
            codex_id_from_path,
        )
    }

    #[test]
    fn rollout_under_root_yields_its_uuid_bound_to_its_pid() {
        let root = Path::new("/home/u/.codex/sessions");
        // Real layout nests YYYY/MM/DD below the root — starts_with must
        // admit the whole subtree, not only direct children.
        let nested = root.join(format!(
            "2026/06/10/rollout-2026-06-10T08-00-00-{UUID}.jsonl"
        ));
        let got = snap_of(root, vec![nested]);
        assert_eq!(
            got.ids().cloned().collect::<Vec<_>>(),
            vec![UUID.to_string()]
        );
        // #223: the snapshot binds each id to the OWNING pid (the exit-watch
        // half) — the (42, path) pair above must survive the join intact.
        assert_eq!(got.pid_of.get(UUID), Some(&42));
    }

    #[test]
    fn shared_rollout_binds_the_larger_pid_regardless_of_enumeration_order() {
        // Two live processes holding ONE rollout (a resume overlap, #252's
        // codex sibling): the binding must be the deterministic tiebreak
        // winner in BOTH presentation orders, never last-writer-wins.
        let root = Path::new("/home/u/.codex/sessions");
        let path = root.join(format!(
            "2026/06/10/rollout-2026-06-10T08-00-00-{UUID}.jsonl"
        ));
        for pids in [[100, 200], [200, 100]] {
            let got = ProbeSnapshot::from_open_fd_pairs(
                root,
                pids.into_iter().map(|p| (p, path.clone())),
                is_rollout_filename,
                codex_id_from_path,
            );
            assert_eq!(
                got.ids().cloned().collect::<Vec<_>>(),
                vec![UUID.to_string()]
            );
            assert_eq!(
                got.pid_of.get(UUID),
                Some(&200),
                "the larger pid must win in both enumeration orders"
            );
        }
    }

    #[test]
    fn rollout_outside_root_is_excluded() {
        let root = Path::new("/home/u/.codex/sessions");
        let outside = PathBuf::from(format!("/tmp/elsewhere/rollout-1-{UUID}.jsonl"));
        let got = snap_of(root, vec![outside]);
        assert!(got.is_empty());
        assert!(got.pid_of.is_empty());
    }

    #[test]
    fn non_rollout_files_under_root_are_excluded() {
        let root = Path::new("/home/u/.codex/sessions");
        let wrong_stem = root.join("2026/06/10/history.jsonl");
        let wrong_ext = root.join(format!("2026/06/10/rollout-1-{UUID}.log"));
        let no_ext = root.join("2026/06/10/rollout-noext");
        assert!(snap_of(root, vec![wrong_stem, wrong_ext, no_ext]).is_empty());
    }

    #[test]
    fn probe_root_requires_dot_codex_sessions_layout() {
        assert_eq!(
            codex_probe_root(Path::new("/home/u/.codex/sessions")),
            Some(PathBuf::from("/home/u/.codex/sessions"))
        );
        // A fixture replay root must get NO probe (pure-mtime behavior).
        assert_eq!(codex_probe_root(Path::new("/tmp/fixture")), None);
        // A bare relative `sessions` has no parent to check.
        assert_eq!(codex_probe_root(Path::new("sessions")), None);
    }

    #[test]
    fn probe_root_accepts_resolved_codex_home_sessions_layout() {
        // A CODEX_HOME-shaped layout: the resolved home is NOT named
        // `.codex`, but its `sessions` child is codex's first-party rollout
        // root for this environment — the probe must attach, or CODEX_HOME
        // users silently lose the entire liveness ladder (admission bypass,
        // ProofOfLife, negative vouch, instant exit). The env→home
        // resolution itself is pinned by `platform::resolve_codex_home`'s
        // unit tests; this pins the probe gate against the resolved value.
        let home = tempfile::tempdir().unwrap();
        let sessions = home.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        assert_eq!(
            codex_probe_root_resolved(&sessions, home.path()),
            Some(sessions.clone())
        );
        // Replay roots stay probe-less even with a custom home resolved.
        assert_eq!(
            codex_probe_root_resolved(Path::new("/tmp/fixture"), home.path()),
            None
        );
        // `sessions` under a parent that is neither `.codex` nor the
        // resolved home is not first-party.
        assert_eq!(
            codex_probe_root_resolved(Path::new("/srv/other/sessions"), home.path()),
            None
        );
    }

    #[test]
    fn live_ids_for_missing_root_is_some_empty_not_a_failure() {
        // canonicalize() fails on a nonexistent dir, but an ABSENT root is
        // not a probe failure — codex may simply never have run. Some(empty)
        // is the healthy "nothing alive" observation (#223: None would freeze
        // the negative-vouch ledger forever on machines without codex).
        let missing = Path::new("/definitely/not/a/real/.codex/sessions");
        let snap = live_codex_rollout_ids(missing).expect("absent root is not a probe failure");
        assert!(snap.is_empty());
        assert!(snap.pid_of.is_empty());
    }

    #[test]
    fn live_ids_for_unrelated_root_is_empty() {
        // Real FFI smoke: whatever processes exist, none hold a rollout open
        // under a fresh tempdir.
        let dir = tempfile::tempdir().unwrap();
        let snap = live_codex_rollout_ids(dir.path())
            .expect("a healthy system's enumeration must succeed");
        assert!(snap.is_empty());
    }
}
