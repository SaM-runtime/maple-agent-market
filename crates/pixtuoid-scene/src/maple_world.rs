//! Presentation-only routing for the two-map Maple floating world.
//!
//! Core's desk/floor assignment is immutable session state.  This module owns
//! a separate, sticky map assignment keyed by the root task so children travel
//! with their parent without teaching the reducer about one visual theme.

use std::collections::{BTreeMap, BTreeSet};

use pixtuoid_core::{id::splitmix64, AgentId, SceneState};

pub use crate::overlay::AgentRelation as MapleAgentRelation;

/// One implemented map in the Maple floating world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapleMapId {
    /// The existing merchant-stall Free Market scene.
    FreeMarket,
    /// The bright forest training field (the first combat-map implementation).
    ForestTraining,
}

impl MapleMapId {
    /// Both implemented maps, in the order used by the compact map switcher.
    pub const ALL: [Self; 2] = [Self::FreeMarket, Self::ForestTraining];

    /// The next implemented map, wrapping at the end.
    pub const fn next(self) -> Self {
        match self {
            Self::FreeMarket => Self::ForestTraining,
            Self::ForestTraining => Self::FreeMarket,
        }
    }

    /// Compact Traditional-Chinese title used by the floating map selector.
    pub const fn title_zh_tw(self) -> &'static str {
        match self {
            Self::FreeMarket => "自由市場 1-1",
            Self::ForestTraining => "弓箭手村訓練場 I",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PartyLineage {
    root_id: AgentId,
    depth: usize,
    cycle: bool,
}

/// Resolve the relationship badge for one live Maple actor.
///
/// Independent roots return `None`; malformed cycles also fail quiet.  A
/// dangling parent remains a child with an unknown root label, matching core's
/// deliberate post-GC `parent_id` preservation without inventing a task name.
pub fn agent_relation(scene: &SceneState, id: AgentId) -> Option<MapleAgentRelation> {
    scene.agents.get(&id)?;
    let lineage = resolve_party_lineage(scene, id);
    if lineage.cycle {
        return None;
    }
    if lineage.depth == 0 {
        let descendant_count = scene
            .agents
            .keys()
            .copied()
            .filter(|candidate| *candidate != id)
            .filter(|candidate| {
                let candidate_lineage = resolve_party_lineage(scene, *candidate);
                !candidate_lineage.cycle && candidate_lineage.root_id == id
            })
            .count();
        return (descendant_count > 0).then_some(MapleAgentRelation::Root {
            root_id: id,
            descendant_count,
        });
    }

    let root_task = scene
        .agents
        .get(&lineage.root_id)
        .map(|root| task_title(&root.label).to_owned());
    Some(MapleAgentRelation::Child {
        root_id: lineage.root_id,
        root_task,
        depth: lineage.depth,
    })
}

fn task_title(label: &str) -> &str {
    label.split_once('\u{b7}').map_or(
        label,
        |(_, task)| if task.is_empty() { label } else { task },
    )
}

fn resolve_party_lineage(scene: &SceneState, start: AgentId) -> PartyLineage {
    let mut current = start;
    let mut depth = 0usize;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(current) {
            return PartyLineage {
                root_id: visited.iter().copied().min().unwrap_or(start),
                depth,
                cycle: true,
            };
        }
        let Some(slot) = scene.agents.get(&current) else {
            return PartyLineage {
                root_id: current,
                depth,
                cycle: false,
            };
        };
        let Some(parent) = slot.parent_id else {
            return PartyLineage {
                root_id: current,
                depth,
                cycle: false,
            };
        };
        depth = depth.saturating_add(1);
        if !scene.agents.contains_key(&parent) {
            return PartyLineage {
                root_id: parent,
                depth,
                cycle: false,
            };
        }
        current = parent;
    }
}

/// Sticky root-party assignments plus the map currently shown by the camera.
#[derive(Debug, Clone)]
pub struct MapleWorldSession {
    current_map: MapleMapId,
    root_maps: BTreeMap<AgentId, MapleMapId>,
    agent_roots: BTreeMap<AgentId, AgentId>,
    market_agents: BTreeSet<AgentId>,
    training_agents: BTreeSet<AgentId>,
}

impl Default for MapleWorldSession {
    fn default() -> Self {
        Self {
            current_map: MapleMapId::FreeMarket,
            root_maps: BTreeMap::new(),
            agent_roots: BTreeMap::new(),
            market_agents: BTreeSet::new(),
            training_agents: BTreeSet::new(),
        }
    }
}

impl MapleWorldSession {
    /// Reconcile the live tree without changing any still-live party's map.
    ///
    /// New roots get a deterministic default, descendants inherit their live
    /// root, and ended roots are forgotten.  A malformed parent cycle safely
    /// collapses to the smallest id in that cycle instead of looping.
    pub fn reconcile(&mut self, scene: &SceneState) {
        let agent_roots = scene
            .agents
            .keys()
            .copied()
            .map(|id| (id, party_root(scene, id)))
            .collect::<BTreeMap<_, _>>();
        let live_roots = agent_roots.values().copied().collect::<BTreeSet<_>>();

        self.root_maps.retain(|root, _| live_roots.contains(root));
        for root in live_roots {
            self.root_maps
                .entry(root)
                .or_insert_with(|| default_map_for(root));
        }
        self.agent_roots = agent_roots;
        self.rebuild_memberships();
    }

