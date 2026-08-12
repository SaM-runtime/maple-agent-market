//! Backend-agnostic free-market placement and label-overlay authority.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use pixtuoid_core::sprite::format::Pack;
use pixtuoid_core::sprite::Sprite;
use pixtuoid_core::state::ToolKind;
use pixtuoid_core::{ActivityState, AgentId, AgentSlot, SceneState};

use crate::anim::elapsed_ms;
use crate::layout::{Bounds, Point, CHARACTER_SPRITE_W};
use crate::overlay::{disambig_suffix, truncate_label, LabelElement, LabelTone};

/// Maximum number of simultaneously placed agents in the first free-market scene.
pub const MARKET_MAX_AGENTS: usize = 8;

// Desk-fixed aliases keep merchants distinguishable without exposing source-native identities.
pub(crate) const MARKET_FAKE_PLAYER_IDS: [&str; MARKET_MAX_AGENTS] = [
    "素材狐",
    "動作貓",
    "介面星",
    "程式熊",
    "測試鳥",
    "文件兔",
    "安全鹿",
    "協作楓",
];

/// Logical Free Market paperdoll canvas at the 240x160 reference viewport.
///
/// A fixed logical canvas lets the renderer and floating shop-card overlay share
/// a pack-independent head anchor. The renderer samples either a legacy 32x24
/// source or a high-resolution 96x72 source into this logical rectangle at the
/// viewport's integer market scale. The original fallback merchant is 8x12; all
/// variants are centred on the same x axis and end on the same foot baseline.
pub const MARKET_AVATAR_WIDTH: u16 = 32;
pub const MARKET_AVATAR_HEIGHT: u16 = 24;
/// Native source canvas that maps one-to-one at the default 720x480 window.
pub const MARKET_AVATAR_HIRES_WIDTH: u16 = 96;
pub const MARKET_AVATAR_HIRES_HEIGHT: u16 = 72;
/// Local-only attack source canvas. The lower-centred 96x72 region preserves
/// body scale while the larger canvas keeps the renderer-authored skill arc.
pub const MARKET_AVATAR_ATTACK_HIRES_WIDTH: u16 = 192;
pub const MARKET_AVATAR_ATTACK_HIRES_HEIGHT: u16 = 144;
/// Logical attack canvas at the 240x160 reference viewport.
pub const MARKET_AVATAR_ATTACK_WIDTH: u16 = 64;
pub const MARKET_AVATAR_ATTACK_HEIGHT: u16 = 48;
const REFERENCE_MERCHANT_WIDTH: u16 = 8;
const REFERENCE_MERCHANT_HEIGHT: u16 = 12;
const REFERENCE_VIEWPORT_HEIGHT: u32 = 160;

/// Duration of one source-authored `walk1` pose.
pub const MARKET_WALK_FRAME_MS: u64 = 180;
/// Number of poses in the source-authored `walk1` cycle.
pub const MARKET_WALK_FRAMES: usize = 4;
/// Duration of one source-authored ladder pose.
pub const MARKET_CLIMB_FRAME_MS: u64 = 220;
/// Number of source-authored ladder poses per paperdoll.
pub const MARKET_CLIMB_FRAMES: usize = 2;
/// Number of source-authored stand1 poses per paperdoll.
pub const MARKET_STAND_FRAMES: usize = 3;
/// Number of source-authored stand2 poses per paperdoll.
pub const MARKET_STAND2_FRAMES: usize = 3;
/// Number of source-authored sit poses per paperdoll.
pub const MARKET_SIT_FRAMES: usize = 1;
/// Number of source-authored alert poses per paperdoll.
pub const MARKET_ALERT_FRAMES: usize = 3;
/// Number of source-authored swingO1 poses per paperdoll.
pub const MARKET_ATTACK_FRAMES: usize = 3;
/// Upper bound kept below core's Maple walkout grace window.
pub const MARKET_MAX_EXIT_MS: u32 = 9_200;
/// Conservative time by which every one of the eight Maple-speed routes is settled.
pub const MARKET_MAX_ENTRY_MS: u32 = 9_000;
const MARKET_PORTAL_IN_MS: u64 = 220;
const MARKET_PORTAL_OUT_MS: u64 = 180;
/// Classic MapleStory's unboosted ground speed in display/reference pixels per second.
const MARKET_WALK_REFERENCE_PX_PER_SECOND: u64 = 125;
/// Classic ladder movement is slower than ground movement (about 3 px per 30 ms frame).
const MARKET_CLIMB_REFERENCE_PX_PER_SECOND: u64 = 100;
const MARKET_STAND_FRAME_MS: u64 = 500;
const MARKET_STATUS_FRAME_MS: u64 = 500;
const MARKET_ALERT_CYCLE_MS: u64 = MARKET_STATUS_FRAME_MS * 4;
/// One-shot command-start flash. It is deliberately shorter than an ordinary
/// command so a long-running shell tool does not loop or cover the shop card.
pub(crate) const MARKET_COMMAND_SUCCESS_MS: u64 = 1_200;
/// One-shot normal-turn completion celebration. Long enough to remain visible
/// under a temporarily throttled floating window, but never loops or delays the
/// agent's existing Active→Idle transition.
pub(crate) const MARKET_TURN_COMPLETE_MS: u64 = 2_200;
const MARKET_IDLE_STALL_PAUSE_MS: u64 = 4_500;
const MARKET_IDLE_AWAY_PAUSE_MS: u64 = 2_000;
/// Authored rows by which the fixed stall overlaps a merchant's foot line.
pub const MARKET_STALL_FOOT_OVERLAP: u16 = 2;
/// Authored row at which the clean lower nameplate interior begins.
pub const MARKET_STALL_NAMEPLATE_Y: u16 = 3;

fn complete_fixed_canvas(
    animation: &Sprite,
    minimum_frames: usize,
    width: u16,
    height: u16,
) -> bool {
    animation.frames.len() >= minimum_frames
        && animation
            .frames
            .iter()
            .all(|frame| frame.width() == width && frame.height() == height)
}

/// Resolve the best complete paperdoll set a pack provides.
///
/// The high-resolution set is intentionally a separate optional animation so
/// existing `market_avatar` packs retain their authored 32x24 contract and can
/// remain byte-for-byte unchanged. An incomplete or wrong-sized high-resolution
/// set falls back to the complete legacy set instead of making agents disappear.
pub fn market_avatar_animation(pack: &Pack) -> Option<&Sprite> {
    pack.animation("market_avatar_hires")
        .filter(|animation| {
            complete_fixed_canvas(
                animation,
                MARKET_MAX_AGENTS,
                MARKET_AVATAR_HIRES_WIDTH,
                MARKET_AVATAR_HIRES_HEIGHT,
            )
        })
        .or_else(|| {
            pack.animation("market_avatar").filter(|animation| {
                complete_fixed_canvas(
                    animation,
                    MARKET_MAX_AGENTS,
                    MARKET_AVATAR_WIDTH,
                    MARKET_AVATAR_HEIGHT,
                )
            })
        })
}

/// Resolve a complete high-resolution four-pose walk cycle for every market slot.
///
/// Packs without all 32 fixed-canvas frames safely retain positional movement
/// with the selected idle paperdoll instead of hiding the merchant.
pub fn market_avatar_walk_animation(pack: &Pack) -> Option<&Sprite> {
    pack.animation("market_avatar_walk_hires")
        .filter(|animation| {
            complete_fixed_canvas(
                animation,
                MARKET_MAX_AGENTS * MARKET_WALK_FRAMES,
                MARKET_AVATAR_HIRES_WIDTH,
                MARKET_AVATAR_HIRES_HEIGHT,
            )
        })
}

/// Resolve the complete source-authored three-pose stand1 breathing cycle.
pub fn market_avatar_stand_animation(pack: &Pack) -> Option<&Sprite> {
    pack.animation("market_avatar_stand_hires")
        .filter(|animation| {
            complete_fixed_canvas(
                animation,
                MARKET_MAX_AGENTS * MARKET_STAND_FRAMES,
                MARKET_AVATAR_HIRES_WIDTH,
                MARKET_AVATAR_HIRES_HEIGHT,
            )
        })
}

/// Resolve the complete source-authored two-pose ladder cycle.
pub fn market_avatar_climb_animation(pack: &Pack) -> Option<&Sprite> {
    pack.animation("market_avatar_climb_hires")
        .filter(|animation| {
            complete_fixed_canvas(
                animation,
                MARKET_MAX_AGENTS * MARKET_CLIMB_FRAMES,
                MARKET_AVATAR_HIRES_WIDTH,
                MARKET_AVATAR_HIRES_HEIGHT,
            )
        })
}

/// Resolve the complete source-authored three-pose stand2 cycle.
pub fn market_avatar_stand2_animation(pack: &Pack) -> Option<&Sprite> {
    pack.animation("market_avatar_stand2_hires")
        .filter(|animation| {
            complete_fixed_canvas(
                animation,
                MARKET_MAX_AGENTS * MARKET_STAND2_FRAMES,
                MARKET_AVATAR_HIRES_WIDTH,
                MARKET_AVATAR_HIRES_HEIGHT,
            )
        })
}

/// Resolve one source-authored sit pose for every paperdoll.
pub fn market_avatar_sit_animation(pack: &Pack) -> Option<&Sprite> {
    pack.animation("market_avatar_sit_hires")
        .filter(|animation| {
            complete_fixed_canvas(
                animation,
                MARKET_MAX_AGENTS * MARKET_SIT_FRAMES,
                MARKET_AVATAR_HIRES_WIDTH,
                MARKET_AVATAR_HIRES_HEIGHT,
            )
        })
}

/// Resolve the complete source-authored three-pose alert cycle.
pub fn market_avatar_alert_animation(pack: &Pack) -> Option<&Sprite> {
    pack.animation("market_avatar_alert_hires")
        .filter(|animation| {
            complete_fixed_canvas(
                animation,
                MARKET_MAX_AGENTS * MARKET_ALERT_FRAMES,
                MARKET_AVATAR_HIRES_WIDTH,
                MARKET_AVATAR_HIRES_HEIGHT,
            )
        })
}

/// Resolve the complete local-only three-pose training attack cycle.
///
/// Public packs are allowed to omit this NEXON-derived local cache. An absent,
/// partial or wrong-sized cycle falls back to the existing alert/idle action in
/// the training painter instead of hiding the adventurer.
pub fn market_avatar_attack_animation(pack: &Pack) -> Option<&Sprite> {
    pack.animation("training_avatar_attack_hires")
        .filter(|animation| {
            complete_fixed_canvas(
                animation,
                MARKET_MAX_AGENTS * MARKET_ATTACK_FRAMES,
                MARKET_AVATAR_ATTACK_HIRES_WIDTH,
                MARKET_AVATAR_ATTACK_HIRES_HEIGHT,
            )
        })
}

