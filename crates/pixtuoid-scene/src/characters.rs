//! Character-slot selection shared by the Maple scene painters.
//!
//! Sprite packs expose a dynamic paperdoll catalog while each Maple map still
//! renders at most eight actors. This module keeps catalog storage separate
//! from Agent identity: users may choose a smaller stable roster, while
//! presentation-only actors may override one appearance without mutating the
//! monitored [`pixtuoid_core::SceneState`].

use std::collections::HashMap;

use pixtuoid_core::{AgentId, GlobalDeskIndex};

/// Resolve a reproducible pseudo-random choice for one actor and one complete
/// behavior segment.  The segment boundary is supplied by the caller, so a
/// low frame rate can skip samples without changing the selected action and a
/// pose never changes halfway through its authored animation.
pub(crate) fn stable_motion_choice(agent_id: AgentId, segment: u64, salt: u64) -> u64 {
    let segment_key = pixtuoid_core::id::splitmix64(segment.wrapping_add(0x9e37_79b9_7f4a_7c15));
    pixtuoid_core::id::splitmix64(agent_id.raw() ^ segment_key ^ salt)
}

/// Legacy/default number of paperdoll identities when a pack has no catalog.
pub const CHARACTER_SLOT_COUNT: usize = 8;

/// Friendly labels for the eight resolved runtime slots.
///
/// A local skin workshop may replace the art behind a slot; the slot label
/// remains stable so saved selections survive those local asset changes.
pub const CHARACTER_NAMES_ZH_TW: [&str; CHARACTER_SLOT_COUNT] = [
    "素材狐",
    "動作貓",
    "介面星",
    "程式熊",
    "測試鳥",
    "文件兔",
    "安全鹿",
    "協作楓",
];

/// Resolve a pack-independent fallback name for one appearance index.
pub fn fallback_character_name_zh_tw(slot: usize) -> String {
    CHARACTER_NAMES_ZH_TW.get(slot).map_or_else(
        || format!("角色 {:02}", slot + 1),
        |name| (*name).to_owned(),
    )
}

/// Canonical non-empty subset of the loaded pack's dynamic character slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterRoster {
    slots: Vec<usize>,
    available_count: usize,
}

impl CharacterRoster {
    /// Normalize a user selection by filtering invalid values, sorting and
    /// deduplicating it.  An empty result falls back to all characters so a
    /// malformed preference can never make every real Agent disappear.
    pub fn new(slots: impl IntoIterator<Item = usize>) -> Self {
        Self::new_with_count(slots, CHARACTER_SLOT_COUNT)
    }

    /// Normalize a selection against the currently loaded pack catalog.
    /// A zero count falls back to the legacy eight procedural identities.
    pub fn new_with_count(slots: impl IntoIterator<Item = usize>, available_count: usize) -> Self {
        let available_count = if available_count == 0 {
            CHARACTER_SLOT_COUNT
        } else {
            available_count
        };
        let mut slots = slots
            .into_iter()
            .filter(|slot| *slot < available_count)
            .collect::<Vec<_>>();
        slots.sort_unstable();
        slots.dedup();
        if slots.is_empty() {
            slots.extend(0..available_count);
        }
        Self {
            slots,
            available_count,
        }
    }

    /// Canonical selected slot indices.
    pub fn slots(&self) -> &[usize] {
        &self.slots
    }

    /// Number of character identities offered by the loaded pack.
    pub const fn available_count(&self) -> usize {
        self.available_count
    }

    /// Whether one runtime character slot is selected for real Agents.
    pub fn contains(&self, slot: usize) -> bool {
        self.slots.binary_search(&slot).is_ok()
    }

    /// Toggle one valid slot while preserving the non-empty invariant.
    /// Removing the final selected character is intentionally a no-op.
    pub fn toggled(&self, slot: usize) -> Self {
        if slot >= self.available_count {
            return self.clone();
        }
        let mut slots = self.slots.clone();
        match slots.binary_search(&slot) {
            Ok(index) if slots.len() > 1 => {
                slots.remove(index);
            }
            Ok(_) => return self.clone(),
            Err(index) => slots.insert(index, slot),
        }
        Self {
            slots,
            available_count: self.available_count,
        }
    }

    /// Resolve a stable selected appearance from an Agent's immutable desk.
    pub fn appearance_for_desk(&self, desk: GlobalDeskIndex) -> usize {
        self.slots[desk.0 % self.slots.len()]
    }
}

impl Default for CharacterRoster {
    fn default() -> Self {
        Self::new(0..CHARACTER_SLOT_COUNT)
    }
}

/// Shared appearance authority for real and presentation-only actors.
#[derive(Debug, Clone, Default)]
pub struct CharacterAppearances {
    roster: CharacterRoster,
    overrides: HashMap<AgentId, usize>,
}

impl CharacterAppearances {
    /// Start with one normalized real-Agent roster and no actor overrides.
    pub fn new(roster: CharacterRoster) -> Self {
        Self {
            roster,
            overrides: HashMap::new(),
        }
    }

