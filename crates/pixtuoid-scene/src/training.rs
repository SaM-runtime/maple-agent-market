//! Pure geometry and elapsed-time motion for the forest training map.
//!
//! The pixel painter and floating label layer both resolve these frames, so a
//! jumping/walking character cannot leave its task card behind.  Motion is
//! stateless and elapsed-time based: a slow frame changes sampling, not speed.

use std::collections::HashMap;
use std::time::SystemTime;

use pixtuoid_core::state::ActivityState;
use pixtuoid_core::{AgentId, AgentSlot, SceneState};

use crate::anim::elapsed_ms;
use crate::layout::{Bounds, Point, CHARACTER_SPRITE_W};
use crate::market::{market_sprite_scale, MARKET_AVATAR_HEIGHT, MARKET_AVATAR_WIDTH};
use crate::overlay::{disambig_suffix, truncate_label, LabelElement, LabelTone};

/// Maximum simultaneously visible adventurers on the training map.
pub const TRAINING_MAX_AGENTS: usize = 8;
/// One active training loop: approach, attack, recover, return and breathe.
pub const TRAINING_WORK_CYCLE_MS: u64 = 6_400;
/// Classic unboosted walking speed on the 720px reference plate.
pub const TRAINING_REFERENCE_WALK_PX_PER_SEC: u64 = 125;
/// Portal materialization pause before the entering walk begins.
pub const TRAINING_PORTAL_IN_MS: u64 = 220;
/// Portal dissolve pause after the exiting walk reaches the portal.
pub const TRAINING_PORTAL_OUT_MS: u64 = 180;

const REFERENCE_WIDTH: u64 = 720;
const REFERENCE_HEIGHT: u64 = 480;
const TRAINING_REFERENCE_CLIMB_PX_PER_SEC: u64 = 100;
const ACTIVE_RUN_END_MS: u64 = 900;
const ACTIVE_ATTACK_START_MS: u64 = ACTIVE_RUN_END_MS;
const ATTACK_FRAME_MS: u64 = 140;
const ATTACK_FRAMES: usize = 3;
const TRAINING_SKILL_FRAME_MS: u64 = 240;
const TRAINING_SKILL_FRAMES: usize = 4;
const ACTIVE_ATTACK_HIT_MS: u64 = ACTIVE_ATTACK_START_MS + ATTACK_FRAME_MS * 2;
const ACTIVE_ATTACK_END_MS: u64 =
    ACTIVE_ATTACK_START_MS + TRAINING_SKILL_FRAME_MS * TRAINING_SKILL_FRAMES as u64;
const CLASSIC_DEATH_TOTAL_MS: u64 = 840;
const ACTIVE_DEATH_END_MS: u64 = ACTIVE_ATTACK_HIT_MS + CLASSIC_DEATH_TOTAL_MS;
const ACTIVE_RECOVERY_END_MS: u64 = 2_400;
const ACTIVE_RETURN_END_MS: u64 = 3_300;
const ACTIVE_RESPAWN_MS: u64 = 5_600;
const WALK_FRAME_MS: u64 = 180;
const STAND_FRAME_MS: u64 = 500;
const MONSTER_PATROL_LEG_MS: u64 = 2_400;
const MONSTER_PATROL_CENTER_REF_PX: i32 = 96;
const MONSTER_PATROL_RADIUS_REF_PX: i32 = 36;
const SLIME_MOVE_DELAYS_MS: [u64; 7] = [200, 50, 150, 150, 100, 100, 300];
const GREEN_MUSHROOM_MOVE_DELAYS_MS: [u64; 4] = [150, 150, 150, 150];
const CLASSIC_DEATH_DELAYS_MS: [u64; 4] = [180, 180, 180, 300];
const IDLE_CYCLE_MS: u64 = 9_000;
const IDLE_ROAM_REF_PX: i32 = 52;

#[derive(Debug, Clone, Copy)]
struct RelativeTrainingSlot {
    x_per_mille: u16,
    y_per_mille: u16,
}

const RELATIVE_SLOTS: [RelativeTrainingSlot; TRAINING_MAX_AGENTS] = [
    RelativeTrainingSlot {
        x_per_mille: 500,
        y_per_mille: 858,
    },
    RelativeTrainingSlot {
        x_per_mille: 500,
        y_per_mille: 558,
    },
    RelativeTrainingSlot {
        x_per_mille: 500,
        y_per_mille: 258,
    },
    RelativeTrainingSlot {
        x_per_mille: 139,
        y_per_mille: 858,
    },
    RelativeTrainingSlot {
        x_per_mille: 722,
        y_per_mille: 858,
    },
    RelativeTrainingSlot {
        x_per_mille: 139,
        y_per_mille: 558,
    },
    RelativeTrainingSlot {
        x_per_mille: 722,
        y_per_mille: 558,
    },
    RelativeTrainingSlot {
        x_per_mille: 722,
        y_per_mille: 258,
    },
];

/// One stable training lane resolved into the current pixel buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainingSlot {
    /// Slot index in the authored central-first order.
    pub index: usize,
    /// Character foot position on its platform.
    pub foot_px: Point,
}

/// Stable assignment of one agent to a training slot and paperdoll skin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainingPlacement {
    /// Agent shown in this slot.
    pub agent_id: AgentId,
    /// Resolved platform lane.
    pub slot: TrainingSlot,
    /// Paperdoll/monster appearance index.
    pub appearance_index: usize,
}

/// Clock and viewport shared by actor, monster and label resolution.
#[derive(Debug, Clone, Copy)]
pub struct TrainingFrameContext {
    /// Whole training-map pixel viewport.
    pub viewport: Bounds,
    /// Sample time for every elapsed-time animation.
    pub now: SystemTime,
}

/// Horizontal direction for walk frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingFacing {
    /// Faces toward decreasing x.
    Left,
    /// Faces toward increasing x.
    Right,
}

/// Paperdoll action selected for one resolved frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrainingActorPose {
    /// Calm breathing/ready pose.
    Stand { frame_index: usize },
    /// Four-frame walk cycle.
    Walk {
        frame_index: usize,
        facing: TrainingFacing,
    },
    /// Two-frame ladder/rope cycle; vertical route segments never use walk.
    Climb { frame_index: usize },
    /// Three-frame source-authored attack; public packs use the alert fallback.
    Attack { frame_index: usize },
    /// Waiting pose using the existing seated action.
    Sit,
}

