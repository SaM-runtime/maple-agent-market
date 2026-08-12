//! Pure-pixel paint pass — no ratatui types, no terminal I/O.
//!
//! Split from `tui/renderer.rs` to separate the pixel-painting pipeline
//! (called by any renderer impl — `TuiRenderer`, a future web canvas, PNG
//! export, GIF capture) from the ratatui-coupled half-block flush + widget
//! overlay (terminal lifecycle lives with the event loop in `tui/mod.rs`).
//!
//! `render_to_rgb_buffer` is the public entry point, and is itself TWO
//! phases behind one seam: `sim_step` (the `sim` module) advances the
//! world — motion, poses, lighting, chitchat — with no pixel access and
//! returns an immutable [`SimFrame`]; the paint pass (`paint_frame`)
//! consumes `&SimFrame` and mutates only the buffer + the paint-local
//! `FrameCache`.
//!
//! `render_to_rgb_buffer` is the world-render seam every painter rides;
//! `character_anchor` is the label/hit-test anchor AUTHORITY (the binary's
//! `tui/hit_test/`, and every label painter indirectly via
//! `overlay::build_overlay`). The rest of the module's public surface is the
//! painters' construction + observation types (`PixelCtx` / `PixelPassResult` /
//! `MascotFrame` / `SimFrame`), the weather + day/night queries the shells read
//! (`weather_names`, `force_weather`, `precipitation_level`, `is_day_at`), and
//! the geometry consts the binary's hit-tests pin to (`NEON_PANEL_INNER_*`,
//! `PANTRY_COFFEE_COLS_*`) — all on the published crate's api golden, so widen
//! it deliberately.

use std::collections::HashMap;
use std::time::SystemTime;

use pixtuoid_core::sprite::format::Pack;
use pixtuoid_core::sprite::{Frame, Rgb, RgbBuffer, Sprite};
use pixtuoid_core::state::{ActivityState, FloorLocalDeskIndex};
use pixtuoid_core::{AgentSlot, SceneState};

use crate::chitchat::{ActiveChitchat, ChitchatBubble};
use crate::floor::LightingState;
use crate::frame_cache::FrameCache;
use crate::layout::{
    z_sort_row, Anchor, Bounds, Layout, PlantItem, PodDecorItem, Point, Size, WallDecorItem,
    DESK_W, ELEVATOR_H, ELEVATOR_W,
};
use crate::motion::MotionState;
use crate::pet::PetFrame;

/// Milliseconds since the Unix epoch for `now` (0 if the clock is before it).
/// The wall-clock decode the pixel-pass animation timers share — 8 callers read
/// better with this name than an inline `elapsed_ms(now, UNIX_EPOCH)`. A one-line
/// forwarder to the scene-wide `anim::elapsed_ms` (same saturate-to-0 semantics).
pub(super) fn epoch_ms(now: SystemTime) -> u64 {
    crate::anim::elapsed_ms(now, SystemTime::UNIX_EPOCH)
}

/// Result of the pure-pixel pass — carries the resolved cat position
/// (for hit-testing), active chitchat bubbles (for widget rendering),
/// and agent ids that were seen carrying coffee this frame (so the
/// caller can persist them into its `CoffeeState`).
pub struct PixelPassResult {
    /// The office pet's resolved frame this tick (for hit-testing), if present.
    pub pet_pos: Option<PetFrame>,
    /// One resolved frame per gateway mascot drawn this tick (for hover
    /// identity). EMPTY when no gateway is present; a Vec because a source can run
    /// ANY number of concurrent instances (N OpenClaw gateways = N lobsters), each
    /// independently hoverable.
    pub mascots: Vec<MascotFrame>,
    /// Active speech bubbles this frame, for the caller's widget pass.
    pub chitchat_bubbles: Vec<ChitchatBubble>,
    /// Agent ids observed in `Walking { carrying_coffee: true }` this
    /// frame. The caller inserts them into the persistent
    /// `CoffeeState` (carrier + steam-window stamp in one map).
    pub new_coffee_carriers: Vec<pixtuoid_core::AgentId>,
    /// Waypoint indices with an occupant this tick (the same sim observation
    /// that drives the appliance feedback animations) — the audio cue
    /// tracker's appliance feed (`crate::audio::AudioCueTracker::observe`).
    pub occupied_waypoints: std::collections::HashSet<usize>,
}

/// The gateway mascot's screen frame — enough to hover-identify it (which
/// gateway, how busy). The wandering position is recomputed every frame, so
/// this is recaptured each render like `PetFrame`.
#[derive(Clone)]
pub struct MascotFrame {
    /// The mascot's top-left screen position this tick.
    pub pos: Point,
    /// The painted sprite's pixel size (from the pack's real frame) — so the
    /// binary's `hit_test_mascot` click box derives from what's drawn, not a
    /// hardcoded constant.
    pub w: u16,
    /// The painted sprite's pixel height (paired with `w`).
    pub h: u16,
    /// Human-readable gateway name (e.g. "OpenClaw").
    pub name: &'static str,
    /// WHICH instance of that gateway this mascot is (an OpenClaw port), so a
    /// hover over one of two concurrent lobsters names the one under the cursor.
    /// `None` when the source runs a single instance whose id carries no meaning
    /// for the user — see `DaemonInstanceId`'s stale-plugin fallback.
    pub instance: Option<String>,
    /// An agent run is in flight (the tooltip's idle-vs-working verb). Keyed on
    /// the run state, NOT the session count — a single-user gateway holds one
    /// persistent session even at rest, so session count is a poor idle/busy tell.
    pub busy: bool,
    /// Gateway up but its model backend is failing every run (#317) — the tooltip
    /// reads "model error" and the lobster renders sickly red.
    pub degraded: bool,
    /// Number of sessions the gateway currently holds (tooltip detail).
    pub active_sessions: u32,
}

mod ambient;
mod anchors;
mod background;
mod debug_overlay;
mod drawable;
mod effects;
mod furniture;
mod palette;
mod seat;
mod sim;
mod wall;

pub use anchors::character_anchor;
// The ToolKind→glow-hue seam the binary's footer tints tool segments with, so a
// footer tool colour matches the sprite's monitor glow exactly.
pub use palette::tool_glow_for_kind;
// The γ3 widening that PR-450 planned for: the observation TYPES a
// `floor::FloorSession::observe` caller reads go pub WITH the facade;
// `sim_step` + `SimStores` (the per-call borrow-set) stay crate-internal —
// the session is the public entry to the sim tick.
pub(crate) use sim::{sim_step, SimStores};
// The flame-crown/ember colors, for render tests (floor.rs) to assert the
// REAL painted values (effects itself stays a private submodule) — test-only,
// hence the cfg: the lib target has no consumer.
#[cfg(test)]
pub(crate) use effects::{FLAME_DEEP, FLAME_TIP};
#[cfg(test)]
pub(crate) use furniture::COOLER_WATER;
pub use sim::{CharacterGlow, CharacterPlacement, SimFrame};

/// The coffee-machine sub-region within the pantry counter sprite, as sprite-local
/// column ranges `[start, end)` per pantry size (the 32-wide `pantry` sprite vs the
/// 20-wide `pantry_small`). THE single source of truth shared by the steam-anchor
/// painter (`drawable`'s `WaypointPantry` arm) and the binary's
/// `hit_test_coffee_machine`, so the clickable machine box can't silently drift
/// from the painted art / steam anchor when the sprite is re-tuned. Pinned to the
/// steam anchor by `steam_anchor_sits_within_the_coffee_machine_columns`.
pub const PANTRY_COFFEE_COLS_LARGE: (u16, u16) = (11, 18);
/// Coffee-machine column range for the 20-wide `pantry_small` sprite (see
/// [`PANTRY_COFFEE_COLS_LARGE`]).
pub const PANTRY_COFFEE_COLS_SMALL: (u16, u16) = (9, 12);

/// The neon wall-sign panel geometry, in PIXELS: origin `(X, Y)` and OUTER size
/// `W×H`, drawn with a `NEON_PANEL_BORDER`-px frame on every side. THE single
/// source of truth shared by the pixel painter (`paint_neon_panel`) and the
/// wall-clock collision clamp. A pixel column maps 1:1 to a terminal cell column
/// in the half-block flush, so these px widths ARE cell widths on the horizontal.
///
/// The board's TEXT overlay lives in the dark INTERIOR (`NEON_PANEL_INNER_*` = the
/// panel minus its frame): the binary's `tui::widgets::wall_board::paint_wall_display`
/// pins its cell-origin AND width to those, so the lit text can't overrun the
/// glowing frame. Laying text to the full OUTER `NEON_PANEL_W` overran it by the
/// border on each side (the board-overflow bug). Only the interior pair + the
/// outer width cross the crate boundary (`pub`); `X`/`Y`/`H`/`BORDER` have no
/// cross-crate consumer (`pub(crate)`, don't widen the semver surface).
pub(crate) const NEON_PANEL_X: u16 = 1;
pub(crate) const NEON_PANEL_Y: u16 = 1;
/// The neon panel's OUTER width in pixels (frame included) — see the panel-geometry doc above.
pub const NEON_PANEL_W: u16 = 30;
pub(crate) const NEON_PANEL_H: u16 = 8;
/// The frame thickness `paint_neon_panel` lights on every side (it reads THIS, so
/// the interior derivation below provably matches the pixels it leaves dark).
pub(crate) const NEON_PANEL_BORDER: u16 = 1;
/// The dark interior's left cell-origin (`X` + the frame) — where board text starts.
pub const NEON_PANEL_INNER_X: u16 = NEON_PANEL_X + NEON_PANEL_BORDER;
/// The dark interior's cell WIDTH (`W` minus the frame on both sides) — the board's
/// usable text width; `BOARD_W` pins to this.
pub const NEON_PANEL_INNER_W: u16 = NEON_PANEL_W - 2 * NEON_PANEL_BORDER;
/// The dark interior's top pixel-origin (`Y` + the frame) — where the floating /
/// wasm painters anchor the board's first text row over the panel.
pub const NEON_PANEL_INNER_Y: u16 = NEON_PANEL_Y + NEON_PANEL_BORDER;
/// The dark interior's pixel HEIGHT (`H` minus the frame on both sides).
pub const NEON_PANEL_INNER_H: u16 = NEON_PANEL_H - 2 * NEON_PANEL_BORDER;
// The interior must be a non-empty strict subset of the outer frame (catches a
// degenerate BORDER=0 / oversized-border config at compile time).
const _: () = assert!(NEON_PANEL_INNER_W > 0 && NEON_PANEL_INNER_W < NEON_PANEL_W);
const _: () = assert!(NEON_PANEL_INNER_H > 0 && NEON_PANEL_INNER_H < NEON_PANEL_H);

use crate::creatures::{gateway_mascot_def, mascot_position, pet_position};
use anchors::compute_door_frame_idx;
use background::{
    daylight_floor_overlay, dim_floor_overlay, paint_ceiling_pool, paint_clock,
    paint_corridor_runner, paint_floor_and_walls, paint_floor_lamp_halo, paint_neon_panel,
    paint_shadow, time_of_day_look, Ellipse,
};
use drawable::{paint_drawable, Drawable, DrawableKind};
use palette::{agent_palette, outfit_seed_for, recolor_frame};
use seat::{paint_character_at, paint_character_at_scaled};
use wall::{
    enqueue_room_walls_h, enqueue_room_walls_v, paint_door_jamb_h, paint_door_jamb_v,
    paint_glass_wall_h, paint_glass_wall_v, DOOR_JAMB_PX,
};

/// The weather names accepted by [`force_weather`], in canonical order.
pub fn weather_names() -> Vec<&'static str> {
    background::Weather::ALL.iter().map(|w| w.name()).collect()
}

/// Force every subsequent render **on this thread** to a specific weather (by
/// name, case-insensitive), or `None` to restore the clock-based selection.
/// `Err` carries the valid names when `name` is unknown.
pub fn force_weather(name: Option<&str>) -> Result<(), Vec<&'static str>> {
    match name {
        None => {
            background::set_weather_override(None);
            Ok(())
        }
        Some(s) => match background::Weather::from_name(s) {
            Some(w) => {
                background::set_weather_override(Some(w));
                Ok(())
            }
            None => Err(weather_names()),
        },
    }
}