    /// The map currently shown by the floating camera.
    pub const fn current_map(&self) -> MapleMapId {
        self.current_map
    }

    /// Select one implemented map without changing any party assignment.
    pub fn set_current_map(&mut self, map: MapleMapId) {
        self.current_map = map;
    }

    /// Show the next implemented map without changing any party assignment.
    pub fn cycle_map(&mut self) {
        self.current_map = self.current_map.next();
    }

    /// The assigned map for one live agent, if it was present at reconciliation.
    pub fn map_for(&self, agent: AgentId) -> Option<MapleMapId> {
        let root = self.agent_roots.get(&agent)?;
        self.root_maps.get(root).copied()
    }

    /// All live agents assigned to `map` in deterministic id order.
    pub fn agents_on(&self, map: MapleMapId) -> &BTreeSet<AgentId> {
        match map {
            MapleMapId::FreeMarket => &self.market_agents,
            MapleMapId::ForestTraining => &self.training_agents,
        }
    }

    /// Clone the map-local agent view while preserving office-global daemons.
    ///
    /// A floating painter gives each map its own `FloorSession`, so this
    /// projection is that session's complete live domain rather than a
    /// per-frame filter that could evict the other map's animation state.
    pub fn project_scene(&self, scene: &SceneState, map: MapleMapId) -> SceneState {
        let mut projected = SceneState::new(scene.floor_capacities);
        for id in self.agents_on(map) {
            if let Some(agent) = scene.agents.get(id) {
                projected.agents.insert(*id, agent.clone());
                projected.clone_turn_completion_from(scene, *id);
            }
        }
        projected.clone_daemons_from(scene);
        projected
    }

    /// Move a whole live party to another map.  This is the explicit seam for
    /// a later UI reassignment; ordinary status updates never call it.
    pub fn assign_party(&mut self, member: AgentId, map: MapleMapId) -> bool {
        let Some(root) = self.agent_roots.get(&member).copied() else {
            return false;
        };
        self.root_maps.insert(root, map);
        self.rebuild_memberships();
        true
    }

