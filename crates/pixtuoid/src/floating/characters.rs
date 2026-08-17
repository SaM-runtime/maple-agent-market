//! Presentation-only character rosters for the two Maple maps.
//!
//! These actors deliberately live beside the renderer instead of inside the
//! monitored `SceneState`; a cloned scene is augmented only for the pixel and
//! label passes, so footer counts, tool tallies and Agent lineage remain true.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use pixtuoid_core::{ActivityState, AgentId, AgentSlot, GlobalDeskIndex, SceneState, ToolKind};
use pixtuoid_scene::characters::{CharacterAppearances, CHARACTER_SLOT_COUNT};

const SHOWCASE_SOURCE: &str = "showcase";
pub(crate) const SHOWCASE_LABEL: &str = "逛街中";
pub(crate) const TRAINING_SHOWCASE_LABEL: &str = "練功中";

/// Render-only destination selected by the user in the character panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShowcaseDestination {
    FreeMarket,
    ForestTraining,
}

impl ShowcaseDestination {
    const fn label(self) -> &'static str {
        match self {
            Self::FreeMarket => SHOWCASE_LABEL,
            Self::ForestTraining => TRAINING_SHOWCASE_LABEL,
        }
    }

    const fn identity(self) -> &'static str {
        match self {
            Self::FreeMarket => "market",
            Self::ForestTraining => "training",
        }
    }

    const fn max_agents(self) -> usize {
        match self {
            Self::FreeMarket => pixtuoid_scene::market::MARKET_MAX_AGENTS,
            Self::ForestTraining => pixtuoid_scene::training::TRAINING_MAX_AGENTS,
        }
    }

    const fn max_exit_ms(self) -> u64 {
        match self {
            Self::FreeMarket => pixtuoid_scene::market::MARKET_MAX_EXIT_MS as u64,
            Self::ForestTraining => pixtuoid_scene::training::TRAINING_MAX_EXIT_MS,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ShowcaseActor {
    slot: usize,
    created_at: SystemTime,
    leaving_at: Option<SystemTime>,
}

/// Result of one manual showcase-row action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShowcaseToggle {
    /// A new presentation-only actor began entering the Free Market.
    Entered,
    /// An existing actor began its full walk-out animation.
    Leaving,
    /// All eight market places are already owned by real or visible actors.
    Full,
    /// The requested sprite slot does not exist.
    Invalid,
}

/// Cross-frame render-only showcase state.
#[derive(Debug)]
pub(crate) struct ShowcaseRoster {
    actors: BTreeMap<usize, ShowcaseActor>,
    available_count: usize,
    destination: ShowcaseDestination,
}

impl Default for ShowcaseRoster {
    fn default() -> Self {
        Self {
            actors: BTreeMap::new(),
            available_count: CHARACTER_SLOT_COUNT,
            destination: ShowcaseDestination::FreeMarket,
        }
    }
}

impl ShowcaseRoster {
    /// Seed active showcase actors from persisted slot choices.
    #[cfg(test)]
    pub(crate) fn from_slots(slots: impl IntoIterator<Item = usize>, now: SystemTime) -> Self {
        Self::from_slots_with_count(slots, CHARACTER_SLOT_COUNT, now)
    }

    /// Seed actors against the dynamic paperdoll count of the active pack.
    #[cfg(test)]
    pub(crate) fn from_slots_with_count(
        slots: impl IntoIterator<Item = usize>,
        available_count: usize,
        now: SystemTime,
    ) -> Self {
        Self::from_slots_for_destination(
            slots,
            available_count,
            ShowcaseDestination::FreeMarket,
            now,
        )
    }

    /// Seed actors for one explicit map. Market and training rosters remain
    /// independent, including when the same paperdoll is visible on both.
    pub(crate) fn from_slots_for_destination(
        slots: impl IntoIterator<Item = usize>,
        available_count: usize,
        destination: ShowcaseDestination,
        now: SystemTime,
    ) -> Self {
        let available_count = available_count.max(1);
        let actors = slots
            .into_iter()
            .filter(|slot| *slot < available_count)
            .map(|slot| {
                (
                    slot,
                    ShowcaseActor {
                        slot,
                        created_at: now,
                        leaving_at: None,
                    },
                )
            })
            .take(destination.max_agents())
            .collect();
        Self {
            actors,
            available_count,
            destination,
        }
    }

    /// Enter an absent character or begin withdrawing a visible one. Real
    /// monitored Agents are counted first and can never be displaced.
    pub(crate) fn toggle(
        &mut self,
        slot: usize,
        real_market_agents: usize,
        now: SystemTime,
    ) -> ShowcaseToggle {
        if slot >= self.available_count {
            return ShowcaseToggle::Invalid;
        }
        if let Some(actor) = self.actors.get_mut(&slot) {
            if actor.leaving_at.is_none() {
                actor.leaving_at = Some(now);
            }
            return ShowcaseToggle::Leaving;
        }
        if real_market_agents.saturating_add(self.actors.len()) >= self.destination.max_agents() {
            return ShowcaseToggle::Full;
        }
        self.actors.insert(
            slot,
            ShowcaseActor {
                slot,
                created_at: now,
                leaving_at: None,
            },
        );
        ShowcaseToggle::Entered
    }

    /// Persistable actors. A withdrawing actor is excluded immediately so a
    /// restart cannot resurrect it, although it remains rendered until exit.
    pub(crate) fn selected_slots(&self) -> Vec<usize> {
        self.actors
            .values()
            .filter(|actor| actor.leaving_at.is_none())
            .map(|actor| actor.slot)
            .collect()
    }

    /// Whether any presentation actor is entering, strolling or leaving.
    pub(crate) fn has_visible_actors(&self) -> bool {
        !self.actors.is_empty()
    }

    /// Showcase actors use time-driven walk/breathe frames, so the floating
    /// window must retain the active (~30 FPS) cadence even with zero Agents.
    pub(crate) fn needs_active_animation(&self) -> bool {
        self.has_visible_actors()
    }

    /// Whether a character is currently entering/strolling (`true`), leaving
    /// (`false`), or absent (`None`).
    pub(crate) fn visible_state(&self, slot: usize) -> Option<bool> {
        self.actors
            .get(&slot)
            .map(|actor| actor.leaving_at.is_none())
    }

    /// Clone an already projected Free Market scene and insert render-only
    /// paperdolls. Completed exits are evicted here, at the same frame time the
    /// market painter receives.
    pub(crate) fn project_into_market(
        &mut self,
        scene: &SceneState,
        appearances: &mut CharacterAppearances,
        now: SystemTime,
    ) -> SceneState {
        debug_assert_eq!(self.destination, ShowcaseDestination::FreeMarket);
        self.project_into(scene, appearances, now)
    }

    /// Clone one map projection and insert only this destination's actors.
    pub(crate) fn project_into(
        &mut self,
        scene: &SceneState,
        appearances: &mut CharacterAppearances,
        now: SystemTime,
    ) -> SceneState {
        let prior_slots = self.actors.keys().copied().collect::<Vec<_>>();
        for slot in prior_slots {
            appearances.remove_override(showcase_agent_id(self.destination, slot));
        }
        let exit_ms = self.destination.max_exit_ms();
        self.actors.retain(|_, actor| {
            actor.leaving_at.is_none_or(|leaving_at| {
                now.duration_since(leaving_at)
                    .map_or(true, |elapsed| elapsed < Duration::from_millis(exit_ms))
            })
        });
        let mut projected = scene.clone();
        for actor in self.actors.values().copied() {
            let agent = showcase_agent(self.destination, actor);
            appearances.set_override(agent.agent_id, actor.slot);
            projected.agents.insert(agent.agent_id, agent);
        }
        projected
    }
}

fn showcase_agent_id(destination: ShowcaseDestination, slot: usize) -> AgentId {
    AgentId::from_parts(
        SHOWCASE_SOURCE,
        &format!("{}-character-{slot}", destination.identity()),
    )
}

fn showcase_agent(destination: ShowcaseDestination, actor: ShowcaseActor) -> AgentSlot {
    let agent_id = showcase_agent_id(destination, actor.slot);
    let state = match destination {
        ShowcaseDestination::FreeMarket => ActivityState::Idle,
        ShowcaseDestination::ForestTraining => ActivityState::Active {
            tool_use_id: Some(Arc::from(format!("showcase-training-{}", actor.slot))),
            detail: Some(Arc::from("練功展示")),
            kind: ToolKind::Bash,
        },
    };
    AgentSlot {
        agent_id,
        source: Arc::from(SHOWCASE_SOURCE),
        session_id: Arc::from(format!(
            "showcase-{}-character-{}",
            destination.identity(),
            actor.slot
        )),
        cwd: Arc::from(Path::new("<showcase>")),
        label: destination.label().into(),
        state,
        state_started_at: actor.created_at,
        last_event_at: actor.created_at,
        created_at: actor.created_at,
        exiting_at: actor.leaving_at,
        pending_idle_at: None,
        // A high index sorts every real Agent first; modulo eight still gives
        // each showcase paperdoll a stable preferred physical market slot.
        desk_index: GlobalDeskIndex(10_000 + actor.slot),
        floor_idx: 0,
        tool_call_count: 0,
        active_ms: 0,
        unknown_cwd: false,
        parent_id: None,
        pid: None,
        model: None,
        effort: None,
        tokens_used: 0,
        last_usage: None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use pixtuoid_core::{ActivityState, SceneState};
    use pixtuoid_scene::characters::CharacterAppearances;

    use super::{ShowcaseDestination, ShowcaseRoster, ShowcaseToggle};

    #[test]
    fn showcase_projection_never_mutates_the_authoritative_agent_scene_or_stats() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let scene = SceneState::uniform(8);
        let mut roster = ShowcaseRoster::from_slots([6], now);
        let mut appearances = CharacterAppearances::default();
        let projected = roster.project_into_market(&scene, &mut appearances, now);

        assert!(
            scene.agents.is_empty(),
            "authoritative stats stay agent-only"
        );
        assert_eq!(projected.agents.len(), 1);
        let authoritative_stats = pixtuoid_scene::board::scene_stats(&scene);
        assert_eq!(authoritative_stats.total, 0);
        let guest = projected.agents.values().next().unwrap();
        assert_eq!(&*guest.source, "showcase");
        assert_eq!(&*guest.label, "逛街中");
        assert_eq!(
            appearances.appearance_for(guest.agent_id, guest.desk_index),
            6
        );
    }

    #[test]
    fn real_agents_own_capacity_and_a_full_market_rejects_a_guest() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut roster = ShowcaseRoster::default();
        assert_eq!(roster.toggle(3, 8, now), ShowcaseToggle::Full);
        assert!(!roster.has_visible_actors());
        assert!(roster.selected_slots().is_empty());
    }

    #[test]
    fn dynamic_catalog_accepts_character_twenty_eight_but_still_caps_visible_guests_at_eight() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut roster = ShowcaseRoster::from_slots_with_count([], 28, now);
        assert_eq!(roster.toggle(27, 0, now), ShowcaseToggle::Entered);
        for slot in 0..7 {
            assert_eq!(roster.toggle(slot, 0, now), ShowcaseToggle::Entered);
        }
        assert_eq!(roster.toggle(8, 0, now), ShowcaseToggle::Full);
        assert_eq!(roster.selected_slots().len(), 8);
        assert!(roster.selected_slots().contains(&27));
    }

    #[test]
    fn withdrawing_keeps_the_actor_for_the_full_exit_animation_then_evicts_it() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut roster = ShowcaseRoster::default();
        assert_eq!(roster.toggle(2, 0, now), ShowcaseToggle::Entered);
        assert_eq!(roster.toggle(2, 0, now), ShowcaseToggle::Leaving);
        assert!(roster.has_visible_actors());
        assert!(roster.selected_slots().is_empty());

        let mut appearances = CharacterAppearances::default();
        let before = roster.project_into_market(
            &SceneState::uniform(8),
            &mut appearances,
            now + Duration::from_millis(u64::from(pixtuoid_scene::market::MARKET_MAX_EXIT_MS - 1)),
        );
        assert_eq!(before.agents.len(), 1);
        let after = roster.project_into_market(
            &SceneState::uniform(8),
            &mut appearances,
            now + Duration::from_millis(u64::from(pixtuoid_scene::market::MARKET_MAX_EXIT_MS)),
        );
        assert!(after.agents.is_empty());
        assert!(!roster.has_visible_actors());
    }

    #[test]
    fn training_showcase_is_active_render_only_and_uses_a_distinct_identity() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let scene = SceneState::uniform(8);
        let mut market = ShowcaseRoster::from_slots_for_destination(
            [6],
            28,
            ShowcaseDestination::FreeMarket,
            now,
        );
        let mut training = ShowcaseRoster::from_slots_for_destination(
            [6],
            28,
            ShowcaseDestination::ForestTraining,
            now,
        );
        let mut appearances = CharacterAppearances::default();
        let market_scene = market.project_into(&scene, &mut appearances, now);
        let training_scene = training.project_into(&scene, &mut appearances, now);

        assert!(
            scene.agents.is_empty(),
            "render-only actors never alter stats"
        );
        let market_guest = market_scene.agents.values().next().unwrap();
        let training_guest = training_scene.agents.values().next().unwrap();
        assert_ne!(market_guest.agent_id, training_guest.agent_id);
        assert_eq!(&*market_guest.label, "逛街中");
        assert!(matches!(market_guest.state, ActivityState::Idle));
        assert_eq!(&*training_guest.label, "練功中");
        assert!(matches!(training_guest.state, ActivityState::Active { .. }));
        assert_eq!(
            appearances.appearance_for(training_guest.agent_id, training_guest.desk_index),
            6
        );
    }

    #[test]
    fn training_withdrawal_survives_the_long_rope_exit_then_evicts() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut roster = ShowcaseRoster::from_slots_for_destination(
            [2],
            28,
            ShowcaseDestination::ForestTraining,
            now,
        );
        assert_eq!(roster.toggle(2, 0, now), ShowcaseToggle::Leaving);

        let mut appearances = CharacterAppearances::default();
        let before = roster.project_into(
            &SceneState::uniform(8),
            &mut appearances,
            now + Duration::from_millis(pixtuoid_scene::training::TRAINING_MAX_EXIT_MS - 1),
        );
        assert_eq!(before.agents.len(), 1);
        let after = roster.project_into(
            &SceneState::uniform(8),
            &mut appearances,
            now + Duration::from_millis(pixtuoid_scene::training::TRAINING_MAX_EXIT_MS),
        );
        assert!(after.agents.is_empty());
    }
}