/// How hard it is raining, as a scalar (0.0 clear … 1.0 storm) — the audio
/// model's weather feed (`crate::audio::stem_levels`). A deliberate SCALAR
/// query so the module-private `background::Weather` enum never widens:
/// consumers get "how much rain", not the weather vocabulary. Snow/fog/etc.
/// are 0.0 — precipitation you can HEAR, not precipitation per se. Honors
/// the same per-thread [`force_weather`] override as every render.
pub fn precipitation_level(now: std::time::SystemTime) -> f32 {
    // rain at the ratified demo level, storm at full — the gap is audible
    // "getting heavier", not a new mix profile
    const RAIN_LEVEL: f32 = 0.6;
    match background::weather_state(now) {
        background::Weather::Storm => 1.0,
        background::Weather::Rain => RAIN_LEVEL,
        _ => 0.0,
    }
}

/// Whether the office's sky shows the SUN at hour-of-day `hour` (0..24), per the
/// engine's own `SUN_RISE_H`/`SUN_SET_H` window (`background/sky.rs`). Exposed so
/// the wasm painter's `Office::is_day` can hand the site's sky-slider the SAME
/// day/night boundary the office renders — one source of truth, no drift across
/// the Rust↔JS boundary.
pub fn hour_is_day(hour: f32) -> bool {
    background::hour_is_day(hour)
}

/// Day/night at `now` on the LOCAL clock — the native painters' feed for
/// the audio track selector (wasm passes its own hour; time is a parameter
/// there). Same sun window the lighting renders: the music follows what
/// the office SHOWS.
pub fn is_day_at(now: std::time::SystemTime) -> bool {
    background::hour_is_day(background::local_hour_frac(now))
}

// The steam gate reads the SAME window `CoffeeState::record` refreshes on —
// a reference, not a second copy of the value.
const COFFEE_STEAM_WINDOW_SECS: u64 = crate::floor::CoffeeState::STEAM_WINDOW_SECS;

/// Z-sort offset from a center-pinned sprite's center to its SOUTH (front) row.
/// A sprite of height `h` blitted at `py = center - h/2` occupies rows
/// `[py, py + h - 1]`, so its south row is `center + (h - 1) / 2`. This works
/// for BOTH parities: the naive `h/2 - 1` is one row short for ODD `h` (e.g. the
/// 11px whiteboard would sort one row in front of its own base). The z-key must
/// land ON the south row — one row past it lets the sprite paint over a
/// character standing immediately in front.
fn center_pin_south_offset(h: u16) -> u16 {
    h.saturating_sub(1) / 2
}

/// South-row (base) offset of the floor-lamp sprite, derived from the one
/// furniture table so the halo / shadow / z-anchor all move together if the
/// lamp's visual height changes (locked by a unit test).
fn floor_lamp_south_offset() -> u16 {
    center_pin_south_offset(
        crate::layout::furniture_def(crate::layout::Furniture::FloorLamp)
            .visual
            .h,
    )
}

/// Bundled input for the pixel-painting pass. Constructed at the `render_floor`
/// / `draw_scene` call site.
pub struct PixelCtx<'a> {
    /// The per-floor sim/paint STORES borrowed as ONE group (was seven flat
    /// fields: `router`/`overlay`/`history`/`cache`/`motion`/`light` +
    /// `door_anim_max_ms`). `render_to_rgb_buffer` reads them as disjoint field
    /// projections (`store.router`, `store.overlay`, …). `buf` stays a SEPARATE
    /// field: it is a sibling of the `FloorCtx` on a `PerFloor`, borrowed
    /// disjointly by a multi-floor painter's `split_at_mut`.
    pub store: &'a mut crate::floor::FloorCtx,
    /// The RGB pixel buffer this pass paints into.
    pub buf: &'a mut RgbBuffer,
    /// The live scene state to render.
    pub scene: &'a SceneState,
    /// The computed office geometry for this frame.
    pub layout: &'a Layout,
    /// The character/furniture sprite pack.
    pub pack: &'a Pack,
    /// The current time (the engine never reads the clock itself — it's a parameter).
    pub now: SystemTime,
    /// The active color theme.
    pub theme: &'a crate::theme::Theme,
    /// Which floor of the office this pass renders.
    pub floor: crate::floor::FloorMeta,
    /// The pet-interaction (heart-anim) state, if a pet is being petted.
    pub active_pet: Option<&'a crate::pet::PetState>,
    /// The pet on this floor (kind drives the sprite; name is unused here — the
    /// pixel pass doesn't render the name, the tooltip does).
    pub floor_pet: Option<&'a crate::pet::Pet>,
    /// Carrier → fetch-time view of [`crate::floor::CoffeeState`] (one map:
    /// key present = has a desk cup, value = steam-window anchor).
    pub coffee: &'a HashMap<pixtuoid_core::AgentId, SystemTime>,
    /// Per-venue active speech-bubble state, advanced across frames.
    pub chitchat_state: &'a mut HashMap<crate::chitchat::VenueKey, ActiveChitchat>,
    /// When set, composite the walkable / approach / route debug layer over the
    /// finished scene (the live `w` toggle). Off by default; transient.
    pub debug_walkable: bool,
}

/// The paint pass's borrow set — everything `paint_frame` may touch. The only
/// `&mut`s are the pixel buffer and the paint-local `FrameCache` (a render
/// cache, not a sim store); the sim stores are absent BY TYPE (`motion` is an
/// immutable view, read by the debug route overlay), so painting cannot move
/// the world — see the `sim` module docs for the classification.
struct PaintCtx<'a> {
    scene: &'a SceneState,
    layout: &'a Layout,
    pack: &'a Pack,
    now: SystemTime,
    buf: &'a mut RgbBuffer,
    cache: &'a mut FrameCache,
    theme: &'a crate::theme::Theme,
    floor: crate::floor::FloorMeta,
    active_pet: Option<&'a crate::pet::PetState>,
    floor_pet: Option<&'a crate::pet::Pet>,
    coffee: &'a HashMap<pixtuoid_core::AgentId, SystemTime>,
    motion: &'a HashMap<pixtuoid_core::AgentId, MotionState>,
    door_anim_max_ms: u64,
    debug_walkable: bool,
}

/// Render `ctx`'s scene into its buffer — the SHARED world render, TWO phases:
/// `sim_step` advances the world (no pixels) into a [`SimFrame`], then the paint
/// pass consumes it, mutating only the buffer + recolor cache. Returns the frame's
/// [`PixelPassResult`] (pet/mascot frames, chitchat bubbles, coffee carriers, occupancy).
pub fn render_to_rgb_buffer(ctx: &mut PixelCtx<'_>) -> PixelPassResult {
    render_to_rgb_buffer_for_map(ctx, None)
}

/// Render one explicitly selected Maple-world map through the same sim/paint
/// seam as the normal office.  Kept doc-hidden because map routing is an
/// in-workspace floating-painter feature, not part of the published engine API.
#[doc(hidden)]
pub fn render_maple_to_rgb_buffer(
    ctx: &mut PixelCtx<'_>,
    map: crate::maple_world::MapleMapId,
) -> PixelPassResult {
    render_to_rgb_buffer_for_map(ctx, Some(map))
}

fn render_to_rgb_buffer_for_map(
    ctx: &mut PixelCtx<'_>,
    maple_map: Option<crate::maple_world::MapleMapId>,
) -> PixelPassResult {
    // Phase 1 — SIM: advance the world (motion/poses/lighting/chitchat),
    // producing no pixels. See `sim::sim_step`.
    let frame = sim_step(
        &mut SimStores {
            router: &mut ctx.store.router,
            overlay: &mut ctx.store.overlay,
            history: &mut ctx.store.history,
            motion: &mut ctx.store.motion,
            light: &mut ctx.store.light,
            chitchat: &mut *ctx.chitchat_state,
        },
        ctx.scene,
        ctx.layout,
        ctx.pack,
        ctx.coffee,
        ctx.floor.floor_idx,
        ctx.now,
    );
    // Phase 2 — PAINT: an immutable read of the SimFrame that mutates only
    // the buffer + the recolor cache. Painting the same frame twice is
    // byte-identical (pinned by `paint_frame_is_pure_and_byte_identical`).
    let (pet_pos, mascots) = paint_frame_for_map(
        &mut PaintCtx {
            scene: ctx.scene,
            layout: ctx.layout,
            pack: ctx.pack,
            now: ctx.now,
            buf: &mut *ctx.buf,
            cache: &mut ctx.store.cache,
            theme: ctx.theme,
            floor: ctx.floor,
            active_pet: ctx.active_pet,
            floor_pet: ctx.floor_pet,
            coffee: ctx.coffee,
            motion: &ctx.store.motion,
            door_anim_max_ms: ctx.store.door_anim_max_ms,
            debug_walkable: ctx.debug_walkable,
        },
        &frame,
        maple_map,
    );
    PixelPassResult {
        pet_pos,
        mascots,
        chitchat_bubbles: frame.chitchat_bubbles,
        new_coffee_carriers: frame.new_coffee_carriers,
        occupied_waypoints: frame.occupied_waypoints,
    }
}

/// The soft floor shadow under one home desk. `cy` sits on the desk's z-sort
/// row — `desk.y + visual.h` — the SAME `visual.h` `enqueue_desk_cubicles` keys
/// the desk sprite on, read from the one authority (`desk_furniture_def`) rather
/// than re-hardcoded, so a `DESK_H` retune moves the shadow WITH the sprite's
/// south base instead of leaving it behind. `half_w` tracks `DESK_W` (the same
/// authority `cx` uses); only `half_h` is a source-less per-piece taste literal.
fn desk_shadow_ellipse(desk: Point) -> Ellipse {
    Ellipse {
        cx: desk.x + DESK_W / 2,
        cy: desk.y + crate::layout::desk_furniture_def().visual.h,
        half_w: DESK_W / 2 + 1,
        half_h: 3,
    }
}

/// The ceiling-fluorescent light pools, in paint order: one narrow tube per
/// desk, then a wider fixture over the pantry and the corridor. THE authority
/// for where the pools sit + how wide each glows, so the paint pass is a short
/// loop and the per-region ellipse extents aren't anonymous inline literals. The
/// floor-lamp halo is deliberately NOT here — it is a different painter
/// (`paint_floor_lamp_halo`) with its own strength + south-offset anchor; the
/// paint order (pools THEN lamp halo) is load-bearing for byte-identity.
fn ceiling_pool_regions(layout: &Layout) -> impl Iterator<Item = Ellipse> + '_ {
    // Half-extents (a fluorescent tube's lit footprint) per pool kind.
    const DESK_POOL_HALF: (u16, u16) = (10, 5);
    const PANTRY_POOL_HALF: (u16, u16) = (12, 6);
    const CORRIDOR_POOL_HALF: (u16, u16) = (14, 5);
    // The desk tube hangs one row-pair NORTH of the desk origin (above the
    // monitor, not on the surface); saturating so a top-row desk can't underflow.
    const DESK_POOL_CY_LIFT: u16 = 2;

    let desks = layout.home_desks.iter().map(|desk| Ellipse {
        cx: desk.x + DESK_W / 2,
        cy: desk.y.saturating_sub(DESK_POOL_CY_LIFT),
        half_w: DESK_POOL_HALF.0,
        half_h: DESK_POOL_HALF.1,
    });
    let pantry = layout.pantry.map(|p| p.bounds).map(|pr| Ellipse {
        cx: pr.x + pr.width / 2,
        cy: pr.y + pr.height / 2,
        half_w: PANTRY_POOL_HALF.0,
        half_h: PANTRY_POOL_HALF.1,
    });
    let corridor = layout.corridor.map(|c| Ellipse {
        cx: c.x + c.width / 2,
        cy: c.y + c.height / 2,
        half_w: CORRIDOR_POOL_HALF.0,
        half_h: CORRIDOR_POOL_HALF.1,
    });
    desks.chain(pantry).chain(corridor)
}