    fn rebuild_memberships(&mut self) {
        self.market_agents.clear();
        self.training_agents.clear();
        for (&agent, root) in &self.agent_roots {
            let Some(map) = self.root_maps.get(root) else {
                continue;
            };
            match map {
                MapleMapId::FreeMarket => {
                    self.market_agents.insert(agent);
                }
                MapleMapId::ForestTraining => {
                    self.training_agents.insert(agent);
                }
            }
        }
    }
}

fn default_map_for(root: AgentId) -> MapleMapId {
    const MAP_ASSIGNMENT_TAG: u64 = 0x6d61_706c_655f_6d61;
    if splitmix64(root.raw() ^ MAP_ASSIGNMENT_TAG) & 1 == 0 {
        MapleMapId::FreeMarket
    } else {
        MapleMapId::ForestTraining
    }
}

fn party_root(scene: &SceneState, start: AgentId) -> AgentId {
    // Core intentionally preserves dangling parent ids after a parent is GC'd.
    // `resolve_party_lineage` retains that id as the root key so surviving
    // children do not teleport to a freshly hashed map mid-exit/waiting state.
    resolve_party_lineage(scene, start).root_id
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use pixtuoid_core::{ActivityState, AgentId, AgentSlot, GlobalDeskIndex, SceneState};

    use super::{agent_relation, MapleAgentRelation, MapleMapId, MapleWorldSession};

    fn agent(id: AgentId, desk: usize, parent_id: Option<AgentId>) -> AgentSlot {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        AgentSlot {
            agent_id: id,
            source: Arc::from("codex"),
            session_id: Arc::from(format!("session-{desk}")),
            cwd: Arc::from(Path::new("/maple-world")),
            label: format!("agent-{desk}").into(),
            state: ActivityState::Idle,
            state_started_at: now,
            last_event_at: now,
            created_at: now,
            exiting_at: None,
            pending_idle_at: None,
            desk_index: GlobalDeskIndex(desk),
            floor_idx: 0,
            tool_call_count: 0,
            active_ms: 0,
            unknown_cwd: false,
            parent_id,
            pid: None,
            model: None,
            effort: None,
            tokens_used: 0,
            last_usage: None,
        }
    }

    #[test]
    fn party_assignment_is_sticky_and_each_agent_projects_to_exactly_one_map() {
        let root_a = AgentId::from_parts("codex", "root-a");
        let child_a = AgentId::from_parts("codex", "child-a");
        let root_b = AgentId::from_parts("codex", "root-b");
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(root_a, agent(root_a, 0, None));
        scene
            .agents
            .insert(child_a, agent(child_a, 1, Some(root_a)));
        scene.agents.insert(root_b, agent(root_b, 2, None));

        let mut world = MapleWorldSession::default();
        world.reconcile(&scene);

        assert_eq!(world.map_for(root_a), world.map_for(child_a));
        let market = world.agents_on(MapleMapId::FreeMarket);
        let training = world.agents_on(MapleMapId::ForestTraining);
        assert!(market.is_disjoint(training));
        assert_eq!(market.len() + training.len(), scene.agents.len());
        assert!(scene
            .agents
            .keys()
            .all(|id| market.contains(id) || training.contains(id)));
        let market_scene = world.project_scene(&scene, MapleMapId::FreeMarket);
        let training_scene = world.project_scene(&scene, MapleMapId::ForestTraining);
        assert_eq!(market_scene.agents.len(), market.len());
        assert_eq!(training_scene.agents.len(), training.len());

        let before = scene
            .agents
            .keys()
            .map(|id| (*id, world.map_for(*id)))
            .collect::<Vec<_>>();
        world.reconcile(&scene);
        let after = scene
            .agents
            .keys()
            .map(|id| (*id, world.map_for(*id)))
            .collect::<Vec<_>>();
        assert_eq!(before, after, "a refresh must not reshuffle live parties");
    }

    #[test]
    fn projected_map_keeps_the_live_turn_completion_edge() {
        use pixtuoid_core::source::{AgentEvent, Transport};
        use pixtuoid_core::Reducer;

        let id = AgentId::from_parts("codex", "completed-root");
        let completed_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let mut scene = SceneState::uniform(8);
        let mut slot = agent(id, 0, None);
        slot.state = ActivityState::Active {
            tool_use_id: None,
            detail: None,
            kind: pixtuoid_core::state::ToolKind::Other,
        };
        scene.agents.insert(id, slot);
        Reducer::new().apply(
            &mut scene,
            AgentEvent::TurnComplete { agent_id: id },
            completed_at,
            Transport::Jsonl,
        );

        let mut world = MapleWorldSession::default();
        world.reconcile(&scene);
        let map = world.map_for(id).expect("completed agent has a map");
        let projected = world.project_scene(&scene, map);
        assert_eq!(projected.last_turn_completed_at(id), Some(completed_at));
    }

    #[test]
    fn lineage_names_the_root_task_and_nested_depth_without_guessing_from_ids() {
        let root = AgentId::from_parts("codex", "root-task");
        let child = AgentId::from_parts("codex", "child-task");
        let grandchild = AgentId::from_parts("codex", "grandchild-task");
        let mut scene = SceneState::uniform(8);
        let mut root_slot = agent(root, 0, None);
        root_slot.label = "cx\u{b7}盤點新廠設備".into();
        scene.agents.insert(root, root_slot);
        scene.agents.insert(child, agent(child, 1, Some(root)));
        scene
            .agents
            .insert(grandchild, agent(grandchild, 2, Some(child)));

        assert_eq!(
            agent_relation(&scene, root),
            Some(MapleAgentRelation::Root {
                root_id: root,
                descendant_count: 2,
            })
        );
        assert_eq!(
            agent_relation(&scene, child),
            Some(MapleAgentRelation::Child {
                root_id: root,
                root_task: Some("盤點新廠設備".to_owned()),
                depth: 1,
            })
        );
        assert_eq!(
            agent_relation(&scene, grandchild),
            Some(MapleAgentRelation::Child {
                root_id: root,
                root_task: Some("盤點新廠設備".to_owned()),
                depth: 2,
            })
        );
    }

    #[test]
    fn independent_roots_stay_unbadged_and_dangling_children_fail_quiet() {
        let independent = AgentId::from_parts("codex", "independent");
        let missing_root = AgentId::from_parts("codex", "missing-root");
        let orphan = AgentId::from_parts("codex", "orphan");
        let mut scene = SceneState::uniform(8);
        scene
            .agents
            .insert(independent, agent(independent, 0, None));
        scene
            .agents
            .insert(orphan, agent(orphan, 1, Some(missing_root)));

        assert_eq!(agent_relation(&scene, independent), None);
        assert_eq!(
            agent_relation(&scene, orphan),
            Some(MapleAgentRelation::Child {
                root_id: missing_root,
                root_task: None,
                depth: 1,
            })
        );
    }

    #[test]
    fn changing_the_view_never_reassigns_a_party() {
        let root = AgentId::from_parts("codex", "root");
        let child = AgentId::from_parts("codex", "child");
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(root, agent(root, 0, None));
        scene.agents.insert(child, agent(child, 1, Some(root)));

        let mut world = MapleWorldSession::default();
        world.reconcile(&scene);
        let assigned = world.map_for(root);

        assert_eq!(world.current_map(), MapleMapId::FreeMarket);
        world.cycle_map();
        assert_eq!(world.current_map(), MapleMapId::ForestTraining);
        assert_eq!(world.map_for(root), assigned);
        assert_eq!(world.map_for(child), assigned);
    }

    #[test]
    fn map_selector_names_the_exact_classic_training_field() {
        assert_eq!(MapleMapId::FreeMarket.title_zh_tw(), "自由市場 1-1");
        assert_eq!(MapleMapId::ForestTraining.title_zh_tw(), "弓箭手村訓練場 I");
    }

    #[test]
    fn parent_cycles_degrade_to_one_stable_party_instead_of_looping() {
        let a = AgentId::from_parts("codex", "cycle-a");
        let b = AgentId::from_parts("codex", "cycle-b");
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(a, agent(a, 0, Some(b)));
        scene.agents.insert(b, agent(b, 1, Some(a)));

        let mut world = MapleWorldSession::default();
        world.reconcile(&scene);

        assert_eq!(world.map_for(a), world.map_for(b));
    }

    #[test]
    fn dangling_child_keeps_its_party_map_after_the_parent_is_collected() {
        let root = AgentId::from_parts("codex", "gone-root");
        let child = AgentId::from_parts("codex", "surviving-child");
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(root, agent(root, 0, None));
        scene.agents.insert(child, agent(child, 1, Some(root)));

        let mut world = MapleWorldSession::default();
        world.reconcile(&scene);
        let before = world.map_for(child);
        scene.agents.remove(&root);
        world.reconcile(&scene);

        assert_eq!(world.map_for(child), before);
    }
}