/// Whether a pack carries either complete fixed-canvas paperdoll set.
///
/// Partial or differently-sized sets degrade to the existing `standing`
/// merchant so the pixel painter and floating overlay can never disagree about
/// the character's head anchor.
pub fn supports_market_avatars(pack: &Pack) -> bool {
    market_avatar_animation(pack).is_some()
}

/// Visual depth band for a free-market standing slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MarketLayer {
    /// Upper/back platform.
    Back,
    /// Central platform.
    Middle,
    /// Lower/front platform.
    Front,
}

/// One resolved free-market position in the renderer's internal pixel space.
///
/// `anchor_px` is the authored resting position. [`resolve_market_actor`]
/// derives both the moving character and label anchors from it each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MarketSlot {
    /// Central-priority slot index (`0..MARKET_MAX_AGENTS`).
    pub index: usize,
    /// Visual depth band containing the slot.
    pub layer: MarketLayer,
    /// Character sprite's top-left anchor relative to the supplied viewport, in buffer pixels.
    pub anchor_px: Point,
}

/// One live agent paired with its authoritative free-market slot.
///
/// `slot.anchor_px` is the legacy merchant target and `avatar_anchor_px` is the
/// feet-aligned paperdoll target. Both are fed into the shared per-frame motion
/// resolver instead of maintaining parallel painter/overlay coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MarketPlacement {
    /// Stable agent identity.
    pub agent_id: AgentId,
    /// Shared character/overlay slot.
    pub slot: MarketSlot,
    /// Stable paperdoll and fake-name identity derived from the immutable desk.
    ///
    /// Slot and appearance both prefer this immutable desk identity, so a
    /// surviving merchant never teleports to another platform when a peer exits.
    pub appearance_index: usize,
    /// Top-left of the logical paperdoll rectangle, feet-aligned to `slot.anchor_px`.
    pub avatar_anchor_px: Point,
}

/// Per-frame market position shared by the character painter and shop-card overlay.
///
/// Keeping both anchors in one value prevents the card from lagging behind a
/// merchant during entry/exit motion. `label_anchor_px` is expressed as the
/// nominal top-left of the standard 8px character: the floating card painter
/// adds half that width when centring the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketActorFrame {
    /// Animated top-left of the character canvas in scene-buffer pixels.
    pub sprite_anchor_px: Point,
    /// Animated nominal anchor consumed by the existing label-card painter.
    pub label_anchor_px: Point,
    /// Source-authored pose selected by the route or current activity state.
    pub pose: MarketActorPose,
    /// True only while an active merchant has finished its alert action.
    pub stall_open: bool,
    foot_px: Point,
}

impl MarketActorFrame {
    /// Feet-aligned route authority shared by walking, climbing and stalls.
    pub fn foot_px(self) -> Point {
        self.foot_px
    }

    pub(crate) fn command_effect_anchor(self, paperdoll: bool, scale: u16) -> Point {
        let authored_height = if paperdoll {
            MARKET_AVATAR_HEIGHT
        } else {
            REFERENCE_MERCHANT_HEIGHT
        };
        let left_shift = if paperdoll { 6u16 } else { 2u16 }.saturating_mul(scale);
        Point {
            x: self.foot_px.x.saturating_sub(left_shift),
            y: self
                .foot_px
                .y
                .saturating_sub(authored_height.saturating_mul(scale) * 4 / 5),
        }
    }
}

fn command_detail(detail: Option<&str>) -> bool {
    let token = detail
        .unwrap_or("")
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(token.as_str(), "bash" | "exec" | "command" | "shell")
}

/// Elapsed time in the one-shot command-success visual, if this agent is
/// currently at the start of a command tool. Codex Desktop reports the real
/// tool as `exec` (`ToolKind::Other`), while hook-based sources usually report
/// `Bash`; both are the same user-visible action here.
pub(crate) fn market_command_success_elapsed(agent: &AgentSlot, now: SystemTime) -> Option<u64> {
    let ActivityState::Active { detail, kind, .. } = &agent.state else {
        return None;
    };
    if *kind != ToolKind::Bash && !command_detail(detail.as_deref()) {
        return None;
    }
    let elapsed = now.duration_since(agent.state_started_at).ok()?.as_millis();
    let elapsed = elapsed.min(u128::from(u64::MAX)) as u64;
    (elapsed < MARKET_COMMAND_SUCCESS_MS).then_some(elapsed)
}

/// Elapsed time in the one-shot normal-turn completion visual for one actor.
///
/// The reducer owns the completion edge; ordinary tool ends, waiting, aborts,
/// and stale idleness never populate it. Requiring a still-live slot makes a
/// stale projected timestamp fail quiet.
pub(crate) fn market_turn_completion_elapsed(
    scene: &SceneState,
    agent_id: AgentId,
    now: SystemTime,
) -> Option<u64> {
    scene.agents.contains_key(&agent_id).then_some(())?;
    let completed_at = scene.last_turn_completed_at(agent_id)?;
    let elapsed = now.duration_since(completed_at).ok()?.as_millis();
    let elapsed = elapsed.min(u128::from(u64::MAX)) as u64;
    (elapsed < MARKET_TURN_COMPLETE_MS).then_some(elapsed)
}

/// Source-authored pose selected for a merchant at one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketActorPose {
    /// Three-pose stand1 ZigzagLoop (0, 1, 2, 1).
    Stand { frame_index: usize },
    /// Three-pose stand2 ZigzagLoop (0, 1, 2, 1).
    Stand2 { frame_index: usize },
    /// Single source-authored sit pose.
    Sit,
    /// One three-pose alert ZigzagLoop on the first active arrival.
    Alert { frame_index: usize },
    /// Four-pose walk1 cycle.
    Walk(MarketWalkFrame),
    /// Two-pose ladder cycle.
    Climb { frame_index: usize },
}

/// Horizontal orientation of a moving market paperdoll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketFacing {
    /// Source artwork faces toward decreasing x.
    Left,
    /// Source artwork faces toward increasing x.
    Right,
}

/// One source-authored pose and orientation in the four-frame market walk cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketWalkFrame {
    /// Zero-based pose index within one merchant's four-frame cycle.
    pub frame_index: usize,
    /// Direction of the merchant's actual horizontal travel.
    pub facing: MarketFacing,
}