/// THE authority for the soft floor shadows, in PAINT ORDER (the shadow twin of
/// [`ceiling_pool_regions`]) — one place for every piece's floor-contact ellipse
/// so the ~120-line inline block became a short loop and the per-piece extents
/// aren't anonymous inline literals scattered through `paint_frame`. Painted
/// BEFORE the y-sorted entity pass so every entity sits on its own shadow. Order
/// is preserved verbatim so the blended overlaps stay byte-identical.
///
/// The per-piece `half_w`/`half_h` are owner-tuned taste literals and the
/// couch/printer/island special-cases are irreducible — this HOUSES them in one
/// spot, it does not normalize them (the island/plant/lamp south offsets DO
/// derive from the shared `center_pin_south_offset`, so those move with a retune;
/// the generic/printer/couch `+2`/`+1` offsets stay literal by design).
fn floor_shadow_ellipses(layout: &Layout) -> impl Iterator<Item = Ellipse> + '_ {
    use crate::layout::{furniture_def, Furniture, WaypointKind};

    let desks = layout
        .home_desks
        .iter()
        .map(|desk| desk_shadow_ellipse(*desk));
    // Generic waypoint blob. Couch/Printer/Island are handled with fitted
    // shadows below, so skip them here: a per-seat couch shadow (3 seats) would
    // overlap-darken; the printer's 4px sprite souths at +1 not the generic +2;
    // the island STANDS are empty floor beside the body (which carries its own
    // shadow) so a blob at each stand paints phantom shadows.
    let generic = layout
        .waypoints
        .iter()
        .filter(|w| {
            !matches!(
                w.kind,
                WaypointKind::Couch | WaypointKind::Printer | WaypointKind::Island
            )
        })
        .map(|wp| {
            // Fit the ellipse to the sprite width — a flat 7 half-width doubles a
            // narrow shelf's shadow; `.min(7)` caps a future wide piece.
            let vis_w = furniture_def(wp.kind.furniture()).visual.w;
            let half_w = if vis_w > 0 { (vis_w / 2 + 1).min(7) } else { 7 };
            Ellipse {
                cx: wp.pos.x,
                cy: wp.pos.y + 2,
                half_w,
                half_h: 2,
            }
        });
    // One fitted shadow under the island BODY (its stands are skipped above):
    // south edge = the visual south row, width tracks the sprite.
    let island = layout.pantry.and_then(|p| p.kitchen_island).map(|island| {
        let vis = furniture_def(Furniture::KitchenIsland).visual;
        Ellipse {
            cx: island.x,
            cy: island.y + center_pin_south_offset(vis.h),
            half_w: vis.w / 2 + 1,
            half_h: 2,
        }
    });
    // Flush against the printer's sprite south (pos.y+1), one per printer.
    let printers = layout
        .waypoints
        .iter()
        .filter(|w| w.kind == WaypointKind::Printer)
        .map(|wp| Ellipse {
            cx: wp.pos.x,
            cy: wp.pos.y + 1,
            half_w: 5,
            half_h: 1,
        });
    let couch = layout.couch_sprite_center().map(|center| Ellipse {
        cx: center.x,
        cy: center.y + 2,
        half_w: 7,
        half_h: 2,
    });
    // Under the sprite's south row — same offset the z-anchor uses, off the same
    // height (a fixed +3 only suited the taller Ficus/Tall, floating the others).
    let plants = layout
        .plants
        .iter()
        .map(|&PlantItem { kind, pos }| Ellipse {
            cx: pos.x,
            cy: pos.y + center_pin_south_offset(furniture_def(kind.furniture()).visual.h),
            half_w: 3,
            half_h: 1,
        });
    let lamp = layout.floor_lamp().map(|lamp| Ellipse {
        cx: lamp.x,
        cy: lamp.y + floor_lamp_south_offset(), // flush with the lamp base (sprite south)
        half_w: 2,
        half_h: 1,
    });

    desks
        .chain(generic)
        .chain(island)
        .chain(printers)
        .chain(couch)
        .chain(plants)
        .chain(lamp)
}

/// Scale a local scene plate over the entire render target with integer
/// nearest-neighbour sampling. Background frames are expected to be opaque,
/// but a transparent custom-pack pixel resolves to `fallback` instead of
/// leaking the office frame that used to live beneath it.
fn paint_market_backdrop(buf: &mut RgbBuffer, backdrop: &Frame, fallback: Rgb) {
    let (src_w, src_h) = (backdrop.width(), backdrop.height());
    let (dst_w, dst_h) = (buf.width(), buf.height());
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return;
    }
    for y in 0..dst_h {
        let src_y = ((y as u32 * src_h as u32) / dst_h as u32) as u16;
        for x in 0..dst_w {
            let src_x = ((x as u32 * src_w as u32) / dst_w as u32) as u16;
            let rgb = backdrop
                .get(src_x, src_y)
                .copied()
                .flatten()
                .unwrap_or(fallback);
            buf.put(x, y, rgb);
        }
    }
}

const MARKET_PORTAL_LOOP_MS: u64 = 1_280;

fn market_portal_bounds(width: u16, height: u16) -> Bounds {
    Bounds {
        x: (u32::from(width) * 892 / 1_000) as u16,
        y: (u32::from(height) * 735 / 1_000) as u16,
        width: ((u32::from(width) * 56 / 1_000) as u16).max(1),
        height: ((u32::from(height) * 150 / 1_000) as u16).max(1),
    }
}

fn training_portal_bounds(width: u16, height: u16) -> Bounds {
    Bounds {
        x: (u32::from(width) * 909 / 1_000) as u16,
        y: (u32::from(height) * 617 / 1_000) as u16,
        width: ((u32::from(width) * 62 / 1_000) as u16).max(1),
        height: ((u32::from(height) * 250 / 1_000) as u16).max(1),
    }
}

/// Add a procedural blue-white shimmer inside the authored right-hand portal.
/// The epoch is reduced before conversion to float so animation remains live
/// after long uptimes, while every pixel outside the doorway stays untouched.
fn paint_market_portal(buf: &mut RgbBuffer, now: SystemTime) {
    let bounds = market_portal_bounds(buf.width(), buf.height());
    paint_portal_shimmer(buf, now, bounds);
}

fn paint_training_portal(buf: &mut RgbBuffer, pack: &Pack, now: SystemTime) {
    let bounds = training_portal_bounds(buf.width(), buf.height());
    let Some(animation) = pack
        .animation("training_portal")
        .filter(|animation| !animation.frames.is_empty())
    else {
        return;
    };
    let frame_ms = u64::from(animation.frame_ms.max(1));
    let frame = &animation.frames[(epoch_ms(now) / frame_ms) as usize % animation.frames.len()];
    blit_frame_nearest_to_size(
        frame,
        Point {
            x: bounds.x,
            y: bounds.y,
        },
        Size {
            w: bounds.width,
            h: bounds.height,
        },
        buf,
    );
}

fn paint_portal_shimmer(buf: &mut RgbBuffer, now: SystemTime, bounds: Bounds) {
    let phase = (epoch_ms(now) % MARKET_PORTAL_LOOP_MS) as f32 / MARKET_PORTAL_LOOP_MS as f32;
    let tau = std::f32::consts::TAU;
    for local_y in 0..bounds.height {
        let y = bounds.y.saturating_add(local_y);
        if y >= buf.height() {
            continue;
        }
        let ny = local_y as f32 / bounds.height.max(1) as f32;
        for local_x in 0..bounds.width {
            let x = bounds.x.saturating_add(local_x);
            if x >= buf.width() {
                continue;
            }
            let nx = local_x as f32 / bounds.width.max(1) as f32;
            let oval = (1.0 - ((nx - 0.5) / 0.52).powi(2)).max(0.0);
            let vertical_fade = (ny / 0.12).clamp(0.0, 1.0) * ((1.0 - ny) / 0.10).clamp(0.0, 1.0);
            let ribbon_a = 0.50 + 0.17 * (ny * 10.0 + phase * tau).sin();
            let ribbon_b = 0.50 + 0.23 * (ny * 7.0 - phase * tau * 1.35).sin();
            let beam = (1.0 - (nx - ribbon_a).abs() / 0.16).clamp(0.0, 1.0);
            let beam2 = (1.0 - (nx - ribbon_b).abs() / 0.10).clamp(0.0, 1.0);
            let pulse = 0.72 + 0.28 * (phase * tau + ny * tau * 2.0).sin().abs();
            let strength = (0.14 + beam * 0.46 + beam2 * 0.28) * oval * vertical_fade * pulse;
            if strength <= 0.01 {
                continue;
            }
            let current = buf.get(x, y);
            let glow = Rgb {
                r: (118.0 + 125.0 * beam2) as u8,
                g: (194.0 + 61.0 * beam2) as u8,
                b: 255,
            };
            let mix = |from: u8, to: u8| {
                (from as f32 + (to as f32 - from as f32) * strength.clamp(0.0, 0.82))
                    .round()
                    .clamp(0.0, 255.0) as u8
            };
            buf.put(
                x,
                y,
                Rgb {
                    r: mix(current.r, glow.r),
                    g: mix(current.g, glow.g),
                    b: mix(current.b, glow.b),
                },
            );
        }
    }
}

fn blit_frame_scaled(frame: &Frame, x: u16, y: u16, scale: u16, buf: &mut RgbBuffer) {
    let scale = scale.max(1);
    for src_y in 0..frame.height() {
        for src_x in 0..frame.width() {
            let Some(rgb) = frame.get(src_x, src_y).copied().flatten() else {
                continue;
            };
            let dst_x = x.saturating_add(src_x.saturating_mul(scale));
            let dst_y = y.saturating_add(src_y.saturating_mul(scale));
            for offset_y in 0..scale {
                for offset_x in 0..scale {
                    let out_x = dst_x.saturating_add(offset_x);
                    let out_y = dst_y.saturating_add(offset_y);
                    if out_x < buf.width() && out_y < buf.height() {
                        buf.put(out_x, out_y, rgb);
                    }
                }
            }
        }
    }
}

/// Sample an authored frame directly into a requested display rectangle using
/// nearest-neighbour coordinates. A 96x72 paperdoll therefore maps one-to-one
/// into the default 720x480 market instead of first collapsing through 32x24.
fn blit_frame_nearest_to_size(frame: &Frame, origin: Point, target: Size, buf: &mut RgbBuffer) {
    blit_frame_nearest_to_size_clipped(
        frame,
        i32::from(origin.x),
        i32::from(origin.y),
        target,
        buf,
    );
}

/// Nearest-neighbour blit with a signed origin. Large effect canvases may begin
/// above the top platform; clipping their off-screen rows must not move the
/// paperdoll's fixed foot baseline down into the map.
fn blit_frame_nearest_to_size_clipped(
    frame: &Frame,
    origin_x: i32,
    origin_y: i32,
    target: Size,
    buf: &mut RgbBuffer,
) {
    let (src_w, src_h) = (frame.width(), frame.height());
    if src_w == 0 || src_h == 0 || target.w == 0 || target.h == 0 {
        return;
    }
    for dst_y in 0..target.h {
        let src_y = ((u32::from(dst_y) * u32::from(src_h)) / u32::from(target.h)) as u16;
        for dst_x in 0..target.w {
            let src_x = ((u32::from(dst_x) * u32::from(src_w)) / u32::from(target.w)) as u16;
            let Some(rgb) = frame.get(src_x, src_y).copied().flatten() else {
                continue;
            };
            let out_x = origin_x + i32::from(dst_x);
            let out_y = origin_y + i32::from(dst_y);
            if out_x >= 0
                && out_y >= 0
                && out_x < i32::from(buf.width())
                && out_y < i32::from(buf.height())
            {
                buf.put(out_x as u16, out_y as u16, rgb);
            }
        }
    }
}

const TRAINING_SKILL_LOGICAL_SIZE: u16 = 32;
const TRAINING_SKILL_LOGICAL_ORIGIN_X: u16 = 16;
const TRAINING_SKILL_LOGICAL_ORIGIN_Y: u16 = 27;