/// Identity of a source-authored skill overlay. Keeping the kind beside its
/// frame index prevents a future multi-skill pack from selecting an unrelated
/// animation or replacing the paperdoll body from a bare number alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingSkillKind {
    /// Compact twin arc. A private pack may replace the public fallback.
    MagicClaw,
    /// Original gold-white falling-light burst for healer-class silhouettes.
    HolyLight,
    /// Original crimson-violet expanding pulse for warrior silhouettes.
    DragonPulse,
}

/// One independently painted skill effect for the current actor frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainingSkillEffect {
    pub kind: TrainingSkillKind,
    pub frame_index: usize,
}

/// Exact paperdoll geometry resolved for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainingActorFrame {
    /// Top-left of the fixed 32x24 logical paperdoll canvas.
    pub sprite_anchor_px: Point,
    /// Existing floating-label anchor convention: character centre minus 4px.
    pub label_anchor_px: Point,
    /// Ground-contact point, independent of transparent sprite bounds.
    pub foot_px: Point,
    /// Action/frame choice for this instant.
    pub pose: TrainingActorPose,
    /// Optional source-authored skill overlay; it never replaces the body pose.
    pub skill_effect: Option<TrainingSkillEffect>,
    /// Waiting agents keep a question bubble attached to their fixed head pivot.
    pub question_bubble: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrainingRouteKind {
    Walk,
    Climb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrainingRouteSegment {
    from: Point,
    to: Point,
    duration_ms: u64,
    kind: TrainingRouteKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrainingRouteSample {
    foot_px: Point,
    pose: TrainingActorPose,
}

/// MapleStory Classic enemy family used for one lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingMonsterKind {
    /// Green Slime (`210100`) from Henesys Hunting Ground I.
    Slime,
    /// Green Mushroom (`1110100`) from Henesys Hunting Ground I.
    GreenMushroom,
}

/// Enemy lifecycle synchronized to one active work loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingMonsterPose {
    /// Normal idle/patrol frame.
    Alive { frame_index: usize },
    /// Four-frame game-authored creature death cycle.
    Dying { frame_index: usize },
    /// Defeated interval before the next respawn.
    Hidden,
    /// Normal respawn interval; uses the game-authored living frames.
    Respawning { frame_index: usize },
}

/// Exact enemy geometry resolved for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainingMonsterFrame {
    /// Enemy family chosen by stable slot parity.
    pub kind: TrainingMonsterKind,
    /// Centre-foot anchor on the same platform as the assigned agent.
    pub foot_px: Point,
    /// Horizontal travel direction; source monster pixels face left by default.
    pub facing: TrainingFacing,
    /// Current living/death/respawn state.
    pub pose: TrainingMonsterPose,
}

/// Resolve up to eight authored training slots in the current viewport.
pub fn training_slots(viewport: Bounds, count: usize) -> Vec<TrainingSlot> {
    if viewport.width == 0 || viewport.height == 0 {
        return Vec::new();
    }
    RELATIVE_SLOTS
        .iter()
        .take(count.min(TRAINING_MAX_AGENTS))
        .enumerate()
        .map(|(index, slot)| TrainingSlot {
            index,
            foot_px: point_at(viewport, slot.x_per_mille, slot.y_per_mille),
        })
        .collect()
}

/// Assign agents by immutable desk index so peer updates never reshuffle lanes.
pub fn build_training_placements(scene: &SceneState, viewport: Bounds) -> Vec<TrainingPlacement> {
    let mut agents = scene.agents.values().collect::<Vec<_>>();
    agents.sort_by_key(|agent| (agent.desk_index, agent.agent_id));
    let slots = training_slots(viewport, TRAINING_MAX_AGENTS);
    if slots.len() != TRAINING_MAX_AGENTS {
        return Vec::new();
    }
    let mut used = [false; TRAINING_MAX_AGENTS];
    agents
        .into_iter()
        .take(TRAINING_MAX_AGENTS)
        .filter_map(|agent| {
            let preferred = agent.desk_index.0 % TRAINING_MAX_AGENTS;
            let slot_index = (0..TRAINING_MAX_AGENTS)
                .map(|offset| (preferred + offset) % TRAINING_MAX_AGENTS)
                .find(|index| !used[*index])?;
            used[slot_index] = true;
            Some(TrainingPlacement {
                agent_id: agent.agent_id,
                slot: slots[slot_index],
                appearance_index: agent.desk_index.0 % TRAINING_MAX_AGENTS,
            })
        })
        .collect()
}

/// Resolve entry, work, waiting, idle-roam or exit motion for one paperdoll.
pub fn resolve_training_actor(
    agent: &AgentSlot,
    placement: TrainingPlacement,
    frame: TrainingFrameContext,
) -> Option<TrainingActorFrame> {
    if let Some(exiting_at) = agent.exiting_at {
        return resolve_exit(agent, placement, frame, exiting_at);
    }
    resolve_non_exit(agent, placement, frame)
}

/// Resolve the lane enemy synchronized to the assigned agent's work cadence.
pub fn resolve_training_monster(
    agent: &AgentSlot,
    placement: TrainingPlacement,
    frame: TrainingFrameContext,
) -> Option<TrainingMonsterFrame> {
    resolve_training_actor(agent, placement, frame)?;
    let animation_elapsed =
        elapsed_ms(frame.now, SystemTime::UNIX_EPOCH) + placement.slot.index as u64 * 300;
    let (patrol, facing) = monster_patrol(animation_elapsed);
    let monster_x = placement
        .slot
        .foot_px
        .x
        .saturating_add(scaled_ref_x(frame.viewport, MONSTER_PATROL_CENTER_REF_PX));
    let foot_px = Point {
        x: add_signed(
            monster_x,
            i32::from(scaled_ref_x(frame.viewport, patrol.unsigned_abs() as i32)) * patrol.signum(),
        ),
        y: placement.slot.foot_px.y,
    };
    let kind = if placement.slot.index % 3 == 1 {
        TrainingMonsterKind::GreenMushroom
    } else {
        TrainingMonsterKind::Slime
    };
    let entry_route = training_entry_route(frame.viewport, placement.slot.foot_px);
    let has_reached_lane = elapsed_ms(frame.now, agent.created_at)
        >= TRAINING_PORTAL_IN_MS + training_route_duration(&entry_route);
    let pose = if matches!(&agent.state, ActivityState::Active { .. })
        && agent.exiting_at.is_none()
        && has_reached_lane
    {
        monster_pose(kind, active_phase(frame.now, placement.slot.index))
    } else {
        TrainingMonsterPose::Alive {
            frame_index: monster_alive_frame(kind, animation_elapsed),
        }
    };
    Some(TrainingMonsterFrame {
        kind,
        foot_px,
        facing,
        pose,
    })
}