/// Geometry and clock shared by the market character and overlay passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketFrameContext {
    pub viewport: Bounds,
    pub now: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarketActorGeometry {
    target_foot: Point,
    layer: MarketLayer,
    display_width: u16,
    display_height: u16,
    viewport: Bounds,
    phase_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarketMotionFrame {
    foot_px: Point,
    pose: MarketActorPose,
    stall_open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarketIdleTrack {
    entry_total: u64,
    turnaround_progress: u64,
    travel_ms: u64,
    cycle_elapsed: u64,
    state_elapsed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketIdlePhase {
    AtStall(u64),
    WalkingOut(u64),
    Away(u64),
    WalkingBack(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketIdleLeg {
    Out,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketRouteKind {
    Portal,
    Walk,
    Climb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarketRouteSegment {
    from: Point,
    to: Point,
    duration_ms: u64,
    kind: MarketRouteKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarketRouteSample {
    foot_px: Point,
    pose: MarketActorPose,
    complete: bool,
}

/// Integer scale of market-authored sprites for a viewport height.
///
/// The 240x160 reference plate is scale 1; the default 720x480 floating scene
/// is scale 3. This is shared rather than duplicated in the painter so label
/// centring uses the exact same displayed paperdoll width.
pub fn market_sprite_scale(viewport_height: u16) -> u16 {
    ((u32::from(viewport_height) + REFERENCE_VIEWPORT_HEIGHT / 2) / REFERENCE_VIEWPORT_HEIGHT)
        .clamp(1, 8) as u16
}

/// Resolve an animated paperdoll and its card anchor for this frame.
///
/// Merchants enter from and exit toward the closest horizontal edge. Moving
/// paperdolls expose the real four-pose gait while their canvas and shop card
/// stay on one stable baseline.
/// Returning `None` after the exit completes hides both character and card
/// during the reducer's remaining exit grace period.
pub fn resolve_market_paperdoll(
    agent: &AgentSlot,
    placement: MarketPlacement,
    frame: MarketFrameContext,
) -> Option<MarketActorFrame> {
    resolve_market_actor(
        agent,
        placement.slot.layer,
        placement.avatar_anchor_px,
        frame,
        MARKET_AVATAR_WIDTH,
        MARKET_AVATAR_HEIGHT,
        placement.appearance_index,
    )
}

/// Resolve the legacy 8px merchant fallback and its card anchor for this frame.
pub fn resolve_market_merchant(
    agent: &AgentSlot,
    placement: MarketPlacement,
    frame: MarketFrameContext,
) -> Option<MarketActorFrame> {
    resolve_market_actor(
        agent,
        placement.slot.layer,
        placement.slot.anchor_px,
        frame,
        REFERENCE_MERCHANT_WIDTH,
        REFERENCE_MERCHANT_HEIGHT,
        placement.appearance_index,
    )
}

fn resolve_market_actor(
    agent: &AgentSlot,
    layer: MarketLayer,
    target: Point,
    frame: MarketFrameContext,
    authored_width: u16,
    authored_height: u16,
    phase_index: usize,
) -> Option<MarketActorFrame> {
    let geometry = market_actor_geometry(
        target,
        layer,
        frame.viewport,
        authored_width,
        authored_height,
        phase_index,
    );
    let motion = resolve_market_motion(agent, geometry, frame.now)?;
    Some(market_actor_frame(geometry, motion))
}

fn market_actor_geometry(
    target: Point,
    layer: MarketLayer,
    viewport: Bounds,
    authored_width: u16,
    authored_height: u16,
    phase_index: usize,
) -> MarketActorGeometry {
    let scale = market_sprite_scale(viewport.height);
    let display_width = authored_width.saturating_mul(scale);
    let display_height = authored_height.saturating_mul(scale);
    MarketActorGeometry {
        target_foot: Point {
            x: target.x.saturating_add(display_width / 2),
            y: target.y.saturating_add(display_height),
        },
        layer,
        display_width,
        display_height,
        viewport,
        phase_index,
    }
}

fn resolve_market_motion(
    agent: &AgentSlot,
    geometry: MarketActorGeometry,
    now: SystemTime,
) -> Option<MarketMotionFrame> {
    let route = market_entry_route(geometry);
    let entry_total = route_duration(&route);
    match agent.exiting_at {
        Some(exiting_at) => {
            resolve_exit_motion(agent, geometry, &route, entry_total, exiting_at, now)
        }
        None => Some(resolve_entry_motion(
            agent,
            geometry,
            &route,
            entry_total,
            now,
        )),
    }
}

fn resolve_entry_motion(
    agent: &AgentSlot,
    geometry: MarketActorGeometry,
    route: &[MarketRouteSegment],
    entry_total: u64,
    now: SystemTime,
) -> MarketMotionFrame {
    let step_elapsed = elapsed_ms(now, agent.created_at);
    if step_elapsed >= entry_total {
        return resolve_settled_motion(agent, geometry, route, entry_total, now);
    }
    let sample = sample_market_route(
        route,
        step_elapsed,
        step_elapsed,
        false,
        geometry.phase_index,
        now,
    );
    MarketMotionFrame {
        foot_px: sample.foot_px,
        pose: sample.pose,
        stall_open: false,
    }
}

fn resolve_settled_motion(
    agent: &AgentSlot,
    geometry: MarketActorGeometry,
    route: &[MarketRouteSegment],
    entry_total: u64,
    now: SystemTime,
) -> MarketMotionFrame {
    let arrival_at = agent
        .created_at
        .checked_add(Duration::from_millis(entry_total))
        .unwrap_or(agent.created_at);
    let state_started_at = agent.state_started_at.max(arrival_at);
    let state_elapsed = elapsed_ms(now, state_started_at);
    let active_on_arrival = agent.state_started_at <= arrival_at;

    match &agent.state {
        ActivityState::Active { .. }
            if active_on_arrival && state_elapsed < MARKET_ALERT_CYCLE_MS =>
        {
            MarketMotionFrame {
                foot_px: geometry.target_foot,
                pose: alert_pose(state_elapsed),
                stall_open: false,
            }
        }
        ActivityState::Active { .. } => MarketMotionFrame {
            foot_px: geometry.target_foot,
            pose: stand_pose(now, geometry.phase_index),
            stall_open: true,
        },
        ActivityState::Waiting { .. } => MarketMotionFrame {
            foot_px: geometry.target_foot,
            pose: MarketActorPose::Sit,
            stall_open: false,
        },
        ActivityState::Idle => resolve_idle_motion(geometry, route, state_elapsed, now),
    }
}

fn resolve_idle_motion(
    geometry: MarketActorGeometry,
    route: &[MarketRouteSegment],
    state_elapsed: u64,
    now: SystemTime,
) -> MarketMotionFrame {
    let track = market_idle_track(route, state_elapsed);
    match market_idle_phase(track) {
        MarketIdlePhase::AtStall(elapsed) => idle_stand_motion(geometry.target_foot, elapsed),
        MarketIdlePhase::WalkingOut(elapsed) => {
            idle_route_motion(geometry, route, track, MarketIdleLeg::Out, elapsed, now)
        }
        MarketIdlePhase::Away(elapsed) => idle_stand_motion(
            market_route_foot(route, track.turnaround_progress, geometry, now),
            elapsed,
        ),
        MarketIdlePhase::WalkingBack(elapsed) => {
            idle_route_motion(geometry, route, track, MarketIdleLeg::Back, elapsed, now)
        }
    }
}

fn market_idle_track(route: &[MarketRouteSegment], state_elapsed: u64) -> MarketIdleTrack {
    let entry_total = route_duration(route);
    let turnaround_progress = market_idle_turnaround_progress(route);
    let travel_ms = entry_total.saturating_sub(turnaround_progress).max(1);
    let cycle_ms = MARKET_IDLE_STALL_PAUSE_MS
        .saturating_add(travel_ms)
        .saturating_add(MARKET_IDLE_AWAY_PAUSE_MS)
        .saturating_add(travel_ms);
    MarketIdleTrack {
        entry_total,
        turnaround_progress,
        travel_ms,
        cycle_elapsed: state_elapsed % cycle_ms,
        state_elapsed,
    }
}

fn market_idle_phase(track: MarketIdleTrack) -> MarketIdlePhase {
    let mut elapsed = track.cycle_elapsed;
    if elapsed < MARKET_IDLE_STALL_PAUSE_MS {
        return MarketIdlePhase::AtStall(elapsed);
    }
    elapsed -= MARKET_IDLE_STALL_PAUSE_MS;
    if elapsed < track.travel_ms {
        return MarketIdlePhase::WalkingOut(elapsed);
    }
    elapsed -= track.travel_ms;
    if elapsed < MARKET_IDLE_AWAY_PAUSE_MS {
        return MarketIdlePhase::Away(elapsed);
    }
    MarketIdlePhase::WalkingBack(elapsed - MARKET_IDLE_AWAY_PAUSE_MS)
}

fn idle_stand_motion(foot_px: Point, elapsed: u64) -> MarketMotionFrame {
    MarketMotionFrame {
        foot_px,
        pose: stand2_pose(elapsed),
        stall_open: false,
    }
}

fn market_idle_turnaround_progress(route: &[MarketRouteSegment]) -> u64 {
    let turnaround_index = route
        .iter()
        .rposition(|segment| segment.kind == MarketRouteKind::Climb)
        .or_else(|| {
            route
                .iter()
                .rposition(|segment| segment.kind == MarketRouteKind::Walk)
        });
    turnaround_index.map_or_else(
        || route_duration(route),
        |index| {
            route[..index]
                .iter()
                .map(|segment| segment.duration_ms)
                .sum()
        },
    )
}

fn market_route_foot(
    route: &[MarketRouteSegment],
    progress: u64,
    geometry: MarketActorGeometry,
    now: SystemTime,
) -> Point {
    sample_market_route(route, progress, progress, false, geometry.phase_index, now).foot_px
}

fn idle_route_motion(
    geometry: MarketActorGeometry,
    route: &[MarketRouteSegment],
    track: MarketIdleTrack,
    leg: MarketIdleLeg,
    elapsed: u64,
    now: SystemTime,
) -> MarketMotionFrame {
    let (progress, reverse) = match leg {
        MarketIdleLeg::Out => (
            track
                .entry_total
                .saturating_sub(elapsed.min(track.travel_ms)),
            true,
        ),
        MarketIdleLeg::Back => (
            track
                .turnaround_progress
                .saturating_add(elapsed.min(track.travel_ms)),
            false,
        ),
    };
    let sample = sample_market_route(
        route,
        progress,
        track.state_elapsed,
        reverse,
        geometry.phase_index,
        now,
    );
    MarketMotionFrame {
        foot_px: sample.foot_px,
        pose: sample.pose,
        stall_open: false,
    }
}

fn resolve_exit_motion(
    agent: &AgentSlot,
    geometry: MarketActorGeometry,
    route: &[MarketRouteSegment],
    entry_total: u64,
    exiting_at: SystemTime,
    now: SystemTime,
) -> Option<MarketMotionFrame> {
    let exit_elapsed = elapsed_ms(now, exiting_at);
    let raw_entry_progress = elapsed_ms(exiting_at, agent.created_at).min(entry_total);
    let (entry_progress, exit_start_foot) = if raw_entry_progress >= entry_total {
        let settled = resolve_settled_motion(agent, geometry, route, entry_total, exiting_at);
        (
            market_route_progress_at_foot(route, settled.foot_px),
            settled.foot_px,
        )
    } else {
        let sample = sample_market_route(
            route,
            raw_entry_progress,
            raw_entry_progress,
            false,
            geometry.phase_index,
            exiting_at,
        );
        (raw_entry_progress, sample.foot_px)
    };
    let exit_total = entry_progress.saturating_add(MARKET_PORTAL_OUT_MS);
    if exit_elapsed >= exit_total.min(u64::from(MARKET_MAX_EXIT_MS)) {
        return None;
    }
    if exit_elapsed >= entry_progress {
        return Some(MarketMotionFrame {
            foot_px: route
                .first()
                .map_or(geometry.target_foot, |segment| segment.from),
            pose: stand_pose(now, geometry.phase_index),
            stall_open: false,
        });
    }
    let rewind_at = entry_progress
        .saturating_sub(exit_elapsed)
        .saturating_sub(1);
    let mut sample = sample_market_route(
        route,
        rewind_at,
        exit_elapsed,
        true,
        geometry.phase_index,
        now,
    );
    if exit_elapsed == 0 {
        sample.foot_px = exit_start_foot;
    }
    Some(MarketMotionFrame {
        foot_px: sample.foot_px,
        pose: sample.pose,
        stall_open: false,
    })
}

fn market_route_progress_at_foot(route: &[MarketRouteSegment], foot: Point) -> u64 {
    let mut segment_started_at = 0_u64;
    let mut best = None;
    for segment in route {
        let axis_progress = if segment.from.x == segment.to.x && foot.x == segment.from.x {
            let min_y = segment.from.y.min(segment.to.y);
            let max_y = segment.from.y.max(segment.to.y);
            (min_y..=max_y).contains(&foot.y).then(|| {
                (
                    u64::from(segment.from.y.abs_diff(segment.to.y)),
                    u64::from(segment.from.y.abs_diff(foot.y)),
                )
            })
        } else if segment.from.y == segment.to.y && foot.y == segment.from.y {
            let min_x = segment.from.x.min(segment.to.x);
            let max_x = segment.from.x.max(segment.to.x);
            (min_x..=max_x).contains(&foot.x).then(|| {
                (
                    u64::from(segment.from.x.abs_diff(segment.to.x)),
                    u64::from(segment.from.x.abs_diff(foot.x)),
                )
            })
        } else {
            None
        };
        if let Some((distance, travelled)) = axis_progress {
            let local = segment
                .duration_ms
                .saturating_mul(travelled)
                .checked_div(distance)
                .unwrap_or(segment.duration_ms);
            let candidate = segment_started_at.saturating_add(local);
            best = Some(best.map_or(candidate, |current: u64| current.max(candidate)));
        }
        segment_started_at = segment_started_at.saturating_add(segment.duration_ms);
    }
    best.unwrap_or(segment_started_at)
}

fn market_walk_frame_index(elapsed: u64) -> usize {
    ((elapsed / MARKET_WALK_FRAME_MS) as usize) % MARKET_WALK_FRAMES
}

fn market_facing(from_x: u16, to_x: u16) -> MarketFacing {
    if to_x < from_x {
        MarketFacing::Left
    } else {
        MarketFacing::Right
    }
}

fn stand_pose(now: SystemTime, phase_index: usize) -> MarketActorPose {
    const ZIGZAG: [usize; 4] = [0, 1, 2, 1];
    let epoch = elapsed_ms(now, SystemTime::UNIX_EPOCH);
    let phase_ms = (phase_index as u64 * 137) % (MARKET_STAND_FRAME_MS * 4);
    let index = ((epoch % (MARKET_STAND_FRAME_MS * 4) + phase_ms) / MARKET_STAND_FRAME_MS) as usize
        % ZIGZAG.len();
    MarketActorPose::Stand {
        frame_index: ZIGZAG[index],
    }
}

fn stand2_pose(elapsed: u64) -> MarketActorPose {
    const ZIGZAG: [usize; 4] = [0, 1, 2, 1];
    let index = ((elapsed % MARKET_ALERT_CYCLE_MS) / MARKET_STATUS_FRAME_MS) as usize;
    MarketActorPose::Stand2 {
        frame_index: ZIGZAG[index],
    }
}

fn alert_pose(elapsed: u64) -> MarketActorPose {
    const ZIGZAG: [usize; 4] = [0, 1, 2, 1];
    let index = ((elapsed % MARKET_ALERT_CYCLE_MS) / MARKET_STATUS_FRAME_MS) as usize;
    MarketActorPose::Alert {
        frame_index: ZIGZAG[index],
    }
}

fn market_actor_frame(
    geometry: MarketActorGeometry,
    motion: MarketMotionFrame,
) -> MarketActorFrame {
    let sprite_anchor_px = Point {
        x: motion.foot_px.x.saturating_sub(geometry.display_width / 2),
        y: motion.foot_px.y.saturating_sub(geometry.display_height),
    };
    let label_anchor_px = Point {
        x: motion.foot_px.x.saturating_sub(CHARACTER_SPRITE_W / 2),
        y: sprite_anchor_px.y,
    };
    MarketActorFrame {
        sprite_anchor_px,
        label_anchor_px,
        pose: motion.pose,
        stall_open: motion.stall_open,
        foot_px: motion.foot_px,
    }
}

fn market_entry_route(geometry: MarketActorGeometry) -> Vec<MarketRouteSegment> {
    let portal = market_route_point(geometry.viewport, 942, 869);
    let approach = market_route_point(geometry.viewport, 869, 869);
    let lower_bottom = market_route_point(geometry.viewport, 582, 869);
    let lower_top = market_route_point(geometry.viewport, 582, 565);
    let upper_bottom = market_route_point(geometry.viewport, 392, 565);
    let upper_top = market_route_point(geometry.viewport, 392, 275);
    let mut route = vec![MarketRouteSegment {
        from: portal,
        to: portal,
        duration_ms: MARKET_PORTAL_IN_MS,
        kind: MarketRouteKind::Portal,
    }];
    push_market_walk(&mut route, portal, approach, geometry.viewport);
    match geometry.layer {
        MarketLayer::Front => {
            push_market_walk(
                &mut route,
                approach,
                geometry.target_foot,
                geometry.viewport,
            );
        }
        MarketLayer::Middle => {
            push_market_walk(&mut route, approach, lower_bottom, geometry.viewport);
            push_market_climb(&mut route, lower_bottom, lower_top, geometry.viewport);
            push_market_walk(
                &mut route,
                lower_top,
                geometry.target_foot,
                geometry.viewport,
            );
        }
        MarketLayer::Back => {
            push_market_walk(&mut route, approach, lower_bottom, geometry.viewport);
            push_market_climb(&mut route, lower_bottom, lower_top, geometry.viewport);
            push_market_walk(&mut route, lower_top, upper_bottom, geometry.viewport);
            push_market_climb(&mut route, upper_bottom, upper_top, geometry.viewport);
            push_market_walk(
                &mut route,
                upper_top,
                geometry.target_foot,
                geometry.viewport,
            );
        }
    }
    route
}

fn market_route_point(viewport: Bounds, x_per_mille: u16, y_per_mille: u16) -> Point {
    Point {
        x: resolve_axis(viewport.x, viewport.width, x_per_mille),
        y: resolve_axis(viewport.y, viewport.height, y_per_mille),
    }
}

fn push_market_walk(route: &mut Vec<MarketRouteSegment>, from: Point, to: Point, viewport: Bounds) {
    if from == to {
        return;
    }
    let reference_distance = market_reference_distance(from, to, viewport);
    let duration_ms =
        reference_distance.saturating_mul(1_000) / MARKET_WALK_REFERENCE_PX_PER_SECOND;
    route.push(MarketRouteSegment {
        from,
        to,
        duration_ms: duration_ms.max(1),
        kind: MarketRouteKind::Walk,
    });
}

fn push_market_climb(
    route: &mut Vec<MarketRouteSegment>,
    from: Point,
    to: Point,
    viewport: Bounds,
) {
    if from == to {
        return;
    }
    let reference_distance = market_reference_distance(from, to, viewport);
    let duration_ms =
        reference_distance.saturating_mul(1_000) / MARKET_CLIMB_REFERENCE_PX_PER_SECOND;
    route.push(MarketRouteSegment {
        from,
        to,
        duration_ms: duration_ms.max(1),
        kind: MarketRouteKind::Climb,
    });
}

fn market_reference_distance(from: Point, to: Point, viewport: Bounds) -> u64 {
    let reference_dx = u64::from(from.x.abs_diff(to.x)) * 720 / u64::from(viewport.width.max(1));
    let reference_dy = u64::from(from.y.abs_diff(to.y)) * 480 / u64::from(viewport.height.max(1));
    reference_dx + reference_dy
}

fn route_duration(route: &[MarketRouteSegment]) -> u64 {
    route.iter().map(|segment| segment.duration_ms).sum()
}

fn sample_market_route(
    route: &[MarketRouteSegment],
    route_elapsed: u64,
    animation_elapsed: u64,
    reverse: bool,
    phase_index: usize,
    now: SystemTime,
) -> MarketRouteSample {
    let total = route_duration(route);
    if route_elapsed >= total {
        return MarketRouteSample {
            foot_px: route
                .last()
                .map_or(Point { x: 0, y: 0 }, |segment| segment.to),
            pose: stand_pose(now, phase_index),
            complete: true,
        };
    }
    let mut remaining = route_elapsed;
    for segment in route {
        if remaining >= segment.duration_ms {
            remaining -= segment.duration_ms;
            continue;
        }
        let foot_px = Point {
            x: lerp_axis_ms(segment.from.x, segment.to.x, remaining, segment.duration_ms),
            y: lerp_axis_ms(segment.from.y, segment.to.y, remaining, segment.duration_ms),
        };
        let pose = match segment.kind {
            MarketRouteKind::Portal => stand_pose(now, phase_index),
            MarketRouteKind::Walk => {
                let facing = if reverse {
                    market_facing(segment.to.x, segment.from.x)
                } else {
                    market_facing(segment.from.x, segment.to.x)
                };
                MarketActorPose::Walk(MarketWalkFrame {
                    frame_index: market_walk_frame_index(animation_elapsed),
                    facing,
                })
            }
            MarketRouteKind::Climb => MarketActorPose::Climb {
                frame_index: ((animation_elapsed / MARKET_CLIMB_FRAME_MS) as usize)
                    % MARKET_CLIMB_FRAMES,
            },
        };
        return MarketRouteSample {
            foot_px,
            pose,
            complete: false,
        };
    }
    MarketRouteSample {
        foot_px: route
            .last()
            .map_or(Point { x: 0, y: 0 }, |segment| segment.to),
        pose: stand_pose(now, phase_index),
        complete: true,
    }
}

fn lerp_axis_ms(from: u16, to: u16, elapsed: u64, duration: u64) -> u16 {
    if duration == 0 {
        return to;
    }
    let from = i64::from(from);
    let delta = i64::from(to) - from;
    let value = from + delta * elapsed.min(duration) as i64 / duration as i64;
    value.clamp(0, i64::from(u16::MAX)) as u16
}

#[derive(Clone, Copy)]
struct RelativeSlot {
    layer: MarketLayer,
    x_per_mille: u16,
    y_per_mille: u16,
}

// Platform-specific anchors measured on the 240x160 clean plate, ordered by
// horizontal distance from its x=120 visual centre. The order intentionally
// spans Front/Middle/Back before expanding toward either edge; the platforms
// themselves are asymmetric, so forcing mirrored pairs would detach feet from
// the authored standing surfaces.
const RELATIVE_SLOTS: [RelativeSlot; MARKET_MAX_AGENTS] = [
    RelativeSlot {
        layer: MarketLayer::Front,
        x_per_mille: 500,
        y_per_mille: 795,
    },
    RelativeSlot {
        layer: MarketLayer::Middle,
        x_per_mille: 521,
        y_per_mille: 490,
    },
    RelativeSlot {
        layer: MarketLayer::Back,
        x_per_mille: 459,
        y_per_mille: 200,
    },
    RelativeSlot {
        layer: MarketLayer::Back,
        x_per_mille: 667,
        y_per_mille: 200,
    },
    RelativeSlot {
        layer: MarketLayer::Middle,
        x_per_mille: 313,
        y_per_mille: 490,
    },
    RelativeSlot {
        layer: MarketLayer::Front,
        x_per_mille: 250,
        y_per_mille: 795,
    },
    RelativeSlot {
        layer: MarketLayer::Back,
        x_per_mille: 834,
        y_per_mille: 200,
    },
    RelativeSlot {
        layer: MarketLayer::Middle,
        x_per_mille: 125,
        y_per_mille: 490,
    },
];

/// Resolve up to eight central-priority free-market slots inside `viewport`.
///
/// The template is stored as per-mille coordinates and resolved against the
/// viewport extent (clamped to its final pixel), so the same authority adapts to TUI,
/// floating, and web buffers. An empty viewport produces no slots.
pub fn market_slots(viewport: Bounds, agent_count: usize) -> Vec<MarketSlot> {
    if viewport.width == 0 || viewport.height == 0 {
        return Vec::new();
    }

    RELATIVE_SLOTS
        .iter()
        .take(agent_count.min(MARKET_MAX_AGENTS))
        .enumerate()
        .map(|(index, spec)| MarketSlot {
            index,
            layer: spec.layer,
            anchor_px: Point {
                x: resolve_axis(viewport.x, viewport.width, spec.x_per_mille),
                y: resolve_axis(viewport.y, viewport.height, spec.y_per_mille),
            },
        })
        .collect()
}

/// Assign the scene's first eight agents to stable desk-derived market slots.
///
/// Allocation follows `AgentSlot::desk_index`, the reducer's immutable lifetime
/// order, with `AgentId` as a deterministic tie-breaker for synthetic scenes.
/// This avoids character reshuffles caused by map-key order.
pub fn build_market_placements(scene: &SceneState, viewport: Bounds) -> Vec<MarketPlacement> {
    let mut agents = scene.agents.values().collect::<Vec<_>>();
    agents.sort_by_key(|agent| (agent.desk_index, agent.agent_id));
    let slots = market_slots(viewport, MARKET_MAX_AGENTS);
    if slots.len() != MARKET_MAX_AGENTS {
        return Vec::new();
    }
    let mut used = [false; MARKET_MAX_AGENTS];

    agents
        .into_iter()
        .take(MARKET_MAX_AGENTS)
        .filter_map(|agent| {
            let preferred = agent.desk_index.0 % MARKET_MAX_AGENTS;
            let slot_index = (0..MARKET_MAX_AGENTS)
                .map(|offset| (preferred + offset) % MARKET_MAX_AGENTS)
                .find(|index| !used[*index])?;
            used[slot_index] = true;
            let slot = slots[slot_index];
            let avatar_anchor_px = paperdoll_anchor(slot.anchor_px, viewport.height);
            Some(MarketPlacement {
                agent_id: agent.agent_id,
                slot,
                appearance_index: agent.desk_index.0 % MARKET_MAX_AGENTS,
                avatar_anchor_px,
            })
        })
        .collect()
}

/// Build free-market name badges from the exact placements used by characters.
///
/// Text collision suffixes, truncation budget, activity tones, exit priority,
/// and hover state match the existing office overlay model. Placements whose
/// agent disappeared from `scene` are skipped, making a stale frame fail quiet.
pub fn build_market_overlay(
    scene: &SceneState,
    placements: &[MarketPlacement],
    hovered: Option<AgentId>,
    frame: MarketFrameContext,
) -> Vec<LabelElement> {
    build_market_overlay_at(MarketOverlayRequest {
        scene,
        placements,
        hovered,
        frame,
        actor_shape: MarketActorShape::Merchant,
    })
}

/// Build the same market labels with their cards attached to paperdoll heads.
pub fn build_market_avatar_overlay(
    scene: &SceneState,
    placements: &[MarketPlacement],
    hovered: Option<AgentId>,
    frame: MarketFrameContext,
) -> Vec<LabelElement> {
    build_market_overlay_at(MarketOverlayRequest {
        scene,
        placements,
        hovered,
        frame,
        actor_shape: MarketActorShape::Paperdoll,
    })
}

#[derive(Clone, Copy)]
enum MarketActorShape {
    Merchant,
    Paperdoll,
}

/// Build settled paperdoll fake IDs inside the fixed stall nameplates.
///
/// A travelling character has no stall-attached ID yet. This prevents a text
/// strip from following the merchant while the authored wooden frame remains at
/// the destination slot.
pub fn build_market_avatar_player_ids(
    scene: &SceneState,
    placements: &[MarketPlacement],
    frame: MarketFrameContext,
) -> Vec<LabelElement> {
    build_market_player_ids_at(scene, placements, frame, MarketActorShape::Paperdoll)
}

/// Build settled fallback-merchant fake IDs inside the fixed stall nameplates.
pub fn build_market_merchant_player_ids(
    scene: &SceneState,
    placements: &[MarketPlacement],
    frame: MarketFrameContext,
) -> Vec<LabelElement> {
    build_market_player_ids_at(scene, placements, frame, MarketActorShape::Merchant)
}

fn build_market_player_ids_at(
    scene: &SceneState,
    placements: &[MarketPlacement],
    frame: MarketFrameContext,
    actor_shape: MarketActorShape,
) -> Vec<LabelElement> {
    let scale = market_sprite_scale(frame.viewport.height);
    placements
        .iter()
        .filter_map(|placement| {
            let agent = scene.agents.get(&placement.agent_id)?;
            if agent.exiting_at.is_some() {
                return None;
            }
            let (actor, authored_height) = match actor_shape {
                MarketActorShape::Merchant => (
                    resolve_market_merchant(agent, *placement, frame)?,
                    REFERENCE_MERCHANT_HEIGHT,
                ),
                MarketActorShape::Paperdoll => (
                    resolve_market_paperdoll(agent, *placement, frame)?,
                    MARKET_AVATAR_HEIGHT,
                ),
            };
            if !actor.stall_open {
                return None;
            }
            let stall_y = actor.sprite_anchor_px.y.saturating_add(
                authored_height
                    .saturating_sub(MARKET_STALL_FOOT_OVERLAP)
                    .saturating_mul(scale),
            );
            Some(LabelElement {
                anchor_px: Point {
                    x: actor.label_anchor_px.x,
                    y: stall_y.saturating_add(MARKET_STALL_NAMEPLATE_Y.saturating_mul(scale)),
                },
                text: MARKET_FAKE_PLAYER_IDS
                    [placement.appearance_index % MARKET_FAKE_PLAYER_IDS.len()]
                .to_owned(),
                tone: market_label_tone(agent),
                hovered: false,
                relation: None,
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
struct MarketOverlayRequest<'a> {
    scene: &'a SceneState,
    placements: &'a [MarketPlacement],
    hovered: Option<AgentId>,
    frame: MarketFrameContext,
    actor_shape: MarketActorShape,
}

fn build_market_overlay_at(request: MarketOverlayRequest<'_>) -> Vec<LabelElement> {
    let mut label_counts: HashMap<&str, usize> = HashMap::new();
    for agent in request.scene.agents.values() {
        *label_counts.entry(&agent.label).or_insert(0) += 1;
    }
    request
        .placements
        .iter()
        .filter_map(|placement| market_label_element(request, *placement, &label_counts))
        .collect()
}

fn market_label_element(
    request: MarketOverlayRequest<'_>,
    placement: MarketPlacement,
    label_counts: &HashMap<&str, usize>,
) -> Option<LabelElement> {
    const LABEL_BUDGET_PAD: u16 = 4;

    let agent = request.scene.agents.get(&placement.agent_id)?;
    let actor = match request.actor_shape {
        MarketActorShape::Merchant => resolve_market_merchant(agent, placement, request.frame),
        MarketActorShape::Paperdoll => resolve_market_paperdoll(agent, placement, request.frame),
    }?;
    if !actor.stall_open {
        return None;
    }
    let text = market_label_text(agent, label_counts, placement.appearance_index);
    Some(LabelElement {
        anchor_px: actor.label_anchor_px,
        text: truncate_label(&text, (crate::layout::DESK_W + LABEL_BUDGET_PAD) as usize)
            .into_owned(),
        tone: market_label_tone(agent),
        hovered: request.hovered == Some(agent.agent_id),
        relation: crate::maple_world::agent_relation(request.scene, agent.agent_id),
    })
}

fn market_label_text(
    agent: &AgentSlot,
    label_counts: &HashMap<&str, usize>,
    appearance_index: usize,
) -> String {
    const LABEL_SEP: char = '\u{b7}';

    let fake_id = MARKET_FAKE_PLAYER_IDS[appearance_index % MARKET_FAKE_PLAYER_IDS.len()];
    let task_title = agent
        .label
        .split_once(LABEL_SEP)
        .map_or(agent.label.as_ref(), |(_, title)| title);
    let needs_disambig = label_counts.get(&*agent.label).copied().unwrap_or(0) > 1
        && agent.session_id.chars().count() >= 4;
    if needs_disambig {
        format!(
            "{fake_id}{LABEL_SEP}{task_title}{LABEL_SEP}{}",
            disambig_suffix(&agent.session_id)
        )
    } else {
        format!("{fake_id}{LABEL_SEP}{task_title}")
    }
}

fn paperdoll_anchor(slot_anchor: Point, viewport_height: u16) -> Point {
    let scale = market_sprite_scale(viewport_height);
    let fallback_width = REFERENCE_MERCHANT_WIDTH.saturating_mul(scale);
    let fallback_height = REFERENCE_MERCHANT_HEIGHT.saturating_mul(scale);
    let avatar_width = MARKET_AVATAR_WIDTH.saturating_mul(scale);
    let avatar_height = MARKET_AVATAR_HEIGHT.saturating_mul(scale);
    let centre_x = slot_anchor.x.saturating_add(fallback_width / 2);
    let foot_y = slot_anchor.y.saturating_add(fallback_height);
    Point {
        x: centre_x.saturating_sub(avatar_width / 2),
        y: foot_y.saturating_sub(avatar_height),
    }
}

fn market_label_tone(agent: &AgentSlot) -> LabelTone {
    if agent.exiting_at.is_some() {
        return LabelTone::Exiting;
    }
    match &agent.state {
        ActivityState::Active { .. } => LabelTone::Active,
        ActivityState::Waiting { .. } => LabelTone::Waiting,
        ActivityState::Idle => LabelTone::Idle,
    }
}

fn resolve_axis(origin: u16, extent: u16, per_mille: u16) -> u16 {
    let offset =
        (u32::from(extent) * u32::from(per_mille) / 1000).min(u32::from(extent.saturating_sub(1)));
    origin.saturating_add(offset.min(u32::from(u16::MAX)) as u16)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use pixtuoid_core::source::{AgentEvent, Transport};
    use pixtuoid_core::{
        ActivityState, AgentId, AgentSlot, GlobalDeskIndex, Reducer, SceneState, ToolKind,
    };

    use super::{
        build_market_avatar_overlay, build_market_overlay, build_market_placements,
        market_command_success_elapsed, market_slots, market_turn_completion_elapsed,
        push_market_walk, resolve_market_paperdoll, MarketActorPose, MarketFacing,
        MarketFrameContext, MarketLayer, MARKET_AVATAR_HEIGHT, MARKET_AVATAR_WIDTH,
        MARKET_COMMAND_SUCCESS_MS, MARKET_MAX_AGENTS, MARKET_MAX_EXIT_MS, MARKET_TURN_COMPLETE_MS,
    };
    use crate::layout::{Bounds, Point};
    use crate::maple_world::MapleAgentRelation;
    use crate::overlay::{disambig_suffix, LabelTone};

    fn agent(
        id: AgentId,
        desk_index: usize,
        label: &str,
        session_id: &str,
        state: ActivityState,
        exiting: bool,
    ) -> AgentSlot {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        AgentSlot {
            agent_id: id,
            source: Arc::from("codex"),
            session_id: Arc::from(session_id),
            cwd: Arc::from(Path::new("/market")),
            label: label.into(),
            state,
            state_started_at: now,
            last_event_at: now,
            created_at: now - Duration::from_secs(5),
            exiting_at: exiting.then_some(now),
            pending_idle_at: None,
            desk_index: GlobalDeskIndex(desk_index),
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
    fn slots_are_central_first_three_layered_and_buffer_relative() {
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 240,
            height: 160,
        };

        for count in 0..=MARKET_MAX_AGENTS {
            assert_eq!(market_slots(viewport, count).len(), count);
        }
        let slots = market_slots(viewport, MARKET_MAX_AGENTS + 1);
        assert_eq!(slots.len(), MARKET_MAX_AGENTS);
        assert_eq!(
            slots.iter().map(|slot| slot.layer).collect::<Vec<_>>(),
            vec![
                MarketLayer::Front,
                MarketLayer::Middle,
                MarketLayer::Back,
                MarketLayer::Back,
                MarketLayer::Middle,
                MarketLayer::Front,
                MarketLayer::Back,
                MarketLayer::Middle,
            ]
        );
        assert_eq!(
            slots.iter().map(|slot| slot.anchor_px).collect::<Vec<_>>(),
            vec![
                Point { x: 120, y: 127 },
                Point { x: 125, y: 78 },
                Point { x: 110, y: 32 },
                Point { x: 160, y: 32 },
                Point { x: 75, y: 78 },
                Point { x: 60, y: 127 },
                Point { x: 200, y: 32 },
                Point { x: 30, y: 78 },
            ]
        );

        let distances = slots
            .iter()
            .map(|slot| slot.anchor_px.x.abs_diff(120))
            .collect::<Vec<_>>();
        assert!(
            distances.windows(2).all(|pair| pair[0] <= pair[1]),
            "slots must expand from the visual centre: {distances:?}"
        );
        assert!(slots.iter().all(|slot| {
            slot.anchor_px.x >= viewport.x
                && slot.anchor_px.x < viewport.x + viewport.width
                && slot.anchor_px.y >= viewport.y
                && slot.anchor_px.y < viewport.y + viewport.height
        }));

        let compact = market_slots(
            Bounds {
                x: 7,
                y: 9,
                width: 120,
                height: 80,
            },
            MARKET_MAX_AGENTS,
        );
        assert_eq!(compact[0].anchor_px, Point { x: 67, y: 72 });
        assert_eq!(compact[2].anchor_px, Point { x: 62, y: 25 });
        assert!(market_slots(
            Bounds {
                x: 0,
                y: 0,
                width: 0,
                height: 160,
            },
            1,
        )
        .is_empty());
    }

    #[test]
    fn codex_exec_and_bash_share_the_command_success_pulse() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        for (kind, detail) in [
            (ToolKind::Other, "exec"),
            (ToolKind::Other, "shell_command"),
            (ToolKind::Bash, "Bash: cargo test"),
        ] {
            let id = AgentId::from_parts("codex", detail);
            let mut live = agent(
                id,
                0,
                "command",
                detail,
                ActivityState::Active {
                    tool_use_id: None,
                    detail: Some(Arc::from(detail)),
                    kind,
                },
                false,
            );
            live.state_started_at = now - Duration::from_millis(360);
            assert_eq!(
                market_command_success_elapsed(&live, now),
                Some(360),
                "{detail} must map to the same command-success visual"
            );
        }
    }

    #[test]
    fn command_success_pulse_ends_instead_of_looping() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let id = AgentId::from_parts("codex", "finished-effect");
        let mut live = agent(
            id,
            0,
            "command",
            "finished-effect",
            ActivityState::Active {
                tool_use_id: None,
                detail: Some(Arc::from("exec")),
                kind: ToolKind::Other,
            },
            false,
        );
        live.state_started_at =
            now - Duration::from_millis(MARKET_COMMAND_SUCCESS_MS.saturating_add(1));

        assert_eq!(market_command_success_elapsed(&live, now), None);
    }

    #[test]
    fn turn_completion_pulse_is_one_shot_and_keyed_to_the_completed_agent() {
        let completed_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let completed_id = AgentId::from_parts("codex", "completed");
        let other_id = AgentId::from_parts("codex", "other");
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(
            completed_id,
            agent(
                completed_id,
                0,
                "completed",
                "completed-session",
                ActivityState::Active {
                    tool_use_id: None,
                    detail: None,
                    kind: ToolKind::Other,
                },
                false,
            ),
        );
        scene.agents.insert(
            other_id,
            agent(
                other_id,
                1,
                "other",
                "other-session",
                ActivityState::Idle,
                false,
            ),
        );
        let mut reducer = Reducer::new();
        reducer.apply(
            &mut scene,
            AgentEvent::TurnComplete {
                agent_id: completed_id,
            },
            completed_at,
            Transport::Jsonl,
        );

        assert_eq!(
            market_turn_completion_elapsed(&scene, completed_id, completed_at),
            Some(0)
        );
        assert_eq!(
            market_turn_completion_elapsed(
                &scene,
                completed_id,
                completed_at + Duration::from_millis(MARKET_TURN_COMPLETE_MS - 1),
            ),
            Some(MARKET_TURN_COMPLETE_MS - 1)
        );
        assert_eq!(
            market_turn_completion_elapsed(&scene, other_id, completed_at),
            None,
            "the pulse must never jump to another visible actor"
        );
        assert_eq!(
            market_turn_completion_elapsed(
                &scene,
                completed_id,
                completed_at + Duration::from_millis(MARKET_TURN_COMPLETE_MS),
            ),
            None,
            "the final frame expires instead of looping"
        );
    }

    #[test]
    fn paperdoll_appearance_does_not_change_when_an_earlier_agent_leaves() {
        let first_id = AgentId::from_parts("codex", "first");
        let remaining_id = AgentId::from_parts("codex", "remaining");
        let mut scene = SceneState::uniform(8);
        for slot in [
            agent(
                first_id,
                0,
                "cx·先完成",
                "first-session",
                ActivityState::Idle,
                false,
            ),
            agent(
                remaining_id,
                1,
                "cx·繼續處理",
                "remaining-session",
                ActivityState::Idle,
                false,
            ),
        ] {
            scene.agents.insert(slot.agent_id, slot);
        }
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 240,
            height: 160,
        };

        let before = build_market_placements(&scene, viewport)
            .into_iter()
            .find(|placement| placement.agent_id == remaining_id)
            .expect("remaining agent is placed before churn");
        scene.agents.remove(&first_id);
        let after = build_market_placements(&scene, viewport)
            .into_iter()
            .find(|placement| placement.agent_id == remaining_id)
            .expect("remaining agent is placed after churn");

        assert_eq!(
            before.slot.index, after.slot.index,
            "an immutable desk keeps the same stall and route when a neighbour leaves"
        );
        assert_eq!(
            before.appearance_index, after.appearance_index,
            "the same agent must not turn into a differently sized paperdoll when a neighbour leaves"
        );
    }

    #[test]
    fn placements_and_overlay_share_anchor_text_and_activity_authority() {
        let ids = [
            AgentId::from_parts("codex", "z-centre"),
            AgentId::from_parts("codex", "a-middle"),
            AgentId::from_parts("codex", "m-back"),
            AgentId::from_parts("codex", "b-edge"),
        ];
        let mut scene = SceneState::uniform(8);
        let agents = [
            agent(
                ids[0],
                0,
                "cx\u{b7}repo",
                "session-alpha",
                ActivityState::Active {
                    tool_use_id: Some(Arc::from("tool-1")),
                    detail: Some(Arc::from("Editing")),
                    kind: ToolKind::Edit,
                },
                false,
            ),
            agent(
                ids[1],
                1,
                "cx\u{b7}repo",
                "session-beta",
                ActivityState::Active {
                    tool_use_id: Some(Arc::from("tool-2")),
                    detail: Some(Arc::from("Reading")),
                    kind: ToolKind::Read,
                },
                false,
            ),
            agent(
                ids[2],
                2,
                "leaving",
                "session-gamma",
                ActivityState::Idle,
                true,
            ),
            agent(
                ids[3],
                3,
                "a-very-long-project-name",
                "session-delta",
                ActivityState::Active {
                    tool_use_id: None,
                    detail: Some(Arc::from("Working")),
                    kind: ToolKind::Other,
                },
                false,
            ),
        ];
        // Deliberately insert in reverse allocation order: placement authority
        // must follow stable desk allocation, not BTreeMap/AgentId order.
        for slot in agents.into_iter().rev() {
            scene.agents.insert(slot.agent_id, slot);
        }

        let placements = build_market_placements(
            &scene,
            Bounds {
                x: 0,
                y: 0,
                width: 240,
                height: 160,
            },
        );
        assert_eq!(
            placements.iter().map(|p| p.agent_id).collect::<Vec<_>>(),
            ids
        );

        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(110);
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 240,
            height: 160,
        };
        let labels = build_market_overlay(
            &scene,
            &placements,
            Some(ids[1]),
            MarketFrameContext { viewport, now },
        );
        assert_eq!(
            labels.len(),
            placements.len() - 1,
            "the exiting shop is already closed"
        );
        for (placement, label) in placements
            .iter()
            .filter(|placement| placement.agent_id != ids[2])
            .zip(&labels)
        {
            assert_eq!(
                label.anchor_px, placement.slot.anchor_px,
                "character and label must consume the identical slot anchor"
            );
        }
        assert_eq!(
            labels.iter().map(|label| label.tone).collect::<Vec<_>>(),
            vec![LabelTone::Active, LabelTone::Active, LabelTone::Active]
        );
        assert!(!labels[0].hovered);
        assert!(labels[1].hovered);
        assert_eq!(
            labels[0].text,
            format!("素材狐\u{b7}repo\u{b7}{}", disambig_suffix("session-alpha"))
        );
        assert_eq!(
            labels[1].text,
            format!("動作貓\u{b7}repo\u{b7}{}", disambig_suffix("session-beta"))
        );
        assert_ne!(labels[0].text, labels[1].text);
        assert_eq!(labels[2].text.chars().count(), 14);
    }

    #[test]
    fn market_shop_ids_are_short_role_names_fixed_to_each_agent_appearance() {
        let expected = [
            "素材狐",
            "動作貓",
            "介面星",
            "程式熊",
            "測試鳥",
            "文件兔",
            "安全鹿",
            "協作楓",
        ];
        let mut scene = SceneState::uniform(MARKET_MAX_AGENTS);
        for desk_index in 0..MARKET_MAX_AGENTS {
            let agent_key = format!("real-session-{desk_index}");
            let session_id = format!("private-session-{desk_index}");
            let id = AgentId::from_parts("codex", &agent_key);
            scene.agents.insert(
                id,
                agent(
                    id,
                    desk_index,
                    "cx\u{b7}整理角色動畫",
                    &session_id,
                    ActivityState::Active {
                        tool_use_id: None,
                        detail: Some(Arc::from("Working")),
                        kind: ToolKind::Other,
                    },
                    false,
                ),
            );
        }
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 240,
            height: 160,
        };
        let placements = build_market_placements(&scene, viewport);
        let labels = build_market_avatar_overlay(
            &scene,
            &placements,
            None,
            MarketFrameContext {
                viewport,
                now: SystemTime::UNIX_EPOCH + Duration::from_secs(110),
            },
        );

        assert_eq!(labels.len(), MARKET_MAX_AGENTS);
        for ((label, placement), expected_id) in labels.iter().zip(&placements).zip(expected) {
            assert!(
                label.text.starts_with(&format!("{expected_id}\u{b7}")),
                "appearance {} should use its role-themed fake player id: {}",
                placement.appearance_index,
                label.text
            );
            assert!(!label.text.contains("cx"));
            assert!(!label.text.contains("private-session"));
            assert!(!label.text.contains("real-session"));
        }
    }

    #[test]
    fn market_cards_carry_root_and_child_lineage_from_parent_id() {
        let root_id = AgentId::from_parts("codex", "market-root");
        let child_id = AgentId::from_parts("codex", "market-child");
        let mut root = agent(
            root_id,
            0,
            "cx\u{b7}盤點新廠",
            "root-session",
            ActivityState::Active {
                tool_use_id: None,
                detail: Some(Arc::from("Working")),
                kind: ToolKind::Other,
            },
            false,
        );
        root.created_at = SystemTime::UNIX_EPOCH + Duration::from_secs(90);
        let mut child = agent(
            child_id,
            1,
            "cx\u{b7}整理設備清單",
            "child-session",
            ActivityState::Active {
                tool_use_id: None,
                detail: Some(Arc::from("Working")),
                kind: ToolKind::Other,
            },
            false,
        );
        child.created_at = root.created_at;
        child.parent_id = Some(root_id);
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(root_id, root);
        scene.agents.insert(child_id, child);
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 240,
            height: 160,
        };
        let placements = build_market_placements(&scene, viewport);
        let labels = build_market_avatar_overlay(
            &scene,
            &placements,
            None,
            MarketFrameContext {
                viewport,
                now: SystemTime::UNIX_EPOCH + Duration::from_secs(110),
            },
        );

        assert_eq!(labels.len(), 2);
        assert_eq!(
            labels[0].relation,
            Some(MapleAgentRelation::Root {
                root_id,
                descendant_count: 1,
            })
        );
        assert_eq!(
            labels[1].relation,
            Some(MapleAgentRelation::Child {
                root_id,
                root_task: Some("盤點新廠".to_owned()),
                depth: 1,
            })
        );
    }

    #[test]
    fn paperdoll_anchor_preserves_feet_and_moves_the_shop_card_to_the_head() {
        let id = AgentId::from_parts("codex", "paperdoll");
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(
            id,
            agent(
                id,
                0,
                "cx\u{b7}paperdoll",
                "session-paperdoll",
                ActivityState::Active {
                    tool_use_id: None,
                    detail: Some(Arc::from("Working")),
                    kind: ToolKind::Other,
                },
                false,
            ),
        );
        let placements = build_market_placements(
            &scene,
            Bounds {
                x: 0,
                y: 0,
                width: 240,
                height: 160,
            },
        );
        let placement = placements[0];

        assert_eq!((MARKET_AVATAR_WIDTH, MARKET_AVATAR_HEIGHT), (32, 24));
        assert_eq!(placement.slot.anchor_px, Point { x: 120, y: 127 });
        assert_eq!(
            placement.avatar_anchor_px,
            Point { x: 108, y: 115 },
            "the 32x24 paperdoll stays centred over the original 8x12 merchant and shares its y=139 foot line"
        );

        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 240,
            height: 160,
        };
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(110);
        let labels = build_market_avatar_overlay(
            &scene,
            &placements,
            None,
            MarketFrameContext { viewport, now },
        );
        assert_eq!(labels.len(), 1);
        assert_eq!(
            labels[0].anchor_px,
            Point {
                x: placement.slot.anchor_px.x,
                y: placement.avatar_anchor_px.y,
            },
            "the shop card's nominal 8px anchor must center over the 32px paperdoll canvas"
        );
        assert_eq!(
            placement.avatar_anchor_px.y + MARKET_AVATAR_HEIGHT,
            placement.slot.anchor_px.y + 12,
            "paperdoll and fallback standing sprite must keep the same foot baseline"
        );
    }

    #[test]
    fn actor_head_anchor_tracks_the_paperdoll_but_shop_card_opens_only_when_settled() {
        let id = AgentId::from_parts("codex", "animated-card");
        let mut live = agent(
            id,
            0,
            "cx\u{b7}animated-card",
            "session-animated-card",
            ActivityState::Active {
                tool_use_id: None,
                detail: Some(Arc::from("Working")),
                kind: ToolKind::Other,
            },
            false,
        );
        live.state_started_at = live.created_at;
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 240,
            height: 160,
        };
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(id, live.clone());
        let placement = build_market_placements(&scene, viewport)[0];
        let created_at = live.created_at;
        let frame_at = |now| MarketFrameContext { viewport, now };

        let assert_centred = |actor: super::MarketActorFrame| {
            assert_eq!(
                actor.label_anchor_px.x + crate::layout::CHARACTER_SPRITE_W / 2,
                actor.sprite_anchor_px.x + MARKET_AVATAR_WIDTH / 2,
                "shop card and paperdoll must share one animated x centre"
            );
            assert_eq!(
                actor.label_anchor_px.y, actor.sprite_anchor_px.y,
                "the card must follow the same one-pixel step cadence as the head"
            );
        };

        for elapsed in [0_u64, 500, 1_000] {
            let actor = resolve_market_paperdoll(
                &live,
                placement,
                frame_at(created_at + Duration::from_millis(elapsed)),
            )
            .expect("entering/settled merchant remains visible");
            assert_centred(actor);
        }

        let entering_labels = build_market_avatar_overlay(
            &scene,
            &[placement],
            None,
            frame_at(created_at + Duration::from_millis(500)),
        );
        assert!(
            entering_labels.is_empty(),
            "the store card does not travel with the character"
        );
        let still_walking_labels = build_market_avatar_overlay(
            &scene,
            &[placement],
            None,
            frame_at(created_at + Duration::from_millis(1_000)),
        );
        assert!(
            still_walking_labels.is_empty(),
            "a Maple-speed walk must not open the store after only one second"
        );
        let settled_labels = build_market_avatar_overlay(
            &scene,
            &[placement],
            None,
            frame_at(created_at + Duration::from_millis(5_000)),
        );
        assert_eq!(
            settled_labels.len(),
            1,
            "the store card opens after arrival"
        );

        let exiting_at = created_at + Duration::from_secs(2);
        live.exiting_at = Some(exiting_at);
        scene.agents.insert(id, live.clone());
        for elapsed in [0_u64, 400, 800] {
            let actor = resolve_market_paperdoll(
                &live,
                placement,
                frame_at(exiting_at + Duration::from_millis(elapsed)),
            )
            .expect("departing merchant remains visible until the final frame");
            assert_centred(actor);
            assert!(!actor.stall_open);
        }
        assert!(
            build_market_avatar_overlay(&scene, &[placement], None, frame_at(exiting_at),)
                .is_empty()
        );
        assert!(resolve_market_paperdoll(
            &live,
            placement,
            frame_at(exiting_at + Duration::from_millis(u64::from(MARKET_MAX_EXIT_MS))),
        )
        .is_none());
    }

    #[test]
    fn paperdoll_uses_the_four_frame_walk1_cadence_and_stops_without_bobbing() {
        let id = AgentId::from_parts("codex", "walk-cycle");
        let live = agent(
            id,
            0,
            "cx\u{b7}walk-cycle",
            "session-walk-cycle",
            ActivityState::Idle,
            false,
        );
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 240,
            height: 160,
        };
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(id, live.clone());
        let placement = build_market_placements(&scene, viewport)[0];

        for (expected_frame, elapsed) in
            [1_usize, 2, 3, 0].into_iter().zip([220_u64, 400, 580, 760])
        {
            let actor = resolve_market_paperdoll(
                &live,
                placement,
                MarketFrameContext {
                    viewport,
                    now: live.created_at + Duration::from_millis(elapsed),
                },
            )
            .expect("entering merchant stays visible");
            let MarketActorPose::Walk(walk) = actor.pose else {
                panic!("entry uses the real walk1 cycle, got {:?}", actor.pose);
            };
            assert_eq!(walk.frame_index, expected_frame);
            assert_eq!(walk.facing, MarketFacing::Left);
            assert_eq!(
                actor.sprite_anchor_px.y, placement.avatar_anchor_px.y,
                "the source animation supplies the gait; the canvas must not add a fake vertical bob"
            );
            assert_eq!(actor.label_anchor_px.y, actor.sprite_anchor_px.y);
        }

        let settled = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: live.created_at + Duration::from_millis(3_000),
            },
        )
        .expect("settled merchant stays visible");
        assert!(
            matches!(settled.pose, MarketActorPose::Stand2 { .. }),
            "idle merchants use the source-authored stand2 pose"
        );
    }

    #[test]
    fn paperdoll_faces_its_actual_horizontal_travel_direction() {
        let id = AgentId::from_parts("codex", "walk-direction");
        let mut live = agent(
            id,
            0,
            "cx\u{b7}walk-direction",
            "session-walk-direction",
            ActivityState::Idle,
            false,
        );
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 240,
            height: 160,
        };
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(id, live.clone());
        let placement = build_market_placements(&scene, viewport)[0];
        let entering = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: live.created_at + Duration::from_millis(300),
            },
        )
        .expect("entry frame");
        let MarketActorPose::Walk(entering_walk) = entering.pose else {
            panic!("entry should be walking after portal reveal");
        };
        assert_eq!(
            entering_walk.facing,
            MarketFacing::Left,
            "slot zero enters from the right edge"
        );

        let exiting_at = live.created_at + Duration::from_secs(2);
        live.exiting_at = Some(exiting_at);
        let exiting = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: exiting_at,
            },
        )
        .expect("exit frame");
        let MarketActorPose::Walk(exiting_walk) = exiting.pose else {
            panic!("exit starts with the reverse walking route");
        };
        assert_eq!(
            exiting_walk.facing,
            MarketFacing::Right,
            "the same merchant turns around before leaving"
        );
    }

    #[test]
    fn merchants_enter_through_the_portal_climb_to_their_floor_then_open_shop() {
        use super::MarketActorPose;

        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 720,
            height: 480,
        };
        let boot = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut scene = SceneState::uniform(8);
        let ids = [
            AgentId::from_parts("codex", "front-route"),
            AgentId::from_parts("codex", "middle-route"),
            AgentId::from_parts("codex", "back-route"),
        ];
        for (desk, id) in ids.into_iter().enumerate() {
            let mut slot = agent(
                id,
                desk,
                "cx\u{b7}route",
                &format!("route-{desk}"),
                ActivityState::Active {
                    tool_use_id: None,
                    detail: Some(Arc::from("Working")),
                    kind: ToolKind::Other,
                },
                false,
            );
            slot.created_at = boot;
            scene.agents.insert(id, slot);
        }
        let placements = build_market_placements(&scene, viewport);

        let start = resolve_market_paperdoll(
            scene.agents.get(&ids[0]).unwrap(),
            placements[0],
            MarketFrameContext {
                viewport,
                now: boot,
            },
        )
        .expect("portal entry is visible");
        assert_eq!(start.foot_px(), Point { x: 678, y: 417 });
        assert!(!start.stall_open);

        let middle_climb = resolve_market_paperdoll(
            scene.agents.get(&ids[1]).unwrap(),
            placements[1],
            MarketFrameContext {
                viewport,
                now: boot + Duration::from_millis(3_000),
            },
        )
        .expect("middle route is visible");
        assert!(matches!(middle_climb.pose, MarketActorPose::Climb { .. }));
        assert!(!middle_climb.stall_open);

        let upper_climb = resolve_market_paperdoll(
            scene.agents.get(&ids[2]).unwrap(),
            placements[2],
            MarketFrameContext {
                viewport,
                now: boot + Duration::from_millis(5_500),
            },
        )
        .expect("upper route is visible");
        assert!(matches!(upper_climb.pose, MarketActorPose::Climb { .. }));
        assert!(!upper_climb.stall_open);

        for (agent_id, placement) in ids.into_iter().zip(&placements) {
            let settled = resolve_market_paperdoll(
                scene.agents.get(&agent_id).unwrap(),
                *placement,
                MarketFrameContext {
                    viewport,
                    now: boot + Duration::from_millis(9_000),
                },
            )
            .expect("all routes settle within the reducer grace budget");
            assert!(settled.stall_open, "shop opens only after arrival");
            assert!(matches!(settled.pose, MarketActorPose::Stand { .. }));
        }
    }

    #[test]
    fn ground_routes_use_maplestorys_125_reference_pixels_per_second() {
        let mut route = Vec::new();
        push_market_walk(
            &mut route,
            Point { x: 100, y: 300 },
            Point { x: 225, y: 300 },
            Bounds {
                x: 0,
                y: 0,
                width: 720,
                height: 480,
            },
        );

        assert_eq!(route.len(), 1);
        assert_eq!(
            route[0].duration_ms, 1_000,
            "125 reference pixels must take one second at MapleStory's base walk speed"
        );
    }

    #[test]
    fn settled_states_use_only_sourced_maple_actions() {
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 720,
            height: 480,
        };
        let boot = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let id = AgentId::from_parts("codex", "status-actions");
        let mut live = agent(
            id,
            0,
            "cx\u{b7}status-actions",
            "status-actions",
            ActivityState::Idle,
            false,
        );
        live.created_at = boot;
        live.state_started_at = boot;
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(id, live.clone());
        let placement = build_market_placements(&scene, viewport)[0];
        let geometry = super::market_actor_geometry(
            placement.avatar_anchor_px,
            placement.slot.layer,
            viewport,
            MARKET_AVATAR_WIDTH,
            MARKET_AVATAR_HEIGHT,
            placement.appearance_index,
        );
        let arrival_at = boot
            + Duration::from_millis(super::route_duration(&super::market_entry_route(geometry)));

        let idle = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: arrival_at,
            },
        )
        .expect("settled idle merchant");
        assert!(matches!(
            idle.pose,
            MarketActorPose::Stand2 { frame_index: 0 }
        ));
        assert!(!idle.stall_open, "idle closes the personal shop");

        live.state = ActivityState::Waiting {
            reason: Arc::from("permission"),
        };
        let waiting = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: arrival_at,
            },
        )
        .expect("settled waiting merchant");
        assert_eq!(waiting.pose, MarketActorPose::Sit);
        assert!(!waiting.stall_open, "waiting closes the personal shop");

        live.state = ActivityState::Active {
            tool_use_id: Some(Arc::from("tool-1")),
            detail: Some(Arc::from("Editing")),
            kind: ToolKind::Edit,
        };
        for (elapsed, expected) in [(0_u64, 0_usize), (500, 1), (1_000, 2), (1_500, 1)] {
            let alert = resolve_market_paperdoll(
                &live,
                placement,
                MarketFrameContext {
                    viewport,
                    now: arrival_at + Duration::from_millis(elapsed),
                },
            )
            .expect("active alert frame");
            assert_eq!(
                alert.pose,
                MarketActorPose::Alert {
                    frame_index: expected
                }
            );
            assert!(!alert.stall_open, "shop opens only after alert completes");
        }
        let active = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: arrival_at + Duration::from_millis(2_000),
            },
        )
        .expect("steady active merchant");
        assert!(matches!(active.pose, MarketActorPose::Stand { .. }));
        assert!(active.stall_open);
    }

    #[test]
    fn a_late_active_tool_start_keeps_the_existing_shop_open() {
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 720,
            height: 480,
        };
        let boot = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let tool_started_at = boot + Duration::from_secs(20);
        let id = AgentId::from_parts("codex", "continuous-shop");
        let mut live = agent(
            id,
            0,
            "cx\u{b7}continuous-shop",
            "continuous-shop",
            ActivityState::Active {
                tool_use_id: Some(Arc::from("tool-late")),
                detail: Some(Arc::from("Editing")),
                kind: ToolKind::Edit,
            },
            false,
        );
        live.created_at = boot;
        live.state_started_at = tool_started_at;
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(id, live.clone());
        let placement = build_market_placements(&scene, viewport)[0];

        let active = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: tool_started_at,
            },
        )
        .expect("settled active merchant");

        assert!(active.stall_open, "a new tool must not close an open shop");
        assert!(matches!(active.pose, MarketActorPose::Stand { .. }));
    }

    #[test]
    fn idle_roam_uses_the_market_ladder_and_exit_keeps_that_visible_foot() {
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 720,
            height: 480,
        };
        let boot = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let state_started_at = boot + Duration::from_secs(10);
        let id = AgentId::from_parts("codex", "idle-walk");
        let mut live = agent(
            id,
            1,
            "cx\u{b7}idle-walk",
            "idle-walk",
            ActivityState::Idle,
            false,
        );
        live.created_at = boot;
        live.state_started_at = state_started_at;
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(id, live.clone());
        let placement = build_market_placements(&scene, viewport)[0];
        let geometry = super::market_actor_geometry(
            placement.avatar_anchor_px,
            placement.slot.layer,
            viewport,
            MARKET_AVATAR_WIDTH,
            MARKET_AVATAR_HEIGHT,
            placement.appearance_index,
        );
        let route = super::market_entry_route(geometry);
        let final_walk_ms = route.last().expect("middle route final walk").duration_ms;

        let resting = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: state_started_at,
            },
        )
        .expect("idle stand2 frame");
        let climbing_at = state_started_at
            + Duration::from_millis(super::MARKET_IDLE_STALL_PAUSE_MS + final_walk_ms + 220);
        let climbing = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: climbing_at,
            },
        )
        .expect("idle ladder frame");
        assert!(matches!(climbing.pose, MarketActorPose::Climb { .. }));
        assert_ne!(climbing.foot_px().y, resting.foot_px().y);
        assert!(!climbing.stall_open);

        live.exiting_at = Some(climbing_at);
        let first_exit = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: climbing_at,
            },
        )
        .expect("first exit frame");
        assert_eq!(
            first_exit.foot_px(),
            climbing.foot_px(),
            "the reverse route begins at the visible idle-roam foot"
        );
        let second_exit = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: climbing_at + Duration::from_millis(1),
            },
        )
        .expect("second exit frame");
        assert!(
            second_exit.foot_px().x.abs_diff(first_exit.foot_px().x) <= 2
                && second_exit.foot_px().y.abs_diff(first_exit.foot_px().y) <= 2,
            "the second exit frame must continue from the visible ladder foot"
        );
    }

    #[test]
    fn far_upper_shop_keeps_walking_for_the_full_route_before_opening() {
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 720,
            height: 480,
        };
        let boot = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let id = AgentId::from_parts("codex", "far-upper-route");
        let mut live = agent(
            id,
            6,
            "cx\u{b7}far-upper",
            "route-far-upper",
            ActivityState::Idle,
            false,
        );
        live.created_at = boot;
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(id, live.clone());
        let placement = build_market_placements(&scene, viewport)[0];

        let late_entry = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: boot + Duration::from_millis(8_000),
            },
        )
        .expect("the far upper merchant is still completing the authored route");
        assert!(!late_entry.stall_open);

        let exiting_at = boot + Duration::from_secs(12);
        live.exiting_at = Some(exiting_at);
        let late_exit = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: exiting_at + Duration::from_millis(8_000),
            },
        )
        .expect("the reverse walk remains visible until the merchant reaches the portal");
        assert!(!late_exit.stall_open);
    }

    #[test]
    fn ending_work_closes_the_shop_immediately_and_rewinds_to_the_portal() {
        use super::{MarketActorPose, MARKET_MAX_EXIT_MS};

        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 720,
            height: 480,
        };
        let boot = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let id = AgentId::from_parts("codex", "exit-route");
        let mut live = agent(
            id,
            2,
            "cx\u{b7}exit",
            "route-exit",
            ActivityState::Idle,
            false,
        );
        live.created_at = boot;
        let exiting_at = boot + Duration::from_secs(10);
        live.exiting_at = Some(exiting_at);
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(id, live.clone());
        let placement = build_market_placements(&scene, viewport)[0];

        let first_exit = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: exiting_at,
            },
        )
        .expect("closing frame remains visible");
        assert!(
            !first_exit.stall_open,
            "SessionEnd closes the shop before movement"
        );
        assert!(matches!(first_exit.pose, MarketActorPose::Walk(_)));

        assert!(
            resolve_market_paperdoll(
                &live,
                placement,
                MarketFrameContext {
                    viewport,
                    now: exiting_at + Duration::from_millis(u64::from(MARKET_MAX_EXIT_MS)),
                },
            )
            .is_none(),
            "the complete reverse route must finish before core removes the agent"
        );
    }

    #[test]
    fn session_end_during_a_climb_reverses_from_the_current_foot_without_a_jump() {
        let viewport = Bounds {
            x: 0,
            y: 0,
            width: 720,
            height: 480,
        };
        let boot = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let id = AgentId::from_parts("codex", "mid-climb-exit");
        let mut live = agent(
            id,
            2,
            "cx\u{b7}mid-climb",
            "route-mid-climb",
            ActivityState::Idle,
            false,
        );
        live.created_at = boot;
        let mut scene = SceneState::uniform(8);
        scene.agents.insert(id, live.clone());
        let placement = build_market_placements(&scene, viewport)[0];
        let exiting_at = boot + Duration::from_millis(3_000);
        let uninterrupted = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: exiting_at,
            },
        )
        .expect("climbing entry frame");

        live.exiting_at = Some(exiting_at);
        let reversing = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: exiting_at,
            },
        )
        .expect("first reverse frame");
        assert_eq!(reversing.foot_px(), uninterrupted.foot_px());
        assert!(!reversing.stall_open);

        let descending = resolve_market_paperdoll(
            &live,
            placement,
            MarketFrameContext {
                viewport,
                now: exiting_at + Duration::from_millis(300),
            },
        )
        .expect("reverse climb frame");
        assert!(
            descending.foot_px().y > reversing.foot_px().y,
            "returning from an interrupted upper route descends toward the portal"
        );
    }
}
