//! Live source/reducer wiring for the Maple floating window.

pub(crate) mod driver;
pub(crate) mod gate;
pub(crate) mod pipeline;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pixtuoid_core::source::manager::SourceDeath;

/// Conservative agent capacity used only when a window is too small to measure.
pub(crate) const FALLBACK_AGENT_CAPACITY: usize = 16;

/// Startup inputs required by the floating Maple Agent Market runtime.
pub struct RunConfig {
    pub socket: Option<PathBuf>,
    pub projects_root: Option<PathBuf>,
    pub codex_sessions_root: Option<PathBuf>,
    /// Optional collaborator-provided local pack. The built-in UI needs no pack.
    pub pack_dir: Option<PathBuf>,
    pub config_path: PathBuf,
    pub theme: &'static pixtuoid_scene::theme::Theme,
    pub connected: HashSet<String>,
    pub audio: crate::config::AudioConfig,
}

/// Thread-safe mirror of the source ids enabled in local configuration.
#[derive(Clone, Default)]
pub struct ConnectedSources(Arc<Mutex<HashSet<String>>>);

impl ConnectedSources {
    pub fn new(initial: HashSet<String>) -> Self {
        Self(Arc::new(Mutex::new(initial)))
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_connected(&self, source_id: &str) -> bool {
        self.guard().contains(source_id)
    }

    pub fn snapshot(&self) -> HashSet<String> {
        self.guard().clone()
    }
}

/// Return the not-yet-surfaced tail of the grow-only source-death list.
pub(crate) fn unseen_deaths<'a>(deaths: &'a [SourceDeath], seen: &mut usize) -> &'a [SourceDeath] {
    let start = (*seen).min(deaths.len());
    *seen = deaths.len();
    &deaths[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_sources_snapshot_matches_the_gate() {
        let connected = ConnectedSources::new(HashSet::from(["codex".to_string()]));
        assert!(connected.is_connected("codex"));
        assert!(!connected.is_connected("claude-code"));
        assert_eq!(connected.snapshot(), HashSet::from(["codex".to_string()]));
    }

    #[test]
    fn unseen_deaths_yields_each_death_once() {
        let mut seen = 0;
        let first = vec![SourceDeath::new("codex", "boom")];
        assert_eq!(unseen_deaths(&first, &mut seen).len(), 1);

        let second = vec![
            SourceDeath::new("codex", "boom"),
            SourceDeath::new("claude-code", "bind"),
        ];
        let fresh = unseen_deaths(&second, &mut seen);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].source, "claude-code");
        assert!(unseen_deaths(&second, &mut seen).is_empty());
    }
}