/// Build task cards from the exact actor frames used by the pixel painter.
pub fn build_training_overlay(
    scene: &SceneState,
    placements: &[TrainingPlacement],
    hovered: Option<AgentId>,
    frame: TrainingFrameContext,
) -> Vec<LabelElement> {
    let mut label_counts: HashMap<&str, usize> = HashMap::new();
    for agent in scene.agents.values() {
        *label_counts.entry(&agent.label).or_insert(0) += 1;
    }
    placements
        .iter()
        .filter_map(|placement| {
            let agent = scene.agents.get(&placement.agent_id)?;
            let actor = resolve_training_actor(agent, *placement, frame)?;
            let fake_id = crate::market::MARKET_FAKE_PLAYER_IDS
                [placement.appearance_index % crate::market::MARKET_FAKE_PLAYER_IDS.len()];
            let task = agent
                .label
                .split_once('\u{b7}')
                .map_or(agent.label.as_ref(), |(_, tail)| tail);
            let mut text = format!("{fake_id}\u{b7}{task}");
            if label_counts.get(&*agent.label).copied().unwrap_or(0) > 1
                && agent.session_id.chars().count() >= 4
            {
                text.push('\u{b7}');
                text.push_str(&disambig_suffix(&agent.session_id));
            }
            Some(LabelElement {
                anchor_px: actor.label_anchor_px,
                text: truncate_label(&text, 16).into_owned(),
                tone: label_tone(agent),
                hovered: hovered == Some(agent.agent_id),
                relation: crate::maple_world::agent_relation(scene, agent.agent_id),
            })
        })
        .collect()
}

fn resolve_non_exit(
    agent: &AgentSlot,
    placement: TrainingPlacement,
    frame: TrainingFrameContext,
) -> Option<TrainingActorFrame> {
    let target = placement.slot.foot_px;
    let route = training_entry_route(frame.viewport, target);
    let entry_walk_ms = training_route_duration(&route);
    let since_created = elapsed_ms(frame.now, agent.created_at);
    if since_created < TRAINING_PORTAL_IN_MS {
        return None;
    }
    if since_created < TRAINING_PORTAL_IN_MS + entry_walk_ms {
        let elapsed = since_created - TRAINING_PORTAL_IN_MS;
        let sample = sample_training_route(&route, elapsed, elapsed, false);
        return Some(actor_frame(
            sample.foot_px,
            sample.pose,
            frame.viewport,
            false,
        ));
    }
    Some(resolve_settled(agent, placement, frame))
}

fn resolve_exit(
    agent: &AgentSlot,
    placement: TrainingPlacement,
    frame: TrainingFrameContext,
    exiting_at: SystemTime,
) -> Option<TrainingActorFrame> {
    let exit_start = resolve_settled(
        agent,
        placement,
        TrainingFrameContext {
            viewport: frame.viewport,
            now: exiting_at,
        },
    )
    .foot_px;
    let portal = portal_foot(frame.viewport);
    let route = training_entry_route(frame.viewport, exit_start);
    let duration = training_route_duration(&route);
    let elapsed = elapsed_ms(frame.now, exiting_at);
    if elapsed >= duration + TRAINING_PORTAL_OUT_MS {
        return None;
    }
    let route_progress = duration
        .saturating_sub(elapsed.min(duration))
        .saturating_sub(u64::from(elapsed < duration));
    let sample = sample_training_route(&route, route_progress, elapsed, true);
    Some(actor_frame(
        if elapsed == 0 {
            exit_start
        } else if elapsed < duration {
            sample.foot_px
        } else {
            portal
        },
        if elapsed < duration {
            sample.pose
        } else {
            TrainingActorPose::Stand { frame_index: 0 }
        },
        frame.viewport,
        false,
    ))
}

fn resolve_settled(
    agent: &AgentSlot,
    placement: TrainingPlacement,
    frame: TrainingFrameContext,
) -> TrainingActorFrame {
    let base = placement.slot.foot_px;
    match &agent.state {
        ActivityState::Active { .. } => {
            let (work_cycle, phase) = active_clock(frame.now, placement.slot.index);
            let contact = Point {
                x: base.x.saturating_add(scaled_ref_x(frame.viewport, 52)),
                y: base.y,
            };
            if phase < ACTIVE_RUN_END_MS {
                let foot = lerp_point(base, contact, phase, ACTIVE_RUN_END_MS);
                actor_frame(
                    foot,
                    walk_pose(phase, TrainingFacing::Right),
                    frame.viewport,
                    false,
                )
            } else if phase < ACTIVE_ATTACK_END_MS {
                let local = phase - ACTIVE_ATTACK_START_MS;
                let mut actor = actor_frame(
                    contact,
                    TrainingActorPose::Attack {
                        // The authored paperdoll has three attack poses. Hold
                        // the release pose while the longer VFX resolves rather
                        // than cycling into another character animation.
                        frame_index: ((local / ATTACK_FRAME_MS) as usize).min(ATTACK_FRAMES - 1),
                    },
                    frame.viewport,
                    false,
                );
                actor.skill_effect = training_skill_effect(
                    training_skill_kind(placement.appearance_index, work_cycle),
                    local,
                );
                actor
            } else if phase < ACTIVE_RECOVERY_END_MS {
                actor_frame(
                    contact,
                    TrainingActorPose::Stand { frame_index: 0 },
                    frame.viewport,
                    false,
                )
            } else if phase < ACTIVE_RETURN_END_MS {
                let local = phase - ACTIVE_RECOVERY_END_MS;
                let total = ACTIVE_RETURN_END_MS - ACTIVE_RECOVERY_END_MS;
                actor_frame(
                    lerp_point(contact, base, local, total),
                    walk_pose(local, TrainingFacing::Left),
                    frame.viewport,
                    false,
                )
            } else {
                actor_frame(
                    base,
                    TrainingActorPose::Stand {
                        frame_index: (phase / STAND_FRAME_MS) as usize % 3,
                    },
                    frame.viewport,
                    false,
                )
            }
        }
        ActivityState::Waiting { .. } => {
            actor_frame(base, TrainingActorPose::Sit, frame.viewport, true)
        }
        ActivityState::Idle => {
            let phase = (elapsed_ms(frame.now, SystemTime::UNIX_EPOCH)
                + placement.slot.index as u64 * 731)
                % IDLE_CYCLE_MS;
            let roam = scaled_ref_x(frame.viewport, IDLE_ROAM_REF_PX);
            let left = Point {
                x: base.x.saturating_sub(roam),
                y: base.y,
            };
            let (foot, pose) = match phase {
                0..=1_799 => (
                    lerp_point(base, left, phase, 1_800),
                    walk_pose(phase, TrainingFacing::Left),
                ),
                1_800..=4_099 => (
                    left,
                    TrainingActorPose::Stand {
                        frame_index: (phase / STAND_FRAME_MS) as usize % 3,
                    },
                ),
                4_100..=5_899 => (
                    lerp_point(left, base, phase - 4_100, 1_800),
                    walk_pose(phase, TrainingFacing::Right),
                ),
                _ => (
                    base,
                    TrainingActorPose::Stand {
                        frame_index: (phase / STAND_FRAME_MS) as usize % 3,
                    },
                ),
            };
            actor_frame(foot, pose, frame.viewport, false)
        }
    }
}