fn paint_training_skill_effect(
    buf: &mut RgbBuffer,
    pack: &Pack,
    actor: crate::training::TrainingActorFrame,
    scale: u16,
) {
    let Some(effect) = actor.skill_effect else {
        return;
    };
    let animation_key = match effect.kind {
        crate::training::TrainingSkillKind::MagicClaw => "training_skill_magic_claw",
        crate::training::TrainingSkillKind::HolyLight => "training_skill_holy_light",
        crate::training::TrainingSkillKind::DragonPulse => "training_skill_dragon_pulse",
    };
    // Always keep the original zero-asset silhouette as a readability halo.
    // The installed local pack may add a licensed/custom frame on top, but a
    // sparse first frame must not make the whole 960 ms cast disappear.
    effects::paint_public_training_skill(
        buf,
        actor.foot_px,
        effect.kind,
        effect.frame_index,
        scale,
    );

    if let Some(frame) = pack
        .animation(animation_key)
        .and_then(|animation| animation.frames.get(effect.frame_index))
    {
        let size = TRAINING_SKILL_LOGICAL_SIZE.saturating_mul(scale);
        blit_frame_nearest_to_size_clipped(
            frame,
            i32::from(actor.foot_px.x)
                - i32::from(TRAINING_SKILL_LOGICAL_ORIGIN_X.saturating_mul(scale)),
            i32::from(actor.foot_px.y)
                - i32::from(TRAINING_SKILL_LOGICAL_ORIGIN_Y.saturating_mul(scale)),
            Size { w: size, h: size },
            buf,
        );
    }
}

/// Paint the bright forest training scene.  Background, monsters, paperdolls
/// and task labels all share `training`'s elapsed-time placement authority.
fn paint_training_frame(
    ctx: &mut PaintCtx<'_>,
    _frame: &SimFrame,
) -> (Option<PetFrame>, Vec<MascotFrame>) {
    if let Some(backdrop) = ctx
        .pack
        .animation("training_background")
        .and_then(|animation| animation.frames.first())
    {
        paint_market_backdrop(ctx.buf, backdrop, ctx.theme.surface.bg_fallback);
    } else {
        paint_training_fallback_backdrop(ctx.buf);
    }
    paint_training_portal(ctx.buf, ctx.pack, ctx.now);

    let viewport = Bounds {
        x: 0,
        y: 0,
        width: ctx.buf.width(),
        height: ctx.buf.height(),
    };
    let frame_ctx = crate::training::TrainingFrameContext {
        viewport,
        now: ctx.now,
    };
    let placements = crate::training::build_training_placements(ctx.scene, viewport);
    let paperdolls = crate::market::market_avatar_animation(ctx.pack);
    let standing = crate::market::market_avatar_stand_animation(ctx.pack);
    let walking = crate::market::market_avatar_walk_animation(ctx.pack);
    let climbing = crate::market::market_avatar_climb_animation(ctx.pack);
    let sitting = crate::market::market_avatar_sit_animation(ctx.pack);
    let alert = crate::market::market_avatar_alert_animation(ctx.pack);
    let attacking = crate::market::market_avatar_attack_animation(ctx.pack);
    let scale = crate::market::market_sprite_scale(viewport.height);

    let mut actors = placements
        .iter()
        .filter_map(|placement| {
            let agent = ctx.scene.agents.get(&placement.agent_id)?;
            let actor = crate::training::resolve_training_actor(agent, *placement, frame_ctx)?;
            let monster = crate::training::resolve_training_monster(agent, *placement, frame_ctx);
            Some((*placement, agent, actor, monster))
        })
        .collect::<Vec<_>>();
    actors.sort_by_key(|(_, _, actor, _)| actor.foot_px.y);

    for (_, _, _, monster) in &actors {
        if let Some(monster) = monster {
            paint_training_monster(ctx.buf, ctx.pack, *monster, scale);
        }
    }

    for (placement, agent, actor, _monster) in actors {
        if let Some(elapsed) =
            crate::market::market_turn_completion_elapsed(ctx.scene, agent.agent_id, ctx.now)
        {
            effects::paint_maple_level_up_pillar(ctx.buf, actor.foot_px, elapsed, scale);
        }
        if let Some(elapsed) = crate::market::market_command_success_elapsed(agent, ctx.now) {
            effects::paint_market_scroll_success(
                ctx.buf,
                Point {
                    x: actor.foot_px.x.saturating_sub(6u16.saturating_mul(scale)),
                    y: actor.sprite_anchor_px.y,
                },
                elapsed,
                scale,
            );
        }
        // Keep the paperdoll identity readable through the source's opaque
        // first flash: the skill is a separate back layer, never a body swap.
        paint_training_skill_effect(ctx.buf, ctx.pack, actor, scale);
        if let Some(animation) = paperdolls {
            let idle = &animation.frames[placement.appearance_index % animation.frames.len()];
            let (avatar, mirror, attack_canvas) = match actor.pose {
                crate::training::TrainingActorPose::Stand { frame_index } => standing
                    .map(|animation| {
                        let index = placement.appearance_index * crate::market::MARKET_STAND_FRAMES
                            + frame_index % crate::market::MARKET_STAND_FRAMES;
                        (&animation.frames[index], false, false)
                    })
                    .unwrap_or((idle, false, false)),
                crate::training::TrainingActorPose::Walk {
                    frame_index,
                    facing,
                } => walking
                    .map(|animation| {
                        let index = placement.appearance_index * crate::market::MARKET_WALK_FRAMES
                            + frame_index % crate::market::MARKET_WALK_FRAMES;
                        (
                            &animation.frames[index],
                            facing == crate::training::TrainingFacing::Right,
                            false,
                        )
                    })
                    .unwrap_or((idle, false, false)),
                crate::training::TrainingActorPose::Climb { frame_index } => climbing
                    .map(|animation| {
                        let index = placement.appearance_index * crate::market::MARKET_CLIMB_FRAMES
                            + frame_index % crate::market::MARKET_CLIMB_FRAMES;
                        (&animation.frames[index], false, false)
                    })
                    .unwrap_or((idle, false, false)),
                crate::training::TrainingActorPose::Attack { frame_index } => attacking
                    .map(|animation| {
                        let index = placement.appearance_index
                            * crate::market::MARKET_ATTACK_FRAMES
                            + frame_index % crate::market::MARKET_ATTACK_FRAMES;
                        (&animation.frames[index], true, true)
                    })
                    .or_else(|| {
                        alert.map(|animation| {
                            let index = placement.appearance_index
                                * crate::market::MARKET_ALERT_FRAMES
                                + frame_index % crate::market::MARKET_ALERT_FRAMES;
                            (&animation.frames[index], true, false)
                        })
                    })
                    .unwrap_or((idle, false, false)),
                crate::training::TrainingActorPose::Sit => sitting
                    .map(|animation| {
                        let index = placement.appearance_index * crate::market::MARKET_SIT_FRAMES;
                        (&animation.frames[index], false, false)
                    })
                    .unwrap_or((idle, false, false)),
            };
            let mirrored = mirror.then(|| avatar.mirror_horizontal());
            let avatar = mirrored.as_ref().unwrap_or(avatar);
            if attack_canvas {
                let target_size = Size {
                    w: crate::market::MARKET_AVATAR_ATTACK_WIDTH.saturating_mul(scale),
                    h: crate::market::MARKET_AVATAR_ATTACK_HEIGHT.saturating_mul(scale),
                };
                blit_frame_nearest_to_size_clipped(
                    avatar,
                    i32::from(actor.foot_px.x) - i32::from(target_size.w / 2),
                    i32::from(actor.foot_px.y) - i32::from(target_size.h),
                    target_size,
                    ctx.buf,
                );
            } else {
                blit_frame_nearest_to_size(
                    avatar,
                    actor.sprite_anchor_px,
                    Size {
                        w: crate::market::MARKET_AVATAR_WIDTH.saturating_mul(scale),
                        h: crate::market::MARKET_AVATAR_HEIGHT.saturating_mul(scale),
                    },
                    ctx.buf,
                );
            }
        } else {
            paint_training_fallback_adventurer(ctx.buf, actor, scale);
        }
        if actor.question_bubble {
            paint_training_question_bubble(ctx.buf, actor.sprite_anchor_px, scale);
        }
    }

    (None, Vec::new())
}

fn paint_training_fallback_backdrop(buf: &mut RgbBuffer) {
    let h = buf.height().max(1);
    let w = buf.width().max(1);
    for y in 0..h {
        let t = f32::from(y) / f32::from(h);
        let sky = Rgb {
            r: (70.0 + 82.0 * t) as u8,
            g: (142.0 + 72.0 * t) as u8,
            b: (224.0 + 24.0 * t) as u8,
        };
        for x in 0..w {
            buf.put(x, y, sky);
        }
    }
    let ridge_y = h * 38 / 100;
    for x in 0..w {
        let wave = ((u32::from(x) * 17 + 23) % 41) as u16;
        let top = ridge_y.saturating_sub(wave / 8);
        for y in top..h * 72 / 100 {
            buf.put(
                x,
                y,
                Rgb {
                    r: 48,
                    g: 126,
                    b: 91,
                },
            );
        }
    }
    for &(line_y, left, right) in &[
        (h * 26 / 100, w * 3 / 100, w * 89 / 100),
        (h * 56 / 100, w * 3 / 100, w * 89 / 100),
        (h * 86 / 100, 0, w),
    ] {
        paint_training_platform(buf, left, right, line_y);
    }
}

fn paint_training_platform(buf: &mut RgbBuffer, left: u16, right: u16, top: u16) {
    let right = right.min(buf.width());
    let grass_h = (buf.height() / 80).max(2);
    let soil_h = (buf.height() / 20).max(5);
    for y in top.saturating_sub(grass_h)..top {
        for x in left..right {
            let color = if (x + y) % 5 == 0 {
                Rgb {
                    r: 164,
                    g: 221,
                    b: 75,
                }
            } else {
                Rgb {
                    r: 78,
                    g: 164,
                    b: 58,
                }
            };
            buf.put(x, y, color);
        }
    }
    for y in top..top.saturating_add(soil_h).min(buf.height()) {
        for x in left..right {
            let color = if (x / 4 + y / 3) % 3 == 0 {
                Rgb {
                    r: 111,
                    g: 66,
                    b: 33,
                }
            } else {
                Rgb {
                    r: 75,
                    g: 43,
                    b: 30,
                }
            };
            buf.put(x, y, color);
        }
    }
}

fn paint_training_monster(
    buf: &mut RgbBuffer,
    pack: &Pack,
    monster: crate::training::TrainingMonsterFrame,
    scale: u16,
) {
    let (animation_name, death_animation_name, base_size) = match monster.kind {
        crate::training::TrainingMonsterKind::Slime => (
            "training_monster_slime",
            "training_monster_slime_die",
            Size { w: 19, h: 22 },
        ),
        crate::training::TrainingMonsterKind::GreenMushroom => (
            "training_monster_green_mushroom",
            "training_monster_green_mushroom_die",
            Size { w: 14, h: 13 },
        ),
    };
    let frame_and_size = match monster.pose {
        crate::training::TrainingMonsterPose::Alive { frame_index }
        | crate::training::TrainingMonsterPose::Respawning { frame_index } => pack
            .animation(animation_name)
            .filter(|animation| !animation.frames.is_empty())
            .map(|animation| {
                (
                    &animation.frames[frame_index % animation.frames.len()],
                    Size {
                        w: base_size.w.saturating_mul(scale),
                        h: base_size.h.saturating_mul(scale),
                    },
                )
            }),
        crate::training::TrainingMonsterPose::Dying { frame_index } => pack
            .animation(death_animation_name)
            .filter(|animation| !animation.frames.is_empty())
            .map(|animation| {
                (
                    &animation.frames[frame_index % animation.frames.len()],
                    Size {
                        w: base_size.w.saturating_mul(scale),
                        h: base_size.h.saturating_mul(scale),
                    },
                )
            }),
        crate::training::TrainingMonsterPose::Hidden => None,
    };
    if let Some((frame, size)) = frame_and_size {
        let mirrored = (monster.facing == crate::training::TrainingFacing::Right)
            .then(|| frame.mirror_horizontal());
        let frame = mirrored.as_ref().unwrap_or(frame);
        let origin = Point {
            x: monster.foot_px.x.saturating_sub(size.w / 2),
            y: monster.foot_px.y.saturating_sub(size.h),
        };
        blit_frame_nearest_to_size(frame, origin, size, buf);
    } else if !matches!(monster.pose, crate::training::TrainingMonsterPose::Hidden) {
        paint_training_fallback_monster(buf, monster, scale);
    }
}