    /// The current real-Agent roster.
    pub fn roster(&self) -> &CharacterRoster {
        &self.roster
    }

    /// Replace the real-Agent roster without disturbing presentation-only
    /// actor overrides.
    pub fn set_roster(&mut self, roster: CharacterRoster) {
        self.roster = roster;
        self.overrides
            .retain(|_, slot| *slot < self.roster.available_count());
    }

    /// Assign one valid explicit appearance to an actor. Invalid slots clear
    /// any previous override so callers safely fall back to the roster.
    pub fn set_override(&mut self, id: AgentId, slot: usize) {
        if slot < self.roster.available_count() {
            self.overrides.insert(id, slot);
        } else {
            self.overrides.remove(&id);
        }
    }

    /// Clear one actor-specific appearance override.
    pub fn remove_override(&mut self, id: AgentId) {
        self.overrides.remove(&id);
    }

    /// Resolve one actor's appearance. Explicit showcase choices win; normal
    /// Agents remain stable through their immutable desk index.
    pub fn appearance_for(&self, id: AgentId, desk: GlobalDeskIndex) -> usize {
        self.overrides
            .get(&id)
            .copied()
            .unwrap_or_else(|| self.roster.appearance_for_desk(desk))
    }
}

#[cfg(test)]
mod tests {
    use pixtuoid_core::{AgentId, GlobalDeskIndex};

    use super::{
        stable_motion_choice, CharacterAppearances, CharacterRoster, CHARACTER_SLOT_COUNT,
    };

    #[test]
    fn motion_choices_are_repeatable_but_change_across_actors_and_complete_segments() {
        let fox = AgentId::from_parts("codex", "motion-fox");
        let cat = AgentId::from_parts("codex", "motion-cat");
        let fox_choices = (0..16)
            .map(|segment| stable_motion_choice(fox, segment, 0x4d41_504c_455f_4944))
            .collect::<Vec<_>>();

        assert_eq!(
            fox_choices,
            (0..16)
                .map(|segment| stable_motion_choice(fox, segment, 0x4d41_504c_455f_4944))
                .collect::<Vec<_>>(),
            "the same actor and completed segment must resolve identically"
        );
        assert!(
            fox_choices.windows(2).any(|pair| pair[0] != pair[1]),
            "successive complete segments need more than one action choice"
        );
        assert_ne!(
            fox_choices,
            (0..16)
                .map(|segment| stable_motion_choice(cat, segment, 0x4d41_504c_455f_4944))
                .collect::<Vec<_>>(),
            "different actors should not march through an identical schedule"
        );
    }

    #[test]
    fn roster_normalizes_duplicates_and_invalid_slots_canonically() {
        let roster = CharacterRoster::new([7, 2, 2, 99]);
        assert_eq!(roster.slots(), &[2, 7]);
        assert_eq!(roster.appearance_for_desk(GlobalDeskIndex(0)), 2);
        assert_eq!(roster.appearance_for_desk(GlobalDeskIndex(1)), 7);
        assert_eq!(roster.appearance_for_desk(GlobalDeskIndex(2)), 2);
    }

    #[test]
    fn an_empty_or_invalid_roster_falls_back_to_every_character() {
        let expected = (0..CHARACTER_SLOT_COUNT).collect::<Vec<_>>();
        assert_eq!(CharacterRoster::new([]).slots(), expected);
        assert_eq!(CharacterRoster::new([88, 99]).slots(), expected);
    }

    #[test]
    fn a_dynamic_catalog_can_select_and_override_all_twenty_eight_characters() {
        let roster = CharacterRoster::new_with_count([27, 8, 27, 99], 28);
        assert_eq!(roster.slots(), &[8, 27]);
        assert_eq!(roster.available_count(), 28);
        assert_eq!(roster.appearance_for_desk(GlobalDeskIndex(1)), 27);

        let id = AgentId::from_parts("showcase", "character-27");
        let mut appearances = CharacterAppearances::new(roster);
        appearances.set_override(id, 27);
        assert_eq!(appearances.appearance_for(id, GlobalDeskIndex(0)), 27);
    }

    #[test]
    fn explicit_actor_override_beats_the_agent_roster_without_changing_it() {
        let id = AgentId::from_parts("showcase", "slot-6");
        let mut appearances = CharacterAppearances::new(CharacterRoster::new([1, 3]));
        appearances.set_override(id, 6);
        assert_eq!(appearances.appearance_for(id, GlobalDeskIndex(0)), 6);
        assert_eq!(
            appearances.appearance_for(
                AgentId::from_parts("codex", "real-agent"),
                GlobalDeskIndex(0)
            ),
            1
        );
        assert_eq!(appearances.roster().slots(), &[1, 3]);
    }

    #[test]
    fn invalid_override_is_ignored() {
        let id = AgentId::from_parts("showcase", "bad-slot");
        let mut appearances = CharacterAppearances::default();
        appearances.set_override(id, CHARACTER_SLOT_COUNT + 1);
        assert_eq!(appearances.appearance_for(id, GlobalDeskIndex(3)), 3);
    }
}