fn actor_frame(
    foot_px: Point,
    pose: TrainingActorPose,
    viewport: Bounds,
    question_bubble: bool,
) -> TrainingActorFrame {
    let scale = market_sprite_scale(viewport.height);
    let width = MARKET_AVATAR_WIDTH.saturating_mul(scale);
    let height = MARKET_AVATAR_HEIGHT.saturating_mul(scale);
    let sprite_anchor_px = Point {
        x: foot_px.x.saturating_sub(width / 2),
        y: foot_px.y.saturating_sub(height),
    };
    TrainingActorFrame {
        sprite_anchor_px,
        label_anchor_px: Point {
            x: foot_px.x.saturating_sub(CHARACTER_SPRITE_W / 2),
            y: sprite_anchor_px.y,
        },
        foot_px,
        pose,
        skill_effect: None,
        question_bubble,
    }
}

fn training_skill_kind(appearance_index: usize, work_cycle: u64) -> TrainingSkillKind {
    match (appearance_index + work_cycle as usize) % 3 {
        0 => TrainingSkillKind::MagicClaw,
        1 => TrainingSkillKind::HolyLight,
        _ => TrainingSkillKind::DragonPulse,
    }
}

fn training_skill_effect(kind: TrainingSkillKind, local_ms: u64) -> Option<TrainingSkillEffect> {
    let frame_index = (local_ms / TRAINING_SKILL_FRAME_MS) as usize;
    (frame_index < TRAINING_SKILL_FRAMES).then_some(TrainingSkillEffect { kind, frame_index })
}

fn monster_pose(kind: TrainingMonsterKind, phase: u64) -> TrainingMonsterPose {
    if phase < ACTIVE_ATTACK_HIT_MS {
        TrainingMonsterPose::Alive {
            frame_index: monster_alive_frame(kind, phase),
        }
    } else if phase < ACTIVE_DEATH_END_MS {
        TrainingMonsterPose::Dying {
            frame_index: death_frame(phase - ACTIVE_ATTACK_HIT_MS).unwrap_or(3),
        }
    } else if phase < ACTIVE_RESPAWN_MS {
        TrainingMonsterPose::Hidden
    } else {
        TrainingMonsterPose::Respawning {
            frame_index: monster_alive_frame(kind, phase - ACTIVE_RESPAWN_MS),
        }
    }
}

fn monster_alive_frame(kind: TrainingMonsterKind, elapsed_ms: u64) -> usize {
    match kind {
        TrainingMonsterKind::Slime => timeline_frame(elapsed_ms, &SLIME_MOVE_DELAYS_MS),
        TrainingMonsterKind::GreenMushroom => {
            timeline_frame(elapsed_ms, &GREEN_MUSHROOM_MOVE_DELAYS_MS)
        }
    }
}

fn monster_patrol(elapsed_ms: u64) -> (i32, TrainingFacing) {
    let cycle_ms = MONSTER_PATROL_LEG_MS * 2;
    let phase = elapsed_ms % cycle_ms;
    let full_span = MONSTER_PATROL_RADIUS_REF_PX * 2;
    if phase < MONSTER_PATROL_LEG_MS {
        let offset = -MONSTER_PATROL_RADIUS_REF_PX
            + (i64::from(full_span) * phase as i64 / MONSTER_PATROL_LEG_MS as i64) as i32;
        (offset, TrainingFacing::Right)
    } else {
        let return_ms = phase - MONSTER_PATROL_LEG_MS;
        let offset = MONSTER_PATROL_RADIUS_REF_PX
            - (i64::from(full_span) * return_ms as i64 / MONSTER_PATROL_LEG_MS as i64) as i32;
        (offset, TrainingFacing::Left)
    }
}

fn timeline_frame(elapsed_ms: u64, delays_ms: &[u64]) -> usize {
    let total = delays_ms.iter().copied().sum::<u64>().max(1);
    let mut local = elapsed_ms % total;
    for (index, delay) in delays_ms.iter().copied().enumerate() {
        if local < delay {
            return index;
        }
        local = local.saturating_sub(delay);
    }
    delays_ms.len().saturating_sub(1)
}

fn death_frame(elapsed_ms: u64) -> Option<usize> {
    if elapsed_ms >= CLASSIC_DEATH_TOTAL_MS {
        return None;
    }
    let mut local = elapsed_ms;
    for (index, delay) in CLASSIC_DEATH_DELAYS_MS.iter().copied().enumerate() {
        if local < delay {
            return Some(index);
        }
        local = local.saturating_sub(delay);
    }
    None
}

fn active_phase(now: SystemTime, slot_index: usize) -> u64 {
    active_clock(now, slot_index).1
}

fn active_clock(now: SystemTime, slot_index: usize) -> (u64, u64) {
    let elapsed = elapsed_ms(now, SystemTime::UNIX_EPOCH).saturating_add(slot_index as u64 * 800);
    (
        elapsed / TRAINING_WORK_CYCLE_MS,
        elapsed % TRAINING_WORK_CYCLE_MS,
    )
}

fn portal_foot(viewport: Bounds) -> Point {
    point_at(viewport, 939, 858)
}

fn training_entry_route(viewport: Bounds, target: Point) -> Vec<TrainingRouteSegment> {
    let portal = portal_foot(viewport);
    let lower_rope_bottom = point_at(viewport, 443, 858);
    let lower_rope_top = point_at(viewport, 443, 558);
    let upper_rope_bottom = point_at(viewport, 754, 558);
    let upper_rope_top = point_at(viewport, 754, 258);
    let mut route = Vec::with_capacity(5);

    if target.y <= upper_rope_top.y {
        push_training_walk(&mut route, portal, lower_rope_bottom, viewport);
        push_training_climb(&mut route, lower_rope_bottom, lower_rope_top, viewport);
        push_training_walk(&mut route, lower_rope_top, upper_rope_bottom, viewport);
        push_training_climb(&mut route, upper_rope_bottom, upper_rope_top, viewport);
        push_training_walk(&mut route, upper_rope_top, target, viewport);
    } else if target.y <= lower_rope_top.y {
        push_training_walk(&mut route, portal, lower_rope_bottom, viewport);
        push_training_climb(&mut route, lower_rope_bottom, lower_rope_top, viewport);
        push_training_walk(&mut route, lower_rope_top, target, viewport);
    } else {
        push_training_walk(&mut route, portal, target, viewport);
    }
    route
}