fn paint_training_fallback_adventurer(
    buf: &mut RgbBuffer,
    actor: crate::training::TrainingActorFrame,
    scale: u16,
) {
    let body = Rgb {
        r: 52,
        g: 111,
        b: 176,
    };
    let skin = Rgb {
        r: 244,
        g: 199,
        b: 154,
    };
    let x = actor.foot_px.x.saturating_sub(3u16.saturating_mul(scale));
    let y = actor.foot_px.y.saturating_sub(10u16.saturating_mul(scale));
    fill_rect(
        buf,
        x,
        y,
        6u16.saturating_mul(scale),
        7u16.saturating_mul(scale),
        body,
    );
    fill_rect(
        buf,
        x.saturating_add(scale),
        y.saturating_sub(4u16.saturating_mul(scale)),
        4u16.saturating_mul(scale),
        4u16.saturating_mul(scale),
        skin,
    );
}

fn paint_training_fallback_monster(
    buf: &mut RgbBuffer,
    monster: crate::training::TrainingMonsterFrame,
    scale: u16,
) {
    let color = match monster.kind {
        crate::training::TrainingMonsterKind::Slime => Rgb {
            r: 92,
            g: 214,
            b: 50,
        },
        crate::training::TrainingMonsterKind::GreenMushroom => Rgb {
            r: 91,
            g: 125,
            b: 67,
        },
    };
    let w = 9u16.saturating_mul(scale);
    let h = 5u16.saturating_mul(scale);
    fill_rect(
        buf,
        monster.foot_px.x.saturating_sub(w / 2),
        monster.foot_px.y.saturating_sub(h),
        w,
        h,
        color,
    );
}

fn paint_training_question_bubble(buf: &mut RgbBuffer, head: Point, scale: u16) {
    let w = 8u16.saturating_mul(scale);
    let h = 7u16.saturating_mul(scale);
    let x = head.x.saturating_add(20u16.saturating_mul(scale));
    let y = head.y.saturating_sub(4u16.saturating_mul(scale));
    fill_rect(
        buf,
        x,
        y,
        w,
        h,
        Rgb {
            r: 251,
            g: 248,
            b: 224,
        },
    );
    fill_rect(
        buf,
        x,
        y,
        w,
        scale,
        Rgb {
            r: 73,
            g: 60,
            b: 45,
        },
    );
    fill_rect(
        buf,
        x,
        y,
        scale,
        h,
        Rgb {
            r: 73,
            g: 60,
            b: 45,
        },
    );
    let ink = Rgb {
        r: 59,
        g: 85,
        b: 128,
    };
    let qx = x.saturating_add(3u16.saturating_mul(scale));
    fill_rect(buf, qx, y.saturating_add(2 * scale), 2 * scale, scale, ink);
    fill_rect(
        buf,
        qx.saturating_add(scale),
        y.saturating_add(3 * scale),
        scale,
        2 * scale,
        ink,
    );
    fill_rect(
        buf,
        qx.saturating_add(scale),
        y.saturating_add(6 * scale),
        scale,
        scale,
        ink,
    );
}

fn fill_rect(buf: &mut RgbBuffer, x: u16, y: u16, w: u16, h: u16, color: Rgb) {
    for py in y..y.saturating_add(h).min(buf.height()) {
        for px in x..x.saturating_add(w).min(buf.width()) {
            buf.put(px, py, color);
        }
    }
}

/// Paint the isolated Free Market scene. This path deliberately bypasses the
/// entire office furniture/wall pipeline: the scene plate already contains the
/// three platform layers. Each live merchant and its floating label consume the
/// same time-resolved entry/exit position authority.
fn paint_market_frame(
    ctx: &mut PaintCtx<'_>,
    frame: &SimFrame,
) -> (Option<PetFrame>, Vec<MascotFrame>) {
    let Some(backdrop) = ctx
        .pack
        .animation("scene_background")
        .and_then(|animation| animation.frames.first())
    else {
        return (None, Vec::new());
    };
    paint_market_backdrop(ctx.buf, backdrop, ctx.theme.surface.bg_fallback);
    paint_market_portal(ctx.buf, ctx.now);

    let viewport = Bounds {
        x: 0,
        y: 0,
        width: ctx.buf.width(),
        height: ctx.buf.height(),
    };
    let placements = crate::market::build_market_placements(ctx.scene, viewport);
    let sprite_scale = crate::market::market_sprite_scale(viewport.height);
    let market_frame = crate::market::MarketFrameContext {
        viewport,
        now: ctx.now,
    };
    let paperdolls = crate::market::market_avatar_animation(ctx.pack);
    let standing_paperdolls = crate::market::market_avatar_stand_animation(ctx.pack);
    let walking_paperdolls = crate::market::market_avatar_walk_animation(ctx.pack);
    let climbing_paperdolls = crate::market::market_avatar_climb_animation(ctx.pack);
    let stand2_paperdolls = crate::market::market_avatar_stand2_animation(ctx.pack);
    let sitting_paperdolls = crate::market::market_avatar_sit_animation(ctx.pack);
    let alert_paperdolls = crate::market::market_avatar_alert_animation(ctx.pack);

    let actors = placements
        .iter()
        .filter_map(|placement| {
            let agent = frame
                .agents
                .iter()
                .find(|agent| agent.agent_id == placement.agent_id)?;
            let actor = if paperdolls.is_some() {
                crate::market::resolve_market_paperdoll(agent, *placement, market_frame)
            } else {
                crate::market::resolve_market_merchant(agent, *placement, market_frame)
            }?;
            Some((*placement, agent, actor))
        })
        .collect::<Vec<_>>();

    // A shop exists only after its merchant reaches the assigned platform and
    // closes immediately on SessionEnd. Paint open stalls before actors so the
    // paperdoll remains readable in front of the counter.
    if let Some(stall_anim) = ctx.pack.animation("market_stall") {
        if !stall_anim.frames.is_empty() {
            let frame_ms = u64::from(stall_anim.frame_ms.max(1));
            let stall_idx = (epoch_ms(ctx.now) / frame_ms) as usize % stall_anim.frames.len();
            let stall = &stall_anim.frames[stall_idx];
            for (_, _, actor) in actors.iter().filter(|(_, _, actor)| actor.stall_open) {
                let scaled_stall_w = stall.width().saturating_mul(sprite_scale);
                let centre_x = actor.foot_px().x;
                let stall_x = centre_x.saturating_sub(scaled_stall_w / 2);
                let stall_y = actor
                    .foot_px()
                    .y
                    .saturating_sub(crate::market::MARKET_STALL_FOOT_OVERLAP * sprite_scale);
                blit_frame_scaled(stall, stall_x, stall_y, sprite_scale, ctx.buf);
            }
        }
    }

    for (placement, agent, actor) in actors {
        if let Some(elapsed_ms) =
            crate::market::market_turn_completion_elapsed(ctx.scene, agent.agent_id, ctx.now)
        {
            effects::paint_maple_level_up_pillar(
                ctx.buf,
                actor.foot_px(),
                elapsed_ms,
                sprite_scale,
            );
        }
        if let Some(elapsed_ms) = crate::market::market_command_success_elapsed(agent, ctx.now) {
            effects::paint_market_scroll_success(
                ctx.buf,
                actor.command_effect_anchor(paperdolls.is_some(), sprite_scale),
                elapsed_ms,
                sprite_scale,
            );
        }
        let actor_anchor = if let Some(animation) = paperdolls {
            let idle = &animation.frames[placement.appearance_index % animation.frames.len()];
            let (avatar, mirror) = match actor.pose {
                crate::market::MarketActorPose::Stand { frame_index } => standing_paperdolls
                    .map(|standing| {
                        let index = placement.appearance_index * crate::market::MARKET_STAND_FRAMES
                            + frame_index % crate::market::MARKET_STAND_FRAMES;
                        (&standing.frames[index], false)
                    })
                    .unwrap_or((idle, false)),
                crate::market::MarketActorPose::Stand2 { frame_index } => stand2_paperdolls
                    .map(|standing| {
                        let index = placement.appearance_index
                            * crate::market::MARKET_STAND2_FRAMES
                            + frame_index % crate::market::MARKET_STAND2_FRAMES;
                        (&standing.frames[index], false)
                    })
                    .unwrap_or((idle, false)),
                crate::market::MarketActorPose::Sit => sitting_paperdolls
                    .map(|sitting| {
                        let index = placement.appearance_index * crate::market::MARKET_SIT_FRAMES;
                        (&sitting.frames[index], false)
                    })
                    .unwrap_or((idle, false)),
                crate::market::MarketActorPose::Alert { frame_index } => alert_paperdolls
                    .map(|alert| {
                        let index = placement.appearance_index * crate::market::MARKET_ALERT_FRAMES
                            + frame_index % crate::market::MARKET_ALERT_FRAMES;
                        (&alert.frames[index], false)
                    })
                    .unwrap_or((idle, false)),
                crate::market::MarketActorPose::Walk(walk) => walking_paperdolls
                    .map(|walking| {
                        let index = placement.appearance_index * crate::market::MARKET_WALK_FRAMES
                            + walk.frame_index % crate::market::MARKET_WALK_FRAMES;
                        (
                            &walking.frames[index],
                            walk.facing == crate::market::MarketFacing::Right,
                        )
                    })
                    .unwrap_or((idle, false)),
                crate::market::MarketActorPose::Climb { frame_index } => climbing_paperdolls
                    .map(|climbing| {
                        let index = placement.appearance_index * crate::market::MARKET_CLIMB_FRAMES
                            + frame_index % crate::market::MARKET_CLIMB_FRAMES;
                        (&climbing.frames[index], false)
                    })
                    .unwrap_or((idle, false)),
            };
            let mirrored = mirror.then(|| avatar.mirror_horizontal());
            let avatar = mirrored.as_ref().unwrap_or(avatar);
            blit_frame_nearest_to_size(
                avatar,
                actor.sprite_anchor_px,
                Size {
                    w: crate::market::MARKET_AVATAR_WIDTH.saturating_mul(sprite_scale),
                    h: crate::market::MARKET_AVATAR_HEIGHT.saturating_mul(sprite_scale),
                },
                ctx.buf,
            );
            actor.sprite_anchor_px
        } else {
            let glow_tint = matches!(&agent.state, ActivityState::Active { .. })
                .then(|| palette::tool_glow_tint(agent, &ctx.theme.tool_glow))
                .flatten();
            paint_character_at_scaled(
                ctx.buf,
                "standing",
                0,
                actor.sprite_anchor_px,
                agent,
                ctx.pack,
                false,
                glow_tint,
                ctx.cache,
                ctx.now,
                sprite_scale,
            );
            actor.sprite_anchor_px
        };
        if matches!(actor.pose, crate::market::MarketActorPose::Sit) {
            effects::paint_waiting_bubble_scaled(ctx.buf, actor_anchor, ctx.theme, sprite_scale);
        }
    }

    (None, Vec::new())
}

/// The PAINT half of the frame: blit the world the sim already advanced.
/// Reads the [`SimFrame`] immutably; every positional/lifecycle decision was
/// made in `sim_step` — this pass only resolves presentation (theme colors,
/// sprite pixels) and composites. Returns the resolved pet frame + every mascot
/// frame for the caller's hit-testing.
#[cfg(test)]
fn paint_frame(ctx: &mut PaintCtx<'_>, frame: &SimFrame) -> (Option<PetFrame>, Vec<MascotFrame>) {
    paint_frame_for_map(ctx, frame, None)
}

fn paint_frame_for_map(
    ctx: &mut PaintCtx<'_>,
    frame: &SimFrame,
    maple_map: Option<crate::maple_world::MapleMapId>,
) -> (Option<PetFrame>, Vec<MascotFrame>) {
    if ctx.theme.name == "maple" {
        match maple_map {
            Some(crate::maple_world::MapleMapId::FreeMarket) => {
                return paint_market_frame(ctx, frame);
            }
            Some(crate::maple_world::MapleMapId::ForestTraining) => {
                return paint_training_frame(ctx, frame);
            }
            None if ctx.pack.animation("scene_background").is_some() => {
                // Compatibility for the existing TUI/snapshot paths that select
                // the Maple theme but do not own a multi-map world session.
                return paint_market_frame(ctx, frame);
            }
            None => {}
        }
    }

    let agents: &[AgentSlot] = &frame.agents;
    let buf_w = ctx.layout.buf_w;
    let buf_h = ctx.layout.buf_h;

    // Compute time-of-day once per frame and pass to every paint
    // helper that depends on it. Avoids recomputing the chrono local
    // hour for each window + ceiling pool + lamp halo.
    let look = time_of_day_look(ctx.now, ctx.theme);
    // Wall band height tracks layout.top_margin (which is buf_h/4 with
    // a floor) — leaves a 4-px buffer between wall trim and cubicles.
    let top_wall_h = ctx.layout.wall_band_h();
    // The elevator door replaces the rightmost window — pass its x-range
    // so `paint_floor_and_walls` skips drawing a window that would
    // otherwise bleed through behind the elevator frame.
    let door_x_range = ctx.layout.door.map(|d| (d.x, d.x + ELEVATOR_W));
    paint_floor_and_walls(
        ctx.buf,
        buf_w,
        buf_h,
        ctx.now,
        &look,
        top_wall_h,
        door_x_range,
        ctx.theme,
        ctx.floor.altitude,
    );

    // Per-floor lighting: `sim_step` already ticked the fade state with the
    // current occupancy. `indoor_scale` smoothly travels from MIN_LEVEL
    // (empty + past debounce) to 1.0 (populated). Windows/skyline are
    // unaffected.
    let indoor_scale = frame.indoor_scale;
    // Empty floors get an extra floor-darken boost on top of the time-of-
    // day dim — there are no monitor/lamp light sources to balance against
    // the overhead darkness, so without the boost they read as "lights
    // off but room weirdly bright."
    let min_level = LightingState::MIN_LEVEL;
    let boost_ceiling = LightingState::EMPTY_FLOOR_DIM_BOOST;
    let empty_floor_boost = 1.0 + (1.0 - indoor_scale) * (boost_ceiling - 1.0) / (1.0 - min_level);

    // The night floor-dim dial (symmetric with `DAYLIGHT_FLOOR_LIFT` below); the
    // per-floor lighting offset it replaced was always 0 (indoor lighting is
    // uniform across floors), so this is now a flat constant.
    const NIGHT_FLOOR_DIM_STRENGTH: f32 = 0.45;
    let dim_strength = NIGHT_FLOOR_DIM_STRENGTH;
    dim_floor_overlay(
        ctx.buf,
        top_wall_h,
        buf_h,
        look.darkness * dim_strength * empty_floor_boost,
        ctx.theme,
    );
    // Daytime warm light-lift — the positive mirror of the night dim above.
    // Brightens/warms the floor in proportion to effective daylight
    // (`spill_strength` = `day_eff`), so sunny days read sunlit instead of flat
    // carpet. Independent of occupancy (sun enters an empty office too) and a
    // no-op at night where `day_eff` is 0. `DAYLIGHT_FLOOR_LIFT` is the dial.
    const DAYLIGHT_FLOOR_LIFT: f32 = 0.22;
    daylight_floor_overlay(
        ctx.buf,
        top_wall_h,
        buf_h,
        look.spill_strength * DAYLIGHT_FLOOR_LIFT,
    );
    let pool_strength = (0.15 + 0.30 * look.darkness) * indoor_scale;
    // Ceiling fluorescents (one narrow tube per desk + a wider fixture over the
    // pantry and the corridor) so the floor is lit consistently with the
    // lounge_band gone — geometry single-sourced in `ceiling_pool_regions`.
    for pool in ceiling_pool_regions(ctx.layout) {
        paint_ceiling_pool(ctx.buf, pool, pool_strength, ctx.theme);
    }
    if let Some(lamp) = ctx.layout.floor_lamp() {
        paint_floor_lamp_halo(
            ctx.buf,
            lamp.x,
            lamp.y + floor_lamp_south_offset(), // glow emanates from the lamp BASE, not the pole
            look.darkness * 0.55 * indoor_scale,
            ctx.theme,
        );
    }

    // Neon sign panel in the wall band — dark bg with glow border.
    // Text overlay (branding, dots, star link) is rendered by the ratatui
    // widget pass in renderer.rs::paint_wall_display.
    paint_neon_panel(
        ctx.buf,
        NEON_PANEL_X,
        NEON_PANEL_Y,
        NEON_PANEL_W,
        NEON_PANEL_H,
        ctx.now,
        ctx.theme,
    );

    // Live wall clock painted after the wall (so hands sit on top of it)
    // but before wall decor — the bookshelf etc. shouldn't cover it.
    // 7x7 sprite, center at clock_x+3; clamp so it never collides with
    // the neon panel on the left (its right edge + a 1px gap).
    let clock_x = (buf_w / 2)
        .saturating_sub(3)
        .max(NEON_PANEL_X + NEON_PANEL_W + 1);
    paint_clock(ctx.buf, clock_x, 1, ctx.now, ctx.theme);
    // Corridor runner — painted over the floor but BEFORE walls/decor
    // so walls cleanly overlap it where they cross.
    if let Some(corridor) = ctx.layout.corridor {
        paint_corridor_runner(ctx.buf, corridor, ctx.theme);
    }
    // Room dividers — frosted-glass partitions (see the module-level glass
    // helpers + WALL_THICK_*_PX). BOTH orientations now join the y-sorted
    // drawable pass below (`enqueue_room_walls_h`/`_v`), anchored at their south
    // base so a walker standing behind either wall's north cap composites behind
    // the frosted glass. Nothing wall-scale paints in this background pass.

    // Meeting sofas + table and the kitchen island paint in the y-sorted
    // Drawable pass below — nothing room-scale belongs in this background
    // pass, or it double-paints under the sorted copy.

    // Procedural room fill — small pixel items that make rooms feel lived-in.
    // Ground footprint rule: walkable mask is NOT affected by these (they're
    // small items characters can walk around or over).
    // Per-room decor: EVERY meeting room, keyed by its own bounds (not room 0).
    for room in &ctx.layout.meeting_rooms {
        furniture::paint_notice_board(ctx.buf, room.bounds, ctx.theme);

        // Coat rack is a y-sorted DrawableKind::CoatRack (pushed in the drawable
        // pass) so characters in front occlude it and those behind are occluded.

        furniture::paint_doormat(ctx.buf, room, ctx.theme);
    }
    // Soft goods (decor arc) paint FIRST: floor-level mats sit under every
    // upright pantry fixture — on a narrow pantry the entry mat's box reaches
    // the water-cooler column, and mats-after-cooler would clip the cooler's
    // west edge.
    furniture::paint_pantry_entry_mat(ctx.buf, ctx.layout, ctx.theme);
    furniture::paint_island_bar_mat(ctx.buf, ctx.layout, ctx.theme);
    if let Some(pantry) = &ctx.layout.pantry {
        furniture::paint_water_cooler(ctx.buf, pantry, ctx.now, ctx.theme);
        furniture::paint_trash_bin(ctx.buf, pantry);
    }

    // Shadow pass — soft floor shadows under desks + lounge furniture
    // so nothing floats. Painted BEFORE the y-sorted entity pass so
    // every entity sits on top of its own shadow. Strength is a
    // function of daylight so noon shadows are crisp and night shadows
    // are subtle.
    let shadow_strength = 0.5 - 0.3 * look.darkness;
    for ell in floor_shadow_ellipses(ctx.layout) {
        paint_shadow(ctx.buf, ell, shadow_strength, ctx.theme);
    }

    // Ceiling halos gate on the sim's `seated_agents` so a tool-glow halo
    // never floats above an empty desk while its Active occupant is mid-walk
    // (entry/snap). `look` was already computed once per frame above —
    // forward it so the ambient sub-passes don't recompute
    // `time_of_day_look(now, theme)`.
    ambient::paint_ambient(ctx, &look, &frame.seated_agents);

    // --- Build the y-sortable middle pass -------------------------------
    //
    // Every entity gets an `anchor_y` representing its front-facing /
    // floor-touching row. Sort ascending and paint in order so things
    // closer to the camera (larger anchor_y) appear in front. This is
    // the painter's algorithm applied to a top-down 2D scene.

    // A HINT, not a bound (the vec still grows): one push per cubicle / character
    // / waypoint / decor item, so the pushes below skip the doubling ladder.
    let mut drawables: Vec<Drawable<'_>> = Vec::with_capacity(
        ctx.layout.home_desks.len()
            + ctx.layout.waypoints.len()
            + ctx.layout.plants.len()
            + ctx.layout.pod_decor.len()
            + ctx.layout.wall_decor.len()
            + agents.len(),
    );

    enqueue_desk_cubicles(ctx, agents, &frame.seated_agents, &mut drawables);

    enqueue_meeting_furniture(ctx.layout, &mut drawables);

    enqueue_lounge_pantry_appliances(ctx.layout, &frame.occupied_waypoints, &mut drawables);

    enqueue_pod_decor_and_plants(ctx.layout, &mut drawables);
    enqueue_floor_fixtures(ctx, agents, &mut drawables);
    enqueue_wall_decor(ctx.layout, &mut drawables);

    let resolved_pet_pos = enqueue_pet(ctx, agents, &mut drawables);
    let resolved_mascots = enqueue_gateway_mascots(ctx, &mut drawables);

    enqueue_characters(ctx, frame, &mut drawables);

    // V before H: at an inside corner the vertical's stitched `y_bot` (extended
    // down into the crossing wall to fill the L-notch) ties the horizontal's
    // south-base anchor. Old behavior painted the vertical in the BACKGROUND and
    // the horizontal over it; inserting V first keeps H winning that tie (stable
    // sort), so the corner pixels don't churn. Both come after the characters, so
    // either wall still occludes a walker tied with its row.
    enqueue_room_walls_v(ctx.layout, top_wall_h, &mut drawables);
    enqueue_room_walls_h(ctx.layout, &mut drawables);

    // Stable sort (Rust's `sort_by_key` is stable) — ties preserve
    // insertion order. Insertion order above: decor first, characters
    // last, so a character tied with a piece of furniture paints
    // BEFORE the furniture (matches the prior pass-1 → pass-1.5
    // → pass-2 layering for waypoint couch / pantry counter).
    drawables.sort_by_key(|d| d.anchor_y);
    // Occlusion is emergent now: every overhanging object's mask footprint is a
    // shallow south-anchored ground strip, so a walker parks DEEP behind it and
    // the object's own sprite (y-sorted at its south base, painted after the
    // walker) hides their lower body — no snapshot, no synthetic back-cap.
    for d in &drawables {
        paint_drawable(d, ctx.buf, ctx.pack, ctx.cache, ctx.now, ctx.theme);
    }

    // Room-wide lightning bounce — LAST, so a Storm strike briefly flares the
    // whole interior (floor, walls, furniture, characters), not just the window
    // strip. No-op outside a strike / non-storm weather.
    background::paint_lightning_flash(ctx.buf, ctx.now, background::weather_state(ctx.now));

    // Debug layer (the `w` toggle) — composited LAST, over the finished scene:
    // walkable mask + approach sides + live A* routes. Off by default.
    if ctx.debug_walkable {
        debug_overlay::paint(ctx.buf, ctx.layout, ctx.scene, ctx.motion);
    }

    (resolved_pet_pos, resolved_mascots)
}