fn push_training_walk(
    route: &mut Vec<TrainingRouteSegment>,
    from: Point,
    to: Point,
    viewport: Bounds,
) {
    push_training_segment(
        route,
        from,
        to,
        viewport,
        TRAINING_REFERENCE_WALK_PX_PER_SEC,
        TrainingRouteKind::Walk,
    );
}

fn push_training_climb(
    route: &mut Vec<TrainingRouteSegment>,
    from: Point,
    to: Point,
    viewport: Bounds,
) {
    push_training_segment(
        route,
        from,
        to,
        viewport,
        TRAINING_REFERENCE_CLIMB_PX_PER_SEC,
        TrainingRouteKind::Climb,
    );
}

fn push_training_segment(
    route: &mut Vec<TrainingRouteSegment>,
    from: Point,
    to: Point,
    viewport: Bounds,
    reference_px_per_sec: u64,
    kind: TrainingRouteKind,
) {
    if from == to {
        return;
    }
    let duration_ms = training_reference_distance(from, to, viewport).saturating_mul(1_000)
        / reference_px_per_sec;
    route.push(TrainingRouteSegment {
        from,
        to,
        duration_ms: duration_ms.max(1),
        kind,
    });
}

fn training_reference_distance(from: Point, to: Point, viewport: Bounds) -> u64 {
    let reference_dx =
        u64::from(from.x.abs_diff(to.x)) * REFERENCE_WIDTH / u64::from(viewport.width.max(1));
    let reference_dy =
        u64::from(from.y.abs_diff(to.y)) * REFERENCE_HEIGHT / u64::from(viewport.height.max(1));
    reference_dx + reference_dy
}

fn training_route_duration(route: &[TrainingRouteSegment]) -> u64 {
    route.iter().map(|segment| segment.duration_ms).sum()
}

fn sample_training_route(
    route: &[TrainingRouteSegment],
    route_elapsed: u64,
    animation_elapsed: u64,
    reverse: bool,
) -> TrainingRouteSample {
    let mut remaining = route_elapsed.min(training_route_duration(route));
    for segment in route {
        if remaining >= segment.duration_ms {
            remaining -= segment.duration_ms;
            continue;
        }
        let foot_px = lerp_point(segment.from, segment.to, remaining, segment.duration_ms);
        let pose = match segment.kind {
            TrainingRouteKind::Walk => {
                let direction = if reverse {
                    facing(segment.to, segment.from)
                } else {
                    facing(segment.from, segment.to)
                };
                walk_pose(animation_elapsed, direction)
            }
            TrainingRouteKind::Climb => TrainingActorPose::Climb {
                frame_index: (animation_elapsed / crate::market::MARKET_CLIMB_FRAME_MS) as usize
                    % crate::market::MARKET_CLIMB_FRAMES,
            },
        };
        return TrainingRouteSample { foot_px, pose };
    }
    TrainingRouteSample {
        foot_px: route
            .last()
            .map_or(Point { x: 0, y: 0 }, |segment| segment.to),
        pose: TrainingActorPose::Stand { frame_index: 0 },
    }
}

fn walk_pose(elapsed: u64, facing: TrainingFacing) -> TrainingActorPose {
    TrainingActorPose::Walk {
        frame_index: (elapsed / WALK_FRAME_MS) as usize % 4,
        facing,
    }
}

fn facing(from: Point, to: Point) -> TrainingFacing {
    if to.x >= from.x {
        TrainingFacing::Right
    } else {
        TrainingFacing::Left
    }
}

fn lerp_point(from: Point, to: Point, elapsed: u64, duration: u64) -> Point {
    if duration == 0 || elapsed >= duration {
        return to;
    }
    let t = elapsed as f64 / duration as f64;
    Point {
        x: lerp_axis(from.x, to.x, t),
        y: lerp_axis(from.y, to.y, t),
    }
}

fn lerp_axis(from: u16, to: u16, t: f64) -> u16 {
    (f64::from(from) + (f64::from(to) - f64::from(from)) * t)
        .round()
        .clamp(0.0, f64::from(u16::MAX)) as u16
}

fn point_at(viewport: Bounds, x_per_mille: u16, y_per_mille: u16) -> Point {
    Point {
        x: resolve_axis(viewport.x, viewport.width, x_per_mille),
        y: resolve_axis(viewport.y, viewport.height, y_per_mille),
    }
}

fn resolve_axis(origin: u16, extent: u16, per_mille: u16) -> u16 {
    let offset = (u32::from(extent) * u32::from(per_mille) / 1_000).min(u32::from(extent - 1));
    origin.saturating_add(offset.min(u32::from(u16::MAX)) as u16)
}

fn scaled_ref_x(viewport: Bounds, px: i32) -> u16 {
    ((i64::from(px) * i64::from(viewport.width) / 720).max(1)) as u16
}

fn add_signed(value: u16, delta: i32) -> u16 {
    (i32::from(value) + delta).clamp(0, i32::from(u16::MAX)) as u16
}