/// Map the sim's resolved [`sim::CharacterPlacement`]s 1:1 onto y-sorted
/// drawables. Every positional decision (pose, anchor, z-key, sprite pick,
/// rank fan-out) was made by `sim_step`; the ONLY paint-side work here is
/// presentation — resolving the theme-free [`CharacterGlow`] to a `Theme`
/// color. The Character drawable borrows its agent from `frame.agents`, so
/// this is the ONE phase tied to the frame's lifetime `'a`.
fn enqueue_characters<'a>(
    ctx: &PaintCtx<'_>,
    frame: &'a SimFrame,
    drawables: &mut Vec<Drawable<'a>>,
) {
    for p in &frame.characters {
        let agent = &frame.agents[p.agent_idx];
        let glow_tint = match p.glow {
            CharacterGlow::None => None,
            CharacterGlow::Thinking => Some(ctx.theme.tool_glow.default),
            CharacterGlow::Tool => palette::tool_glow_tint(agent, &ctx.theme.tool_glow),
        };
        drawables.push(Drawable {
            anchor_y: p.anchor_y,
            kind: DrawableKind::Character {
                agent,
                anim_name: p.anim_name,
                frame_idx: p.frame_idx,
                anchor: p.anchor,
                flip_x: p.flip_x,
                glow_tint,
                sleep_z_seed: p.sleep_z_seed,
                waiting_bubble: p.waiting_bubble,
                walking_dust_frame: p.walking_dust_frame,
            },
        });
    }
}

/// The frame to paint for `idx`, clamped into range: a custom `--pack-dir`
/// animation whose sprite has fewer frames than the shared cycle's `frame_idx`
/// would otherwise yield `None` and vanish the sprite, so fall back to the
/// first frame. `None` only for a genuinely empty animation (the caller skips).
/// The ONE spelling of this out-of-range guard — was open-coded at four sites.
pub(super) fn frame_at(anim: &Sprite, idx: usize) -> Option<&Frame> {
    anim.frames.get(idx).or_else(|| anim.frames.first())
}

/// Desk cubicles — each carries its divider + cabinet + screen glow.
/// The desk sprite (14×7) sorts at `desk.y + visual.h` = `desk.y + 7`
/// (`DESK_H + 2`) — one row past its visual south
/// row, just past the seated worker's feet (`desk.y + 4`) so the sitter stays
/// visually behind the desk. Z is a VISUAL property: it tracks the sprite, not
/// the blocked ground — which is why the walk-behind footprint change (shrinking
/// the ground to a shallow south strip, #551) was z-neutral by construction.
/// `seated_agents` (built once before the ambient pass) gates the screen glow
/// so it only paints for a worker actually at the desk. The DeskCubicle
/// drawable is Copy, so this borrows nothing from the agent set.
///
/// A pod divider divides two pod-MATES, so `divider_x` is `Some` only where the
/// east mate exists, and `home_desks` is the authority for that: the band clamp
/// in `compute_pod_desks` drops a pod's second column when it wouldn't fit, so
/// pitch/band arithmetic here would be a second, drifting copy of the emission
/// rule. The column sits mid-aisle between the two desk SPRITES, clear of both.
fn enqueue_desk_cubicles<'a>(
    ctx: &PaintCtx<'_>,
    agents: &[AgentSlot],
    seated_agents: &HashMap<FloorLocalDeskIndex, bool>,
    drawables: &mut Vec<Drawable<'a>>,
) {
    for (i, &desk) in ctx.layout.home_desks.iter().enumerate() {
        let local = FloorLocalDeskIndex(i);
        let desk_def = crate::layout::desk_furniture_def();
        let Some(Size { w: desk_fp_w, .. }) = desk_def.footprint else {
            continue;
        };
        let mate_x = desk.x + DESK_W + crate::layout::INTRA_POD_GAP_X;
        let divider_x = ctx
            .layout
            .home_desks
            .iter()
            .any(|d| d.y == desk.y && d.x == mate_x)
            .then(|| (desk.x + desk_fp_w + mate_x) / 2);
        let occupant = agents
            .iter()
            .find(|a| a.desk_index.single_floor_local() == local && a.exiting_at.is_none());
        let screen_glow = occupant
            .filter(|_| seated_agents.get(&local).copied().unwrap_or(false))
            .and_then(|a| palette::tool_glow_tint(a, &ctx.theme.tool_glow));
        let has_coffee = occupant.is_some_and(|a| ctx.coffee.contains_key(&a.agent_id));
        let coffee_steam = has_coffee
            && occupant.is_some_and(|a| {
                ctx.coffee
                    .get(&a.agent_id)
                    .and_then(|t| ctx.now.duration_since(*t).ok())
                    .is_some_and(|d| d.as_secs() < COFFEE_STEAM_WINDOW_SECS)
            });
        // Token meter (#632): the tower tracks the OCCUPANT's cumulative
        // fresh-token counter; an exiting/empty desk shows no tower (the
        // occupant filter above already excludes exiting slots).
        let token_tier = occupant.map_or(0, |a| crate::token_meter::token_tier(a.tokens_used));
        let sheet_fall = occupant.and_then(|a| crate::token_meter::sheet_fall_dist(a, ctx.now));
        drawables.push(Drawable {
            anchor_y: desk.y + desk_def.visual.h,
            kind: DrawableKind::DeskCubicle {
                desk,
                divider_x,
                has_cabinet: i % 2 == 0,
                screen_glow,
                has_coffee,
                coffee_steam,
                token_tier,
                sheet_fall,
            },
        });
    }
}

/// Nudge a CENTER-anchored sprite's position so the whole sprite lands inside
/// the canvas.
///
/// Free-roaming creatures draw their destination from the WHOLE walkable mask
/// (`creatures::walkable_target`), which reaches within a few columns of the
/// buffer edge — and `blit_centered` spans `pos ± size/2` and clips silently, so
/// a lobster resting there rendered sliced in half. Overhanging FURNITURE and
/// walls is invariant #6 and stays; overhanging the CANVAS is not a thing the
/// mask can express. Clamping HERE keeps `mascot_position`/`pet_position` pure
/// functions of `now` + presence + seed, and keeps the hover box (`MascotFrame`
/// / `PetFrame` carry this same point) on the pixels actually drawn.
fn keep_sprite_on_canvas(pos: Point, w: u16, h: u16, buf_w: u16, buf_h: u16) -> Point {
    // `min` before `max`: on a buffer narrower than the sprite the lower bound
    // wins (sprite flush left/top) instead of `clamp`'s inverted-range panic.
    Point {
        x: pos.x.min(buf_w.saturating_sub(w.div_ceil(2))).max(w / 2),
        y: pos.y.min(buf_h.saturating_sub(h.div_ceil(2))).max(h / 2),
    }
}

/// The office pet (one per floor). An `active_pet` (mid heart-animation) is
/// pinned in place; otherwise `pet_position` roams it around the idle desks.
/// Returns the resolved `PetFrame` (for hit-testing) and enqueues the Pet
/// drawable, y-sorted at the chosen anim's south row (the h=4 sleep sprite
/// sorts one row shallower than the h=6 walk/sit sprites — a hardcoded +2 once
/// painted a sleeping pet over a character whose feet land at pos.y+1).
fn enqueue_pet<'a>(
    ctx: &PaintCtx<'_>,
    agents: &[AgentSlot],
    drawables: &mut Vec<Drawable<'a>>,
) -> Option<PetFrame> {
    let kind = ctx.floor_pet.map(|p| p.kind)?;
    let idle_desk_indices: Vec<FloorLocalDeskIndex> = agents
        .iter()
        .filter(|a| {
            matches!(a.state, ActivityState::Idle)
                && ctx
                    .layout
                    .home_desk(a.desk_index.single_floor_local())
                    .is_some()
                && a.exiting_at.is_none()
        })
        .map(|a| a.desk_index.single_floor_local())
        .collect();
    let all_idle = agents
        .iter()
        .all(|a| matches!(a.state, ActivityState::Idle));

    let active_pet = ctx
        .active_pet
        .filter(|p| p.is_active(ctx.now) && p.kind == kind && p.floor_idx == ctx.floor.floor_idx);
    let pet_data = if let Some(pet) = active_pet {
        Some((
            pet.pet_pos,
            false,
            kind.sit_anim(),
            0usize,
            Some(pet.elapsed_ms(ctx.now)),
        ))
    } else {
        pet_position(
            kind,
            ctx.layout,
            ctx.pack,
            ctx.now,
            &idle_desk_indices,
            all_idle,
            ctx.floor.floor_seed,
        )
        .map(|(pos, flip, anim, frame)| (pos, flip, anim, frame, None))
    };
    let (pos, flip, anim_name, frame_idx, pet_elapsed) = pet_data?;
    /// Fallback when a custom pack lacks the resolved pet anim: the bundled cat's
    /// size (the z-anchor's long-standing `6`), so the z-sort row and the canvas
    /// clamp stay sane — the blit itself no-ops, `paint_drawable` bails.
    const PET_FALLBACK: Size = Size { w: 8, h: 6 };
    let (pet_w, pet_h) = ctx
        .pack
        .animation(anim_name)
        .and_then(|a| a.frames.first())
        .map_or((PET_FALLBACK.w, PET_FALLBACK.h), |f| {
            (f.width(), f.height())
        });
    let pos = keep_sprite_on_canvas(pos, pet_w, pet_h, ctx.layout.buf_w, ctx.layout.buf_h);
    drawables.push(Drawable {
        anchor_y: z_sort_row(Anchor::Center, pos, pet_h),
        kind: DrawableKind::Pet {
            kind,
            pos,
            flip,
            anim_name,
            frame_idx,
            pet_elapsed_ms: pet_elapsed,
        },
    });
    Some(PetFrame {
        pos,
        anim: anim_name,
        kind,
    })
}

/// Enqueue every gateway mascot present in `daemons` (only the ground floor
/// carries the roster, so each mascot shows once). Presence-gated: an absent
/// entry draws nothing, so the ~99% who don't run a gateway see a normal office.
/// The runtime is responsible for KEEPING the roster honest — a never-connected or
/// panel-disconnected gateway has no live entry (the driver's presence
/// connection-gate drops its hooks and the sweep walks any lingering entry out),
/// so "entry present" tracks "connected + alive", not merely "a hook arrived".
/// y-sorted at each mascot's south row. Returns ONE frame per drawn mascot — N
/// concurrent gateways are N independently hoverable lobsters, with no arity
/// assumption anywhere on this path (the roster is a map, not a pair).
fn enqueue_gateway_mascots<'a>(
    ctx: &PaintCtx<'_>,
    drawables: &mut Vec<Drawable<'a>>,
) -> Vec<MascotFrame> {
    let mut frames = Vec::new();
    for (source, instance, presence) in ctx.scene.daemons() {
        let Some(def) = gateway_mascot_def(source) else {
            continue;
        };
        let seed = crate::creatures::mascot_seed(source, instance);
        let Some((pos, anim_name, frame_idx)) = mascot_position(
            ctx.layout, ctx.pack, presence, def.walk, def.rest, ctx.now, seed,
        ) else {
            continue;
        };
        let (mascot_w, mascot_h) = ctx
            .pack
            .animation(anim_name)
            .and_then(|a| a.frames.first())
            .map_or((14, 12), |f| (f.width(), f.height()));
        let pos =
            keep_sprite_on_canvas(pos, mascot_w, mascot_h, ctx.layout.buf_w, ctx.layout.buf_h);
        let run_count = presence.in_flight_runs.len() as u32;
        let degraded = presence.display_state() == pixtuoid_core::state::DaemonState::Degraded;
        drawables.push(Drawable {
            anchor_y: z_sort_row(Anchor::Center, pos, mascot_h),
            kind: DrawableKind::GatewayMascot {
                pos,
                anim_name,
                frame_idx,
                run_count,
                degraded,
            },
        });
        frames.push(MascotFrame {
            pos,
            w: mascot_w,
            h: mascot_h,
            name: def.display_name,
            // Only worth showing when there is something to disambiguate, and that
            // is per SOURCE: two gateways of ONE daemon need their ports, while a
            // second daemon source with one instance each already reads apart by
            // name and sprite. Re-counting per mascot is free — the roster holds one
            // row per LIVE gateway (1–2 in practice), and this fn only runs at all
            // when it is non-empty.
            instance: (ctx.scene.daemons().filter(|(s, _, _)| *s == source).count() > 1)
                .then(|| instance.as_str().to_string()),
            busy: presence.is_busy(),
            degraded,
            active_sessions: presence.active_sessions,
        });
    }
    frames
}