fn label_tone(agent: &AgentSlot) -> LabelTone {
    if agent.exiting_at.is_some() {
        return LabelTone::Exiting;
    }
    match &agent.state {
        ActivityState::Active { .. } => LabelTone::Active,
        ActivityState::Waiting { .. } => LabelTone::Waiting,
        ActivityState::Idle => LabelTone::Idle,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use pixtuoid_core::{ActivityState, GlobalDeskIndex, ToolKind};

    use super::*;

    fn viewport() -> Bounds {
        Bounds {
            x: 0,
            y: 0,
            width: 240,
            height: 160,
        }
    }

    fn active(id: &str) -> ActivityState {
        ActivityState::Active {
            tool_use_id: Some(Arc::from(id)),
            detail: Some(Arc::from("exec")),
            kind: ToolKind::Bash,
        }
    }

    fn agent(id: AgentId, desk: usize, state: ActivityState) -> AgentSlot {
        let created = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        AgentSlot {
            agent_id: id,
            source: Arc::from("codex"),
            session_id: Arc::from(format!("session-{desk}")),
            cwd: Arc::from(Path::new("/training")),
            label: format!("task-{desk}").into(),
            state,
            state_started_at: created,
            last_event_at: created,
            created_at: created,
            exiting_at: None,
            pending_idle_at: None,
            desk_index: GlobalDeskIndex(desk),
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

    #[test]
    fn training_cards_preserve_the_same_root_child_lineage_as_the_market() {
        let root_id = AgentId::from_parts("codex", "training-root");
        let child_id = AgentId::from_parts("codex", "training-child");
        let mut root = agent(root_id, 0, ActivityState::Idle);
        root.label = "cx\u{b7}盤點新廠".into();
        let mut child = agent(child_id, 1, ActivityState::Idle);
        child.label = "cx\u{b7}拍攝設備".into();
        child.parent_id = Some(root_id);
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(root_id, root);
        scene.agents.insert(child_id, child);
        let placements = build_training_placements(&scene, viewport());
        let labels = build_training_overlay(
            &scene,
            &placements,
            None,
            TrainingFrameContext {
                viewport: viewport(),
                now: SystemTime::UNIX_EPOCH + Duration::from_secs(100),
            },
        );

        assert_eq!(labels.len(), 2);
        assert_eq!(
            labels[0].relation,
            Some(crate::maple_world::MapleAgentRelation::Root {
                root_id,
                descendant_count: 1,
            })
        );
        assert_eq!(
            labels[1].relation,
            Some(crate::maple_world::MapleAgentRelation::Child {
                root_id,
                root_task: Some("盤點新廠".to_owned()),
                depth: 1,
            })
        );
    }

    fn placement(id: AgentId, slot: usize) -> TrainingPlacement {
        TrainingPlacement {
            agent_id: id,
            slot: training_slots(viewport(), TRAINING_MAX_AGENTS)[slot],
            appearance_index: slot,
        }
    }

    #[test]
    fn authored_slots_are_inside_platform_lanes_and_clear_of_the_portal() {
        let slots = training_slots(viewport(), TRAINING_MAX_AGENTS);
        assert_eq!(slots.len(), TRAINING_MAX_AGENTS);
        assert_eq!(slots[0].foot_px, Point { x: 120, y: 137 });
        assert_eq!(slots[1].foot_px, Point { x: 120, y: 89 });
        assert_eq!(slots[2].foot_px, Point { x: 120, y: 41 });
        let portal = portal_foot(viewport());
        assert!(slots.iter().all(|slot| slot.foot_px.x + 20 < portal.x));
    }

    #[test]
    fn tool_id_churn_does_not_reset_the_active_training_cycle() {
        let id = AgentId::from_parts("codex", "active");
        let mut before = agent(id, 0, active("tool-a"));
        let mut after = before.clone();
        after.state = active("tool-b");
        after.state_started_at += Duration::from_secs(12);
        before.state_started_at = SystemTime::UNIX_EPOCH;
        let frame = TrainingFrameContext {
            viewport: viewport(),
            now: SystemTime::UNIX_EPOCH + Duration::from_millis(65_100),
        };

        assert_eq!(
            resolve_training_actor(&before, placement(id, 0), frame),
            resolve_training_actor(&after, placement(id, 0), frame)
        );
    }

    #[test]
    fn waiting_is_stationary_with_question_bubble_and_idle_roams_a_real_distance() {
        let id = AgentId::from_parts("codex", "waiting");
        let mut waiting = agent(
            id,
            0,
            ActivityState::Waiting {
                reason: Arc::from("user"),
            },
        );
        waiting.created_at = SystemTime::UNIX_EPOCH;
        let p = placement(id, 0);
        let frame = TrainingFrameContext {
            viewport: viewport(),
            now: SystemTime::UNIX_EPOCH + Duration::from_secs(100),
        };
        let waiting_actor = resolve_training_actor(&waiting, p, frame).unwrap();
        assert_eq!(waiting_actor.pose, TrainingActorPose::Sit);
        assert!(waiting_actor.question_bubble);

        waiting.state = ActivityState::Idle;
        let a = resolve_training_actor(
            &waiting,
            p,
            TrainingFrameContext {
                now: SystemTime::UNIX_EPOCH + Duration::from_millis(99_000),
                ..frame
            },
        )
        .unwrap();
        let b = resolve_training_actor(
            &waiting,
            p,
            TrainingFrameContext {
                now: SystemTime::UNIX_EPOCH + Duration::from_millis(100_700),
                ..frame
            },
        )
        .unwrap();
        assert!(a.foot_px.x.abs_diff(b.foot_px.x) >= 10);
    }

    #[test]
    fn active_attack_drives_death_hidden_and_respawn_enemy_phases() {
        assert!(matches!(
            monster_pose(TrainingMonsterKind::Slime, ACTIVE_ATTACK_HIT_MS),
            TrainingMonsterPose::Dying { frame_index: 0 }
        ));
        assert_eq!(
            monster_pose(TrainingMonsterKind::Slime, ACTIVE_DEATH_END_MS),
            TrainingMonsterPose::Hidden
        );
        assert!(matches!(
            monster_pose(TrainingMonsterKind::Slime, ACTIVE_RESPAWN_MS),
            TrainingMonsterPose::Respawning { frame_index: 0 }
        ));
    }

    #[test]
    fn slime_move_cycle_uses_the_classic_variable_frame_delays() {
        assert_eq!(monster_alive_frame(TrainingMonsterKind::Slime, 0), 0);
        assert_eq!(monster_alive_frame(TrainingMonsterKind::Slime, 199), 0);
        assert_eq!(monster_alive_frame(TrainingMonsterKind::Slime, 200), 1);
        assert_eq!(monster_alive_frame(TrainingMonsterKind::Slime, 249), 1);
        assert_eq!(monster_alive_frame(TrainingMonsterKind::Slime, 250), 2);
        assert_eq!(monster_alive_frame(TrainingMonsterKind::Slime, 1_049), 6);
        assert_eq!(monster_alive_frame(TrainingMonsterKind::Slime, 1_050), 0);
    }

    #[test]
    fn living_monster_patrol_crosses_a_visibly_distinct_part_of_its_platform() {
        let id = AgentId::from_parts("codex", "monster-patrol");
        let idle_agent = agent(id, 0, ActivityState::Idle);
        let placement = placement(id, 0);
        let at_left = resolve_training_monster(
            &idle_agent,
            placement,
            TrainingFrameContext {
                viewport: viewport(),
                now: SystemTime::UNIX_EPOCH + Duration::from_millis(62_400),
            },
        )
        .expect("settled lane monster should be visible");
        let at_right = resolve_training_monster(
            &idle_agent,
            placement,
            TrainingFrameContext {
                viewport: viewport(),
                now: SystemTime::UNIX_EPOCH + Duration::from_millis(64_800),
            },
        )
        .expect("settled lane monster should remain visible");

        assert!(
            at_left.foot_px.x.abs_diff(at_right.foot_px.x) >= 24,
            "a full-size render should show a real patrol, not an in-place shuffle"
        );
        assert_eq!(at_left.foot_px.y, at_right.foot_px.y);
        assert_eq!(at_left.facing, TrainingFacing::Right);
        assert_eq!(at_right.facing, TrainingFacing::Left);
    }

    #[test]
    fn classic_four_frame_death_cycle_lasts_840_milliseconds() {
        for (elapsed, expected) in [(0, 0), (179, 0), (180, 1), (360, 2), (540, 3), (839, 3)] {
            assert_eq!(death_frame(elapsed), Some(expected));
        }
        assert_eq!(death_frame(840), None);
    }

    #[test]
    fn active_work_uses_the_three_authored_attack_frames_before_the_hit() {
        let id = AgentId::from_parts("codex", "attack-cycle");
        let active_agent = agent(id, 0, active("tool"));
        let p = placement(id, 0);

        for (phase, expected_frame) in [
            (ACTIVE_ATTACK_START_MS, 0),
            (ACTIVE_ATTACK_START_MS + ATTACK_FRAME_MS, 1),
            (ACTIVE_ATTACK_HIT_MS, 2),
        ] {
            let actor = resolve_training_actor(
                &active_agent,
                p,
                TrainingFrameContext {
                    viewport: viewport(),
                    now: SystemTime::UNIX_EPOCH + Duration::from_millis(64_000 + phase),
                },
            )
            .expect("the public actor seam is settled after 64 seconds");
            assert_eq!(
                actor.pose,
                TrainingActorPose::Attack {
                    frame_index: expected_frame,
                }
            );
            assert_eq!(actor.foot_px.y, p.slot.foot_px.y);
        }
    }

    #[test]
    fn training_skill_effect_stays_legible_for_four_240_ms_frames() {
        let id = AgentId::from_parts("codex", "skill-overlay");
        let active_agent = agent(id, 0, active("tool"));
        let placement = placement(id, 0);

        for (local_ms, expected) in [
            (0, Some(0)),
            (239, Some(0)),
            (240, Some(1)),
            (479, Some(1)),
            (480, Some(2)),
            (719, Some(2)),
            (720, Some(3)),
            (959, Some(3)),
            (960, None),
        ] {
            let actor = resolve_training_actor(
                &active_agent,
                placement,
                TrainingFrameContext {
                    viewport: viewport(),
                    now: SystemTime::UNIX_EPOCH
                        + Duration::from_millis(
                            TRAINING_WORK_CYCLE_MS * 9 + ACTIVE_ATTACK_START_MS + local_ms,
                        ),
                },
            )
            .expect("the settled active actor remains visible throughout the skill");

            assert_eq!(
                actor.skill_effect,
                expected.map(|frame_index| TrainingSkillEffect {
                    kind: TrainingSkillKind::MagicClaw,
                    frame_index,
                })
            );
            if expected.is_some() {
                assert!(matches!(actor.pose, TrainingActorPose::Attack { .. }));
            } else {
                assert!(matches!(actor.pose, TrainingActorPose::Stand { .. }));
            }
            assert_eq!(actor.foot_px.y, placement.slot.foot_px.y);
        }
    }

    #[test]
    fn resolved_attack_offsets_the_skill_deck_by_paperdoll() {
        for (slot, expected) in [
            (0, TrainingSkillKind::MagicClaw),
            (1, TrainingSkillKind::HolyLight),
            (2, TrainingSkillKind::DragonPulse),
            (3, TrainingSkillKind::MagicClaw),
        ] {
            let id = AgentId::from_parts("codex", &format!("skill-{slot}"));
            let placement = placement(id, slot);
            let now_ms = TRAINING_WORK_CYCLE_MS * 9 + ACTIVE_ATTACK_START_MS - slot as u64 * 800;
            let actor = resolve_training_actor(
                &agent(id, slot, active("tool")),
                placement,
                TrainingFrameContext {
                    viewport: viewport(),
                    now: SystemTime::UNIX_EPOCH + Duration::from_millis(now_ms),
                },
            )
            .expect("settled actor remains visible during its attack");

            assert_eq!(
                actor.skill_effect,
                Some(TrainingSkillEffect {
                    kind: expected,
                    frame_index: 0,
                })
            );
        }
    }

    #[test]
    fn one_active_paperdoll_rotates_through_every_skill_across_work_loops() {
        let id = AgentId::from_parts("codex", "skill-deck");
        let active_agent = agent(id, 0, active("tool"));
        let placement = placement(id, 0);
        let base_ms = TRAINING_WORK_CYCLE_MS * 9;

        for (cycle_offset, expected) in [
            TrainingSkillKind::MagicClaw,
            TrainingSkillKind::HolyLight,
            TrainingSkillKind::DragonPulse,
        ]
        .into_iter()
        .enumerate()
        {
            let actor = resolve_training_actor(
                &active_agent,
                placement,
                TrainingFrameContext {
                    viewport: viewport(),
                    now: SystemTime::UNIX_EPOCH
                        + Duration::from_millis(
                            base_ms
                                + cycle_offset as u64 * TRAINING_WORK_CYCLE_MS
                                + ACTIVE_ATTACK_START_MS,
                        ),
                },
            )
            .expect("the same settled actor remains visible on every work loop");

            assert_eq!(
                actor.skill_effect.map(|effect| effect.kind),
                Some(expected),
                "work loop {cycle_offset} must advance the visible skill deck"
            );
            assert_eq!(placement.appearance_index, 0, "the paperdoll never changes");
        }
    }

    #[test]
    fn entering_agent_cannot_kill_its_monster_before_reaching_the_lane() {
        let id = AgentId::from_parts("codex", "entry-gate");
        let active_agent = agent(id, 0, active("tool"));
        let upper_placement = placement(id, 1);
        let now_ms = TRAINING_WORK_CYCLE_MS * 2 + ACTIVE_ATTACK_HIT_MS - 800;
        let route_ms = training_route_duration(&training_entry_route(
            viewport(),
            upper_placement.slot.foot_px,
        ));
        assert!(
            now_ms - 10_000 < TRAINING_PORTAL_IN_MS + route_ms,
            "the regression sample must still be on the authored entry route"
        );
        let monster = resolve_training_monster(
            &active_agent,
            upper_placement,
            TrainingFrameContext {
                viewport: viewport(),
                // Slot 1's combat phase is exactly the hit frame, while its
                // longer rope route is still in progress.
                now: SystemTime::UNIX_EPOCH + Duration::from_millis(now_ms),
            },
        )
        .expect("the entering actor is visible after portal materialization");

        assert!(matches!(monster.pose, TrainingMonsterPose::Alive { .. }));
    }

    #[test]
    fn active_return_walk_crosses_the_lane_instead_of_animating_in_place() {
        let id = AgentId::from_parts("codex", "return-walk");
        let active_agent = agent(id, 0, active("tool"));
        let p = placement(id, 0);
        let at_recovery_end = resolve_settled(
            &active_agent,
            p,
            TrainingFrameContext {
                viewport: viewport(),
                now: SystemTime::UNIX_EPOCH + Duration::from_millis(ACTIVE_RECOVERY_END_MS),
            },
        );
        let halfway_home = resolve_settled(
            &active_agent,
            p,
            TrainingFrameContext {
                viewport: viewport(),
                now: SystemTime::UNIX_EPOCH
                    + Duration::from_millis((ACTIVE_RECOVERY_END_MS + ACTIVE_RETURN_END_MS) / 2),
            },
        );
        let base = p.slot.foot_px;

        assert!(at_recovery_end.foot_px.x > halfway_home.foot_px.x);
        assert!(halfway_home.foot_px.x > base.x);
        assert_eq!(halfway_home.foot_px.y, base.y);
        assert!(matches!(
            halfway_home.pose,
            TrainingActorPose::Walk {
                facing: TrainingFacing::Left,
                ..
            }
        ));
    }

    #[test]
    fn upper_floor_entry_walks_on_platforms_and_climbs_the_authored_rope() {
        let id = AgentId::from_parts("codex", "rope-entry");
        let entering = agent(id, 1, ActivityState::Idle);
        let p = placement(id, 1);
        let materialized_at = entering.created_at + Duration::from_millis(TRAINING_PORTAL_IN_MS);

        let first_walk = resolve_training_actor(
            &entering,
            p,
            TrainingFrameContext {
                viewport: viewport(),
                now: materialized_at + Duration::from_millis(400),
            },
        )
        .expect("the actor is visible after portal materialization");
        assert_eq!(first_walk.foot_px.y, portal_foot(viewport()).y);
        assert!(matches!(
            first_walk.pose,
            TrainingActorPose::Walk {
                facing: TrainingFacing::Left,
                ..
            }
        ));

        let on_first_rope = resolve_training_actor(
            &entering,
            p,
            TrainingFrameContext {
                viewport: viewport(),
                now: materialized_at + Duration::from_millis(3_200),
            },
        )
        .expect("the actor remains visible while climbing");
        assert!(matches!(
            on_first_rope.pose,
            TrainingActorPose::Climb { .. }
        ));
        assert_ne!(on_first_rope.foot_px.y, portal_foot(viewport()).y);
    }

    #[test]
    fn every_entry_route_uses_only_platform_walks_and_vertical_rope_climbs() {
        for slot in 0..TRAINING_MAX_AGENTS {
            let id = AgentId::from_parts("codex", &format!("axis-route-{slot}"));
            let route = training_entry_route(viewport(), placement(id, slot).slot.foot_px);
            assert!(!route.is_empty(), "slot {slot} needs an authored route");
            for segment in route {
                match segment.kind {
                    TrainingRouteKind::Walk => {
                        assert_eq!(
                            segment.from.y, segment.to.y,
                            "slot {slot} walk cannot float diagonally"
                        );
                        assert_ne!(segment.from.x, segment.to.x);
                    }
                    TrainingRouteKind::Climb => {
                        assert_eq!(
                            segment.from.x, segment.to.x,
                            "slot {slot} climb must stay on one rope"
                        );
                        assert_ne!(segment.from.y, segment.to.y);
                    }
                }
            }
        }
    }

    #[test]
    fn upper_floor_exit_reverses_the_same_walk_and_climb_route() {
        let id = AgentId::from_parts("codex", "rope-exit");
        let mut exiting = agent(
            id,
            2,
            ActivityState::Waiting {
                reason: Arc::from("user"),
            },
        );
        exiting.created_at = SystemTime::UNIX_EPOCH;
        let exit_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        exiting.exiting_at = Some(exit_at);
        let p = placement(id, 2);

        let first_step = resolve_training_actor(
            &exiting,
            p,
            TrainingFrameContext {
                viewport: viewport(),
                now: exit_at + Duration::from_millis(200),
            },
        )
        .expect("the exiting actor starts on the upper platform");
        assert_eq!(first_step.foot_px.y, p.slot.foot_px.y);
        assert!(matches!(first_step.pose, TrainingActorPose::Walk { .. }));

        let on_upper_rope = resolve_training_actor(
            &exiting,
            p,
            TrainingFrameContext {
                viewport: viewport(),
                now: exit_at + Duration::from_millis(1_900),
            },
        )
        .expect("the exiting actor remains visible while descending");
        assert!(matches!(
            on_upper_rope.pose,
            TrainingActorPose::Climb { .. }
        ));
        assert!(on_upper_rope.foot_px.y > p.slot.foot_px.y);
    }

    #[test]
    fn exit_walk_reaches_the_portal_then_hides() {
        let id = AgentId::from_parts("codex", "exit");
        let mut exiting = agent(id, 0, ActivityState::Idle);
        exiting.created_at = SystemTime::UNIX_EPOCH;
        let exit_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        exiting.exiting_at = Some(exit_at);
        let p = placement(id, 0);
        let exit_start = resolve_settled(
            &exiting,
            p,
            TrainingFrameContext {
                viewport: viewport(),
                now: exit_at,
            },
        )
        .foot_px;
        let duration = training_route_duration(&training_entry_route(viewport(), exit_start));

        let at_portal = resolve_training_actor(
            &exiting,
            p,
            TrainingFrameContext {
                viewport: viewport(),
                now: exit_at + Duration::from_millis(duration),
            },
        )
        .unwrap();
        assert_eq!(at_portal.foot_px, portal_foot(viewport()));
        assert!(resolve_training_actor(
            &exiting,
            p,
            TrainingFrameContext {
                viewport: viewport(),
                now: exit_at + Duration::from_millis(duration + TRAINING_PORTAL_OUT_MS),
            }
        )
        .is_none());
    }
}