/// Meeting-room rugs + sofas + tables. For dual-meeting layouts sofas come in
/// pairs (2 per room), tables 1 per room. A south-of-table sofa faces away
/// (`Facing::North` → `back_couch`), so it y-sorts +3 to occlude its sitter
/// (whose key is `sofa.y + 2`); the north sofa stays +2 so insertion order
/// breaks the tie in its sitter's favor.
fn enqueue_meeting_furniture<'a>(layout: &'a Layout, drawables: &mut Vec<Drawable<'a>>) {
    for trio in layout.meeting_rooms.iter().filter_map(|r| r.trio.as_ref()) {
        let table = trio.table;
        let [ts, bs] = trio.sofas;
        let rug_w = 18u16;
        let rug_h =
            bs.y.saturating_sub(ts.y)
                .saturating_add(8)
                .min(layout.buf_h.saturating_sub(table.y).saturating_add(8));
        drawables.push(Drawable {
            anchor_y: table.y.saturating_sub(rug_h / 2),
            kind: DrawableKind::AreaRug {
                pos: table,
                width: rug_w,
                height: rug_h,
            },
        });
    }
    for trio in layout.meeting_rooms.iter().filter_map(|r| r.trio.as_ref()) {
        for (i, sofa) in trio.sofas.into_iter().enumerate() {
            // sofas[0] is the north sofa, sofas[1] the south — the south sofa
            // faces away (`mirrored`) and y-sorts +3 to occlude its sitter.
            let mirrored = i % 2 != 0;
            let faces_away = sofa.y >= trio.table.y;
            drawables.push(Drawable {
                anchor_y: sofa.y + if faces_away { 3 } else { 2 },
                kind: DrawableKind::MeetingSofa {
                    pos: sofa,
                    mirrored,
                },
            });
        }
    }
    for trio in layout.meeting_rooms.iter().filter_map(|r| r.trio.as_ref()) {
        drawables.push(Drawable {
            // z-key = sprite south row, derived from the table (== +2 for the
            // 11×5 meeting-table sprite) so it can't drift from a visual edit.
            anchor_y: z_sort_row(
                Anchor::Center,
                trio.table,
                crate::layout::furniture_def(crate::layout::Furniture::MeetingTable)
                    .visual
                    .h,
            ),
            kind: DrawableKind::MeetingTable { pos: trio.table },
        });
    }
}

/// The kitchen island, the lounge couch (emitted ONCE via
/// `couch_sprite_center` — 3 seat waypoints share one sprite), and the
/// center-pinned waypoint appliances (pantry counter, vending, printer).
/// PhoneBooth/StandingDesk render via pod-decor; meeting slots ride the
/// sofa/table — so those waypoint kinds emit nothing here.
fn enqueue_lounge_pantry_appliances<'a>(
    layout: &'a Layout,
    occupied_waypoints: &std::collections::HashSet<usize>,
    drawables: &mut Vec<Drawable<'a>>,
) {
    if let Some(island) = layout.pantry.and_then(|p| p.kitchen_island) {
        drawables.push(Drawable {
            anchor_y: z_sort_row(
                Anchor::Center,
                island,
                crate::layout::furniture_def(crate::layout::Furniture::KitchenIsland)
                    .visual
                    .h,
            ),
            kind: DrawableKind::KitchenIsland { pos: island },
        });
    }

    // Lounge couch — pushed before the character loop so the y-sort tie-break
    // keeps the couch behind its sitters. The rug anchors north of the couch
    // (y-sort at its top) so the couch sits on it.
    if let Some(center) = layout.couch_sprite_center() {
        drawables.push(Drawable {
            anchor_y: center.y.saturating_sub(2),
            kind: DrawableKind::AreaRug {
                pos: Point {
                    x: center.x,
                    y: center.y + 3,
                },
                width: 22,
                height: 7,
            },
        });
        drawables.push(Drawable {
            anchor_y: z_sort_row(
                Anchor::Center,
                center,
                crate::layout::furniture_def(crate::layout::Furniture::Couch)
                    .visual
                    .h,
            ),
            // The lounge couch IS a vertical-mirrored meeting sofa (same 20×7
            // sprite, back facing NORTH toward the windows) — folded into the
            // MeetingSofa arm rather than a duplicate DrawableKind. Its z-key
            // stays the Couch furniture row (unchanged above).
            kind: DrawableKind::MeetingSofa {
                pos: center,
                mirrored: true,
            },
        });
        if let Some(table) = layout.lounge_side_table() {
            drawables.push(Drawable {
                anchor_y: z_sort_row(
                    Anchor::Center,
                    table,
                    crate::layout::furniture_def(crate::layout::Furniture::LoungeSideTable)
                        .visual
                        .h,
                ),
                kind: DrawableKind::LoungeSideTable { pos: table },
            });
        }
    }

    for (wp_idx, wp) in layout.waypoints.iter().enumerate() {
        use crate::layout::{furniture_def, WaypointKind};
        let busy = occupied_waypoints.contains(&wp_idx);
        // y-sort baseline = the sprite's south row (these appliances are
        // center-pinned at `pos`). Read the VISUAL height, not the (shallow)
        // footprint, so an overhang would still sort by what's painted.
        let visual_h = furniture_def(wp.kind.furniture()).visual.h;
        match wp.kind {
            WaypointKind::Couch => {}
            WaypointKind::Pantry => {
                let Size { w: cw, h: ch } = layout.pantry_counter_size(); // runtime-sized
                drawables.push(Drawable {
                    anchor_y: z_sort_row(Anchor::Center, wp.pos, ch),
                    kind: DrawableKind::WaypointPantry {
                        pos: wp.pos,
                        use_large: cw >= crate::layout::PANTRY_COUNTER_LARGE_W,
                    },
                });
            }
            WaypointKind::PhoneBooth | WaypointKind::StandingDesk => {}
            WaypointKind::VendingMachine => {
                drawables.push(Drawable {
                    anchor_y: z_sort_row(Anchor::Center, wp.pos, visual_h),
                    kind: DrawableKind::VendingMachine { pos: wp.pos, busy },
                });
            }
            WaypointKind::Printer => {
                drawables.push(Drawable {
                    anchor_y: z_sort_row(Anchor::Center, wp.pos, visual_h),
                    kind: DrawableKind::Printer { pos: wp.pos, busy },
                });
            }
            WaypointKind::SnackShelf => {
                drawables.push(Drawable {
                    anchor_y: z_sort_row(Anchor::Center, wp.pos, visual_h),
                    kind: DrawableKind::SnackShelf { pos: wp.pos },
                });
            }
            // Island stands carry no art of their own: the island BODY draws
            // via `layout.kitchen_island` (like the meeting furniture).
            WaypointKind::MeetingSofa | WaypointKind::MeetingChair | WaypointKind::Island => {}
        }
    }
}

/// Pod-aisle decor (plant / whiteboard / TV / phone booth / standing desk)
/// and free-standing plants — all center-pinned, y-sorted at the sprite's
/// south row from the one furniture table (the mask reads the separate,
/// shallower `footprint` off the same row, so a tall canopy sorts without
/// blocking the aisle).
fn enqueue_pod_decor_and_plants<'a>(layout: &'a Layout, drawables: &mut Vec<Drawable<'a>>) {
    for &PodDecorItem { kind, pos } in &layout.pod_decor {
        let Size { h, .. } = crate::layout::furniture_def(kind.furniture()).visual;
        drawables.push(Drawable {
            anchor_y: z_sort_row(Anchor::Center, pos, h),
            kind: DrawableKind::PodDecorItem { kind, pos },
        });
    }
    for &PlantItem { kind, pos } in &layout.plants {
        drawables.push(Drawable {
            anchor_y: z_sort_row(
                Anchor::Center,
                pos,
                crate::layout::furniture_def(kind.furniture()).visual.h,
            ),
            kind: DrawableKind::Plant { kind, pos },
        });
    }
}

/// Free-standing fixtures: the floor lamp, the meeting-room coat rack, and the
/// elevator door (whose open/close frame is computed stateless from the agents
/// currently in their entry/exit window — the MAX frame so the door is at least
/// as open as the most-in-progress agent needs).
fn enqueue_floor_fixtures<'a>(
    ctx: &PaintCtx<'_>,
    agents: &[AgentSlot],
    drawables: &mut Vec<Drawable<'a>>,
) {
    if let Some(lamp) = ctx.layout.floor_lamp() {
        drawables.push(Drawable {
            anchor_y: lamp.y + floor_lamp_south_offset(),
            kind: DrawableKind::FloorLamp { pos: lamp },
        });
    }
    for wp in ctx
        .layout
        .waypoints
        .iter()
        .filter(|w| w.kind == crate::layout::WaypointKind::MeetingChair)
    {
        drawables.push(Drawable {
            // One row UNDER the sitter's z — derived from the occupant's
            // OWN view's seat key, so the pair can't drift apart.
            anchor_y: seat::SeatView::of(wp.kind, wp.facing).z_key_for_seat(wp.pos) - 1,
            kind: DrawableKind::MeetingChair {
                pos: wp.pos,
                // The backrest rides the side AWAY from the table: a chair
                // FACING East sits west of the table, bar on its west.
                back_west: wp.facing == crate::layout::Facing::East,
            },
        });
    }
    if let Some(tank) = ctx.layout.fish_tank() {
        // Center anchor: z at the sprite's south (cabinet base) row, via the
        // SAME center-pin helper the lamp derives its base from.
        let h = crate::layout::furniture_def(crate::layout::Furniture::FishTank)
            .visual
            .h;
        drawables.push(Drawable {
            anchor_y: tank.y + center_pin_south_offset(h),
            kind: DrawableKind::FishTank { pos: tank },
        });
    }
    // One coat rack per meeting room (#555: room 1 used to go without);
    // placement + the narrow-fitted-room yield live in coat_rack_pos — THE
    // one authority the hover hit-test shares.
    for rack in ctx
        .layout
        .meeting_rooms
        .iter()
        .filter_map(|r| r.coat_rack_pos())
    {
        drawables.push(Drawable {
            anchor_y: rack.y + 7,
            kind: DrawableKind::CoatRack { pos: rack },
        });
    }
    if let Some(door_pos) = ctx.layout.door {
        let frame_idx = compute_door_frame_idx(agents, ctx.now, ctx.door_anim_max_ms);
        drawables.push(Drawable {
            anchor_y: door_pos.y + ELEVATOR_H,
            kind: DrawableKind::Door {
                pos: door_pos,
                frame_idx,
            },
        });
    }
}

/// Enqueue wall decor (clocks/whiteboards hung on walls). TOP-LEFT anchored
/// at `pos`, so the y-sort row is the sprite's south base (`pos.y + h - 1`),
/// the same `z_sort_row` helper the mask and every other drawable use. A
/// pure furniture phase of `render_to_rgb_buffer` — borrows nothing from the
/// agent set, so it carries no character lifetime.
fn enqueue_wall_decor<'a>(layout: &'a Layout, drawables: &mut Vec<Drawable<'a>>) {
    for &WallDecorItem { kind, pos } in &layout.wall_decor {
        let Size { h, .. } = crate::layout::furniture_def(kind.furniture()).visual;
        drawables.push(Drawable {
            anchor_y: z_sort_row(Anchor::TopLeft, pos, h),
            kind: DrawableKind::WallDecor { kind, pos },
        });
    }
}

#[cfg(test)]
mod tests;
