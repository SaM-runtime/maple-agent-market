//! Headless Maple world → `RgbBuffer` rendering for the desktop window.
//!
//! The window renders at a downscaled resolution and nearest-neighbor upscales
//! it so the procedural Maple art stays crisp. This module owns one
//! `pixtuoid_scene::floor::FloorSession` across frames so movement and other
//! persistent scene state remain continuous.

use std::time::SystemTime;

use pixtuoid_core::sprite::{format::Pack, Rgb, RgbBuffer};
use pixtuoid_core::state::SceneState;

use pixtuoid_scene::floor::{FloorMeta, FloorSession, FrameInputs};
use pixtuoid_scene::footer::{
    build_footer, footer_tone_rgb, footer_tool_tally, FooterInputs, FooterModel, FooterTone,
    RungKind,
};
use pixtuoid_scene::layout::{Bounds, Size};
use pixtuoid_scene::theme::Theme;

/// Pack an `Rgb` into the softbuffer word format, `0x00RRGGBB` (XRGB) — the ONE
/// definition of the floating painter's surface pixel format. The scene blit
/// (`window.rs`) and this label overlay write into the SAME surface, so they must
/// agree on channel order / shift widths; a lone edit to one would color-swap the
/// badges while the scene renders correctly, with no compile error. (The test
/// oracle re-derives the packing independently ON PURPOSE, so a bug here can't
/// hide behind a shared helper — don't route it through this.)
pub(crate) fn pack_xrgb(c: Rgb) -> u32 {
    (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32
}

/// Expand one packed-XRGB scene buffer into the native window surface without
/// doing an integer division for every destination pixel. Each source pixel is
/// written as one contiguous run and each completed row is copied vertically.
/// Remainder edges repeat the final source pixel/row, matching the original
/// nearest-neighbour projection used by the floating window.
#[allow(clippy::too_many_arguments)] // the two explicit buffer geometries prevent stride drift
pub(crate) fn upscale_xrgb_nearest(
    source: &[u32],
    source_width: usize,
    source_height: usize,
    destination: &mut [u32],
    destination_width: usize,
    destination_height: usize,
    scale: usize,
) -> bool {
    let Some(source_len) = source_width.checked_mul(source_height) else {
        return false;
    };
    let Some(destination_len) = destination_width.checked_mul(destination_height) else {
        return false;
    };
    if scale == 0
        || source_width == 0
        || source_height == 0
        || destination_width == 0
        || destination_height == 0
        || source.len() < source_len
        || destination.len() < destination_len
    {
        return false;
    }

    let mut destination_y = 0usize;
    for source_y in 0..source_height {
        if destination_y >= destination_height {
            break;
        }
        let source_start = source_y * source_width;
        let source_row = &source[source_start..source_start + source_width];
        let destination_start = destination_y * destination_width;
        let destination_row =
            &mut destination[destination_start..destination_start + destination_width];

        if scale == 1 {
            let copied = source_width.min(destination_width);
            destination_row[..copied].copy_from_slice(&source_row[..copied]);
            if copied < destination_width {
                destination_row[copied..].fill(source_row[source_width - 1]);
            }
        } else {
            let mut destination_x = 0usize;
            for &pixel in source_row {
                if destination_x >= destination_width {
                    break;
                }
                let run_end = destination_x.saturating_add(scale).min(destination_width);
                destination_row[destination_x..run_end].fill(pixel);
                destination_x = run_end;
            }
            if destination_x < destination_width {
                destination_row[destination_x..].fill(source_row[source_width - 1]);
            }
        }

        let repeated_rows = scale.min(destination_height - destination_y);
        for repeat in 1..repeated_rows {
            destination.copy_within(
                destination_start..destination_start + destination_width,
                (destination_y + repeat) * destination_width,
            );
        }
        destination_y += repeated_rows;
    }

    if destination_y < destination_height {
        let last_row_start = (destination_y - 1) * destination_width;
        while destination_y < destination_height {
            destination.copy_within(
                last_row_start..last_row_start + destination_width,
                destination_y * destination_width,
            );
            destination_y += 1;
        }
    }
    true
}

/// Owns the two persistent Maple map sessions and their reusable RGB buffers.
/// One renderer lives for the window's lifetime, which keeps entry, exit and
/// activity animation continuous across frames.
pub struct MapleRenderer {
    session: FloorSession,
    /// Independent sim/cache domain for the forest map.  The market/scene
    /// session therefore never evicts training-map actors when the camera flips.
    training_session: FloorSession,
    /// Reusable output for the side-by-side Maple world. Each map keeps its own
    /// session buffer; only the final RGB rows are copied into this surface.
    composite: RgbBuffer,
    /// Sticky root-party routing and the camera's selected Maple map.
    maple_world: pixtuoid_scene::maple_world::MapleWorldSession,
    /// User-selected real-Agent roster plus explicit showcase appearances.
    character_appearances: pixtuoid_scene::characters::CharacterAppearances,
    /// Presentation-only Free Market actors. Never written into the watched
    /// authoritative scene, footer statistics or Agent lineage.
    market_showcase: super::characters::ShowcaseRoster,
    /// Presentation-only training actors, isolated from both the monitored
    /// scene and the market roster.
    training_showcase: super::characters::ShowcaseRoster,
    /// Exact augmented market scene painted by the last frame, retained so
    /// native-surface labels use the same actors and anchors.
    last_market_scene: Option<SceneState>,
    /// Exact augmented training scene painted by the last frame. Native label
    /// placement must consume this same render-only projection.
    last_training_scene: Option<SceneState>,
    /// User-selected camera mode. Dual is the default; compact buffers render
    /// the selected single map without mutating this preference.
    view_mode: MapleViewMode,
    /// Whether the last successful frame actually fit both authored plates.
    dual_rendered: bool,
    /// The market viewport captured by the last successful Maple scene frame.
    /// `labels()` rebuilds placements from this exact geometry instead of the
    /// scene layout, keeping each shop card attached to its merchant.
    market_viewport: Option<Bounds>,
    /// Training viewport captured by the last selected forest-map frame.
    training_viewport: Option<Bounds>,
    /// Whether the last market frame used the fixed 32x24 paperdoll set.
    /// Labels must choose the same anchor mode as the pixel painter.
    market_avatars: bool,
    /// Ambient-audio gateway (#633). Inert unless installed. The per-frame
    /// `AudioFrame` is composed through the session's shared `AudioObserver`
    /// (`FloorSession::audio_frame`) — stems plus door and appliance one-shots,
    /// with floor reprime handled automatically by the observer
    /// (so floor nav, if ever added, needs no hand-mirrored guard).
    audio: crate::audio::AudioHandle,
}

/// Floating-only camera mode for the two-map Maple world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapleViewMode {
    /// Free Market and training map are rendered side by side.
    Dual,
    /// One readable map selected by `MapleWorldSession::current_map`.
    Single,
}

/// Explicit user selection behind the floating `1`/`2`/`3` map shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapleViewSelection {
    Dual,
    FreeMarket,
    ForestTraining,
}

/// One map's native-surface overlays and the exact buffer-space panel they
/// belong to. The window uses the panel to keep cards out of the other map.
#[doc(hidden)]
pub struct MapleOverlayBatch {
    pub map: pixtuoid_scene::maple_world::MapleMapId,
    pub viewport: Bounds,
    pub labels: Vec<pixtuoid_scene::overlay::LabelElement>,
    pub player_ids: Vec<pixtuoid_scene::overlay::LabelElement>,
}

const MAPLE_DUAL_PANEL_MIN_WIDTH: u16 = 240;
const MAPLE_DUAL_MIN_HEIGHT: u16 = 160;

impl MapleRenderer {
    pub fn new() -> Self {
        Self {
            session: FloorSession::new(),
            training_session: FloorSession::new(),
            composite: RgbBuffer::filled(1, 1, Rgb { r: 0, g: 0, b: 0 }),
            maple_world: pixtuoid_scene::maple_world::MapleWorldSession::default(),
            character_appearances: pixtuoid_scene::characters::CharacterAppearances::default(),
            market_showcase: super::characters::ShowcaseRoster::default(),
            training_showcase: super::characters::ShowcaseRoster::from_slots_for_destination(
                [],
                pixtuoid_scene::characters::CHARACTER_SLOT_COUNT,
                super::characters::ShowcaseDestination::ForestTraining,
                SystemTime::UNIX_EPOCH,
            ),
            last_market_scene: None,
            last_training_scene: None,
            view_mode: MapleViewMode::Dual,
            dual_rendered: false,
            market_viewport: None,
            training_viewport: None,
            market_avatars: false,
            audio: crate::audio::AudioHandle::disabled(),
        }
    }

    pub(crate) fn set_audio(&mut self, audio: crate::audio::AudioHandle) {
        self.audio = audio;
    }

    /// Apply persisted character choices before the first frame.
    pub(crate) fn configure_characters(
        &mut self,
        config: crate::config::CharacterConfig,
        now: SystemTime,
    ) {
        let available_count = config.agent_roster.available_count();
        self.character_appearances.set_roster(config.agent_roster);
        self.market_showcase = super::characters::ShowcaseRoster::from_slots_for_destination(
            config.showcase_slots,
            available_count,
            super::characters::ShowcaseDestination::FreeMarket,
            now,
        );
        self.training_showcase = super::characters::ShowcaseRoster::from_slots_for_destination(
            config.training_showcase_slots,
            available_count,
            super::characters::ShowcaseDestination::ForestTraining,
            now,
        );
    }

    /// Number of character appearances exposed by the active pack.
    pub(crate) fn available_character_count(&self) -> usize {
        self.character_appearances.roster().available_count()
    }

    /// Canonical real-Agent appearance slots selected by the user.
    pub(crate) fn selected_character_slots(&self) -> &[usize] {
        self.character_appearances.roster().slots()
    }

    /// Canonical persistable showcase slots (with withdrawing actors omitted).
    pub(crate) fn selected_market_showcase_slots(&self) -> Vec<usize> {
        self.market_showcase.selected_slots()
    }

    /// Canonical persistable training showcase slots.
    pub(crate) fn selected_training_showcase_slots(&self) -> Vec<usize> {
        self.training_showcase.selected_slots()
    }

    /// Toggle one real-Agent appearance while preserving at least one slot.
    pub(crate) fn toggle_character_slot(&mut self, slot: usize) -> bool {
        let before = self.character_appearances.roster().clone();
        let after = before.toggled(slot);
        let changed = before != after;
        self.character_appearances.set_roster(after);
        changed
    }

    /// Enter/withdraw one presentation-only character. The current routed
    /// Free Market Agent count is evaluated first so a guest never evicts one.
    pub(crate) fn toggle_market_showcase_slot(
        &mut self,
        scene: &SceneState,
        slot: usize,
        now: SystemTime,
    ) -> super::characters::ShowcaseToggle {
        self.maple_world.reconcile(scene);
        let real_count = self
            .maple_world
            .project_scene(scene, pixtuoid_scene::maple_world::MapleMapId::FreeMarket)
            .agents
            .len()
            .min(pixtuoid_scene::market::MARKET_MAX_AGENTS);
        self.market_showcase.toggle(slot, real_count, now)
    }

    /// Enter/withdraw one presentation-only training character without
    /// displacing a routed Agent.
    pub(crate) fn toggle_training_showcase_slot(
        &mut self,
        scene: &SceneState,
        slot: usize,
        now: SystemTime,
    ) -> super::characters::ShowcaseToggle {
        self.maple_world.reconcile(scene);
        let real_count = self
            .maple_world
            .project_scene(
                scene,
                pixtuoid_scene::maple_world::MapleMapId::ForestTraining,
            )
            .agents
            .len()
            .min(pixtuoid_scene::training::TRAINING_MAX_AGENTS);
        self.training_showcase.toggle(slot, real_count, now)
    }

    /// Number of real monitored Agents currently routed to the Free Market.
    pub(crate) fn real_market_agent_count(&self, scene: &SceneState) -> usize {
        self.maple_world
            .agents_on(pixtuoid_scene::maple_world::MapleMapId::FreeMarket)
            .iter()
            .filter(|id| scene.agents.contains_key(id))
            .count()
            .min(pixtuoid_scene::market::MARKET_MAX_AGENTS)
    }

    /// Number of real monitored Agents currently routed to the training map.
    pub(crate) fn real_training_agent_count(&self, scene: &SceneState) -> usize {
        self.maple_world
            .agents_on(pixtuoid_scene::maple_world::MapleMapId::ForestTraining)
            .iter()
            .filter(|id| scene.agents.contains_key(id))
            .count()
            .min(pixtuoid_scene::training::TRAINING_MAX_AGENTS)
    }

    /// Entering/strolling/leaving state for one showcase row.
    pub(crate) fn market_showcase_visible_state(&self, slot: usize) -> Option<bool> {
        self.market_showcase.visible_state(slot)
    }

    /// Entering/training/leaving state for one training showcase row.
    pub(crate) fn training_showcase_visible_state(&self, slot: usize) -> Option<bool> {
        self.training_showcase.visible_state(slot)
    }

    /// Whether showcase motion needs the window's active animation cadence.
    pub(crate) fn showcase_needs_active_animation(&self) -> bool {
        self.market_showcase.needs_active_animation()
            || self.training_showcase.needs_active_animation()
    }

    fn project_market_scene(&mut self, scene: &SceneState, now: SystemTime) -> SceneState {
        let base = self
            .maple_world
            .project_scene(scene, pixtuoid_scene::maple_world::MapleMapId::FreeMarket);
        self.market_showcase
            .project_into_market(&base, &mut self.character_appearances, now)
    }

    fn project_training_scene(&mut self, scene: &SceneState, now: SystemTime) -> SceneState {
        let base = self.maple_world.project_scene(
            scene,
            pixtuoid_scene::maple_world::MapleMapId::ForestTraining,
        );
        self.training_showcase
            .project_into(&base, &mut self.character_appearances, now)
    }

    /// The Maple map currently selected by the floating camera.
    pub(crate) fn current_map(&self) -> pixtuoid_scene::maple_world::MapleMapId {
        self.maple_world.current_map()
    }

    #[cfg(test)]
    const fn view_mode(&self) -> MapleViewMode {
        self.view_mode
    }

    pub(crate) const fn prefers_dual_map(&self) -> bool {
        matches!(self.view_mode, MapleViewMode::Dual)
    }

    /// Select a Maple map for deterministic snapshot/proof tooling.  The live
    /// window normally uses the adjacent `Tab`/click cycle instead.
    #[doc(hidden)]
    pub fn set_maple_map(&mut self, map: pixtuoid_scene::maple_world::MapleMapId) {
        self.view_mode = MapleViewMode::Single;
        self.maple_world.set_current_map(map);
    }

    /// Select the simultaneous two-map surface for deterministic proof tooling.
    #[doc(hidden)]
    pub fn set_maple_dual_view(&mut self) {
        self.view_mode = MapleViewMode::Dual;
    }

    /// Put every synthetic snapshot party on one requested map.  The live
    /// renderer never calls this; it exists so an eight-agent visual fixture
    /// can exercise every authored lane instead of showing a hash-selected half.
    #[doc(hidden)]
    pub fn assign_snapshot_scene_to_map(
        &mut self,
        scene: &SceneState,
        map: pixtuoid_scene::maple_world::MapleMapId,
    ) {
        self.maple_world.reconcile(scene);
        for agent_id in scene.agents.keys().copied() {
            self.maple_world.assign_party(agent_id, map);
        }
    }

    /// Deterministically split a synthetic proof scene across both maps. Live
    /// routing never calls this; it keeps snapshot coverage independent of hash.
    #[doc(hidden)]
    pub fn assign_snapshot_scene_across_maps(&mut self, scene: &SceneState) {
        self.maple_world.reconcile(scene);
        let roots = scene
            .agents
            .keys()
            .copied()
            .map(
                |agent_id| match pixtuoid_scene::maple_world::agent_relation(scene, agent_id) {
                    Some(pixtuoid_scene::maple_world::MapleAgentRelation::Root {
                        root_id, ..
                    })
                    | Some(pixtuoid_scene::maple_world::MapleAgentRelation::Child {
                        root_id,
                        ..
                    }) => root_id,
                    None => agent_id,
                },
            )
            .collect::<std::collections::BTreeSet<_>>();
        for (index, root_id) in roots.into_iter().enumerate() {
            let map = if index % 2 == 0 {
                pixtuoid_scene::maple_world::MapleMapId::FreeMarket
            } else {
                pixtuoid_scene::maple_world::MapleMapId::ForestTraining
            };
            self.maple_world.assign_party(root_id, map);
        }
    }

    /// Cycle the camera through the built-in two-map world.
    pub(crate) fn cycle_map(&mut self) -> bool {
        // A compact window keeps Dual as the preference but can only paint one
        // plate. Toggle that visible fallback immediately; resizing wide later
        // still restores the preferred simultaneous view.
        if self.view_mode == MapleViewMode::Dual && !self.dual_rendered {
            self.maple_world.cycle_map();
            return true;
        }
        match self.view_mode {
            MapleViewMode::Dual => {
                self.view_mode = MapleViewMode::Single;
                self.maple_world
                    .set_current_map(pixtuoid_scene::maple_world::MapleMapId::FreeMarket);
            }
            MapleViewMode::Single
                if self.current_map() == pixtuoid_scene::maple_world::MapleMapId::FreeMarket =>
            {
                self.maple_world
                    .set_current_map(pixtuoid_scene::maple_world::MapleMapId::ForestTraining);
            }
            MapleViewMode::Single => {
                self.view_mode = MapleViewMode::Dual;
                self.maple_world
                    .set_current_map(pixtuoid_scene::maple_world::MapleMapId::FreeMarket);
            }
        }
        true
    }

    pub(crate) fn select_map_view(&mut self, selection: MapleViewSelection) -> bool {
        match selection {
            MapleViewSelection::Dual => {
                self.view_mode = MapleViewMode::Dual;
            }
            MapleViewSelection::FreeMarket => {
                self.view_mode = MapleViewMode::Single;
                self.maple_world
                    .set_current_map(pixtuoid_scene::maple_world::MapleMapId::FreeMarket);
            }
            MapleViewSelection::ForestTraining => {
                self.view_mode = MapleViewMode::Single;
                self.maple_world
                    .set_current_map(pixtuoid_scene::maple_world::MapleMapId::ForestTraining);
            }
        }
        true
    }

    /// Compact title for the current floating map selector.
    pub(crate) fn current_map_title(&self) -> &'static str {
        self.current_map().title_zh_tw()
    }

    /// Visible compact map-switch affordance for the native floating surface.
    #[doc(hidden)]
    pub fn map_selector_text(&self) -> Option<String> {
        let selection = match self.view_mode {
            MapleViewMode::Dual if self.dual_rendered => "雙圖".to_owned(),
            MapleViewMode::Dual => format!("雙圖準備｜{}", self.current_map_title()),
            MapleViewMode::Single => self.current_map_title().to_owned(),
        };
        Some(format!("地圖：{selection} [1/2/3]"))
    }

    /// Render the selected Maple map(s) into the owned `buf_w × buf_h` RGB
    /// surface. Wide windows receive both maps; compact windows receive the
    /// selected map. The public build always has both procedural maps, so this
    /// path never falls back to Pixtuoid's former scene renderer.
    #[allow(clippy::too_many_arguments)] // the render inputs are genuinely flat (scene/pack/theme/clock/size/floor)
    pub fn render(
        &mut self,
        scene: &SceneState,
        pack: &Pack,
        theme: &'static Theme,
        now: SystemTime,
        buf_w: u16,
        buf_h: u16,
        floor_meta: FloorMeta,
    ) -> &RgbBuffer {
        // Maple Agent Market owns a complete procedural two-map fallback.
        // A local pack may replace either plate, but it is never the feature
        // switch: a clean clone must open the product UI without Pixtuoid's
        // embedded scene or any separately downloaded game assets.
        self.maple_world.reconcile(scene);
        if self.view_mode == MapleViewMode::Dual {
            if let Some((market_panel, training_panel)) = split_map_panels(buf_w, buf_h) {
                let market_scene = self.project_market_scene(scene, now);
                let training_scene = self.project_training_scene(scene, now);
                let market_rendered = self
                    .session
                    .render_maple(
                        FrameInputs {
                            scene: &market_scene,
                            character_appearances: Some(&self.character_appearances),
                            pack,
                            theme,
                            now,
                            size: Size {
                                w: market_panel.width,
                                h: market_panel.height,
                            },
                            floor_meta,
                            active_pet: None,
                            floor_pet: None,
                            debug_walkable: false,
                        },
                        pixtuoid_scene::maple_world::MapleMapId::FreeMarket,
                    )
                    .is_some();
                let training_rendered = self
                    .training_session
                    .render_maple(
                        FrameInputs {
                            scene: &training_scene,
                            character_appearances: Some(&self.character_appearances),
                            pack,
                            theme,
                            now,
                            size: Size {
                                w: training_panel.width,
                                h: training_panel.height,
                            },
                            floor_meta,
                            active_pet: None,
                            floor_pet: None,
                            debug_walkable: false,
                        },
                        pixtuoid_scene::maple_world::MapleMapId::ForestTraining,
                    )
                    .is_some();

                self.composite
                    .resize_fill(buf_w, buf_h, theme.surface.bg_fallback);
                if market_rendered {
                    copy_rgb_panel(&mut self.composite, self.session.buf(), market_panel);
                }
                if training_rendered {
                    copy_rgb_panel(
                        &mut self.composite,
                        self.training_session.buf(),
                        training_panel,
                    );
                }
                self.market_viewport = market_rendered.then_some(market_panel);
                self.training_viewport = training_rendered.then_some(training_panel);
                self.market_avatars = market_rendered;
                self.dual_rendered = market_rendered && training_rendered;
                self.last_market_scene = market_rendered.then_some(market_scene.clone());
                self.last_training_scene = training_rendered.then_some(training_scene.clone());

                let market_audio =
                    self.session
                        .audio_frame(&market_scene, floor_meta.floor_idx, now);
                let _training_audio =
                    self.training_session
                        .audio_frame(&training_scene, floor_meta.floor_idx, now);
                if self.audio.is_enabled() {
                    self.audio.frame(market_audio);
                }
                return &self.composite;
            }
        }

        self.dual_rendered = false;
        let map = self.maple_world.current_map();
        let projected = match map {
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket => {
                self.project_market_scene(scene, now)
            }
            pixtuoid_scene::maple_world::MapleMapId::ForestTraining => {
                self.project_training_scene(scene, now)
            }
        };
        let inputs = FrameInputs {
            scene: &projected,
            character_appearances: Some(&self.character_appearances),
            pack,
            theme,
            now,
            size: Size { w: buf_w, h: buf_h },
            floor_meta,
            active_pet: None,
            floor_pet: None,
            debug_walkable: false,
        };
        let rendered = match map {
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket => {
                self.session.render_maple(inputs, map)
            }
            pixtuoid_scene::maple_world::MapleMapId::ForestTraining => {
                self.training_session.render_maple(inputs, map)
            }
        };
        let viewport = rendered.is_some().then_some(Bounds {
            x: 0,
            y: 0,
            width: buf_w,
            height: buf_h,
        });
        self.market_viewport = (map == pixtuoid_scene::maple_world::MapleMapId::FreeMarket)
            .then_some(viewport)
            .flatten();
        self.training_viewport = (map == pixtuoid_scene::maple_world::MapleMapId::ForestTraining)
            .then_some(viewport)
            .flatten();
        self.market_avatars = self.market_viewport.is_some();
        self.last_market_scene = self.market_viewport.map(|_| projected.clone());
        self.last_training_scene = self.training_viewport.map(|_| projected.clone());
        let audio_frame = match map {
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket => {
                self.session
                    .audio_frame(&projected, floor_meta.floor_idx, now)
            }
            pixtuoid_scene::maple_world::MapleMapId::ForestTraining => self
                .training_session
                .audio_frame(&projected, floor_meta.floor_idx, now),
        };
        if self.audio.is_enabled() {
            self.audio.frame(audio_frame);
        }
        match map {
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket => self.session.buf(),
            pixtuoid_scene::maple_world::MapleMapId::ForestTraining => self.training_session.buf(),
        }
    }

    /// Build the name-badge overlay for the LAST rendered frame (call right after `render`).
    /// Uses the SAME layout + per-floor route state the sprite pass used, so labels align 1:1
    /// with the painted characters. Floating has no agent-hover yet → `hovered = None`.
    pub fn labels(
        &mut self,
        scene: &SceneState,
        now: SystemTime,
    ) -> Vec<pixtuoid_scene::overlay::LabelElement> {
        self.maple_overlay_batches(scene, now)
            .into_iter()
            .flat_map(|batch| batch.labels)
            .collect()
    }

    #[doc(hidden)]
    pub fn maple_overlay_batches(
        &mut self,
        scene: &SceneState,
        now: SystemTime,
    ) -> Vec<MapleOverlayBatch> {
        let mut batches = Vec::with_capacity(2);
        if let Some(viewport) = self.market_viewport {
            let projected = self.last_market_scene.clone().unwrap_or_else(|| {
                self.maple_world
                    .project_scene(scene, pixtuoid_scene::maple_world::MapleMapId::FreeMarket)
            });
            let market_scene = &projected;
            let placements = pixtuoid_scene::market::build_market_placements_with_appearances(
                market_scene,
                viewport,
                &self.character_appearances,
            );
            let market_frame = pixtuoid_scene::market::MarketFrameContext { viewport, now };
            let labels = if self.market_avatars {
                pixtuoid_scene::market::build_market_avatar_overlay(
                    market_scene,
                    &placements,
                    None,
                    market_frame,
                )
            } else {
                pixtuoid_scene::market::build_market_overlay(
                    market_scene,
                    &placements,
                    None,
                    market_frame,
                )
            };
            let player_ids = if self.market_avatars {
                pixtuoid_scene::market::build_market_avatar_player_ids(
                    market_scene,
                    &placements,
                    market_frame,
                )
            } else {
                pixtuoid_scene::market::build_market_merchant_player_ids(
                    market_scene,
                    &placements,
                    market_frame,
                )
            };
            batches.push(MapleOverlayBatch {
                map: pixtuoid_scene::maple_world::MapleMapId::FreeMarket,
                viewport,
                labels,
                player_ids,
            });
        }
        if let Some(viewport) = self.training_viewport {
            let projected = self.last_training_scene.clone().unwrap_or_else(|| {
                self.maple_world.project_scene(
                    scene,
                    pixtuoid_scene::maple_world::MapleMapId::ForestTraining,
                )
            });
            let placements = pixtuoid_scene::training::build_training_placements_with_appearances(
                &projected,
                viewport,
                &self.character_appearances,
            );
            let labels = pixtuoid_scene::training::build_training_overlay(
                &projected,
                &placements,
                None,
                pixtuoid_scene::training::TrainingFrameContext { viewport, now },
            );
            batches.push(MapleOverlayBatch {
                map: pixtuoid_scene::maple_world::MapleMapId::ForestTraining,
                viewport,
                labels,
                player_ids: Vec::new(),
            });
        }
        batches
    }

    /// Build short fake player names inside settled merchants' stall nameplates.
    pub fn market_player_ids(
        &mut self,
        scene: &SceneState,
        now: SystemTime,
    ) -> Vec<pixtuoid_scene::overlay::LabelElement> {
        let Some(viewport) = self.market_viewport else {
            return Vec::new();
        };
        let projected = self.last_market_scene.clone().unwrap_or_else(|| {
            self.maple_world
                .project_scene(scene, pixtuoid_scene::maple_world::MapleMapId::FreeMarket)
        });
        let market_scene = &projected;
        let placements = pixtuoid_scene::market::build_market_placements_with_appearances(
            market_scene,
            viewport,
            &self.character_appearances,
        );
        let market_frame = pixtuoid_scene::market::MarketFrameContext { viewport, now };
        if self.market_avatars {
            pixtuoid_scene::market::build_market_avatar_player_ids(
                market_scene,
                &placements,
                market_frame,
            )
        } else {
            pixtuoid_scene::market::build_market_merchant_player_ids(
                market_scene,
                &placements,
                market_frame,
            )
        }
    }

    /// The neon wall-board model for the current scene — one floor, so `floor =
    /// None`. Delegates to `FloorSession::board` (shared with the web painter).
    pub fn board(&self, scene: &SceneState, now: SystemTime) -> pixtuoid_scene::board::BoardModel {
        self.session.board(scene, now, None)
    }

    /// The shared status-footer model for the current scene. Single-floor, so
    /// `floor = None` (no breadcrumb).
    /// `budget` is the caller's column budget ([`footer_budget`] at the live
    /// width); `audio_audible`/`volume_flash` drive the ♩ suffix. Source-death
    /// is deferred here (`source_warning: None`) — floating
    /// doesn't thread the `SourceDeath` health channel yet; the seam is ready the
    /// day it does.
    pub fn footer(
        &self,
        scene: &SceneState,
        budget: u16,
        audio_audible: bool,
        volume_flash: Option<u8>,
    ) -> FooterModel {
        let per_floor = pixtuoid_scene::board::per_floor_counts(scene);
        let tools = footer_tool_tally(scene);
        let inputs = FooterInputs {
            counts: pixtuoid_scene::board::scene_stats(scene),
            per_floor: &per_floor,
            gateway: pixtuoid_scene::board::gateway_rollup(scene.daemons().map(|(_, _, p)| p)),
            floor: None,
            tools: &tools,
            audio_audible,
            volume_flash,
            source_warning: None,
            keys_stats: FOOTER_KEYS,
            keys_alert: FOOTER_KEYS,
        };
        build_footer(&inputs, budget)
    }
}

fn split_map_panels(buf_w: u16, buf_h: u16) -> Option<(Bounds, Bounds)> {
    if buf_h < MAPLE_DUAL_MIN_HEIGHT || buf_w < MAPLE_DUAL_PANEL_MIN_WIDTH.saturating_mul(2) {
        return None;
    }
    let market_width = buf_w / 2;
    let training_width = buf_w.saturating_sub(market_width);
    if market_width < MAPLE_DUAL_PANEL_MIN_WIDTH || training_width < MAPLE_DUAL_PANEL_MIN_WIDTH {
        return None;
    }
    Some((
        Bounds {
            x: 0,
            y: 0,
            width: market_width,
            height: buf_h,
        },
        Bounds {
            x: market_width,
            y: 0,
            width: training_width,
            height: buf_h,
        },
    ))
}

fn copy_rgb_panel(destination: &mut RgbBuffer, source: &RgbBuffer, panel: Bounds) {
    let copy_width = source
        .width()
        .min(panel.width)
        .min(destination.width().saturating_sub(panel.x));
    let copy_height = source
        .height()
        .min(panel.height)
        .min(destination.height().saturating_sub(panel.y));
    let source_width = usize::from(source.width());
    let destination_width = usize::from(destination.width());
    let copy_width = usize::from(copy_width);
    for y in 0..usize::from(copy_height) {
        let source_start = y * source_width;
        let destination_start =
            (y + usize::from(panel.y)) * destination_width + usize::from(panel.x);
        destination.as_mut_slice()[destination_start..destination_start + copy_width]
            .copy_from_slice(&source.as_slice()[source_start..source_start + copy_width]);
    }
}

impl Default for MapleRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Native-resolution rendering is the default for direct EXE and launcher
/// starts. An explicit `PIXTUOID_FLOATING_SCALE=2..8` remains available as a
/// low-render-resolution mode for unusually constrained machines.
const fn default_maple_scale(_win_h: u32) -> u32 {
    1
}

const FLOATING_SCALE_ENV: &str = "PIXTUOID_FLOATING_SCALE";

fn configured_floating_scale() -> Option<u32> {
    std::env::var(FLOATING_SCALE_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|scale| (1..=8).contains(scale))
}

pub fn maple_scale(win_h: u32) -> u32 {
    configured_floating_scale().unwrap_or_else(|| default_maple_scale(win_h))
}

fn window_buffer_geometry_with_scale_override(
    win_w: u32,
    win_h: u32,
    scale_override: Option<u32>,
) -> (u32, u16, u16) {
    let scale = scale_override
        .filter(|scale| (1..=8).contains(scale))
        .unwrap_or_else(|| default_maple_scale(win_h));
    let buf_w = (win_w / scale).clamp(1, u16::MAX as u32) as u16;
    let buf_h = (win_h / scale).clamp(1, u16::MAX as u32) as u16;
    (scale, buf_w, buf_h)
}

/// The window→scene-buffer projection for a `win_w`×`win_h` PHYSICAL-px window: the
/// integer `maple_scale` plus the render-buffer dims (`window / scale`,
/// clamped non-zero, NO footer row). The ONE place this geometry lives — shared
/// by `window::redraw` (which needs `scale` for the upscale blit and the buffer
/// dims for `sync_floor_caps` + the render) and the boot seed
/// (`boot_capacities_for_window`) — so the agent capacity they derive cannot drift
/// when the internal scale or clamp changes.
pub(crate) fn window_buffer_geometry(win_w: u32, win_h: u32) -> (u32, u16, u16) {
    window_buffer_geometry_with_scale_override(win_w, win_h, configured_floating_scale())
}

/// Per-floor boot capacities for the floating window, from a physical-pixel
/// window size. Uses the same `window_buffer_geometry` as the first redraw's
/// `window::sync_floor_caps`. In the sub-frame boot race before the first redraw, a
/// `SessionStart` could land at a `desk_index` the smaller real layout lacks
/// (immutable → invisible-but-alive until a resize). A floor whose layout rejects
/// the size falls back to a conservative agent capacity.
///
/// Known residual: this path intentionally keeps the historical boot seed. The
/// caller has only the configured LOGICAL size, while the first redraw sees the
/// real physical-pixel `window.inner_size()` on HiDPI displays. The redraw
/// corrects capacity immediately; eliminating the brief seed mismatch requires
/// moving `spawn_pipeline` into `resumed`, where `ActiveEventLoop` exists.
pub(crate) fn boot_capacities_for_window(
    win_w: u32,
    win_h: u32,
) -> [usize; pixtuoid_core::state::MAX_FLOORS] {
    let (_scale, buf_w, buf_h) = window_buffer_geometry(win_w, win_h);
    std::array::from_fn(|i| {
        let seed = pixtuoid_scene::floor::floor_seed(i);
        let cap = pixtuoid_scene::floor::floor_capacity(buf_w, buf_h, seed);
        if cap == 0 {
            crate::runtime::FALLBACK_AGENT_CAPACITY
        } else {
            cap
        }
    })
}

/// The bundled character sprite width (px), from the ONE cross-crate authority
/// `scene::layout::CHARACTER_SPRITE_W`. Labels only center ±half a glyph, so the
/// default width (not a custom pack's real `frame.width`) is fine here — ±1px on
/// a non-8-wide pack is cosmetically irrelevant (same rationale as `character_anchor`).
const FLOATING_SPRITE_W: i32 = pixtuoid_scene::layout::CHARACTER_SPRITE_W as i32;

/// Name-badge AA font size (px), drawn at NATIVE surface res (not upscaled by the
/// scene `scale`) so a badge stays a crisp fixed-height caption over the chunky
/// sprites — the same "fixed px, not upscaled" intent the old 8px bitmap had, now
/// anti-aliased. Tuned by eye against `examples/floating_snapshot`.
const LABEL_FONT_PX: f32 = 12.0;
/// Optional child-process-only multiplier used by the zh-TW launcher. The live
/// window also multiplies by winit's DPI scale so this value stays a logical-size
/// preference instead of shrinking on HiDPI displays.
pub(crate) const LABEL_SCALE_ENV: &str = "PIXTUOID_LABEL_SCALE";
const LABEL_SCALE_MIN: f32 = 0.75;
const LABEL_SCALE_MAX: f32 = 2.0;

/// Parse a configured task-label multiplier, falling back safely and bounding
/// arbitrary environment input. The launcher emits 1.0 / 1.25 / 1.5, while the
/// wider bounds keep hand-set values usable without allowing invisible or huge text.
pub(crate) fn parse_label_scale(value: Option<&str>) -> f32 {
    value
        .and_then(|raw| raw.trim().parse::<f32>().ok())
        .filter(|scale| scale.is_finite())
        .unwrap_or(1.0)
        .clamp(LABEL_SCALE_MIN, LABEL_SCALE_MAX)
}

/// Resolve the native-surface label font size from winit's physical/logical DPI
/// ratio and the user's logical-size preference.
pub(crate) fn label_font_px(dpi_scale: f64, user_scale: f32) -> f32 {
    let dpi = if dpi_scale.is_finite() && dpi_scale > 0.0 {
        dpi_scale as f32
    } else {
        1.0
    };
    let preference = if user_scale.is_finite() {
        user_scale.clamp(LABEL_SCALE_MIN, LABEL_SCALE_MAX)
    } else {
        1.0
    };
    LABEL_FONT_PX * dpi * preference
}
/// Near-black badge drop-shadow (`0x00RRGGBB`) — the AA text draws straight over
/// the scene, so a 1px offset shadow keeps it legible
/// over bright windows / plants.
const BADGE_SHADOW: u32 = 0x0000_0000;
/// The near-white AA ink for foreground captions with no theme cell behind them
/// — the hovered name badge AND the volume-flash readout share it (one
/// definition so a future softening can't split them).
const HOVER_INK: Rgb = Rgb {
    r: 240,
    g: 240,
    b: 240,
};

/// The floating footer's keybind-hint tail — floating's REAL controls (`m` mute,
/// `+`/`-` volume). This is the painter-specific input to the shared footer model.
const FOOTER_KEYS: &str = " [m]ute [+/-]vol ";
/// Breathing room from the window edges for the footer band (both the paint and
/// the [`footer_budget`] column math read it, so they can't drift).
const FOOTER_MARGIN_PX: i32 = 6;

/// Alpha-composite `color` over the surface pixel at `(x, y)` by `coverage` (the
/// AA rasterizer's per-pixel strength), a straight linear blend in `0x00RRGGBB`
/// space — the badge/board sit on opaque scene pixels, no alpha channel to keep.
fn blend_xrgb(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    x: i32,
    y: i32,
    color: u32,
    coverage: f32,
) {
    if x < 0 || y < 0 || (x as usize) >= win_w || (y as usize) >= win_h {
        return;
    }
    let idx = y as usize * win_w + x as usize;
    let bg = sb[idx];
    // the ONE blend curve — see aa_text::blend_channel
    let chan = |v: u32, sh: u32| ((v >> sh) & 0xff) as u8;
    let mix =
        |sh: u32| crate::aa_text::blend_channel(chan(bg, sh), chan(color, sh), coverage) as u32;
    sb[idx] = (mix(16) << 16) | (mix(8) << 8) | mix(0);
}

/// Rasterize `text` at `(x, top_y)` in the shared AA face, `color` over a 1px
/// down-right near-black shadow (shadow drawn first, both coverage-composited).
#[allow(clippy::too_many_arguments)] // flat surface + placement + style inputs, like paint_labels
fn draw_badge_text(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    text: &str,
    x: i32,
    top_y: i32,
    px: f32,
    color: u32,
) {
    crate::aa_text::draw_label_text_at(text, x + 1, top_y + 1, px, |gx, gy, cov| {
        blend_xrgb(sb, win_w, win_h, gx, gy, BADGE_SHADOW, cov)
    });
    crate::aa_text::draw_label_text_at(text, x, top_y, px, |gx, gy, cov| {
        blend_xrgb(sb, win_w, win_h, gx, gy, color, cov)
    });
}

/// Rasterize dark text on the roster's opaque light controls.  Unlike a badge
/// floating over a busy map, this surface already supplies contrast: adding a
/// shifted shadow here doubles small CJK strokes and makes them look blurred.
#[allow(clippy::too_many_arguments)]
fn draw_panel_text(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    text: &str,
    x: i32,
    top_y: i32,
    px: f32,
    color: u32,
) {
    crate::aa_text::draw_label_text_at(text, x, top_y, px, |gx, gy, cov| {
        blend_xrgb(sb, win_w, win_h, gx, gy, color, cov)
    });
}

const MAP_SELECTOR_MARGIN_PX: i32 = 8;
const MAP_SELECTOR_PAD_X: i32 = 8;
const MAP_SELECTOR_PAD_Y: i32 = 4;
const MAP_SELECTOR_GAP_PX: i32 = 4;
const MAP_SELECTOR_FONT_SCALE: f32 = 0.9;
const TOPMOST_SELECTOR_MIN_HEIGHT: usize = 120;
const CHARACTER_SELECTOR_MIN_HEIGHT: usize = 156;
const STARTUP_SELECTOR_MIN_HEIGHT: usize = 192;
const ESCAPE_HINT_MIN_HEIGHT: usize = 228;
const MAP_SELECTOR_BG: Rgb = Rgb {
    r: 248,
    g: 243,
    b: 222,
};
const MAP_SELECTOR_BORDER: Rgb = Rgb {
    r: 47,
    g: 78,
    b: 118,
};
const MAP_SELECTOR_INK: Rgb = Rgb {
    r: 39,
    g: 48,
    b: 58,
};
const MAP_SELECTOR_ACCENT: Rgb = Rgb {
    r: 85,
    g: 143,
    b: 194,
};
const SIZE_SELECTOR_ACCENT: Rgb = Rgb {
    r: 0x67,
    g: 0x9f,
    b: 0x54,
};
const TOPMOST_SELECTOR_ACCENT: Rgb = Rgb {
    r: 0xd6,
    g: 0x96,
    b: 0x38,
};
const CHARACTER_SELECTOR_ACCENT: Rgb = Rgb {
    r: 0x9a,
    g: 0x6f,
    b: 0xb0,
};
const STARTUP_SELECTOR_ACCENT: Rgb = Rgb {
    r: 0x4f,
    g: 0x9f,
    b: 0x9a,
};
const ESCAPE_HINT_ACCENT: Rgb = Rgb {
    r: 0x85,
    g: 0x7f,
    b: 0x76,
};

fn selector_rect(text: &str, font_px: f32, win_w: usize, win_h: usize, row: i32) -> LabelCardRect {
    let selector_font = font_px * MAP_SELECTOR_FONT_SCALE;
    let width = crate::aa_text::label_text_width(text, selector_font) + MAP_SELECTOR_PAD_X * 2;
    let height = crate::aa_text::line_height(selector_font) + MAP_SELECTOR_PAD_Y * 2;
    LabelCardRect {
        x: MAP_SELECTOR_MARGIN_PX,
        y: MAP_SELECTOR_MARGIN_PX + row.max(0) * (height + MAP_SELECTOR_GAP_PX),
        width: width
            .min(win_w.saturating_sub(MAP_SELECTOR_MARGIN_PX as usize * 2) as i32)
            .max(1),
        height: height
            .min(win_h.saturating_sub(MAP_SELECTOR_MARGIN_PX as usize * 2) as i32)
            .max(1),
    }
}

fn map_selector_rect(text: &str, font_px: f32, win_w: usize, win_h: usize) -> LabelCardRect {
    selector_rect(text, font_px, win_w, win_h, 0)
}

fn size_selector_rect(text: &str, font_px: f32, win_w: usize, win_h: usize) -> LabelCardRect {
    selector_rect(text, font_px, win_w, win_h, 1)
}

fn topmost_selector_rect(text: &str, font_px: f32, win_w: usize, win_h: usize) -> LabelCardRect {
    selector_rect(text, font_px, win_w, win_h, 2)
}

fn character_selector_rect(text: &str, font_px: f32, win_w: usize, win_h: usize) -> LabelCardRect {
    selector_rect(text, font_px, win_w, win_h, 3)
}

fn startup_selector_rect(text: &str, font_px: f32, win_w: usize, win_h: usize) -> LabelCardRect {
    selector_rect(text, font_px, win_w, win_h, 4)
}

#[cfg(test)]
fn escape_hint_rect(text: &str, font_px: f32, win_w: usize, win_h: usize) -> LabelCardRect {
    selector_rect(text, font_px, win_w, win_h, 5)
}

/// The third control row would collide with the Maple chat footer at the
/// 160×96 extreme. Keep map/size visible there and leave `T` as the compact
/// topmost control; at 120px and above the full stack fits without overlap.
pub(crate) const fn topmost_selector_visible(win_h: usize) -> bool {
    win_h >= TOPMOST_SELECTOR_MIN_HEIGHT
}

/// The fourth compact row is hidden at tiny heights; `C` remains available.
pub(crate) const fn character_selector_visible(win_h: usize) -> bool {
    win_h >= CHARACTER_SELECTOR_MIN_HEIGHT
}

/// The fifth row is the optional Windows sign-in startup toggle.
pub(crate) const fn startup_selector_visible(win_h: usize) -> bool {
    win_h >= STARTUP_SELECTOR_MIN_HEIGHT
}

/// Keep the non-clickable Escape reminder visible only when all six rows fit.
pub(crate) const fn escape_hint_visible(win_h: usize) -> bool {
    win_h >= ESCAPE_HINT_MIN_HEIGHT
}

fn selector_contains(rect: LabelCardRect, cursor: (f64, f64)) -> bool {
    cursor.0 >= f64::from(rect.x)
        && cursor.0 < f64::from(rect.right())
        && cursor.1 >= f64::from(rect.y)
        && cursor.1 < f64::from(rect.bottom())
}

/// Whether a physical-window cursor press lands on the compact Maple map tab.
pub(crate) fn map_selector_hit_test(
    cursor: (f64, f64),
    text: &str,
    font_px: f32,
    win_w: usize,
    win_h: usize,
) -> bool {
    selector_contains(map_selector_rect(text, font_px, win_w, win_h), cursor)
}

/// Whether a physical-window cursor press lands on the size-preset tab.
pub(crate) fn size_selector_hit_test(
    cursor: (f64, f64),
    text: &str,
    font_px: f32,
    win_w: usize,
    win_h: usize,
) -> bool {
    selector_contains(size_selector_rect(text, font_px, win_w, win_h), cursor)
}

/// Whether a physical-window cursor press lands on the always-on-top toggle.
pub(crate) fn topmost_selector_hit_test(
    cursor: (f64, f64),
    text: &str,
    font_px: f32,
    win_w: usize,
    win_h: usize,
) -> bool {
    selector_contains(topmost_selector_rect(text, font_px, win_w, win_h), cursor)
}

/// Whether a physical-window cursor press lands on the character-list tab.
pub(crate) fn character_selector_hit_test(
    cursor: (f64, f64),
    text: &str,
    font_px: f32,
    win_w: usize,
    win_h: usize,
) -> bool {
    selector_contains(character_selector_rect(text, font_px, win_w, win_h), cursor)
}

/// Whether a physical-window cursor press lands on the per-user startup toggle.
pub(crate) fn startup_selector_hit_test(
    cursor: (f64, f64),
    text: &str,
    font_px: f32,
    win_w: usize,
    win_h: usize,
) -> bool {
    selector_contains(startup_selector_rect(text, font_px, win_w, win_h), cursor)
}

/// Paint a parchment-like, clickable map tab above the pixel scene.
#[doc(hidden)]
pub fn paint_map_selector_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    text: &str,
    font_px: f32,
) {
    paint_selector_into_surface(sb, win_w, win_h, text, font_px, 0, MAP_SELECTOR_ACCENT);
}

/// Paint the adjacent mini/small/medium/large size control.
#[doc(hidden)]
pub fn paint_size_selector_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    text: &str,
    font_px: f32,
) {
    paint_selector_into_surface(sb, win_w, win_h, text, font_px, 1, SIZE_SELECTOR_ACCENT);
}

/// Paint the persisted always-on-top toggle in the same compact control stack.
#[doc(hidden)]
pub fn paint_topmost_selector_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    text: &str,
    font_px: f32,
) {
    paint_selector_into_surface(sb, win_w, win_h, text, font_px, 2, TOPMOST_SELECTOR_ACCENT);
}

/// Paint the character roster/showcase control in the compact stack.
#[doc(hidden)]
pub fn paint_character_selector_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    text: &str,
    font_px: f32,
) {
    paint_selector_into_surface(
        sb,
        win_w,
        win_h,
        text,
        font_px,
        3,
        CHARACTER_SELECTOR_ACCENT,
    );
}

/// Paint the Windows sign-in startup toggle in the compact control stack.
#[doc(hidden)]
pub fn paint_startup_selector_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    text: &str,
    font_px: f32,
) {
    paint_selector_into_surface(sb, win_w, win_h, text, font_px, 4, STARTUP_SELECTOR_ACCENT);
}

/// Paint a quiet, non-clickable reminder for the frameless close gesture.
#[doc(hidden)]
pub fn paint_escape_hint_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    text: &str,
    font_px: f32,
) {
    paint_selector_into_surface(sb, win_w, win_h, text, font_px, 5, ESCAPE_HINT_ACCENT);
}

/// Presentation state shown on the right side of a character row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharacterShowcaseState {
    Absent,
    Present,
    Leaving,
}

/// One fixed runtime character slot in the roster panel.
#[derive(Debug, Clone)]
pub(crate) struct CharacterPanelRow {
    pub name: String,
    /// Slot in the currently loaded active pack. The preview must use the same
    /// appearance index the world renderer assigns to this row.
    pub appearance_index: usize,
    pub agent_enabled: bool,
    /// Only install-local imported appearances may be removed. The eight
    /// required built-ins stay visible as fixed catalog rows.
    pub deletable: bool,
    /// The first delete click arms this row; a second click performs the
    /// recoverable removal.
    pub delete_confirmation: bool,
    pub market_showcase: CharacterShowcaseState,
    pub training_showcase: CharacterShowcaseState,
}

/// Complete panel copy/state, built from the renderer after each scene frame.
#[derive(Debug, Clone)]
pub(crate) struct CharacterPanelModel {
    pub real_market_agents: usize,
    pub real_training_agents: usize,
    pub market_showcase_count: usize,
    pub training_showcase_count: usize,
    pub rows: Vec<CharacterPanelRow>,
    /// Zero-based visible catalog page.
    pub page: usize,
    pub notice: Option<String>,
}

pub(crate) const CHARACTER_PANEL_PAGE_SIZE: usize = 8;

fn character_panel_page_count(row_count: usize) -> usize {
    row_count.div_ceil(CHARACTER_PANEL_PAGE_SIZE).max(1)
}

fn normalized_character_panel_page(page: usize, row_count: usize) -> usize {
    page.min(character_panel_page_count(row_count).saturating_sub(1))
}

/// Click action returned by [`character_panel_hit_test`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharacterPanelAction {
    ClosePanel,
    ToggleAgentSlot(usize),
    RequestDeleteSlot(usize),
    ToggleMarketShowcaseSlot(usize),
    ToggleTrainingShowcaseSlot(usize),
    /// Begin the install-local helper that reads a Maple Atelier appearance
    /// from the Windows clipboard. This is intentionally separate from the
    /// catalog row actions so it can never toggle a current character.
    StartMapleAtelierClipboardImport,
    PreviousPage,
    NextPage,
}

#[derive(Debug, Clone, Copy)]
struct CharacterPanelRowLayout {
    agent_toggle: LabelCardRect,
    delete: LabelCardRect,
    market_showcase_toggle: LabelCardRect,
    training_showcase_toggle: LabelCardRect,
    preview: LabelCardRect,
}

#[derive(Debug, Clone)]
struct CharacterPanelLayout {
    panel: LabelCardRect,
    rows: Vec<CharacterPanelRowLayout>,
    title_y: i32,
    subtitle_y: i32,
    footer_y: i32,
    close: LabelCardRect,
    import_from_clipboard: LabelCardRect,
    previous_page: LabelCardRect,
    next_page: LabelCardRect,
    font_px: f32,
}

fn character_panel_layout(
    win_w: usize,
    win_h: usize,
    font_px: f32,
) -> Option<CharacterPanelLayout> {
    const EDGE: i32 = 10;
    const PAD: i32 = 9;
    const COLUMN_GAP: i32 = 8;
    const ROW_GAP: i32 = 4;
    if win_w < 560 || win_h < 176 {
        return None;
    }
    // This panel is painted at the native window resolution.  Do not apply the
    // old compact-control shrink to CJK text: the launcher's default 15 px
    // label should remain roughly 16 px here, with enough room for Noto Sans TC.
    let panel_font = (font_px * 1.05).clamp(13.0, 18.0);
    let line = crate::aa_text::label_line_height(panel_font);
    let row_h = (line + 9).max(36);
    let header_h = line * 2 + 13;
    let footer_h = line + 12;
    let required_h = PAD * 2 + header_h + row_h * 4 + ROW_GAP * 3 + footer_h;
    let panel_w = (win_w as i32 - EDGE * 2).min(780);
    let panel_h = required_h.min(win_h as i32 - EDGE * 2);
    if panel_w < 340 || panel_h < required_h {
        return None;
    }
    let panel = LabelCardRect {
        x: (win_w as i32 - panel_w) / 2,
        y: (win_h as i32 - panel_h) / 2,
        width: panel_w,
        height: panel_h,
    };
    let content_x = panel.x + PAD;
    let content_w = panel.width - PAD * 2;
    let column_w = (content_w - COLUMN_GAP) / 2;
    let rows_top = panel.y + PAD + header_h;
    let mut rows = Vec::with_capacity(CHARACTER_PANEL_PAGE_SIZE);
    for slot in 0..CHARACTER_PANEL_PAGE_SIZE {
        let column = (slot / 4) as i32;
        let row = (slot % 4) as i32;
        let row_rect = LabelCardRect {
            x: content_x + column * (column_w + COLUMN_GAP),
            y: rows_top + row * (row_h + ROW_GAP),
            width: column_w,
            height: row_h,
        };
        let action_gap = 4;
        let showcase_w = (row_rect.width * 15 / 100).clamp(42, 58);
        let delete_w = (row_rect.width * 10 / 100).clamp(30, 38);
        let agent_toggle = LabelCardRect {
            x: row_rect.x,
            y: row_rect.y,
            width: row_rect.width - showcase_w * 2 - delete_w - action_gap * 3,
            height: row_rect.height,
        };
        let delete = LabelCardRect {
            x: agent_toggle.right() + action_gap,
            y: row_rect.y,
            width: delete_w,
            height: row_rect.height,
        };
        let market_showcase_toggle = LabelCardRect {
            x: delete.right() + action_gap,
            y: row_rect.y,
            width: showcase_w,
            height: row_rect.height,
        };
        rows.push(CharacterPanelRowLayout {
            preview: LabelCardRect {
                x: agent_toggle.x + 22,
                y: agent_toggle.y + (agent_toggle.height - 28) / 2,
                width: 36,
                height: 28,
            },
            agent_toggle,
            delete,
            market_showcase_toggle,
            training_showcase_toggle: LabelCardRect {
                x: market_showcase_toggle.right() + action_gap,
                y: row_rect.y,
                width: showcase_w,
                height: row_rect.height,
            },
        });
    }
    let nav_width = 52;
    let nav_gap = 5;
    let nav_height = line + 6;
    let nav_y = panel.bottom() - PAD - nav_height;
    let next_page = LabelCardRect {
        x: panel.right() - PAD - nav_width,
        y: nav_y,
        width: nav_width,
        height: nav_height,
    };
    let previous_page = LabelCardRect {
        x: next_page.x - nav_gap - nav_width,
        y: nav_y,
        width: nav_width,
        height: nav_height,
    };
    let import_font = (panel_font * 0.8).max(12.0);
    let import_text = "＋ 新增造型（剪貼簿）";
    let import_width =
        (crate::aa_text::label_text_width(import_text, import_font) + 12).clamp(150, 190);
    let import_height = crate::aa_text::label_line_height(import_font) + 6;
    let close = LabelCardRect {
        x: panel.right() - PAD - import_height,
        y: panel.y + PAD - 2,
        width: import_height,
        height: import_height,
    };
    Some(CharacterPanelLayout {
        panel,
        rows,
        title_y: panel.y + PAD,
        subtitle_y: panel.y + PAD + line,
        footer_y: panel.bottom() - PAD - line,
        close,
        import_from_clipboard: LabelCardRect {
            x: close.x - 5 - import_width,
            y: panel.y + PAD - 2,
            width: import_width,
            height: import_height,
        },
        previous_page,
        next_page,
        font_px: panel_font,
    })
}

/// Resolve a roster-panel click into the independent left/right row action.
pub(crate) fn character_panel_hit_test(
    cursor: (f64, f64),
    font_px: f32,
    win_w: usize,
    win_h: usize,
    page: usize,
    row_count: usize,
) -> Option<CharacterPanelAction> {
    let layout = character_panel_layout(win_w, win_h, font_px)?;
    if selector_contains(layout.close, cursor) {
        return Some(CharacterPanelAction::ClosePanel);
    }
    if selector_contains(layout.import_from_clipboard, cursor) {
        return Some(CharacterPanelAction::StartMapleAtelierClipboardImport);
    }
    if selector_contains(layout.previous_page, cursor) {
        return Some(CharacterPanelAction::PreviousPage);
    }
    if selector_contains(layout.next_page, cursor) {
        return Some(CharacterPanelAction::NextPage);
    }
    let page = normalized_character_panel_page(page, row_count);
    let start = page * CHARACTER_PANEL_PAGE_SIZE;
    for (local_slot, row) in layout.rows.iter().enumerate() {
        let slot = start + local_slot;
        if slot >= row_count {
            break;
        }
        if selector_contains(row.agent_toggle, cursor) {
            return Some(CharacterPanelAction::ToggleAgentSlot(slot));
        }
        if slot >= pixtuoid_scene::characters::CHARACTER_SLOT_COUNT
            && selector_contains(row.delete, cursor)
        {
            return Some(CharacterPanelAction::RequestDeleteSlot(slot));
        }
        if selector_contains(row.market_showcase_toggle, cursor) {
            return Some(CharacterPanelAction::ToggleMarketShowcaseSlot(slot));
        }
        if selector_contains(row.training_showcase_toggle, cursor) {
            return Some(CharacterPanelAction::ToggleTrainingShowcaseSlot(slot));
        }
    }
    None
}

fn paint_panel_button(
    canvas: &mut LabelCardCanvas<'_>,
    rect: LabelCardRect,
    fill: Rgb,
    text: &str,
    font_px: f32,
) {
    paint_panel_button_shell(canvas, rect, fill);
    let text_y = rect.y + (rect.height - crate::aa_text::label_line_height(font_px)) / 2;
    draw_panel_text(
        canvas.pixels,
        canvas.width,
        canvas.height,
        text,
        rect.x + 5,
        text_y,
        font_px,
        pack_xrgb(MAP_SELECTOR_INK),
    );
}

fn paint_panel_button_shell(canvas: &mut LabelCardCanvas<'_>, rect: LabelCardRect, fill: Rgb) {
    canvas.fill_rect(rect, pack_xrgb(fill), 0.9);
    for edge in [
        LabelCardRect { height: 1, ..rect },
        LabelCardRect {
            y: rect.bottom() - 1,
            height: 1,
            ..rect
        },
        LabelCardRect { width: 1, ..rect },
        LabelCardRect {
            x: rect.right() - 1,
            width: 1,
            ..rect
        },
    ] {
        canvas.fill_rect(edge, pack_xrgb(MAP_SELECTOR_BORDER), 0.85);
    }
}

fn paint_character_preview(
    canvas: &mut LabelCardCanvas<'_>,
    rect: LabelCardRect,
    pack: &Pack,
    appearance_index: usize,
) {
    let Some(frame) = pixtuoid_scene::market::market_avatar_animation(pack)
        .and_then(|animation| animation.frames.get(appearance_index))
    else {
        return;
    };
    for dy in 0..rect.height.max(0) {
        let source_y = (dy as u32 * u32::from(frame.height()) / rect.height.max(1) as u32) as u16;
        for dx in 0..rect.width.max(0) {
            let source_x = (dx as u32 * u32::from(frame.width()) / rect.width.max(1) as u32) as u16;
            let Some(rgb) = frame.get(source_x, source_y).copied().flatten() else {
                continue;
            };
            let x = rect.x + dx;
            let y = rect.y + dy;
            if x >= 0 && y >= 0 && x < canvas.width as i32 && y < canvas.height as i32 {
                canvas.pixels[y as usize * canvas.width + x as usize] = pack_xrgb(rgb);
            }
        }
    }
}

fn paint_character_panel_fallback(canvas: &mut LabelCardCanvas<'_>, font_px: f32) {
    let message = "角色清單需要較大視窗｜按 Z 放大";
    let px = (font_px * 1.05).clamp(13.0, 18.0);
    let width = (crate::aa_text::label_text_width(message, px) + 20)
        .min(canvas.width.saturating_sub(12) as i32)
        .max(1);
    let height = crate::aa_text::label_line_height(px) + 16;
    let rect = LabelCardRect {
        x: (canvas.width as i32 - width) / 2,
        y: (canvas.height as i32 - height) / 2,
        width,
        height,
    };
    canvas.fill_rect(rect, pack_xrgb(MAP_SELECTOR_BG), 0.96);
    draw_panel_text(
        canvas.pixels,
        canvas.width,
        canvas.height,
        message,
        rect.x + 10,
        rect.y + 8,
        px,
        pack_xrgb(MAP_SELECTOR_INK),
    );
}

fn paint_character_panel_shell(canvas: &mut LabelCardCanvas<'_>, layout: &CharacterPanelLayout) {
    let shadow = LabelCardRect {
        x: layout.panel.x + 3,
        y: layout.panel.y + 3,
        ..layout.panel
    };
    canvas.fill_rect(
        shadow,
        pack_xrgb(Rgb {
            r: 18,
            g: 22,
            b: 27,
        }),
        0.58,
    );
    canvas.fill_rect(layout.panel, pack_xrgb(MAP_SELECTOR_BG), 0.97);
    canvas.paint_frame(
        layout.panel,
        pack_xrgb(MAP_SELECTOR_BG),
        pack_xrgb(MAP_SELECTOR_BORDER),
    );
}

fn paint_character_panel_header(
    canvas: &mut LabelCardCanvas<'_>,
    layout: &CharacterPanelLayout,
    model: &CharacterPanelModel,
) {
    let title = format!(
        "角色 {} 款｜市 A{}／展{}｜訓 A{}／展{}",
        model.rows.len(),
        model.real_market_agents,
        model.market_showcase_count,
        model.real_training_agents,
        model.training_showcase_count
    );
    draw_panel_text(
        canvas.pixels,
        canvas.width,
        canvas.height,
        &title,
        layout.panel.x + 10,
        layout.title_y,
        layout.font_px,
        pack_xrgb(MAP_SELECTOR_INK),
    );
    paint_panel_button(
        canvas,
        layout.import_from_clipboard,
        Rgb {
            r: 243,
            g: 215,
            b: 161,
        },
        "＋ 新增造型（剪貼簿）",
        (layout.font_px * 0.8).max(12.0),
    );
    paint_panel_button(
        canvas,
        layout.close,
        Rgb {
            r: 231,
            g: 186,
            b: 173,
        },
        "X",
        (layout.font_px * 0.88).max(12.5),
    );
    let secondary_font = (layout.font_px * 0.9).max(12.5);
    draw_panel_text(
        canvas.pixels,
        canvas.width,
        canvas.height,
        "綠勾：真 Agent 可用造型　右側：逛街／練功（非 Agent）",
        layout.panel.x + 10,
        layout.subtitle_y,
        secondary_font,
        pack_xrgb(Rgb {
            r: 68,
            g: 76,
            b: 82,
        }),
    );
}

fn showcase_button_style(
    state: CharacterShowcaseState,
    absent_text: &'static str,
) -> (&'static str, Rgb) {
    match state {
        CharacterShowcaseState::Absent => (
            absent_text,
            Rgb {
                r: 198,
                g: 218,
                b: 237,
            },
        ),
        CharacterShowcaseState::Present => (
            "撤回",
            Rgb {
                r: 244,
                g: 205,
                b: 151,
            },
        ),
        CharacterShowcaseState::Leaving => (
            "退場中",
            Rgb {
                r: 214,
                g: 205,
                b: 225,
            },
        ),
    }
}

fn paint_character_panel_rows(
    canvas: &mut LabelCardCanvas<'_>,
    layout: &CharacterPanelLayout,
    model: &CharacterPanelModel,
    pack: &Pack,
) {
    let page = normalized_character_panel_page(model.page, model.rows.len());
    let start = page * CHARACTER_PANEL_PAGE_SIZE;
    for (row_model, row_layout) in model
        .rows
        .iter()
        .skip(start)
        .take(CHARACTER_PANEL_PAGE_SIZE)
        .zip(layout.rows.iter())
    {
        let checked = if row_model.agent_enabled { "✓" } else { " " };
        let agent_fill = if row_model.agent_enabled {
            Rgb {
                r: 206,
                g: 229,
                b: 190,
            }
        } else {
            Rgb {
                r: 224,
                g: 221,
                b: 209,
            }
        };
        paint_panel_button_shell(canvas, row_layout.agent_toggle, agent_fill);
        let text_y = row_layout.agent_toggle.y
            + (row_layout.agent_toggle.height - crate::aa_text::label_line_height(layout.font_px))
                / 2;
        draw_panel_text(
            canvas.pixels,
            canvas.width,
            canvas.height,
            checked,
            row_layout.agent_toggle.x + 5,
            text_y,
            layout.font_px,
            pack_xrgb(MAP_SELECTOR_INK),
        );
        paint_character_preview(canvas, row_layout.preview, pack, row_model.appearance_index);
        let name_x = row_layout.preview.right() + 7;
        let name = truncate_label_to_px(
            &row_model.name,
            (row_layout.agent_toggle.right() - name_x - 5).max(1),
            layout.font_px,
        );
        draw_panel_text(
            canvas.pixels,
            canvas.width,
            canvas.height,
            &name,
            name_x,
            text_y,
            layout.font_px,
            pack_xrgb(MAP_SELECTOR_INK),
        );
        let (delete_text, delete_fill) = if row_model.delete_confirmation {
            (
                "確認",
                Rgb {
                    r: 224,
                    g: 128,
                    b: 116,
                },
            )
        } else if row_model.deletable {
            (
                "刪除",
                Rgb {
                    r: 235,
                    g: 185,
                    b: 173,
                },
            )
        } else {
            (
                "固定",
                Rgb {
                    r: 224,
                    g: 221,
                    b: 209,
                },
            )
        };
        paint_panel_button(
            canvas,
            row_layout.delete,
            delete_fill,
            delete_text,
            (layout.font_px * 0.72).max(11.5),
        );
        let (market_text, market_fill) = showcase_button_style(row_model.market_showcase, "逛街");
        paint_panel_button(
            canvas,
            row_layout.market_showcase_toggle,
            market_fill,
            market_text,
            (layout.font_px * 0.82).max(12.0),
        );
        let (training_text, training_fill) =
            showcase_button_style(row_model.training_showcase, "練功");
        paint_panel_button(
            canvas,
            row_layout.training_showcase_toggle,
            training_fill,
            training_text,
            (layout.font_px * 0.9).max(12.5),
        );
    }
}

fn paint_character_panel_footer(
    canvas: &mut LabelCardCanvas<'_>,
    layout: &CharacterPanelLayout,
    model: &CharacterPanelModel,
) {
    let footer = model
        .notice
        .as_deref()
        .unwrap_or("[C]／右上 X 關閉｜自訂角色連按兩次「刪除」｜至少保留 1 個 Agent 外觀");
    let secondary_font = (layout.font_px * 0.9).max(12.5);
    draw_panel_text(
        canvas.pixels,
        canvas.width,
        canvas.height,
        footer,
        layout.panel.x + 10,
        layout.footer_y,
        secondary_font,
        pack_xrgb(Rgb {
            r: 89,
            g: 62,
            b: 45,
        }),
    );

    let page = normalized_character_panel_page(model.page, model.rows.len());
    let page_count = character_panel_page_count(model.rows.len());
    let page_text = format!("{}/{}", page + 1, page_count);
    let page_width = crate::aa_text::label_text_width(&page_text, secondary_font);
    draw_panel_text(
        canvas.pixels,
        canvas.width,
        canvas.height,
        &page_text,
        layout.previous_page.x - page_width - 8,
        layout.footer_y,
        secondary_font,
        pack_xrgb(MAP_SELECTOR_INK),
    );
    let previous_fill = if page > 0 {
        Rgb {
            r: 198,
            g: 218,
            b: 237,
        }
    } else {
        Rgb {
            r: 224,
            g: 221,
            b: 209,
        }
    };
    let next_fill = if page + 1 < page_count {
        Rgb {
            r: 198,
            g: 218,
            b: 237,
        }
    } else {
        Rgb {
            r: 224,
            g: 221,
            b: 209,
        }
    };
    paint_panel_button(
        canvas,
        layout.previous_page,
        previous_fill,
        "上一頁",
        secondary_font,
    );
    paint_panel_button(
        canvas,
        layout.next_page,
        next_fill,
        "下一頁",
        secondary_font,
    );
}

/// Paint the two-column character roster and manual showcase panel.
#[doc(hidden)]
pub(crate) fn paint_character_panel_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    model: &CharacterPanelModel,
    pack: &Pack,
    font_px: f32,
) {
    if sb.len() < win_w.saturating_mul(win_h) || win_w == 0 || win_h == 0 {
        return;
    }
    let mut canvas = LabelCardCanvas::new(sb, win_w, win_h);
    let Some(layout) = character_panel_layout(win_w, win_h, font_px) else {
        paint_character_panel_fallback(&mut canvas, font_px);
        return;
    };
    paint_character_panel_shell(&mut canvas, &layout);
    paint_character_panel_header(&mut canvas, &layout, model);
    paint_character_panel_rows(&mut canvas, &layout, model, pack);
    paint_character_panel_footer(&mut canvas, &layout, model);
}

#[allow(clippy::too_many_arguments)]
fn paint_selector_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    text: &str,
    font_px: f32,
    row: i32,
    accent: Rgb,
) {
    if sb.len() < win_w.saturating_mul(win_h) || win_w == 0 || win_h == 0 {
        return;
    }
    let rect = selector_rect(text, font_px, win_w, win_h, row);
    let shadow = LabelCardRect {
        x: rect.x + 2,
        y: rect.y + 2,
        ..rect
    };
    let mut canvas = LabelCardCanvas::new(sb, win_w, win_h);
    canvas.fill_rect(
        shadow,
        pack_xrgb(Rgb {
            r: 24,
            g: 29,
            b: 32,
        }),
        0.45,
    );
    canvas.fill_rect(rect, pack_xrgb(MAP_SELECTOR_BG), 0.9);
    for edge in [
        LabelCardRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: 1,
        },
        LabelCardRect {
            x: rect.x,
            y: rect.bottom() - 1,
            width: rect.width,
            height: 1,
        },
        LabelCardRect {
            x: rect.x,
            y: rect.y,
            width: 1,
            height: rect.height,
        },
        LabelCardRect {
            x: rect.right() - 1,
            y: rect.y,
            width: 1,
            height: rect.height,
        },
    ] {
        canvas.fill_rect(edge, pack_xrgb(MAP_SELECTOR_BORDER), 1.0);
    }
    canvas.fill_rect(
        LabelCardRect {
            x: rect.x + 2,
            y: rect.y + 2,
            width: 3,
            height: (rect.height - 4).max(1),
        },
        pack_xrgb(accent),
        1.0,
    );
    let selector_font = font_px * MAP_SELECTOR_FONT_SCALE;
    let text_y = rect.y + (rect.height - crate::aa_text::line_height(selector_font)) / 2;
    draw_badge_text(
        canvas.pixels,
        win_w,
        win_h,
        text,
        rect.x + MAP_SELECTOR_PAD_X,
        text_y,
        selector_font,
        pack_xrgb(MAP_SELECTOR_INK),
    );
}

const LABEL_CARD_COLLISION_GAP: i32 = 4;
const LABEL_CARD_HEAD_GAP: i32 = 5;
const LABEL_CARD_EDGE_MARGIN: i32 = 2;
const LABEL_CARD_TITLE_WIDTH_EM: f32 = 4.25;
const LABEL_CARD_BACKGROUND_COVERAGE: f32 = 0.72;
const LABEL_CARD_MAX_NUDGE_PX: i32 = 20;
const MAPLE_CARD_HEAD_GAP: i32 = 2;
const MAPLE_CARD_TITLE_WIDTH_EM: f32 = 7.4;
const MAPLE_CARD_MAX_TITLE_LINES: usize = 2;
const MAPLE_CARD_BODY_COVERAGE: f32 = 0.88;
const MAPLE_CARD_INFO_COVERAGE: f32 = 0.88;
const MAPLE_CARD_LATIN_INK_STRENGTH: f32 = 0.82;
const MAPLE_CARD_SOURCE_FONT_SCALE: f32 = 0.84;
const MAPLE_CARD_SOURCE_FONT_MIN_PX: f32 = 10.0;
const MAPLE_PLAYER_ID_FONT_MAX_PX: f32 = 12.0;
const MAPLE_CARD_DECOR_GAP_PX: i32 = 3;
const MAPLE_CARD_ICON_MIN_PX: i32 = 5;
const MAPLE_CARD_ICON_MAX_PX: i32 = 11;
const MAPLE_CARD_ICON_FONT_SCALE: f32 = 0.82;
const MAPLE_CARD_INNER_HIGHLIGHT_COVERAGE: f32 = 0.62;
const TRAINING_CARD_HEAD_GAP: i32 = 2;
const TRAINING_CARD_TITLE_WIDTH_EM: f32 = 7.6;
const TRAINING_CARD_MAX_TITLE_LINES: usize = 1;
const TRAINING_CARD_BODY_COVERAGE: f32 = 0.78;
const TRAINING_CARD_SOURCE_COVERAGE: f32 = 0.52;
const TRAINING_CARD_SOURCE_FONT_SCALE: f32 = 0.78;
const TRAINING_CARD_SOURCE_FONT_MIN_PX: f32 = 9.0;
const MAPLE_CARD_BODY: Rgb = Rgb {
    r: 0xff,
    g: 0xfd,
    b: 0xf4,
};
const MAPLE_CARD_BORDER: Rgb = Rgb {
    r: 0x48,
    g: 0x5a,
    b: 0x6a,
};
const MAPLE_CARD_INFO: Rgb = Rgb {
    r: 0xe7,
    g: 0xf3,
    b: 0xf8,
};
const MAPLE_CARD_TITLE_INK: Rgb = Rgb {
    r: 0x22,
    g: 0x2d,
    b: 0x36,
};
const TRAINING_CARD_BODY: Rgb = Rgb {
    r: 0x14,
    g: 0x20,
    b: 0x2b,
};
const TRAINING_CARD_SOURCE_BODY: Rgb = Rgb {
    r: 0x25,
    g: 0x3d,
    b: 0x4c,
};
const TRAINING_CARD_BORDER: Rgb = Rgb {
    r: 0xd8,
    g: 0xe8,
    b: 0xf0,
};
const TRAINING_CARD_TITLE_INK: Rgb = Rgb {
    r: 0xff,
    g: 0xfa,
    b: 0xe8,
};
const TRAINING_CARD_SOURCE_INK: Rgb = Rgb {
    r: 0xc8,
    g: 0xe2,
    b: 0xec,
};
const MAPLE_PLAYER_ID_INK: Rgb = Rgb {
    r: 0xff,
    g: 0xf4,
    b: 0xc7,
};
const MAPLE_PLAYER_ID_SHADOW: Rgb = Rgb {
    r: 0x24,
    g: 0x17,
    b: 0x10,
};
const MAPLE_PLAYER_ID_PLATE_BODY: Rgb = Rgb {
    r: 0x1d,
    g: 0x1a,
    b: 0x17,
};
const MAPLE_PLAYER_ID_PLATE_BORDER: Rgb = Rgb {
    r: 0x76,
    g: 0x58,
    b: 0x37,
};
const MAPLE_PLAYER_ID_PLATE_BODY_COVERAGE: f32 = 0.82;
const MAPLE_PLAYER_ID_PLATE_PAD_X: i32 = 4;
const MAPLE_PLAYER_ID_PLATE_PAD_TOP: i32 = 2;
const MAPLE_PLAYER_ID_PLATE_PAD_BOTTOM: i32 = 2;
const MAPLE_CARD_DIVIDER: Rgb = Rgb {
    r: 0x8d,
    g: 0xa6,
    b: 0xb3,
};
const MAPLE_CARD_INNER_HIGHLIGHT: Rgb = Rgb {
    r: 0xff,
    g: 0xff,
    b: 0xff,
};
const MAPLE_CARD_COIN: Rgb = Rgb {
    r: 0xd4,
    g: 0x94,
    b: 0x1e,
};
const MAPLE_CARD_COIN_HIGHLIGHT: Rgb = Rgb {
    r: 0xff,
    g: 0xdf,
    b: 0x70,
};
const MAPLE_CARD_PERMIT: Rgb = Rgb {
    r: 0xc6,
    g: 0xb8,
    b: 0xd3,
};
const MAPLE_CARD_PERMIT_HIGHLIGHT: Rgb = Rgb {
    r: 0xee,
    g: 0xe8,
    b: 0xf2,
};
const MAPLE_CARD_ACTION_BLUE: Rgb = Rgb {
    r: 0x4c,
    g: 0xa4,
    b: 0xcf,
};
const MAPLE_CARD_INACTIVE_RING: Rgb = Rgb {
    r: 0xe2,
    g: 0xd6,
    b: 0xe4,
};
const MAPLE_CARD_FOLDED_CORNER: Rgb = Rgb {
    r: 0xdc,
    g: 0x6d,
    b: 0x55,
};
const MAPLE_CHAT_BORDER: Rgb = Rgb {
    r: 0x55,
    g: 0x4b,
    b: 0x5f,
};
const MAPLE_CHAT_BODY: Rgb = Rgb {
    r: 0xe8,
    g: 0xb7,
    b: 0xcf,
};
const MAPLE_CHAT_HIGHLIGHT: Rgb = Rgb {
    r: 0xff,
    g: 0xe7,
    b: 0xf1,
};
const MAPLE_CHAT_SHADOW: Rgb = Rgb {
    r: 0x9c,
    g: 0x5d,
    b: 0x82,
};
const MAPLE_CHAT_CHANNEL: Rgb = Rgb {
    r: 0xc8,
    g: 0x74,
    b: 0xa4,
};
const MAPLE_CHAT_INK: Rgb = Rgb {
    r: 0x51,
    g: 0x2d,
    b: 0x43,
};
const MAPLE_CHAT_CHANNEL_INK: Rgb = Rgb {
    r: 0xff,
    g: 0xf7,
    b: 0xfb,
};
const MAPLE_CHAT_BUTTON: Rgb = Rgb {
    r: 0x5b,
    g: 0xa9,
    b: 0xc9,
};
const MAPLE_CHAT_BUTTON_DARK: Rgb = Rgb {
    r: 0x35,
    g: 0x69,
    b: 0x87,
};
const MAPLE_CHAT_BUTTON_INK: Rgb = Rgb {
    r: 0xf5,
    g: 0xfc,
    b: 0xff,
};
const MAPLE_CHAT_MARGIN_X: i32 = 4;
const MAPLE_CHAT_MARGIN_BOTTOM: i32 = 3;
const MAPLE_CHAT_INNER_GAP: i32 = 5;
const MAPLE_CHAT_BUTTON_GAP: i32 = 2;
const MAPLE_CHAT_BODY_COVERAGE: f32 = 0.92;
const MAPLE_CHAT_FONT_MIN_PX: f32 = 12.0;
const MAPLE_CHAT_FONT_MAX_PX: f32 = 18.0;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LabelCardText {
    source: String,
    title_lines: Vec<String>,
    title_width_px: i32,
}

#[derive(Debug, Clone, Copy)]
struct LabelCardMetrics {
    width: i32,
    height: i32,
    pad_x: i32,
    pad_y: i32,
    title_line_height: i32,
    source_font_px: f32,
    source_band_height: i32,
    maple_icon_size: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelCardStyle {
    Generic,
    MapleShop,
    Training,
}

#[derive(Debug, Clone, Copy)]
struct MapleShopFrameSpec {
    source_band_height: i32,
    icon_size: i32,
    status_color: u32,
}

#[derive(Debug, Clone, Copy)]
struct LabelCardSpec {
    label_index: usize,
    anchor_x: i32,
    anchor_y: i32,
    width: i32,
    height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LabelCardRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl LabelCardRect {
    fn right(self) -> i32 {
        self.x + self.width
    }

    fn bottom(self) -> i32 {
        self.y + self.height
    }

    fn overlaps_with_gap(self, other: Self, gap: i32) -> bool {
        self.x < other.right() + gap
            && self.right() + gap > other.x
            && self.y < other.bottom() + gap
            && self.bottom() + gap > other.y
    }

    fn overlap_area(self, other: Self) -> i32 {
        let width = (self.right().min(other.right()) - self.x.max(other.x)).max(0);
        let height = (self.bottom().min(other.bottom()) - self.y.max(other.y)).max(0);
        width * height
    }
}

#[derive(Debug, Clone, Copy)]
struct PlacedLabelCard {
    label_index: usize,
    anchor_x: i32,
    anchor_y: i32,
    rect: LabelCardRect,
}

fn build_label_card_text(text: &str, font_px: f32) -> LabelCardText {
    build_label_card_text_with_layout(text, font_px, LABEL_CARD_TITLE_WIDTH_EM, None)
}

#[cfg(test)]
fn build_maple_label_card_text(text: &str, font_px: f32) -> LabelCardText {
    build_maple_label_card_text_with_relation(text, font_px, None)
}

fn build_maple_label_card_text_with_relation(
    text: &str,
    font_px: f32,
    relation: Option<&pixtuoid_scene::overlay::AgentRelation>,
) -> LabelCardText {
    let title_line_limit = if relation.is_some() {
        1
    } else {
        MAPLE_CARD_MAX_TITLE_LINES
    };
    let mut card = build_label_card_text_with_layout(
        text,
        font_px,
        MAPLE_CARD_TITLE_WIDTH_EM,
        Some(title_line_limit),
    );
    if let Some(relation) = relation {
        let relation_text = lineage_card_text(relation);
        let relation_width_px = (font_px * 9.2).round().max(card.title_width_px as f32) as i32;
        card.title_lines.push(truncate_label_to_px(
            &relation_text,
            relation_width_px,
            font_px,
        ));
        card.title_width_px = relation_width_px;
    }
    card
}

fn lineage_card_text(relation: &pixtuoid_scene::overlay::AgentRelation) -> String {
    use pixtuoid_scene::overlay::AgentRelation;
    match relation {
        AgentRelation::Root {
            descendant_count, ..
        } => format!("◆ 主 task｜子 {descendant_count}"),
        AgentRelation::Child {
            root_task, depth, ..
        } => format!("子{depth}→主：{}", root_task.as_deref().unwrap_or("已離場")),
    }
}

fn lineage_family_color(relation: &pixtuoid_scene::overlay::AgentRelation) -> Rgb {
    use pixtuoid_scene::overlay::AgentRelation;
    const FAMILY_COLORS: [Rgb; 6] = [
        Rgb {
            r: 0x2e,
            g: 0x86,
            b: 0xc1,
        },
        Rgb {
            r: 0x1f,
            g: 0xa7,
            b: 0x8b,
        },
        Rgb {
            r: 0x8e,
            g: 0x62,
            b: 0xb7,
        },
        Rgb {
            r: 0xd3,
            g: 0x7a,
            b: 0x45,
        },
        Rgb {
            r: 0xc1,
            g: 0x4f,
            b: 0x64,
        },
        Rgb {
            r: 0x72,
            g: 0x8f,
            b: 0x3e,
        },
    ];
    const FAMILY_COLOR_TAG: u64 = 0x6661_6d69_6c79_5f31;
    let root_id = match relation {
        AgentRelation::Root { root_id, .. } | AgentRelation::Child { root_id, .. } => *root_id,
    };
    let bucket = pixtuoid_core::id::splitmix64(root_id.raw() ^ FAMILY_COLOR_TAG) as usize
        % FAMILY_COLORS.len();
    FAMILY_COLORS[bucket]
}

fn training_status_text(tone: pixtuoid_scene::overlay::LabelTone) -> &'static str {
    use pixtuoid_scene::overlay::LabelTone;
    match tone {
        LabelTone::Active => "工作中",
        LabelTone::Waiting => "待回覆",
        LabelTone::Idle => "閒置",
        LabelTone::Exiting => "離場中",
    }
}

#[cfg(test)]
fn build_training_label_card_text(
    text: &str,
    font_px: f32,
    tone: pixtuoid_scene::overlay::LabelTone,
) -> LabelCardText {
    build_training_label_card_text_with_relation(text, font_px, tone, None)
}

fn build_training_label_card_text_with_relation(
    text: &str,
    font_px: f32,
    tone: pixtuoid_scene::overlay::LabelTone,
    relation: Option<&pixtuoid_scene::overlay::AgentRelation>,
) -> LabelCardText {
    let Some((fake_id, task)) = text.split_once('\u{00b7}') else {
        // Presentation-only training actors intentionally use one bare label
        // ("練功中").  Keeping that representation single-line prevents the
        // renderer from inventing Agent/work metadata for a non-Agent actor.
        return build_label_card_text_with_layout(
            text,
            font_px,
            TRAINING_CARD_TITLE_WIDTH_EM,
            Some(TRAINING_CARD_MAX_TITLE_LINES),
        );
    };
    let normalized = format!("{fake_id}\u{00b7}{task}");
    let mut card = build_label_card_text_with_layout(
        &normalized,
        font_px,
        TRAINING_CARD_TITLE_WIDTH_EM,
        Some(TRAINING_CARD_MAX_TITLE_LINES),
    );
    card.source = match relation {
        None => format!("{fake_id} · {}", training_status_text(tone)),
        Some(pixtuoid_scene::overlay::AgentRelation::Root {
            descendant_count, ..
        }) => format!(
            "{fake_id}｜主 task｜子{descendant_count}｜{}",
            training_status_text(tone)
        ),
        Some(pixtuoid_scene::overlay::AgentRelation::Child {
            root_task, depth, ..
        }) => format!(
            "子{depth}｜主：{}｜{}",
            root_task.as_deref().unwrap_or("已離場"),
            training_status_text(tone)
        ),
    };
    card
}

fn build_label_card_text_with_layout(
    text: &str,
    font_px: f32,
    title_width_em: f32,
    max_title_lines: Option<usize>,
) -> LabelCardText {
    let (source, title) = text
        .split_once('\u{00b7}')
        .map_or(("", text), |(source, title)| (source, title));
    let title_width_px = (font_px * title_width_em).round().max(1.0) as i32;
    let mut title_lines = Vec::new();
    let mut current = String::new();

    for ch in title.chars() {
        let mut candidate = current.clone();
        candidate.push(ch);
        if !current.is_empty()
            && crate::aa_text::label_text_width(&candidate, font_px) > title_width_px
        {
            title_lines.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        title_lines.push(current);
    }
    if title_lines.is_empty() {
        title_lines.push(source.to_owned());
    }
    if let Some(max_lines) = max_title_lines {
        if title_lines.len() > max_lines {
            title_lines.truncate(max_lines);
            let last = title_lines
                .last_mut()
                .expect("a positive Maple line limit retains one title row");
            let ellipsis = '\u{2026}';
            while !last.is_empty() {
                let mut candidate = last.clone();
                candidate.push(ellipsis);
                if crate::aa_text::label_text_width(&candidate, font_px) <= title_width_px {
                    break;
                }
                last.pop();
            }
            last.push(ellipsis);
        }
    }

    LabelCardText {
        source: source.to_owned(),
        title_lines,
        title_width_px,
    }
}

fn label_card_metrics(card: &LabelCardText, font_px: f32) -> LabelCardMetrics {
    let pad_x = (font_px * 0.32).ceil().max(4.0) as i32;
    let pad_y = (font_px * 0.20).ceil().max(3.0) as i32;
    let title_line_height = (font_px * 1.22).ceil() as i32;
    let source_font_px = (font_px * 0.74).max(8.0);
    let source_line_height = (source_font_px * 1.24).ceil() as i32;
    let title_width = card
        .title_lines
        .iter()
        .map(|line| crate::aa_text::label_text_width(line, font_px))
        .max()
        .unwrap_or(1);
    let source_width = if card.source.is_empty() {
        0
    } else {
        crate::aa_text::label_text_width(&format!("\u{25cf} {}", card.source), source_font_px)
    };
    let source_block_height = if card.source.is_empty() {
        0
    } else {
        source_line_height + 2
    };

    LabelCardMetrics {
        width: title_width.max(source_width) + pad_x * 2,
        height: pad_y * 2 + title_line_height * card.title_lines.len() as i32 + source_block_height,
        pad_x,
        pad_y,
        title_line_height,
        source_font_px,
        source_band_height: source_block_height,
        maple_icon_size: 0,
    }
}

fn maple_card_icon_size(source_font_px: f32) -> i32 {
    let rounded = (source_font_px * MAPLE_CARD_ICON_FONT_SCALE).round() as i32;
    (rounded | 1).clamp(MAPLE_CARD_ICON_MIN_PX, MAPLE_CARD_ICON_MAX_PX)
}

fn maple_label_card_metrics(card: &LabelCardText, font_px: f32) -> LabelCardMetrics {
    let pad_x = (font_px * 0.26).ceil().max(3.0) as i32;
    let pad_y = (font_px * 0.14).ceil().max(2.0) as i32;
    let title_line_height = (font_px * 1.12).ceil() as i32;
    let source_font_px =
        (font_px * MAPLE_CARD_SOURCE_FONT_SCALE).max(MAPLE_CARD_SOURCE_FONT_MIN_PX);
    let source_line_height = (source_font_px * 1.08).ceil() as i32;
    let source_band_height = if card.source.is_empty() {
        0
    } else {
        source_line_height + 3
    };
    let maple_icon_size = maple_card_icon_size(source_font_px);
    let title_width = card
        .title_lines
        .iter()
        .map(|line| crate::aa_text::label_text_width(line, font_px))
        .max()
        .unwrap_or(1);
    let footer_width = if card.source.is_empty() {
        0
    } else {
        (maple_icon_size + MAPLE_CARD_DECOR_GAP_PX * 2) * 4
    };
    let title_block_width = card.title_width_px;

    LabelCardMetrics {
        width: title_width.max(title_block_width).max(footer_width) + pad_x * 2,
        height: 2
            + pad_y * 2
            + title_line_height * MAPLE_CARD_MAX_TITLE_LINES as i32
            + source_band_height,
        pad_x,
        pad_y,
        title_line_height,
        source_font_px,
        source_band_height,
        maple_icon_size,
    }
}

fn training_label_card_metrics(card: &LabelCardText, font_px: f32) -> LabelCardMetrics {
    let pad_x = (font_px * 0.34).ceil().max(4.0) as i32;
    let pad_y = (font_px * 0.18).ceil().max(2.0) as i32;
    let title_line_height = (font_px * 1.16).ceil() as i32;
    let source_font_px =
        (font_px * TRAINING_CARD_SOURCE_FONT_SCALE).max(TRAINING_CARD_SOURCE_FONT_MIN_PX);
    let source_line_height = (source_font_px * 1.18).ceil() as i32;
    let source_band_height = if card.source.is_empty() {
        0
    } else {
        source_line_height + 3
    };
    let title_width = card
        .title_lines
        .iter()
        .map(|line| crate::aa_text::label_text_width(line, font_px))
        .max()
        .unwrap_or(1);
    let source_width = crate::aa_text::label_text_width(&card.source, source_font_px);

    LabelCardMetrics {
        width: title_width
            .max(source_width)
            .max(card.title_width_px)
            .saturating_add(pad_x * 2),
        height: pad_y * 2 + title_line_height + source_band_height,
        pad_x,
        pad_y,
        title_line_height,
        source_font_px,
        source_band_height,
        maple_icon_size: 0,
    }
}

fn contains_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x3400..=0x4dbf
                | 0x4e00..=0x9fff
                | 0xf900..=0xfaff
                | 0x20000..=0x2fa1f
        )
    })
}

fn maple_ink_strength(text: &str) -> f32 {
    if contains_cjk(text) {
        1.0
    } else {
        MAPLE_CARD_LATIN_INK_STRENGTH
    }
}

#[cfg(test)]
fn place_label_cards(specs: Vec<LabelCardSpec>, win_w: i32, head_gap: i32) -> Vec<PlacedLabelCard> {
    place_label_cards_in_range(specs, 0, win_w, head_gap)
}

fn place_label_cards_in_range(
    mut specs: Vec<LabelCardSpec>,
    panel_left: i32,
    panel_right: i32,
    head_gap: i32,
) -> Vec<PlacedLabelCard> {
    specs.sort_by_key(|spec| (spec.anchor_x, spec.anchor_y, spec.label_index));
    let mut placed: Vec<PlacedLabelCard> = Vec::with_capacity(specs.len());

    for spec in specs {
        let min_x = panel_left.saturating_add(LABEL_CARD_EDGE_MARGIN);
        let max_x = (panel_right - spec.width - LABEL_CARD_EDGE_MARGIN).max(min_x);
        let centered_x = (spec.anchor_x - spec.width / 2).clamp(min_x, max_x);
        let y = spec.anchor_y - head_gap - spec.height;
        let half_nudge = LABEL_CARD_MAX_NUDGE_PX / 2;
        let mut candidates = [
            centered_x,
            centered_x + half_nudge,
            centered_x - half_nudge,
            centered_x + LABEL_CARD_MAX_NUDGE_PX,
            centered_x - LABEL_CARD_MAX_NUDGE_PX,
        ];
        for x in &mut candidates {
            *x = (*x).clamp(min_x, max_x);
        }
        let rect = candidates
            .into_iter()
            .map(|x| LabelCardRect {
                x,
                y,
                width: spec.width,
                height: spec.height,
            })
            .min_by_key(|rect| {
                let collisions = placed
                    .iter()
                    .filter(|other| rect.overlaps_with_gap(other.rect, LABEL_CARD_COLLISION_GAP))
                    .count();
                let overlap: i32 = placed
                    .iter()
                    .map(|other| rect.overlap_area(other.rect))
                    .sum();
                (collisions, overlap, (rect.x - centered_x).abs())
            })
            .expect("the centered card lane is always a candidate");

        placed.push(PlacedLabelCard {
            label_index: spec.label_index,
            anchor_x: spec.anchor_x,
            anchor_y: spec.anchor_y,
            rect,
        });
    }
    placed
}

struct LabelCardCanvas<'a> {
    pixels: &'a mut [u32],
    width: usize,
    height: usize,
}

impl<'a> LabelCardCanvas<'a> {
    fn new(pixels: &'a mut [u32], width: usize, height: usize) -> Self {
        Self {
            pixels,
            width,
            height,
        }
    }

    fn fill_rect(&mut self, rect: LabelCardRect, color: u32, coverage: f32) {
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                blend_xrgb(self.pixels, self.width, self.height, x, y, color, coverage);
            }
        }
    }

    fn paint_frame(&mut self, rect: LabelCardRect, background: u32, border: u32) {
        let inner = LabelCardRect {
            x: rect.x + 1,
            y: rect.y,
            width: (rect.width - 2).max(1),
            height: rect.height,
        };
        self.fill_rect(inner, background, LABEL_CARD_BACKGROUND_COVERAGE);
        self.fill_rect(
            LabelCardRect {
                x: rect.x + 2,
                y: rect.y,
                width: (rect.width - 4).max(1),
                height: 1,
            },
            border,
            1.0,
        );
        self.fill_rect(
            LabelCardRect {
                x: rect.x + 2,
                y: rect.bottom() - 1,
                width: (rect.width - 4).max(1),
                height: 1,
            },
            border,
            0.85,
        );
        for y in (rect.y + 2)..(rect.bottom() - 2) {
            blend_xrgb(
                self.pixels,
                self.width,
                self.height,
                rect.x,
                y,
                border,
                0.85,
            );
            blend_xrgb(
                self.pixels,
                self.width,
                self.height,
                rect.right() - 1,
                y,
                border,
                0.85,
            );
        }
    }

    fn paint_pixel_ring(&mut self, rect: LabelCardRect, color: u32) {
        let inner_width = (rect.width - 2).max(1);
        let inner_height = (rect.height - 2).max(1);
        let edges = [
            LabelCardRect {
                x: rect.x + 1,
                y: rect.y,
                width: inner_width,
                height: 1,
            },
            LabelCardRect {
                x: rect.x + 1,
                y: rect.bottom() - 1,
                width: inner_width,
                height: 1,
            },
            LabelCardRect {
                x: rect.x,
                y: rect.y + 1,
                width: 1,
                height: inner_height,
            },
            LabelCardRect {
                x: rect.right() - 1,
                y: rect.y + 1,
                width: 1,
                height: inner_height,
            },
        ];
        for edge in edges {
            self.fill_rect(edge, color, 1.0);
        }
    }

    fn paint_pixel_disc(&mut self, rect: LabelCardRect, color: u32) {
        self.paint_pixel_ring(rect, color);
        self.fill_rect(
            LabelCardRect {
                x: rect.x + 1,
                y: rect.y + 1,
                width: (rect.width - 2).max(1),
                height: (rect.height - 2).max(1),
            },
            color,
            1.0,
        );
    }

    fn paint_maple_shop_body(&mut self, rect: LabelCardRect, source_top: i32) {
        let body_height = (source_top - rect.y - 1).max(0);
        if body_height == 0 {
            return;
        }
        self.fill_rect(
            LabelCardRect {
                x: rect.x + 1,
                y: rect.y + 1,
                width: (rect.width - 2).max(1),
                height: body_height,
            },
            pack_xrgb(MAPLE_CARD_BODY),
            MAPLE_CARD_BODY_COVERAGE,
        );
        self.fill_rect(
            LabelCardRect {
                x: rect.x + 2,
                y: rect.y + 1,
                width: (rect.width - 4).max(1),
                height: 1,
            },
            pack_xrgb(MAPLE_CARD_INNER_HIGHLIGHT),
            MAPLE_CARD_INNER_HIGHLIGHT_COVERAGE,
        );
    }

    fn centred_icon_rect(cell: LabelCardRect, icon_size: i32) -> LabelCardRect {
        LabelCardRect {
            x: cell.x + (cell.width - icon_size) / 2,
            y: cell.y + (cell.height - icon_size) / 2,
            width: icon_size,
            height: icon_size,
        }
    }

    fn paint_maple_shop_coin(&mut self, cell: LabelCardRect, icon_size: i32) {
        let coin_rect = LabelCardRect {
            ..Self::centred_icon_rect(cell, icon_size)
        };
        self.paint_pixel_disc(coin_rect, pack_xrgb(MAPLE_CARD_COIN));
        self.fill_rect(
            LabelCardRect {
                x: coin_rect.x + 2,
                y: coin_rect.y + 1,
                width: (icon_size / 3).max(1),
                height: (icon_size / 3).max(1),
            },
            pack_xrgb(MAPLE_CARD_COIN_HIGHLIGHT),
            1.0,
        );
    }

    fn paint_maple_shop_permit(&mut self, cell: LabelCardRect, icon_size: i32) {
        let glyph = Self::centred_icon_rect(cell, icon_size);
        let body = LabelCardRect {
            x: glyph.x + 1,
            y: glyph.y + 3,
            width: (glyph.width - 2).max(1),
            height: (glyph.height - 4).max(1),
        };
        self.paint_pixel_disc(body, pack_xrgb(MAPLE_CARD_PERMIT));
        self.fill_rect(
            LabelCardRect {
                x: glyph.x + 2,
                y: glyph.y + 1,
                width: (glyph.width - 4).max(1),
                height: 1,
            },
            pack_xrgb(MAPLE_CARD_PERMIT),
            1.0,
        );
        self.fill_rect(
            LabelCardRect {
                x: body.x + 2,
                y: body.y + 1,
                width: (body.width / 3).max(1),
                height: 1,
            },
            pack_xrgb(MAPLE_CARD_PERMIT_HIGHLIGHT),
            1.0,
        );
    }

    fn paint_maple_shop_ring(&mut self, cell: LabelCardRect, icon_size: i32, color: u32) {
        self.paint_pixel_ring(Self::centred_icon_rect(cell, icon_size), color);
    }

    fn paint_maple_shop_folded_corner(&mut self, rect: LabelCardRect, source_top: i32) {
        let color = pack_xrgb(MAPLE_CARD_FOLDED_CORNER);
        for row in 0..3 {
            self.fill_rect(
                LabelCardRect {
                    x: rect.right() - 2 - row,
                    y: source_top - 3 + row,
                    width: row + 1,
                    height: 1,
                },
                color,
                1.0,
            );
        }
    }

    fn paint_maple_shop_footer(
        &mut self,
        rect: LabelCardRect,
        source_top: i32,
        spec: MapleShopFrameSpec,
    ) {
        self.fill_rect(
            LabelCardRect {
                x: rect.x + 1,
                y: source_top,
                width: (rect.width - 2).max(1),
                height: spec.source_band_height,
            },
            pack_xrgb(MAPLE_CARD_INFO),
            MAPLE_CARD_INFO_COVERAGE,
        );
        self.fill_rect(
            LabelCardRect {
                x: rect.x + 1,
                y: source_top,
                width: (rect.width - 2).max(1),
                height: 1,
            },
            pack_xrgb(MAPLE_CARD_DIVIDER),
            1.0,
        );
        let cells = Self::maple_shop_footer_cells(rect, source_top, spec.source_band_height);
        self.paint_maple_shop_footer_dividers(&cells);
        self.paint_maple_shop_footer_icons(cells, spec);
    }

    fn maple_shop_footer_cells(
        rect: LabelCardRect,
        source_top: i32,
        source_band_height: i32,
    ) -> [LabelCardRect; 4] {
        let inner_x = rect.x + 1;
        let inner_width = (rect.width - 2).max(1);
        std::array::from_fn(|index| {
            let start = inner_x + inner_width * index as i32 / 4;
            let end = inner_x + inner_width * (index as i32 + 1) / 4;
            LabelCardRect {
                x: start,
                y: source_top + 1,
                width: (end - start).max(1),
                height: (source_band_height - 1).max(1),
            }
        })
    }

    fn paint_maple_shop_footer_dividers(&mut self, cells: &[LabelCardRect; 4]) {
        for cell in cells.iter().skip(1) {
            self.fill_rect(
                LabelCardRect {
                    x: cell.x,
                    y: cell.y,
                    width: 1,
                    height: cell.height,
                },
                pack_xrgb(MAPLE_CARD_DIVIDER),
                0.55,
            );
        }
    }

    fn paint_maple_shop_footer_icons(
        &mut self,
        cells: [LabelCardRect; 4],
        spec: MapleShopFrameSpec,
    ) {
        self.paint_maple_shop_coin(cells[0], spec.icon_size);
        self.paint_maple_shop_permit(cells[1], spec.icon_size);
        self.paint_maple_shop_ring(cells[2], spec.icon_size, pack_xrgb(MAPLE_CARD_ACTION_BLUE));
        self.paint_maple_shop_ring(
            cells[3],
            spec.icon_size,
            pack_xrgb(MAPLE_CARD_INACTIVE_RING),
        );
        let status = Self::centred_icon_rect(cells[3], spec.icon_size);
        self.fill_rect(
            LabelCardRect {
                x: status.x + status.width / 2,
                y: status.y + status.height / 2,
                width: 1,
                height: 1,
            },
            spec.status_color,
            1.0,
        );
    }

    fn paint_maple_shop_outline(&mut self, rect: LabelCardRect) {
        let border = pack_xrgb(MAPLE_CARD_BORDER);
        let edges = [
            LabelCardRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: 1,
            },
            LabelCardRect {
                x: rect.x,
                y: rect.bottom() - 1,
                width: rect.width,
                height: 1,
            },
            LabelCardRect {
                x: rect.x,
                y: rect.y + 1,
                width: 1,
                height: (rect.height - 2).max(0),
            },
            LabelCardRect {
                x: rect.right() - 1,
                y: rect.y + 1,
                width: 1,
                height: (rect.height - 2).max(0),
            },
        ];
        for edge in edges {
            self.fill_rect(edge, border, 1.0);
        }
    }

    fn paint_maple_shop_frame(&mut self, rect: LabelCardRect, spec: MapleShopFrameSpec) {
        let spec = MapleShopFrameSpec {
            source_band_height: spec.source_band_height.clamp(0, (rect.height - 2).max(0)),
            ..spec
        };
        let source_top = rect.bottom() - 1 - spec.source_band_height;
        self.paint_maple_shop_body(rect, source_top);
        if spec.source_band_height > 0 {
            self.paint_maple_shop_footer(rect, source_top, spec);
            self.paint_maple_shop_folded_corner(rect, source_top);
        }
        self.paint_maple_shop_outline(rect);
    }

    fn paint_training_card_frame(
        &mut self,
        rect: LabelCardRect,
        source_band_height: i32,
        status_color: u32,
    ) {
        let source_band_height = source_band_height.clamp(0, (rect.height - 2).max(0));
        let source_top = rect.bottom() - 1 - source_band_height;
        self.fill_rect(
            LabelCardRect {
                x: rect.x + 1,
                y: rect.y + 1,
                width: (rect.width - 2).max(1),
                height: (source_top - rect.y - 1).max(1),
            },
            pack_xrgb(TRAINING_CARD_BODY),
            TRAINING_CARD_BODY_COVERAGE,
        );
        if source_band_height > 0 {
            self.fill_rect(
                LabelCardRect {
                    x: rect.x + 1,
                    y: source_top,
                    width: (rect.width - 2).max(1),
                    height: source_band_height,
                },
                pack_xrgb(TRAINING_CARD_SOURCE_BODY),
                TRAINING_CARD_SOURCE_COVERAGE,
            );
            self.fill_rect(
                LabelCardRect {
                    x: rect.x + 1,
                    y: source_top,
                    width: (rect.width - 2).max(1),
                    height: 1,
                },
                status_color,
                0.72,
            );
        }
        self.paint_pixel_ring(rect, pack_xrgb(TRAINING_CARD_BORDER));
        self.fill_rect(
            LabelCardRect {
                x: rect.x + 1,
                y: rect.y + 1,
                width: 2.min((rect.width - 2).max(1)),
                height: (rect.height - 2).max(1),
            },
            status_color,
            0.92,
        );
    }

    fn draw_ink(&mut self, text: &str, origin: (i32, i32), font_px: f32, color: u32) {
        self.draw_ink_with_strength(text, origin, font_px, color, 1.0);
    }

    fn draw_ink_with_strength(
        &mut self,
        text: &str,
        origin: (i32, i32),
        font_px: f32,
        color: u32,
        strength: f32,
    ) {
        crate::aa_text::draw_label_text_at(text, origin.0, origin.1, font_px, |x, y, coverage| {
            blend_xrgb(
                self.pixels,
                self.width,
                self.height,
                x,
                y,
                color,
                coverage * strength,
            )
        });
    }

    fn draw_leader(&mut self, start: (i32, i32), end: (i32, i32), color: u32) {
        let (mut x, mut y) = start;
        let dx = (end.0 - x).abs();
        let step_x = if x < end.0 { 1 } else { -1 };
        let dy = -(end.1 - y).abs();
        let step_y = if y < end.1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            blend_xrgb(self.pixels, self.width, self.height, x, y, color, 0.72);
            if (x, y) == end {
                break;
            }
            let twice_error = error * 2;
            if twice_error >= dy {
                error += dy;
                x += step_x;
            }
            if twice_error <= dx {
                error += dx;
                y += step_y;
            }
        }
    }
}

/// Paint name cards into the upscaled `u32` surface (`0x00RRGGBB`). Each label's `anchor_px`
/// is scene-buffer space → multiply by `scale` for screen space; its card stays on the
/// head-height baseline and may move horizontally only within the small bounded head zone.
/// Crisp single-pass anti-aliased text plus the optional missing-glyph fallback keeps it a
/// sharp caption over the chunky sprites. Shared by the live window (`window::redraw`) and
/// the `floating_snapshot` verify example, so both blit identically.
pub fn paint_labels_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    labels: &[pixtuoid_scene::overlay::LabelElement],
    scale: i32,
    theme: &Theme,
) {
    paint_labels_into_surface_with_font_px(sb, win_w, win_h, labels, scale, LABEL_FONT_PX, theme);
}

/// Map-explicit sibling used by the two-map Maple renderer.  The legacy
/// wrapper above intentionally keeps its Free Market shop-card behavior.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn paint_map_labels_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    labels: &[pixtuoid_scene::overlay::LabelElement],
    scale: i32,
    theme: &Theme,
    map: pixtuoid_scene::maple_world::MapleMapId,
) {
    paint_map_labels_into_surface_with_font_px(
        sb,
        win_w,
        win_h,
        labels,
        scale,
        LABEL_FONT_PX,
        theme,
        map,
    );
}

/// DPI/user-scaled variant used by the live floating window. The snapshot and
/// existing callers keep the 12px wrapper above for stable visual fixtures.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_labels_into_surface_with_font_px(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    labels: &[pixtuoid_scene::overlay::LabelElement],
    scale: i32,
    font_px: f32,
    theme: &Theme,
) {
    let style = if theme.name == "maple" {
        LabelCardStyle::MapleShop
    } else {
        LabelCardStyle::Generic
    };
    paint_labels_with_style(
        sb, win_w, win_h, labels, scale, font_px, theme, style, 0, win_w,
    );
}

#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn paint_map_labels_into_surface_with_font_px(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    labels: &[pixtuoid_scene::overlay::LabelElement],
    scale: i32,
    font_px: f32,
    theme: &Theme,
    map: pixtuoid_scene::maple_world::MapleMapId,
) {
    let style = if theme.name != "maple" {
        LabelCardStyle::Generic
    } else {
        match map {
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket => LabelCardStyle::MapleShop,
            pixtuoid_scene::maple_world::MapleMapId::ForestTraining => LabelCardStyle::Training,
        }
    };
    paint_labels_with_style(
        sb, win_w, win_h, labels, scale, font_px, theme, style, 0, win_w,
    );
}

/// Panel-clamped sibling for the simultaneous Maple view. Anchors remain in
/// composite-buffer coordinates; only card collision/clamping is panel-local.
#[allow(clippy::too_many_arguments)]
#[doc(hidden)]
pub fn paint_map_labels_into_surface_with_font_px_in_panel(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    labels: &[pixtuoid_scene::overlay::LabelElement],
    scale: i32,
    font_px: f32,
    theme: &Theme,
    map: pixtuoid_scene::maple_world::MapleMapId,
    panel_left: usize,
    panel_width: usize,
) {
    let style = if theme.name != "maple" {
        LabelCardStyle::Generic
    } else {
        match map {
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket => LabelCardStyle::MapleShop,
            pixtuoid_scene::maple_world::MapleMapId::ForestTraining => LabelCardStyle::Training,
        }
    };
    let panel_right = panel_left.saturating_add(panel_width).min(win_w);
    paint_labels_with_style(
        sb,
        win_w,
        win_h,
        labels,
        scale,
        font_px,
        theme,
        style,
        panel_left,
        panel_right,
    );
}

#[allow(clippy::too_many_arguments)]
fn paint_labels_with_style(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    labels: &[pixtuoid_scene::overlay::LabelElement],
    scale: i32,
    font_px: f32,
    theme: &Theme,
    style: LabelCardStyle,
    panel_left: usize,
    panel_right: usize,
) {
    let font_px = if font_px.is_finite() && font_px > 0.0 {
        font_px
    } else {
        LABEL_FONT_PX
    };
    let prepared: Vec<_> = labels
        .iter()
        .map(|el| {
            let text = match style {
                LabelCardStyle::Generic => build_label_card_text(&el.text, font_px),
                LabelCardStyle::MapleShop => build_maple_label_card_text_with_relation(
                    &el.text,
                    font_px,
                    el.relation.as_ref(),
                ),
                LabelCardStyle::Training => build_training_label_card_text_with_relation(
                    &el.text,
                    font_px,
                    el.tone,
                    el.relation.as_ref(),
                ),
            };
            let metrics = match style {
                LabelCardStyle::Generic => label_card_metrics(&text, font_px),
                LabelCardStyle::MapleShop => maple_label_card_metrics(&text, font_px),
                LabelCardStyle::Training => training_label_card_metrics(&text, font_px),
            };
            (text, metrics)
        })
        .collect();
    let specs = labels
        .iter()
        .zip(&prepared)
        .enumerate()
        .map(|(label_index, (el, (_, metrics)))| LabelCardSpec {
            label_index,
            anchor_x: el.anchor_px.x as i32 * scale + (FLOATING_SPRITE_W * scale) / 2,
            anchor_y: el.anchor_px.y as i32 * scale,
            width: metrics.width,
            height: metrics.height,
        })
        .collect();
    let head_gap = match style {
        LabelCardStyle::Generic => LABEL_CARD_HEAD_GAP,
        LabelCardStyle::MapleShop => MAPLE_CARD_HEAD_GAP,
        LabelCardStyle::Training => TRAINING_CARD_HEAD_GAP,
    };
    let placed = place_label_cards_in_range(
        specs,
        panel_left.min(i32::MAX as usize) as i32,
        panel_right.min(i32::MAX as usize) as i32,
        head_gap,
    );
    let mut canvas = LabelCardCanvas::new(sb, win_w, win_h);

    if style == LabelCardStyle::Generic {
        // Generic scene cards retain their short ownership leader. Both Maple
        // card styles attach directly above the character, without a tail.
        for card in &placed {
            let el = &labels[card.label_index];
            let tone = pack_xrgb(pixtuoid_scene::overlay::label_tone_rgb(el.tone, theme));
            let leader_min_x = card.rect.x + 2;
            let leader_max_x = (card.rect.right() - 3).max(leader_min_x);
            let card_x = card.anchor_x.clamp(leader_min_x, leader_max_x);
            canvas.draw_leader(
                (card_x, card.rect.bottom()),
                (card.anchor_x, card.anchor_y - 1),
                tone,
            );
        }
    }

    for card in placed {
        let el = &labels[card.label_index];
        let (content, metrics) = &prepared[card.label_index];
        let tone = if el.hovered {
            HOVER_INK
        } else {
            pixtuoid_scene::overlay::label_tone_rgb(el.tone, theme)
        };
        let source_hue = (!el.hovered)
            .then(|| pixtuoid_scene::overlay::badge_hue(&el.text, theme))
            .flatten()
            .unwrap_or(tone);
        match style {
            LabelCardStyle::MapleShop => canvas.paint_maple_shop_frame(
                card.rect,
                MapleShopFrameSpec {
                    source_band_height: metrics.source_band_height,
                    icon_size: metrics.maple_icon_size,
                    status_color: pack_xrgb(tone),
                },
            ),
            LabelCardStyle::Training => canvas.paint_training_card_frame(
                card.rect,
                metrics.source_band_height,
                pack_xrgb(tone),
            ),
            LabelCardStyle::Generic => canvas.paint_frame(
                card.rect,
                pack_xrgb(theme.ui.tooltip_bg),
                pack_xrgb(source_hue),
            ),
        }
        if style != LabelCardStyle::Generic {
            if let Some(relation) = el.relation.as_ref() {
                canvas.fill_rect(
                    LabelCardRect {
                        x: card.rect.x + 1,
                        y: card.rect.y + 1,
                        width: 2,
                        height: (card.rect.height - 2).max(1),
                    },
                    pack_xrgb(lineage_family_color(relation)),
                    0.94,
                );
            }
        }

        let mut y = card.rect.y + metrics.pad_y + i32::from(style == LabelCardStyle::MapleShop);
        for line in &content.title_lines {
            let width = crate::aa_text::label_text_width(line, font_px);
            let x = if style == LabelCardStyle::MapleShop {
                card.rect.x + metrics.pad_x
            } else {
                card.rect.x + (card.rect.width - width) / 2
            };
            match style {
                LabelCardStyle::MapleShop => canvas.draw_ink_with_strength(
                    line,
                    (x, y),
                    font_px,
                    pack_xrgb(MAPLE_CARD_TITLE_INK),
                    maple_ink_strength(line),
                ),
                LabelCardStyle::Training => canvas.draw_ink_with_strength(
                    line,
                    (x, y),
                    font_px,
                    pack_xrgb(TRAINING_CARD_TITLE_INK),
                    maple_ink_strength(line),
                ),
                LabelCardStyle::Generic => {
                    canvas.draw_ink(line, (x, y), font_px, pack_xrgb(HOVER_INK))
                }
            }
            y += metrics.title_line_height;
        }

        if !content.source.is_empty() && style != LabelCardStyle::MapleShop {
            y += 1;
            let marker = if el.hovered { "\u{25b8}" } else { "\u{25cf}" };
            let marker_width = crate::aa_text::label_text_width(marker, metrics.source_font_px);
            let gap = (metrics.source_font_px * 0.28).ceil() as i32;
            let source_width =
                crate::aa_text::label_text_width(&content.source, metrics.source_font_px);
            let x = card.rect.x + (card.rect.width - marker_width - gap - source_width) / 2;
            canvas.draw_ink(marker, (x, y), metrics.source_font_px, pack_xrgb(tone));
            canvas.draw_ink(
                &content.source,
                (x + marker_width + gap, y),
                metrics.source_font_px,
                pack_xrgb(if style == LabelCardStyle::Training {
                    TRAINING_CARD_SOURCE_INK
                } else {
                    source_hue
                }),
            );
        }
    }
}

/// Paint Maple-style fake player names directly into the authored stall plaques.
pub fn paint_market_player_ids_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    player_ids: &[pixtuoid_scene::overlay::LabelElement],
    scale: i32,
    theme: &Theme,
) {
    paint_market_player_ids_into_surface_with_font_px(
        sb,
        win_w,
        win_h,
        player_ids,
        scale,
        LABEL_FONT_PX,
        theme,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_market_player_ids_into_surface_with_font_px(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    player_ids: &[pixtuoid_scene::overlay::LabelElement],
    scale: i32,
    font_px: f32,
    theme: &Theme,
) {
    paint_market_player_ids_into_surface_with_font_px_in_panel(
        sb, win_w, win_h, player_ids, scale, font_px, theme, 0, win_w,
    );
}

#[allow(clippy::too_many_arguments)]
#[doc(hidden)]
pub fn paint_market_player_ids_into_surface_with_font_px_in_panel(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    player_ids: &[pixtuoid_scene::overlay::LabelElement],
    scale: i32,
    font_px: f32,
    theme: &Theme,
    panel_left: usize,
    panel_width: usize,
) {
    if theme.name != "maple" || player_ids.is_empty() {
        return;
    }
    let font_px = if font_px.is_finite() && font_px > 0.0 {
        (font_px * MAPLE_CARD_SOURCE_FONT_SCALE)
            .clamp(MAPLE_CARD_SOURCE_FONT_MIN_PX, MAPLE_PLAYER_ID_FONT_MAX_PX)
    } else {
        MAPLE_CARD_SOURCE_FONT_MIN_PX
    };
    let panel_left = panel_left.min(win_w);
    let panel_right = panel_left.saturating_add(panel_width).min(win_w);
    let mut canvas = LabelCardCanvas::new(sb, win_w, win_h);

    for id in player_ids {
        let text_width = crate::aa_text::label_text_width(&id.text, font_px);
        let centre_x = i32::from(id.anchor_px.x) * scale + (FLOATING_SPRITE_W * scale) / 2;
        let text_y = i32::from(id.anchor_px.y) * scale;
        let plate_width = text_width + MAPLE_PLAYER_ID_PLATE_PAD_X * 2;
        let plate_height = font_px.ceil() as i32
            + MAPLE_PLAYER_ID_PLATE_PAD_TOP
            + MAPLE_PLAYER_ID_PLATE_PAD_BOTTOM;
        let min_plate_x =
            (panel_left.min(i32::MAX as usize) as i32).saturating_add(LABEL_CARD_EDGE_MARGIN);
        let max_plate_x =
            ((panel_right.min(i32::MAX as usize) as i32) - plate_width - LABEL_CARD_EDGE_MARGIN)
                .max(min_plate_x);
        let plate_x = (centre_x - plate_width / 2).clamp(min_plate_x, max_plate_x);
        let plate = LabelCardRect {
            x: plate_x,
            y: text_y - MAPLE_PLAYER_ID_PLATE_PAD_TOP,
            width: plate_width,
            height: plate_height,
        };
        // The original stall sprite still supplies the larger wooden plaque.
        // This compact, cut-corner inset mirrors the game's dark name strip
        // while leaving that authored outer frame visible around it.
        canvas.fill_rect(
            LabelCardRect {
                x: plate.x + 1,
                y: plate.y,
                width: (plate.width - 2).max(1),
                height: plate.height,
            },
            pack_xrgb(MAPLE_PLAYER_ID_PLATE_BODY),
            MAPLE_PLAYER_ID_PLATE_BODY_COVERAGE,
        );
        canvas.fill_rect(
            LabelCardRect {
                x: plate.x,
                y: plate.y + 1,
                width: plate.width,
                height: (plate.height - 2).max(1),
            },
            pack_xrgb(MAPLE_PLAYER_ID_PLATE_BODY),
            MAPLE_PLAYER_ID_PLATE_BODY_COVERAGE,
        );
        canvas.paint_pixel_ring(plate, pack_xrgb(MAPLE_PLAYER_ID_PLATE_BORDER));
        let text_x = plate.x + MAPLE_PLAYER_ID_PLATE_PAD_X;
        canvas.draw_ink(
            &id.text,
            (text_x + 1, text_y + 1),
            font_px,
            pack_xrgb(MAPLE_PLAYER_ID_SHADOW),
        );
        canvas.draw_ink(
            &id.text,
            (text_x, text_y),
            font_px,
            pack_xrgb(MAPLE_PLAYER_ID_INK),
        );
    }
}

/// Paint the neon wall-board text over the already-painted panel, into the upscaled
/// surface. The panel interior is `NEON_PANEL_INNER_*` in scene-buffer px, so the
/// board text ANCHORS to it and SCALES with the scene `scale` (unlike the fixed-height
/// name badges) — the three rows always fit inside the glowing frame. At a very small
/// scene scale the rows would be sub-legible; there we leave the panel empty rather
/// than paint mush. Shared by
/// the live window and the `floating_snapshot` verify example.
pub fn paint_wall_board_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    board: &pixtuoid_scene::board::BoardModel,
    scale: i32,
    theme: &Theme,
) {
    // The local Free Market plate has no scene neon panel. Leaving this text
    // pass enabled would float scene telemetry over the sky at top-left.
    if theme.name == "maple" {
        return;
    }
    use pixtuoid_scene::pixel_painter::{
        NEON_PANEL_INNER_H, NEON_PANEL_INNER_W, NEON_PANEL_INNER_X, NEON_PANEL_INNER_Y,
    };
    if scale <= 0 {
        return;
    }
    let inner_x = NEON_PANEL_INNER_X as i32 * scale;
    let inner_y = NEON_PANEL_INNER_Y as i32 * scale;
    let inner_w = NEON_PANEL_INNER_W as i32 * scale;
    let row_h = NEON_PANEL_INNER_H as i32 * scale / 3;
    // Below this a row can't hold a legible glyph — leave the empty glowing panel.
    const MIN_ROW_PX: i32 = 4;
    if row_h < MIN_ROW_PX {
        return;
    }
    // Fill ~85% of the row so descenders don't collide with the next row.
    let font_px = row_h as f32 * 0.85;
    // Tone→role map is single-sourced in `scene::board`; the painter only packs
    // the resolved `Rgb` into the surface's XRGB.
    let glow = |tone| pack_xrgb(pixtuoid_scene::board::tone_rgb(tone, theme));

    // L1: brand left, ★ Star right-flushed to the interior's right edge.
    draw_badge_text(
        sb,
        win_w,
        win_h,
        &board.brand.text,
        inner_x,
        inner_y,
        font_px,
        glow(board.brand.tone),
    );
    let star_w = crate::aa_text::text_width(&board.star.text, font_px);
    let star_x = inner_x + (inner_w - star_w).max(0);
    draw_badge_text(
        sb,
        win_w,
        win_h,
        &board.star.text,
        star_x,
        inner_y,
        font_px,
        glow(board.star.tone),
    );

    // L2 (mood) + L3 (context): tone-mapped segments laid left-to-right on their row.
    for (row, segs) in [(1, &board.mood), (2, &board.context)] {
        let mut x = inner_x;
        let y = inner_y + row * row_h;
        for seg in segs {
            draw_badge_text(sb, win_w, win_h, &seg.text, x, y, font_px, glow(seg.tone));
            x += crate::aa_text::text_width(&seg.text, font_px);
        }
    }
}

/// Column budget for the floating footer at `win_w` px — how many monospace
/// Monaspace advances fit between the margins. [`build_footer`] right-flushes to
/// it, so the footer spans margin-to-margin; Monaspace is fixed-advance, so a
/// column budget maps cleanly to pixels (the board's `chars().count()` discipline).
pub fn footer_budget(win_w: usize) -> u16 {
    let advance = crate::aa_text::text_width("M", LABEL_FONT_PX).max(1);
    (((win_w as i32 - 2 * FOOTER_MARGIN_PX).max(0)) / advance) as u16
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MapleFooterRun {
    text: String,
    tone: FooterTone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MapleFooterSummary {
    total: usize,
    runs: Vec<MapleFooterRun>,
    audio_label: Option<String>,
}

fn first_number(text: &str) -> Option<usize> {
    let digits = text
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn maple_tool_label(kind: pixtuoid_core::state::ToolKind, raw: &str) -> &'static str {
    use pixtuoid_core::state::ToolKind;

    match kind {
        ToolKind::Task => "呼叫子代理",
        ToolKind::Edit => "編輯檔案",
        ToolKind::Read => "讀取檔案",
        ToolKind::Bash => "執行指令",
        ToolKind::Search => "搜尋資料",
        ToolKind::Other => {
            let token = raw
                .split('\u{d7}')
                .next()
                .unwrap_or(raw)
                .trim()
                .to_ascii_lowercase();
            if token.contains("exec") || token.contains("command") || token.contains("shell") {
                "執行指令"
            } else if token.contains("read") {
                "讀取檔案"
            } else if token.contains("write") || token.contains("edit") {
                "編輯檔案"
            } else if token.contains("grep") || token.contains("search") {
                "搜尋資料"
            } else {
                "處理任務"
            }
        }
    }
}

fn maple_footer_summary(model: &FooterModel) -> MapleFooterSummary {
    let total = model
        .segments
        .iter()
        .filter(|seg| seg.tone == FooterTone::Neutral)
        .find_map(|seg| {
            let trimmed = seg.text.trim_start();
            trimmed
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit())
                .then(|| first_number(trimmed))
                .flatten()
        })
        .unwrap_or(0);

    let whole_line = model.text();
    let audio_label = whole_line.find('\u{2669}').map(|note| {
        let suffix = &whole_line[note + '\u{2669}'.len_utf8()..];
        suffix
            .split_whitespace()
            .next()
            .filter(|value| value.ends_with('%'))
            .map_or_else(|| "音效".to_string(), |value| format!("音量 {value}"))
    });

    let warning = model
        .segments
        .iter()
        .find(|seg| seg.tone == FooterTone::Warning)
        .map(|seg| MapleFooterRun {
            text: seg
                .text
                .trim()
                .replace("need you", "等待回覆")
                .replace('\u{26a0}', "系統："),
            tone: FooterTone::Warning,
        });
    if let Some(warning) = warning {
        return MapleFooterSummary {
            total,
            runs: vec![warning],
            audio_label,
        };
    }

    let mut runs = Vec::new();
    // Waiting leads: it is the one state that requires the player's attention.
    for kind in [
        RungKind::Waiting,
        RungKind::Active,
        RungKind::Idle,
        RungKind::Exiting,
    ] {
        let Some(segment) = model
            .segments
            .iter()
            .find(|seg| seg.tone == FooterTone::Rung(kind))
        else {
            continue;
        };
        let Some(count) = first_number(&segment.text) else {
            continue;
        };
        let label = match kind {
            RungKind::Active => "執行中",
            RungKind::Waiting => "等待回覆",
            RungKind::Idle => "閒置",
            RungKind::Exiting => "離場中",
        };
        runs.push(MapleFooterRun {
            text: format!("{label} {count}"),
            tone: segment.tone,
        });
    }
    for segment in &model.segments {
        match segment.tone {
            FooterTone::Tool(kind) => {
                let count = segment
                    .text
                    .split_once('\u{d7}')
                    .and_then(|(_, count)| first_number(count))
                    .unwrap_or(1);
                runs.push(MapleFooterRun {
                    text: format!("{} {count}", maple_tool_label(kind, &segment.text)),
                    tone: segment.tone,
                });
            }
            FooterTone::Gateway(state) => {
                use pixtuoid_core::state::DaemonState;
                let label = match state {
                    DaemonState::Idle => "閘道待命",
                    DaemonState::Busy => "閘道運作中",
                    DaemonState::Degraded => "閘道異常",
                    DaemonState::Down => "閘道離線",
                };
                runs.push(MapleFooterRun {
                    text: label.to_string(),
                    tone: segment.tone,
                });
            }
            _ => {}
        }
    }
    if runs.is_empty() {
        runs.push(MapleFooterRun {
            text: if total == 0 {
                "目前沒有進行中的代理".to_string()
            } else {
                "代理監控中".to_string()
            },
            tone: FooterTone::Neutral,
        });
    }

    MapleFooterSummary {
        total,
        runs,
        audio_label,
    }
}

fn maple_chat_font_px(win_h: usize) -> f32 {
    (LABEL_FONT_PX * (win_h as f32 / 240.0).clamp(1.0, 1.5))
        .clamp(MAPLE_CHAT_FONT_MIN_PX, MAPLE_CHAT_FONT_MAX_PX)
}

fn truncate_label_to_px(text: &str, max_width: i32, font_px: f32) -> String {
    if crate::aa_text::label_text_width(text, font_px) <= max_width {
        return text.to_string();
    }
    let mut clipped = String::new();
    for ch in text.chars() {
        let mut candidate = clipped.clone();
        candidate.push(ch);
        candidate.push('\u{2026}');
        if crate::aa_text::label_text_width(&candidate, font_px) > max_width {
            break;
        }
        clipped.push(ch);
    }
    if !clipped.is_empty() {
        clipped.push('\u{2026}');
    }
    clipped
}

fn paint_maple_chat_footer(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    model: &FooterModel,
    theme: &Theme,
) {
    if win_w == 0 || win_h == 0 {
        return;
    }
    let font_px = maple_chat_font_px(win_h);
    let line_height = crate::aa_text::line_height(font_px).max(12);
    let height = (line_height + 10).min(win_h as i32);
    let width = (win_w as i32 - MAPLE_CHAT_MARGIN_X * 2).max(1);
    let rect = LabelCardRect {
        x: MAPLE_CHAT_MARGIN_X.min(win_w as i32 - 1),
        y: (win_h as i32 - MAPLE_CHAT_MARGIN_BOTTOM - height).max(0),
        width,
        height,
    };
    let summary = maple_footer_summary(model);
    let channel_text = format!("代理 {}", summary.total);
    let channel_width = (crate::aa_text::label_text_width(&channel_text, font_px) + 10)
        .clamp(36, (rect.width / 3).max(36));
    let button_size = (rect.height - 8).clamp(10, 20);
    let button_symbols: &[&str] = if rect.width >= 180 {
        &["M", "-", "+"]
    } else {
        &["M"]
    };
    let controls_width = button_size * button_symbols.len() as i32
        + MAPLE_CHAT_BUTTON_GAP * (button_symbols.len().saturating_sub(1)) as i32;
    let controls_x = rect.right() - 4 - controls_width;

    {
        let mut canvas = LabelCardCanvas::new(sb, win_w, win_h);
        canvas.fill_rect(rect, pack_xrgb(MAPLE_CHAT_BORDER), 1.0);
        canvas.fill_rect(
            LabelCardRect {
                x: rect.x + 1,
                y: rect.y + 1,
                width: (rect.width - 2).max(1),
                height: (rect.height - 2).max(1),
            },
            pack_xrgb(MAPLE_CHAT_BODY),
            MAPLE_CHAT_BODY_COVERAGE,
        );
        canvas.fill_rect(
            LabelCardRect {
                x: rect.x + 2,
                y: rect.y + 2,
                width: (rect.width - 4).max(1),
                height: 1,
            },
            pack_xrgb(MAPLE_CHAT_HIGHLIGHT),
            0.95,
        );
        canvas.fill_rect(
            LabelCardRect {
                x: rect.x + 1,
                y: rect.bottom() - 3,
                width: (rect.width - 2).max(1),
                height: 2,
            },
            pack_xrgb(MAPLE_CHAT_SHADOW),
            0.72,
        );
        let channel_rect = LabelCardRect {
            x: rect.x + 4,
            y: rect.y + 4,
            width: channel_width,
            height: (rect.height - 8).max(1),
        };
        canvas.fill_rect(channel_rect, pack_xrgb(MAPLE_CHAT_CHANNEL), 1.0);
        canvas.paint_pixel_ring(channel_rect, pack_xrgb(MAPLE_CHAT_BORDER));

        let button_y = rect.y + (rect.height - button_size) / 2;
        for (index, _) in button_symbols.iter().enumerate() {
            let button = LabelCardRect {
                x: controls_x + index as i32 * (button_size + MAPLE_CHAT_BUTTON_GAP),
                y: button_y,
                width: button_size,
                height: button_size,
            };
            canvas.fill_rect(button, pack_xrgb(MAPLE_CHAT_BUTTON_DARK), 1.0);
            canvas.fill_rect(
                LabelCardRect {
                    x: button.x + 1,
                    y: button.y + 1,
                    width: (button.width - 2).max(1),
                    height: (button.height - 2).max(1),
                },
                pack_xrgb(MAPLE_CHAT_BUTTON),
                1.0,
            );
            canvas.fill_rect(
                LabelCardRect {
                    x: button.x + 2,
                    y: button.y + 2,
                    width: (button.width - 4).max(1),
                    height: 1,
                },
                pack_xrgb(MAPLE_CHAT_HIGHLIGHT),
                0.72,
            );
        }
    }

    let text_y = rect.y + (rect.height - line_height) / 2 - 1;
    let mut canvas = LabelCardCanvas::new(sb, win_w, win_h);
    let channel_text_width = crate::aa_text::label_text_width(&channel_text, font_px);
    canvas.draw_ink(
        &channel_text,
        (
            rect.x + 4 + (channel_width - channel_text_width) / 2,
            text_y,
        ),
        font_px,
        pack_xrgb(MAPLE_CHAT_CHANNEL_INK),
    );

    let button_font_px = (font_px * 0.82).max(10.0);
    let button_y = rect.y + (rect.height - button_size) / 2;
    for (index, symbol) in button_symbols.iter().enumerate() {
        let button_x = controls_x + index as i32 * (button_size + MAPLE_CHAT_BUTTON_GAP);
        let symbol_width = crate::aa_text::label_text_width(symbol, button_font_px);
        canvas.draw_ink(
            symbol,
            (
                button_x + (button_size - symbol_width) / 2,
                button_y + (button_size - crate::aa_text::line_height(button_font_px)) / 2 - 1,
            ),
            button_font_px,
            pack_xrgb(MAPLE_CHAT_BUTTON_INK),
        );
    }

    let mut content_x = rect.x + 4 + channel_width + MAPLE_CHAT_INNER_GAP;
    let audio_width = summary
        .audio_label
        .as_deref()
        .map(|label| crate::aa_text::label_text_width(label, font_px) + MAPLE_CHAT_INNER_GAP)
        .unwrap_or(0);
    let content_right = (controls_x - MAPLE_CHAT_INNER_GAP - audio_width).max(content_x);
    let dot_size = (font_px / 3.0).round().clamp(4.0, 6.0) as i32;
    let separator_width = MAPLE_CHAT_INNER_GAP;
    let mut painted = false;

    for run in &summary.runs {
        let has_dot = run.tone != FooterTone::Neutral;
        let dot_width = if has_dot {
            dot_size + MAPLE_CHAT_INNER_GAP - 1
        } else {
            0
        };
        let available = content_right - content_x - dot_width;
        if available <= 0 {
            break;
        }
        let text = truncate_label_to_px(&run.text, available, font_px);
        if text.is_empty() {
            continue;
        }
        let text_width = crate::aa_text::label_text_width(&text, font_px);
        if content_x + dot_width + text_width > content_right {
            continue;
        }
        if painted {
            let separator_x = content_x - separator_width / 2;
            canvas.fill_rect(
                LabelCardRect {
                    x: separator_x,
                    y: rect.y + 6,
                    width: 1,
                    height: 1,
                },
                pack_xrgb(MAPLE_CHAT_SHADOW),
                0.72,
            );
        }
        if has_dot {
            let dot_y = rect.y + (rect.height - dot_size) / 2;
            canvas.fill_rect(
                LabelCardRect {
                    x: content_x,
                    y: dot_y,
                    width: dot_size,
                    height: dot_size,
                },
                pack_xrgb(footer_tone_rgb(run.tone, theme)),
                1.0,
            );
            content_x += dot_width;
        }
        canvas.draw_ink(
            &text,
            (content_x, text_y),
            font_px,
            pack_xrgb(MAPLE_CHAT_INK),
        );
        content_x += text_width + separator_width;
        painted = true;
    }

    if let Some(audio) = summary.audio_label {
        let audio_x =
            controls_x - MAPLE_CHAT_INNER_GAP - crate::aa_text::label_text_width(&audio, font_px);
        if audio_x > content_x {
            canvas.draw_ink(
                &audio,
                (audio_x, text_y),
                font_px,
                pack_xrgb(MAPLE_CHAT_INK),
            );
        }
    }
}

/// Paint the shared status footer as a bottom-overlay band. Each segment is
/// toned via the shared [`footer_tone_rgb`], then
/// packed to the surface XRGB; laid left-to-right from the left margin, the model's
/// baked right-flush padding pushes the ♩/keys suffix to the right edge. Fixed
/// caption height like the name badges (crisp at any scene scale); an OVERLAY over
/// the scene's bottom rows — it never insets the buffer (that would shift the
/// agent-capacity lockstep). This carries the ♩/♩N% audio feedback and remains
/// silent when muted.
pub fn paint_footer_into_surface(
    sb: &mut [u32],
    win_w: usize,
    win_h: usize,
    model: &FooterModel,
    theme: &Theme,
) {
    if theme.name == "maple" {
        paint_maple_chat_footer(sb, win_w, win_h, model, theme);
        return;
    }
    let y = (win_h as i32 - crate::aa_text::line_height(LABEL_FONT_PX) - FOOTER_MARGIN_PX).max(0);
    let mut x = FOOTER_MARGIN_PX;
    for seg in &model.segments {
        let color = pack_xrgb(footer_tone_rgb(seg.tone, theme));
        draw_badge_text(sb, win_w, win_h, &seg.text, x, y, LABEL_FONT_PX, color);
        x += crate::aa_text::text_width(&seg.text, LABEL_FONT_PX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Maple production uses its shop/training cards and chat strip. This
    /// renamed clone keeps the retained generic compositor covered without
    /// re-registering a second production theme.
    static OFFICE_TEST_THEME: std::sync::LazyLock<Theme> = std::sync::LazyLock::new(|| {
        let mut theme = pixtuoid_scene::theme::MAPLE.clone();
        theme.name = "maple-office-test";
        theme
    });

    fn office_test_theme() -> &'static Theme {
        &OFFICE_TEST_THEME
    }

    #[test]
    fn pack_xrgb_is_0x00rrggbb() {
        // Pin the surface pixel format (channel order + shift widths) so the two
        // production packers (scene blit + label overlay) can't re-drift. The
        // per-tone label test below independently cross-checks it via `as_u32`.
        assert_eq!(
            pack_xrgb(Rgb {
                r: 255,
                g: 128,
                b: 0
            }),
            0x00FF_8000
        );
        assert_eq!(pack_xrgb(Rgb { r: 0, g: 0, b: 0 }), 0x0000_0000);
        assert_eq!(pack_xrgb(Rgb { r: 1, g: 2, b: 3 }), 0x0001_0203);
    }

    #[test]
    fn nearest_xrgb_upscale_repeats_pixels_and_clamps_remainder_edges() {
        let source = [1, 2, 3, 4];
        let mut scaled = [0; 7 * 5];

        assert!(upscale_xrgb_nearest(&source, 2, 2, &mut scaled, 7, 5, 3));
        assert_eq!(
            scaled,
            [
                1, 1, 1, 2, 2, 2, 2, // first source row, repeated vertically
                1, 1, 1, 2, 2, 2, 2, //
                1, 1, 1, 2, 2, 2, 2, //
                3, 3, 3, 4, 4, 4, 4, // second source row
                3, 3, 3, 4, 4, 4, 4, //
            ]
        );

        let mut one_to_one = [0; 3 * 3];
        assert!(upscale_xrgb_nearest(
            &source,
            2,
            2,
            &mut one_to_one,
            3,
            3,
            1,
        ));
        assert_eq!(one_to_one, [1, 2, 2, 3, 4, 4, 3, 4, 4]);
    }

    #[test]
    fn embedded_public_runtime_opens_the_maple_dual_world_without_a_custom_pack() {
        let scene = SceneState::new([8; pixtuoid_core::state::MAX_FLOORS]);
        let pack =
            pixtuoid_scene::embedded_pack::load_sprite_pack(None).expect("embedded pack loads");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut renderer = MapleRenderer::new();

        let buf = renderer.render(&scene, &pack, theme, now, 640, 240, FloorMeta::ground());

        assert_eq!((buf.width(), buf.height()), (640, 240));
        assert!(
            renderer.dual_rendered,
            "both built-in maps must render after clone"
        );
        assert!(renderer.market_viewport.is_some());
        assert!(renderer.training_viewport.is_some());
        assert!(
            renderer.market_avatars,
            "the original built-in chibi renderer uses the paperdoll-sized label geometry"
        );
    }

    #[test]
    fn dual_map_showcases_render_independently_without_changing_agent_stats() {
        use pixtuoid_scene::maple_world::MapleMapId;

        let scene = SceneState::uniform(8);
        let pack =
            pixtuoid_scene::embedded_pack::load_sprite_pack(None).expect("embedded pack loads");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let created = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let now = created + std::time::Duration::from_secs(20);
        let mut renderer = MapleRenderer::new();
        renderer.configure_characters(
            crate::config::CharacterConfig {
                agent_roster: pixtuoid_scene::characters::CharacterRoster::default(),
                showcase_slots: vec![1],
                training_showcase_slots: vec![2],
            },
            created,
        );

        renderer.render(&scene, &pack, theme, now, 640, 240, FloorMeta::ground());
        let batches = renderer.maple_overlay_batches(&scene, now);
        let market = batches
            .iter()
            .find(|batch| batch.map == MapleMapId::FreeMarket)
            .expect("market batch");
        let training = batches
            .iter()
            .find(|batch| batch.map == MapleMapId::ForestTraining)
            .expect("training batch");

        assert!(market.labels.iter().any(|label| label.text == "逛街中"));
        assert!(
            training.labels.iter().any(|label| label.text == "練功中"),
            "training overlays must use the same augmented scene as the pixel pass"
        );
        assert_eq!(pixtuoid_scene::board::scene_stats(&scene).total, 0);
    }

    #[test]
    fn training_only_showcase_keeps_the_active_animation_cadence() {
        let created = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut renderer = MapleRenderer::new();
        renderer.configure_characters(
            crate::config::CharacterConfig {
                agent_roster: pixtuoid_scene::characters::CharacterRoster::default(),
                showcase_slots: Vec::new(),
                training_showcase_slots: vec![3],
            },
            created,
        );

        assert!(renderer.selected_market_showcase_slots().is_empty());
        assert_eq!(renderer.selected_training_showcase_slots(), vec![3]);
        assert!(renderer.showcase_needs_active_animation());
    }

    #[test]
    fn renders_a_sized_nonblank_maple_buffer() {
        // A fresh scene still paints the selected procedural map and sizes the
        // RGB buffer exactly to the requested dimensions.
        let scene = SceneState::new([8; pixtuoid_core::state::MAX_FLOORS]);
        let pack =
            pixtuoid_scene::embedded_pack::load_sprite_pack(None).expect("embedded pack loads");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut renderer = MapleRenderer::new();
        let buf = renderer.render(&scene, &pack, theme, now, 160, 96, FloorMeta::ground());
        assert_eq!((buf.width(), buf.height()), (160, 96));
        // Assert painted content, not the pre-fill.
        let bg = theme.surface.bg_fallback;
        assert!(
            buf.as_slice()
                .iter()
                .any(|p| *p != Rgb { r: 0, g: 0, b: 0 } && *p != bg),
            "the painter draws scene content beyond the cleared background"
        );
    }

    #[test]
    fn default_maple_scale_keeps_every_window_at_native_resolution() {
        assert_eq!(maple_scale(180), 1);
        assert_eq!(maple_scale(360), 1);
        assert_eq!(maple_scale(480), 1);
        assert_eq!(maple_scale(720), 1);
        assert_eq!(maple_scale(90), 1);
        assert_eq!(maple_scale(0), 1);
        assert_eq!(
            window_buffer_geometry_with_scale_override(1_440, 480, None),
            (1, 1_440, 480),
            "direct EXE launches must not blur a large private map pack"
        );
    }

    #[test]
    fn label_font_size_is_dpi_aware_user_selectable_and_fail_safe() {
        assert_eq!(parse_label_scale(None), 1.0);
        assert_eq!(parse_label_scale(Some("1.0")), 1.0);
        assert_eq!(parse_label_scale(Some("1.25")), 1.25);
        assert_eq!(parse_label_scale(Some("1.5")), 1.5);
        assert_eq!(parse_label_scale(Some("not-a-number")), 1.0);
        assert_eq!(parse_label_scale(Some("9")), 2.0, "cap oversized input");
        assert_eq!(parse_label_scale(Some("0.1")), 0.75, "floor tiny input");

        assert_eq!(label_font_px(1.0, 1.0), 12.0);
        assert_eq!(label_font_px(2.0, 1.0), 24.0);
        assert_eq!(label_font_px(2.0, 1.25), 30.0);
        assert_eq!(
            label_font_px(f64::NAN, 1.25),
            15.0,
            "an invalid DPI value falls back to 1x"
        );
    }

    #[test]
    fn boot_capacities_for_window_match_the_first_redraw_geometry() {
        // A 4x-upscaled window (720px tall → maple_scale 4): the boot seed must
        // match what the first redraw's `sync_floor_caps` stores — `floor_capacity`
        // at the downscaled buffer (window / scale), not the full-window size.
        let (w, h) = (1280u32, 720u32);
        let scale = maple_scale(h);
        let buf_w = (w / scale) as u16;
        let buf_h = (h / scale) as u16;
        let boot = boot_capacities_for_window(w, h);
        for (i, &got) in boot.iter().enumerate() {
            let cap = pixtuoid_scene::floor::floor_capacity(
                buf_w,
                buf_h,
                pixtuoid_scene::floor::floor_seed(i),
            );
            let want = if cap == 0 {
                crate::runtime::FALLBACK_AGENT_CAPACITY
            } else {
                cap
            };
            assert_eq!(
                got, want,
                "floor {i} boot cap must match the rendered geometry"
            );
        }
    }

    #[test]
    fn explicit_scene_scale_one_keeps_the_market_at_window_resolution() {
        assert_eq!(
            window_buffer_geometry_with_scale_override(720, 480, Some(1)),
            (1, 720, 480)
        );
        assert_eq!(
            window_buffer_geometry_with_scale_override(960, 640, Some(1)),
            (1, 960, 640)
        );
        assert_eq!(
            window_buffer_geometry_with_scale_override(720, 480, Some(0)),
            window_buffer_geometry_with_scale_override(720, 480, None),
            "zero is invalid and must retain the native-resolution default"
        );
    }

    #[test]
    fn label_card_frame_uses_activity_tone_and_brighter_hover_highlight() {
        use pixtuoid_scene::layout::Point;
        use pixtuoid_scene::overlay::{LabelElement, LabelTone};
        let theme = office_test_theme();
        let as_u32 = |c: Rgb| (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32;
        let badge = |tone, hovered| {
            vec![LabelElement {
                anchor_px: Point { x: 20, y: 20 },
                text: "cc".into(),
                tone,
                hovered,
                relation: None,
            }]
        };
        // The one-pixel top border reaches full coverage, so this catches an
        // incorrect tone mapping instead of merely proving that the card painted.
        for (tone, expected) in [
            (LabelTone::Active, theme.ui.label_active),
            (LabelTone::Waiting, theme.ui.label_waiting),
            (LabelTone::Idle, theme.ui.label_idle),
            (LabelTone::Exiting, theme.ui.label_exiting),
        ] {
            let mut sb = vec![0u32; 100 * 100];
            paint_labels_into_surface(&mut sb, 100, 100, &badge(tone, false), 2, theme);
            assert!(
                sb.contains(&as_u32(expected)),
                "tone {tone:?} must paint its theme color {expected:?}"
            );
        }
        // Hover replaces the idle frame hue with the shared near-white highlight.
        let brightness = |sb: &[u32]| {
            sb.iter()
                .map(|&p| (p & 0xff) + ((p >> 8) & 0xff) + ((p >> 16) & 0xff))
                .max()
                .unwrap_or(0)
        };
        let mut hover_sb = vec![0u32; 100 * 100];
        paint_labels_into_surface(
            &mut hover_sb,
            100,
            100,
            &badge(LabelTone::Idle, true),
            2,
            theme,
        );
        let mut idle_sb = vec![0u32; 100 * 100];
        paint_labels_into_surface(
            &mut idle_sb,
            100,
            100,
            &badge(LabelTone::Idle, false),
            2,
            theme,
        );
        assert!(
            brightness(&hover_sb) > brightness(&idle_sb),
            "hover paints a brighter frame highlight than the idle tone it overrides"
        );
    }

    #[test]
    fn requested_label_font_size_changes_the_painted_badge_extent() {
        use pixtuoid_scene::layout::Point;
        use pixtuoid_scene::overlay::{LabelElement, LabelTone};

        let theme = office_test_theme();
        let label = [LabelElement {
            anchor_px: Point { x: 90, y: 55 },
            text: "cx·task".into(),
            tone: LabelTone::Active,
            hovered: false,
            relation: None,
        }];
        let (win_w, win_h) = (420usize, 180usize);
        let painted_extent = |font_px| {
            let mut sb = vec![0u32; win_w * win_h];
            paint_labels_into_surface_with_font_px(
                &mut sb, win_w, win_h, &label, 2, font_px, theme,
            );
            let points: Vec<(usize, usize)> = sb
                .iter()
                .enumerate()
                .filter(|(_, pixel)| **pixel != 0)
                .map(|(i, _)| (i % win_w, i / win_w))
                .collect();
            let min_x = points.iter().map(|p| p.0).min().expect("badge paints x");
            let max_x = points.iter().map(|p| p.0).max().expect("badge paints x");
            let min_y = points.iter().map(|p| p.1).min().expect("badge paints y");
            let max_y = points.iter().map(|p| p.1).max().expect("badge paints y");
            (max_x - min_x + 1, max_y - min_y + 1)
        };

        let standard = painted_extent(12.0);
        let large = painted_extent(24.0);
        assert!(
            large.0 > standard.0,
            "large badge is wider: {standard:?} -> {large:?}"
        );
        assert!(
            large.1 > standard.1,
            "large badge is taller: {standard:?} -> {large:?}"
        );
    }

    #[test]
    fn map_size_and_topmost_selectors_have_separate_click_targets_in_one_compact_stack() {
        assert!(!topmost_selector_visible(96));
        assert!(topmost_selector_visible(120));
        let map_text = "地圖：雙圖 [Tab]";
        let size_text = "大小：小 [Z]";
        let topmost_text = "置頂：開 [T]";
        let map = map_selector_rect(map_text, 12.0, 640, 360);
        let size = size_selector_rect(size_text, 12.0, 640, 360);
        let topmost = topmost_selector_rect(topmost_text, 12.0, 640, 360);

        assert!(size.y >= map.bottom() + MAP_SELECTOR_GAP_PX);
        assert!(topmost.y >= size.bottom() + MAP_SELECTOR_GAP_PX);
        assert!(map_selector_hit_test(
            (f64::from(map.x + 2), f64::from(map.y + 2)),
            map_text,
            12.0,
            640,
            360,
        ));
        assert!(!size_selector_hit_test(
            (f64::from(map.x + 2), f64::from(map.y + 2)),
            size_text,
            12.0,
            640,
            360,
        ));
        assert!(size_selector_hit_test(
            (f64::from(size.x + 2), f64::from(size.y + 2)),
            size_text,
            12.0,
            640,
            360,
        ));
        assert!(!topmost_selector_hit_test(
            (f64::from(size.x + 2), f64::from(size.y + 2)),
            topmost_text,
            12.0,
            640,
            360,
        ));
        assert!(topmost_selector_hit_test(
            (f64::from(topmost.x + 2), f64::from(topmost.y + 2)),
            topmost_text,
            12.0,
            640,
            360,
        ));
    }

    #[test]
    fn startup_toggle_and_escape_hint_extend_the_control_stack_without_overlap() {
        assert!(!startup_selector_visible(191));
        assert!(startup_selector_visible(192));
        assert!(!escape_hint_visible(227));
        assert!(escape_hint_visible(228));

        let character = character_selector_rect("角色：8｜展示：1 [C]", 12.0, 640, 360);
        let startup = startup_selector_rect("開機啟動：關 [A]", 12.0, 640, 360);
        let escape = escape_hint_rect("ESC：關閉程式", 12.0, 640, 360);
        assert!(startup.y >= character.bottom() + MAP_SELECTOR_GAP_PX);
        assert!(escape.y >= startup.bottom() + MAP_SELECTOR_GAP_PX);
        assert!(startup_selector_hit_test(
            (f64::from(startup.x + 2), f64::from(startup.y + 2)),
            "開機啟動：關 [A]",
            12.0,
            640,
            360,
        ));

        let mut surface = vec![0u32; 640 * 360];
        paint_escape_hint_into_surface(&mut surface, 640, 360, "ESC：關閉程式", 12.0);
        assert!(surface.iter().any(|pixel| *pixel != 0));
    }

    #[test]
    fn character_panel_uses_three_clear_actions_per_character_and_hides_when_too_small() {
        assert!(character_panel_layout(160, 96, 12.0).is_none());
        let layout = character_panel_layout(640, 360, 12.0).expect("room for roster panel");
        assert_eq!(layout.rows.len(), 8);
        let first = layout.rows[0];
        let agent_action = character_panel_hit_test(
            (
                f64::from(first.agent_toggle.x + 2),
                f64::from(first.agent_toggle.y + 2),
            ),
            12.0,
            640,
            360,
            0,
            26,
        );
        let market_action = character_panel_hit_test(
            (
                f64::from(first.market_showcase_toggle.x + 2),
                f64::from(first.market_showcase_toggle.y + 2),
            ),
            12.0,
            640,
            360,
            0,
            26,
        );
        let training_action = character_panel_hit_test(
            (
                f64::from(first.training_showcase_toggle.x + 2),
                f64::from(first.training_showcase_toggle.y + 2),
            ),
            12.0,
            640,
            360,
            0,
            26,
        );
        assert_eq!(agent_action, Some(CharacterPanelAction::ToggleAgentSlot(0)));
        assert_eq!(
            market_action,
            Some(CharacterPanelAction::ToggleMarketShowcaseSlot(0))
        );
        assert_eq!(
            training_action,
            Some(CharacterPanelAction::ToggleTrainingShowcaseSlot(0))
        );

        let second_page = character_panel_hit_test(
            (
                f64::from(first.agent_toggle.x + 2),
                f64::from(first.agent_toggle.y + 2),
            ),
            12.0,
            640,
            360,
            1,
            26,
        );
        assert_eq!(
            second_page,
            Some(CharacterPanelAction::ToggleAgentSlot(8)),
            "the first row on page two must target appearance 8"
        );

        let next_page = character_panel_hit_test(
            (
                f64::from(layout.next_page.x + 2),
                f64::from(layout.next_page.y + 2),
            ),
            12.0,
            640,
            360,
            0,
            26,
        );
        assert_eq!(next_page, Some(CharacterPanelAction::NextPage));
    }

    #[test]
    fn character_panel_exposes_a_separate_clipboard_import_action_without_stealing_paging() {
        let layout = character_panel_layout(640, 360, 12.0).expect("room for roster panel");
        let import_action = character_panel_hit_test(
            (
                f64::from(layout.import_from_clipboard.x + 2),
                f64::from(layout.import_from_clipboard.y + 2),
            ),
            12.0,
            640,
            360,
            0,
            26,
        );
        assert_eq!(
            import_action,
            Some(CharacterPanelAction::StartMapleAtelierClipboardImport)
        );
        assert_eq!(
            character_panel_hit_test(
                (
                    f64::from(layout.next_page.x + 2),
                    f64::from(layout.next_page.y + 2),
                ),
                12.0,
                640,
                360,
                0,
                26,
            ),
            Some(CharacterPanelAction::NextPage),
            "the import control must not consume the existing paging target"
        );
    }

    #[test]
    fn character_panel_exposes_a_top_right_close_button() {
        let layout = character_panel_layout(640, 360, 12.0).expect("room for roster panel");

        let close_action = character_panel_hit_test(
            (
                f64::from(layout.close.x + layout.close.width / 2),
                f64::from(layout.close.y + layout.close.height / 2),
            ),
            12.0,
            640,
            360,
            0,
            26,
        );

        assert_eq!(close_action, Some(CharacterPanelAction::ClosePanel));
        assert!(
            layout.import_from_clipboard.right() < layout.close.x,
            "the import and close targets must remain independent"
        );
    }

    #[test]
    fn character_panel_only_exposes_delete_for_imported_characters() {
        let layout = character_panel_layout(640, 360, 12.0).expect("room for roster panel");
        let first = layout.rows[0];
        let cursor = (
            f64::from(first.delete.x + first.delete.width / 2),
            f64::from(first.delete.y + first.delete.height / 2),
        );

        assert_eq!(
            character_panel_hit_test(cursor, 12.0, 640, 360, 0, 26),
            None,
            "the eight built-in paperdolls are required assets"
        );
        assert_eq!(
            character_panel_hit_test(cursor, 12.0, 640, 360, 1, 26),
            Some(CharacterPanelAction::RequestDeleteSlot(8)),
            "the first catalog import is appearance eight"
        );
    }

    #[test]
    fn character_panel_keeps_traditional_chinese_readable_at_large_window_size() {
        // The zh-TW launcher defaults to a 1440x480 window with 125% labels,
        // which reaches this function as a 15 px native-surface font.  The
        // roster must not shrink that already-small CJK face or leave most of
        // the wide dual-map window unused.
        let layout = character_panel_layout(1440, 480, 15.0).expect("large roster panel");
        assert!(
            layout.font_px >= 15.5,
            "the roster CJK face stays at a legible native size: {} px",
            layout.font_px
        );
        assert!(
            layout.panel.width >= 760,
            "the roster uses the available width instead of compressing labels: {} px",
            layout.panel.width
        );
        assert!(
            layout.rows[0].agent_toggle.height >= 36,
            "CJK rows retain enough vertical breathing room: {} px",
            layout.rows[0].agent_toggle.height
        );
    }

    #[test]
    fn character_panel_text_uses_one_crisp_raster_pass() {
        let background = 0x00f4_efdc;
        let ink = pack_xrgb(MAP_SELECTOR_INK);
        let mut expected = vec![background; 180 * 48];
        crate::aa_text::draw_label_text_at(
            "角色名單｜展示逛街",
            7,
            8,
            16.0,
            |x, y, cov| blend_xrgb(&mut expected, 180, 48, x, y, ink, cov),
        );

        let mut actual = vec![background; 180 * 48];
        draw_panel_text(&mut actual, 180, 48, "角色名單｜展示逛街", 7, 8, 16.0, ink);

        assert_eq!(
            actual, expected,
            "opaque roster controls must not add the shifted badge shadow that doubles CJK strokes"
        );
    }

    #[test]
    fn character_panel_paints_the_loaded_pack_appearance_next_to_each_name() {
        const PACK_TOML: &str = r##"
[pack]
name = "roster-preview-test"
version = "0.0.0"

[palette]
"B" = "#101112"
"H" = "#202122"
"S" = "#303132"
"P" = "#404142"
"A" = "#ff3366"

[animations.market_avatar]
frames = [
  "avatar_0.sprite", "avatar_1.sprite", "avatar_2.sprite", "avatar_3.sprite",
  "avatar_4.sprite", "avatar_5.sprite", "avatar_6.sprite", "avatar_7.sprite",
]
frame_ms = 1000
"##;
        let row = std::iter::repeat_n("A", 32).collect::<Vec<_>>().join(" ");
        let avatar = format!(
            "@frame 0\n{}\n",
            std::iter::repeat_n(row, 24).collect::<Vec<_>>().join("\n")
        );
        let names = [
            "avatar_0.sprite",
            "avatar_1.sprite",
            "avatar_2.sprite",
            "avatar_3.sprite",
            "avatar_4.sprite",
            "avatar_5.sprite",
            "avatar_6.sprite",
            "avatar_7.sprite",
        ];
        let sources = names.map(|name| (name, avatar.as_str()));
        let pack = pixtuoid_core::sprite::format::load_pack_from_strings(PACK_TOML, &sources)
            .expect("preview pack");
        let model = CharacterPanelModel {
            real_market_agents: 0,
            real_training_agents: 0,
            market_showcase_count: 0,
            training_showcase_count: 0,
            rows: pixtuoid_scene::characters::CHARACTER_NAMES_ZH_TW
                .iter()
                .enumerate()
                .map(|(appearance_index, name)| CharacterPanelRow {
                    name: (*name).to_owned(),
                    appearance_index,
                    agent_enabled: true,
                    deletable: false,
                    delete_confirmation: false,
                    market_showcase: CharacterShowcaseState::Absent,
                    training_showcase: CharacterShowcaseState::Absent,
                })
                .collect(),
            page: 0,
            notice: None,
        };
        let mut surface = vec![0u32; 640 * 360];
        paint_character_panel_into_surface(&mut surface, 640, 360, &model, &pack, 12.0);

        let layout = character_panel_layout(640, 360, 12.0).expect("panel layout");
        let preview = layout.rows[0].preview;
        assert!(
            (preview.y..preview.bottom()).any(|y| {
                (preview.x..preview.right()).any(|x| {
                    surface[y as usize * 640 + x as usize]
                        == pack_xrgb(Rgb {
                            r: 255,
                            g: 51,
                            b: 102,
                        })
                })
            }),
            "the preview rectangle contains the selected active-pack paperdoll"
        );
    }

    #[test]
    fn long_task_name_becomes_a_narrow_vertical_card() {
        let card = build_label_card_text("cx·修復浮動視窗名稱", 15.0);

        assert_eq!(card.source, "cx");
        assert_eq!(card.title_lines.concat(), "修復浮動視窗名稱");
        assert!(
            card.title_lines.len() >= 2,
            "a long task title must stack upward instead of remaining one wide row"
        );
        assert!(
            card.title_lines
                .iter()
                .all(|line| crate::aa_text::label_text_width(line, 15.0) <= card.title_width_px),
            "every task row fits the narrow card width"
        );
    }

    #[test]
    fn maple_fake_id_band_keeps_small_traditional_chinese_crisp() {
        let card = build_maple_label_card_text("素材狐·整理角色動畫", LABEL_FONT_PX);
        let metrics = maple_label_card_metrics(&card, LABEL_FONT_PX);

        assert_eq!(card.source, "素材狐");
        assert!(
            metrics.source_font_px >= 10.0,
            "the 1x-DPI fake id must not fall back to an 8px CJK raster"
        );
        assert_eq!(maple_ink_strength(&card.source), 1.0);
        assert_eq!(maple_ink_strength("agent"), MAPLE_CARD_LATIN_INK_STRENGTH);
    }

    #[test]
    fn maple_shop_card_reserves_its_second_line_for_root_child_identity() {
        use pixtuoid_core::AgentId;
        use pixtuoid_scene::overlay::AgentRelation;

        let root_id = AgentId::from_parts("codex", "factory-root");
        let root = AgentRelation::Root {
            root_id,
            descendant_count: 3,
        };
        let child = AgentRelation::Child {
            root_id,
            root_task: Some("盤點新廠".to_owned()),
            depth: 2,
        };
        let root_card = build_maple_label_card_text_with_relation(
            "隊長熊\u{b7}盤點新廠設備清單",
            LABEL_FONT_PX,
            Some(&root),
        );
        let child_card = build_maple_label_card_text_with_relation(
            "動作貓\u{b7}拍攝設備照片",
            LABEL_FONT_PX,
            Some(&child),
        );

        assert_eq!(root_card.title_lines.len(), 2);
        assert!(root_card.title_lines[1].contains("主 task"));
        assert!(root_card.title_lines[1].contains('3'));
        assert_eq!(child_card.title_lines.len(), 2);
        assert!(child_card.title_lines[1].contains("子2"));
        assert!(child_card.title_lines[1].contains("盤點新廠"));
        assert_eq!(lineage_family_color(&root), lineage_family_color(&child));
    }

    #[test]
    fn maple_player_id_stays_inside_a_compact_inset_of_the_authored_stall_plaque() {
        use pixtuoid_scene::layout::Point;
        use pixtuoid_scene::overlay::{LabelElement, LabelTone};

        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let background = 0x0070_8090;
        let (win_w, win_h, scale) = (240usize, 160usize, 2i32);
        let nameplate_text_y = 55u16;
        let id = LabelElement {
            anchor_px: Point {
                x: 46,
                y: nameplate_text_y,
            },
            text: "素材狐".into(),
            tone: LabelTone::Active,
            hovered: false,
            relation: None,
        };
        let mut surface = vec![background; win_w * win_h];

        paint_market_player_ids_into_surface_with_font_px(
            &mut surface,
            win_w,
            win_h,
            std::slice::from_ref(&id),
            scale,
            LABEL_FONT_PX,
            theme,
        );

        let changed = surface
            .iter()
            .enumerate()
            .filter(|(_, pixel)| **pixel != background)
            .map(|(index, _)| ((index % win_w) as i32, (index / win_w) as i32))
            .collect::<Vec<_>>();
        assert!(!changed.is_empty(), "the readable player id paints");
        assert!(
            changed
                .iter()
                .all(|(_, y)| *y >= i32::from(nameplate_text_y) * scale - 2),
            "the inset plate must start only two pixels above the authored text row: {changed:?}"
        );
        let rendered_font_px = (LABEL_FONT_PX * MAPLE_CARD_SOURCE_FONT_SCALE)
            .clamp(MAPLE_CARD_SOURCE_FONT_MIN_PX, MAPLE_PLAYER_ID_FONT_MAX_PX);
        let text_width = crate::aa_text::label_text_width(&id.text, rendered_font_px);
        let centre_x = i32::from(id.anchor_px.x) * scale + (FLOATING_SPRITE_W * scale) / 2;
        let outside_ink_x = centre_x - text_width / 2 - 7;
        let outside_ink_y = i32::from(nameplate_text_y) * scale - 2;
        assert_eq!(
            surface[outside_ink_y as usize * win_w + outside_ink_x as usize],
            background,
            "the compact inset must preserve the authored wooden frame around it"
        );
    }

    #[test]
    fn maple_player_id_adds_a_compact_dark_inset_plate_around_the_ink() {
        use pixtuoid_scene::layout::Point;
        use pixtuoid_scene::overlay::{LabelElement, LabelTone};

        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let background = 0x0070_8090;
        let (win_w, win_h, scale) = (240usize, 160usize, 2i32);
        let text_y = 55u16;
        let id = LabelElement {
            anchor_px: Point { x: 46, y: text_y },
            text: "素材狐".into(),
            tone: LabelTone::Active,
            hovered: false,
            relation: None,
        };
        let mut surface = vec![background; win_w * win_h];

        paint_market_player_ids_into_surface_with_font_px(
            &mut surface,
            win_w,
            win_h,
            std::slice::from_ref(&id),
            scale,
            LABEL_FONT_PX,
            theme,
        );

        let centre_x = i32::from(id.anchor_px.x) * scale + (FLOATING_SPRITE_W * scale) / 2;
        let plate_top_y = i32::from(text_y) * scale - 2;
        let top_rail_changed = ((centre_x - 24)..=(centre_x + 24))
            .filter(|x| surface[plate_top_y as usize * win_w + *x as usize] != background)
            .count();
        assert!(
            top_rail_changed >= 12,
            "the name needs a visible inset-plate top rail outside the glyph row"
        );

        let outside_x = centre_x - 32;
        assert_eq!(
            surface[plate_top_y as usize * win_w + outside_x as usize],
            background,
            "the inset plate must remain compact enough to expose the authored wooden frame"
        );
    }

    #[test]
    fn maple_player_id_uses_bright_ink_with_a_dark_game_style_shadow() {
        use pixtuoid_scene::layout::Point;
        use pixtuoid_scene::overlay::{LabelElement, LabelTone};

        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let background = 0x0050_2f1e;
        let background_luma = 299 * 0x50 + 587 * 0x2f + 114 * 0x1e;
        let (win_w, win_h, scale) = (240usize, 160usize, 2i32);
        let id = LabelElement {
            anchor_px: Point { x: 46, y: 55 },
            text: "動作貓".into(),
            tone: LabelTone::Active,
            hovered: false,
            relation: None,
        };
        let mut surface = vec![background; win_w * win_h];

        paint_market_player_ids_into_surface_with_font_px(
            &mut surface,
            win_w,
            win_h,
            std::slice::from_ref(&id),
            scale,
            LABEL_FONT_PX,
            theme,
        );

        let rendered_luma = surface
            .into_iter()
            .filter(|pixel| *pixel != background)
            .map(|pixel| {
                let red = (pixel >> 16) & 0xff;
                let green = (pixel >> 8) & 0xff;
                let blue = pixel & 0xff;
                299 * red + 587 * green + 114 * blue
            })
            .collect::<Vec<_>>();
        assert!(
            !rendered_luma.is_empty(),
            "the player id must paint visible ink"
        );
        assert!(
            rendered_luma.iter().copied().max().unwrap() >= background_luma + 100_000,
            "the main ink must remain visibly bright over the brown authored plaque"
        );
        assert!(
            rendered_luma.iter().copied().min().unwrap() + 5_000 < background_luma,
            "a dark one-pixel shadow must separate the small name from the plaque"
        );
    }

    #[test]
    fn nearby_name_cards_stay_in_a_bounded_head_zone() {
        let specs = vec![
            LabelCardSpec {
                label_index: 0,
                anchor_x: 100,
                anchor_y: 140,
                width: 64,
                height: 54,
            },
            LabelCardSpec {
                label_index: 1,
                anchor_x: 128,
                anchor_y: 140,
                width: 64,
                height: 54,
            },
        ];
        let placed = place_label_cards(specs, 240, LABEL_CARD_HEAD_GAP);

        assert_eq!(placed.len(), 2);
        assert_eq!(
            placed[0].rect.y, placed[1].rect.y,
            "both cards should stay at the head-height baseline when a small side nudge fits"
        );
        assert!(
            (placed[1].rect.x - (128 - 64 / 2)).abs() <= LABEL_CARD_MAX_NUDGE_PX,
            "the card must never leave its agent's bounded head zone: {placed:?}"
        );
        let centered = LabelCardRect {
            x: 128 - 64 / 2,
            y: placed[1].rect.y,
            width: 64,
            height: 54,
        };
        assert!(
            placed[1].rect.overlap_area(placed[0].rect) <= centered.overlap_area(placed[0].rect),
            "the bounded nudge should reduce overlap when it can"
        );
    }

    #[test]
    fn dual_map_cards_cannot_cross_the_panel_seam() {
        let specs = vec![
            LabelCardSpec {
                label_index: 0,
                anchor_x: 721,
                anchor_y: 140,
                width: 100,
                height: 54,
            },
            LabelCardSpec {
                label_index: 1,
                anchor_x: 1_439,
                anchor_y: 140,
                width: 100,
                height: 54,
            },
        ];
        let placed = place_label_cards_in_range(specs, 720, 1_440, LABEL_CARD_HEAD_GAP);

        assert!(placed.iter().all(|card| {
            card.rect.x >= 720 + LABEL_CARD_EDGE_MARGIN
                && card.rect.x + card.rect.width <= 1_440 - LABEL_CARD_EDGE_MARGIN
        }));
    }

    #[test]
    fn crowded_cards_keep_the_same_height_without_leaving_their_head_zone() {
        let specs = (0..3)
            .map(|label_index| LabelCardSpec {
                label_index,
                anchor_x: 150,
                anchor_y: 140,
                width: 64,
                height: 54,
            })
            .collect();
        let placed = place_label_cards(specs, 320, LABEL_CARD_HEAD_GAP);

        assert!(
            placed.iter().all(|card| card.rect.y == 81),
            "every card bottom stays five pixels above the same head position: {placed:?}"
        );
        assert!(
            placed
                .iter()
                .all(|card| (card.rect.x - (150 - 64 / 2)).abs() <= LABEL_CARD_MAX_NUDGE_PX),
            "crowding may overlap translucent cards but must not detach them: {placed:?}"
        );
    }

    #[test]
    fn card_background_is_readably_translucent_not_an_opaque_block() {
        let white = 0x00ff_ffff;
        let mut sb = vec![white; 40 * 40];
        LabelCardCanvas::new(&mut sb, 40, 40).paint_frame(
            LabelCardRect {
                x: 5,
                y: 5,
                width: 24,
                height: 24,
            },
            0x0000_0000,
            0x00ff_ffff,
        );

        let body = sb[15 * 40 + 15] & 0xff;
        assert!(
            (55..=105).contains(&body),
            "the scene must remain visible through the dark card body; channel={body}"
        );
    }

    #[test]
    fn training_showcase_card_never_claims_to_be_an_agent() {
        use pixtuoid_scene::overlay::LabelTone;

        let card = build_training_label_card_text("練功中", 12.0, LabelTone::Active);

        assert_eq!(card.title_lines, vec!["練功中"]);
        assert!(
            card.source.is_empty(),
            "presentation-only training actors must not render Agent or 工作中 metadata"
        );
        let showcase_metrics = training_label_card_metrics(&card, 12.0);
        let agent_metrics = training_label_card_metrics(
            &build_training_label_card_text("動作貓·整理代理站位", 12.0, LabelTone::Active),
            12.0,
        );
        assert_eq!(showcase_metrics.source_band_height, 0);
        assert!(showcase_metrics.height < agent_metrics.height);
    }

    #[test]
    fn training_labels_are_compact_two_line_status_cards_without_shop_controls() {
        use pixtuoid_scene::layout::Point;
        use pixtuoid_scene::maple_world::MapleMapId;
        use pixtuoid_scene::overlay::{LabelElement, LabelTone};

        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let card = build_training_label_card_text(
            "動作貓·整理代理站位與狀態動畫",
            12.0,
            LabelTone::Active,
        );
        assert_eq!(card.title_lines.len(), 1);
        assert!(card.title_lines[0].ends_with('…'));
        assert_eq!(card.source, "動作貓 · 工作中");
        let metrics = training_label_card_metrics(&card, 12.0);
        assert!((80..=122).contains(&metrics.width));
        assert!((28..=42).contains(&metrics.height));

        let background = Rgb {
            r: 0x52,
            g: 0x84,
            b: 0x78,
        };
        let ground = pack_xrgb(background);
        let (win_w, win_h, scale) = (220usize, 120usize, 1i32);
        let label = LabelElement {
            anchor_px: Point { x: 104, y: 96 },
            text: "動作貓·整理代理站位".into(),
            tone: LabelTone::Active,
            hovered: false,
            relation: None,
        };
        let mut sb = vec![ground; win_w * win_h];
        paint_map_labels_into_surface_with_font_px(
            &mut sb,
            win_w,
            win_h,
            std::slice::from_ref(&label),
            scale,
            12.0,
            theme,
            MapleMapId::ForestTraining,
        );

        let body = pack_xrgb(Rgb {
            r: crate::aa_text::blend_channel(
                background.r,
                TRAINING_CARD_BODY.r,
                TRAINING_CARD_BODY_COVERAGE,
            ),
            g: crate::aa_text::blend_channel(
                background.g,
                TRAINING_CARD_BODY.g,
                TRAINING_CARD_BODY_COVERAGE,
            ),
            b: crate::aa_text::blend_channel(
                background.b,
                TRAINING_CARD_BODY.b,
                TRAINING_CARD_BODY_COVERAGE,
            ),
        });
        assert!(
            sb.contains(&body),
            "the training title field is translucent"
        );
        assert!(
            !sb.contains(&pack_xrgb(MAPLE_CARD_COIN))
                && !sb.contains(&pack_xrgb(MAPLE_CARD_PERMIT)),
            "training cards do not inherit Free Market shop controls"
        );

        let anchor_x = label.anchor_px.x as i32 + FLOATING_SPRITE_W / 2;
        let prepared = build_training_label_card_text(&label.text, 12.0, label.tone);
        let metrics = training_label_card_metrics(&prepared, 12.0);
        let placed = place_label_cards(
            vec![LabelCardSpec {
                label_index: 0,
                anchor_x,
                anchor_y: label.anchor_px.y as i32,
                width: metrics.width,
                height: metrics.height,
            }],
            win_w as i32,
            TRAINING_CARD_HEAD_GAP,
        );
        let rect = placed[0].rect;
        assert_eq!(
            rect.bottom(),
            label.anchor_px.y as i32 - TRAINING_CARD_HEAD_GAP
        );
        for y in rect.bottom()..label.anchor_px.y as i32 {
            assert_eq!(
                sb[y as usize * win_w + anchor_x as usize],
                ground,
                "the status card keeps a clean two-pixel head gap"
            );
        }
    }

    #[test]
    fn training_child_status_names_its_root_task_without_losing_activity() {
        use pixtuoid_core::AgentId;
        use pixtuoid_scene::overlay::{AgentRelation, LabelTone};

        let relation = AgentRelation::Child {
            root_id: AgentId::from_parts("codex", "factory-root"),
            root_task: Some("盤點新廠".to_owned()),
            depth: 1,
        };
        let card = build_training_label_card_text_with_relation(
            "動作貓\u{b7}整理照片",
            12.0,
            LabelTone::Active,
            Some(&relation),
        );

        assert!(card.source.contains("子1"));
        assert!(card.source.contains("盤點新廠"));
        assert!(card.source.contains("工作中"));
    }

    #[test]
    fn eight_training_cards_do_not_collide_at_supported_window_sizes() {
        use pixtuoid_scene::layout::Bounds;
        use pixtuoid_scene::overlay::LabelTone;

        let labels = [
            "動作貓·巡查訓練場怪物",
            "雲朵兔·整理隊伍站位",
            "劍士熊·等待需求確認",
            "星光狐·同步角色戰鬥狀態",
            "素材鼠·檢查繁中字體",
            "測試犬·驗證浮動視窗",
            "魔法羊·追蹤子代理狀態",
            "弓箭鳥·整理地圖傳點",
        ];
        for (win_w, win_h) in [(540i32, 360i32), (720, 480), (960, 640)] {
            let viewport = Bounds {
                x: 0,
                y: 0,
                width: win_w as u16,
                height: win_h as u16,
            };
            let avatar_height = i32::from(
                pixtuoid_scene::market::MARKET_AVATAR_HEIGHT
                    .saturating_mul(pixtuoid_scene::market::market_sprite_scale(win_h as u16)),
            );
            let specs = pixtuoid_scene::training::training_slots(viewport, labels.len())
                .into_iter()
                .enumerate()
                .map(|(label_index, slot)| {
                    let card = build_training_label_card_text(
                        labels[label_index],
                        15.0,
                        LabelTone::Active,
                    );
                    let metrics = training_label_card_metrics(&card, 15.0);
                    LabelCardSpec {
                        label_index,
                        anchor_x: i32::from(slot.foot_px.x),
                        anchor_y: i32::from(slot.foot_px.y) - avatar_height,
                        width: metrics.width,
                        height: metrics.height,
                    }
                })
                .collect();
            let placed = place_label_cards(specs, win_w, TRAINING_CARD_HEAD_GAP);
            assert_eq!(placed.len(), labels.len());
            for (index, card) in placed.iter().enumerate() {
                assert!(card.rect.x >= LABEL_CARD_EDGE_MARGIN);
                assert!(card.rect.right() <= win_w - LABEL_CARD_EDGE_MARGIN);
                for other in placed.iter().skip(index + 1) {
                    assert!(
                        !card
                            .rect
                            .overlaps_with_gap(other.rect, LABEL_CARD_COLLISION_GAP),
                        "{win_w}x{win_h} cards {} and {} overlap",
                        card.label_index,
                        other.label_index
                    );
                }
            }
        }
    }

    #[test]
    fn maple_labels_match_the_free_market_personal_shop_signage() {
        use pixtuoid_scene::layout::Point;
        use pixtuoid_scene::overlay::{LabelElement, LabelTone};

        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let background = Rgb {
            r: 0x20,
            g: 0x30,
            b: 0x40,
        };
        let ground = pack_xrgb(background);
        let (win_w, win_h, scale) = (280usize, 160usize, 2i32);
        let label = LabelElement {
            anchor_px: Point { x: 55, y: 55 },
            text: "素材狐\u{b7}測試代理商店".into(),
            tone: LabelTone::Active,
            hovered: false,
            relation: None,
        };
        let mut sb = vec![ground; win_w * win_h];
        paint_labels_into_surface_with_font_px(
            &mut sb,
            win_w,
            win_h,
            std::slice::from_ref(&label),
            scale,
            15.0,
            theme,
        );

        let blend_over_ground = |foreground: Rgb, coverage: f32| {
            pack_xrgb(Rgb {
                r: crate::aa_text::blend_channel(background.r, foreground.r, coverage),
                g: crate::aa_text::blend_channel(background.g, foreground.g, coverage),
                b: crate::aa_text::blend_channel(background.b, foreground.b, coverage),
            })
        };
        let shop_body = blend_over_ground(
            Rgb {
                r: 0xff,
                g: 0xfd,
                b: 0xf4,
            },
            0.88,
        );
        let shop_strip = blend_over_ground(
            Rgb {
                r: 0xe7,
                g: 0xf3,
                b: 0xf8,
            },
            0.88,
        );
        let border = 0x0048_5a6a;
        let title_ink = 0x0022_2d36;
        let coin = 0x00d4_941e;
        let permit = 0x00c6_b8d3;
        let action_blue = 0x004c_a4cf;
        let inactive_ring = 0x00e2_d6e4;
        let folded_corner = 0x00dc_6d55;
        let info_ink = 0x0043_5663;
        let status = pack_xrgb(pixtuoid_scene::overlay::label_tone_rgb(label.tone, theme));
        let locate = |color| {
            sb.iter()
                .enumerate()
                .filter(|(_, pixel)| **pixel == color)
                .map(|(index, _)| ((index % win_w) as i32, (index / win_w) as i32))
                .collect::<Vec<_>>()
        };
        let body_pixels = locate(shop_body);
        let strip_pixels = locate(shop_strip);
        let border_pixels = locate(border);
        let title_pixels = locate(title_ink);
        let coin_pixels = locate(coin);
        let permit_pixels = locate(permit);
        let action_pixels = locate(action_blue);
        let inactive_pixels = locate(inactive_ring);
        let folded_corner_pixels = locate(folded_corner);
        let info_ink_pixels = locate(info_ink);
        let status_pixels = locate(status);

        assert!(
            body_pixels.len() >= 20,
            "the shop title sits on a broad near-white translucent field"
        );
        assert!(
            strip_pixels.len() >= 12,
            "the player id and controls sit on a pale icy-blue shop strip"
        );
        assert!(
            !border_pixels.is_empty(),
            "a dark one-pixel outline separates the shop card from the scene"
        );
        assert_ne!(
            shop_body, 0x00ff_fdf4,
            "the white shop body must blend with, rather than replace, the scene"
        );
        assert!(
            !coin_pixels.is_empty(),
            "the lower strip includes a Maple-style coin glyph"
        );
        assert!(
            !permit_pixels.is_empty(),
            "the second shop slot includes the pale personal-store permit glyph"
        );
        assert!(
            !action_pixels.is_empty() && !inactive_pixels.is_empty(),
            "the third and fourth slots reproduce the blue and pale-pink game controls"
        );
        assert!(
            !folded_corner_pixels.is_empty(),
            "the title field keeps the tiny red folded-corner marker seen in the game"
        );

        let frame_pixels = body_pixels
            .iter()
            .chain(&strip_pixels)
            .chain(&border_pixels)
            .copied()
            .collect::<Vec<_>>();
        let min_x = frame_pixels.iter().map(|(x, _)| *x).min().unwrap();
        let max_x = frame_pixels.iter().map(|(x, _)| *x).max().unwrap();
        let min_y = frame_pixels.iter().map(|(_, y)| *y).min().unwrap();
        let max_y = frame_pixels.iter().map(|(_, y)| *y).max().unwrap();
        let anchor_x = label.anchor_px.x as i32 * scale + (FLOATING_SPRITE_W * scale) / 2;
        let anchor_y = label.anchor_px.y as i32 * scale;
        assert_eq!(
            max_y,
            anchor_y - 3,
            "the frame bottom leaves only a two-pixel breathing space over the head"
        );
        assert!(
            (100..145).contains(&(max_x - min_x + 1)),
            "the shop keeps the wider fixed sign proportion used by in-game personal stores"
        );
        assert!(
            (48..66).contains(&(max_y - min_y + 1)),
            "the title field reserves two game-like rows above the control strip"
        );
        assert_eq!(
            border_pixels.iter().filter(|(_, y)| *y == min_y).count() as i32,
            max_x - min_x + 1,
            "the dark outline is exactly the full top edge"
        );
        assert!(
            strip_pixels.iter().all(|(_, y)| *y > min_y),
            "the icy-blue strip is confined below the primary shop title"
        );
        let strip_top = strip_pixels.iter().map(|(_, y)| *y).min().unwrap();
        assert!(
            info_ink_pixels.iter().all(|(_, y)| *y < strip_top),
            "the footer is icon-only; the fake player id belongs on the stall plaque"
        );
        assert!(
            !title_pixels.is_empty(),
            "full-strength CJK title strokes stay crisp on the white field"
        );
        let title_raster_pixels = ((min_y + 2)..strip_top)
            .flat_map(|y| ((min_x + 1)..max_x).map(move |x| (x, y)))
            .filter(|(x, y)| sb[*y as usize * win_w + *x as usize] != shop_body)
            .collect::<Vec<_>>();
        assert!(
            !title_raster_pixels.is_empty(),
            "anti-aliased CJK title strokes rasterize inside the shop body"
        );
        let title_min_x = title_raster_pixels.iter().map(|(x, _)| *x).min().unwrap();
        assert!(
            title_min_x - min_x <= 10,
            "the shop title is left-aligned like the in-game personal-shop sign: frame_x={min_x}, raster_x={title_min_x}"
        );
        assert!(
            status_pixels
                .iter()
                .any(|(x, y)| *x >= max_x - 24 && *y >= max_y - 18),
            "the agent state lives in the right-side shop control cell"
        );
        for y in (max_y + 1)..anchor_y {
            assert_eq!(
                sb[y as usize * win_w + anchor_x as usize],
                ground,
                "Maple shop signs float cleanly without a speech-bubble leader"
            );
        }
        let mut latin = vec![ground; win_w * win_h];
        let latin_label = LabelElement {
            text: "cx\u{b7}Agent".into(),
            ..label
        };
        paint_labels_into_surface_with_font_px(
            &mut latin,
            win_w,
            win_h,
            std::slice::from_ref(&latin_label),
            scale,
            15.0,
            theme,
        );
        assert!(
            !latin.contains(&title_ink),
            "Latin task strokes use a lighter coverage than the CJK title weight"
        );
    }

    #[test]
    fn card_ink_uses_one_crisp_raster_pass_for_latin_and_cjk_text() {
        // Regression: on 2026-08-03 a shifted second CJK pass made live task
        // names visibly blurry at the floating window's small label sizes.
        let render_plain = |text: &str| {
            let mut sb = vec![0u32; 120 * 40];
            crate::aa_text::draw_label_text_at(text, 5, 5, 16.0, |x, y, cov| {
                blend_xrgb(&mut sb, 120, 40, x, y, 0x00f0_f0f0, cov)
            });
            sb
        };
        let render_card = |text: &str| {
            let mut sb = vec![0u32; 120 * 40];
            LabelCardCanvas::new(&mut sb, 120, 40).draw_ink(text, (5, 5), 16.0, 0x00f0_f0f0);
            sb
        };

        assert_eq!(
            render_card("Agent"),
            render_plain("Agent"),
            "Latin task text keeps the font's native weight and raster"
        );
        assert_eq!(
            render_card("中文"),
            render_plain("中文"),
            "CJK task text must not get a shifted duplicate stroke that blurs small glyphs"
        );
    }

    #[test]
    fn paint_labels_split_the_status_dot_tone_from_the_cli_name_hue() {
        // #657 owner-ratified split: the ● dot keeps the activity tone while the
        // NAME paints in the source's by_prefix badge hue. A registered prefix
        // (`cc·`) exercises the `Some(hue)` arm the tone-only tests above skip.
        use pixtuoid_scene::layout::Point;
        use pixtuoid_scene::overlay::{LabelElement, LabelTone};
        let theme = office_test_theme();
        let as_u32 = |c: Rgb| (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32;
        // Idle grey dot vs the cc badge hue — deliberately distinct colors, so
        // "both present" proves a genuine split, not one color bleeding into both.
        let tone_rgb = theme.ui.label_idle;
        let name_rgb = theme.source.claude_code;
        assert_ne!(tone_rgb, name_rgb, "premise: idle tone != cc badge hue");
        let label = vec![LabelElement {
            anchor_px: Point { x: 20, y: 20 },
            text: "cc\u{b7}api".into(),
            tone: LabelTone::Idle,
            hovered: false,
            relation: None,
        }];
        let mut sb = vec![0u32; 120 * 120];
        paint_labels_into_surface(&mut sb, 120, 120, &label, 2, theme);
        assert!(
            sb.contains(&as_u32(tone_rgb)),
            "the ● dot must paint the activity tone {tone_rgb:?}"
        );
        assert!(
            sb.contains(&as_u32(name_rgb)),
            "the name must paint the cc badge hue {name_rgb:?}"
        );
    }

    #[test]
    fn paint_labels_do_not_render_cjk_task_names_as_primary_tofu() {
        use pixtuoid_scene::layout::Point;
        use pixtuoid_scene::overlay::{LabelElement, LabelTone};

        #[cfg(not(windows))]
        if std::env::var_os("PIXTUOID_CJK_FONT").is_none() {
            return; // portable CI images are not required to ship a CJK face
        }

        let theme = office_test_theme();
        let (win_w, win_h, scale) = (240usize, 80usize, 2i32);
        let label = LabelElement {
            anchor_px: Point { x: 50, y: 25 },
            text: "zz·尋".into(),
            tone: LabelTone::Idle,
            hovered: false,
            relation: None,
        };

        let mut rendered = vec![0u32; win_w * win_h];
        paint_labels_into_surface(
            &mut rendered,
            win_w,
            win_h,
            std::slice::from_ref(&label),
            scale,
            theme,
        );

        // Independent old-path oracle: render the same badge through the bundled
        // primary face only. A correctly wired CJK fallback must differ from this
        // tofu rendering in glyph shape and/or centering.
        let text = format!("●{}", label.text);
        let tw = crate::aa_text::text_width(&text, LABEL_FONT_PX);
        let x = label.anchor_px.x as i32 * scale + (FLOATING_SPRITE_W * scale) / 2 - tw / 2;
        let y = label.anchor_px.y as i32 * scale - 12;
        let color = pack_xrgb(pixtuoid_scene::overlay::label_tone_rgb(label.tone, theme));
        let mut primary_only = vec![0u32; win_w * win_h];
        crate::aa_text::draw_text_at(&text, x + 1, y + 1, LABEL_FONT_PX, |gx, gy, cov| {
            blend_xrgb(&mut primary_only, win_w, win_h, gx, gy, BADGE_SHADOW, cov)
        });
        crate::aa_text::draw_text_at(&text, x, y, LABEL_FONT_PX, |gx, gy, cov| {
            blend_xrgb(&mut primary_only, win_w, win_h, gx, gy, color, cov)
        });

        assert!(
            rendered != primary_only,
            "configured CJK task text must not use the bundled face's .notdef glyph"
        );
    }

    #[test]
    fn paint_labels_render_antialiased_partial_coverage_not_binary_pixels() {
        use pixtuoid_scene::layout::Point;
        use pixtuoid_scene::overlay::{LabelElement, LabelTone};
        let theme = office_test_theme();
        // Paint over a WHITE ground: an AA glyph's edges emit partial coverage, so
        // some pixels land STRICTLY between white and any fully-lit ink — the exact
        // thing the old all-or-nothing 8×8 bitmap font could never produce.
        let white = 0x00FF_FFFFu32;
        let mut sb = vec![white; 200 * 60];
        let badge = vec![LabelElement {
            anchor_px: Point { x: 20, y: 20 },
            text: "active".into(),
            tone: LabelTone::Active,
            hovered: false,
            relation: None,
        }];
        paint_labels_into_surface(&mut sb, 200, 60, &badge, 2, theme);
        let ink = pack_xrgb(theme.ui.label_active);
        let shadow = 0x0000_0000u32;
        let intermediate = sb.iter().any(|&p| p != white && p != ink && p != shadow);
        assert!(
            intermediate,
            "AA text must blend edge pixels between the ground and the ink"
        );
        // And a fully-covered stroke interior still reaches the exact tone color.
        assert!(
            sb.contains(&ink),
            "glyph interior reaches full-coverage tone color"
        );
    }

    #[test]
    fn wall_board_paints_brand_and_mood_tones_into_the_panel() {
        let theme = office_test_theme();
        // 2 work + 1 wait + 1 idle, a busy gateway → the board carries the brand, a
        // ●work mood segment, and the ⬢gw chip. Rendered at a generous scale so the
        // full-coverage stroke interiors reach the exact tone colors.
        let counts = pixtuoid_scene::board::StateCounts {
            active: 2,
            waiting: 1,
            idle: 1,
            exiting: 0,
            total: 4,
        };
        let board = pixtuoid_scene::board::build_board(counts, 90, None, None);
        let scale = 8i32;
        let (w, h) = (320usize, 96usize);
        let mut sb = vec![0u32; w * h];
        paint_wall_board_into_surface(&mut sb, w, h, &board, scale, theme);
        assert!(
            sb.contains(&pack_xrgb(theme.ui.neon_brand)),
            "L1 brand paints the neon-brand hue"
        );
        assert!(
            sb.contains(&pack_xrgb(theme.ui.label_active)),
            "the ● work mood segment paints the active hue"
        );
        // Below the min row size the board leaves the panel empty (no mush).
        let mut tiny = vec![0u32; w * h];
        paint_wall_board_into_surface(&mut tiny, w, h, &board, 1, theme);
        assert!(
            tiny.iter().all(|&p| p == 0),
            "a scale-1 scene suppresses the sub-legible board"
        );
    }

    #[test]
    fn maple_market_suppresses_the_scene_wall_board_overlay() {
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let board = pixtuoid_scene::board::build_board(
            pixtuoid_scene::board::StateCounts {
                active: 2,
                waiting: 1,
                idle: 0,
                exiting: 0,
                total: 3,
            },
            90,
            None,
            None,
        );
        let sentinel = 0x0012_3456;
        let mut surface = vec![sentinel; 320 * 96];
        paint_wall_board_into_surface(&mut surface, 320, 96, &board, 8, theme);
        assert!(
            surface.iter().all(|pixel| *pixel == sentinel),
            "scene telemetry must not float over the Free Market sky"
        );
    }

    /// Build an active slot for floating-renderer tests.
    fn active_on(path: &str, floor_idx: usize, desk: usize) -> pixtuoid_core::state::AgentSlot {
        use pixtuoid_core::state::{ActivityState, AgentSlot, GlobalDeskIndex, ToolKind};
        use std::sync::Arc;
        let started = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        AgentSlot {
            agent_id: pixtuoid_core::AgentId::from_transcript_path(path),
            source: Arc::from("cc"),
            session_id: Arc::from("s"),
            cwd: Arc::from(std::path::Path::new("/repo")),
            label: "a".into(),
            state: ActivityState::Active {
                tool_use_id: Some(Arc::from("t")),
                detail: Some(Arc::from("Edit")),
                kind: ToolKind::from_display("Edit"),
            },
            state_started_at: started,
            created_at: started,
            last_event_at: started,
            exiting_at: None,
            pending_idle_at: None,
            desk_index: GlobalDeskIndex(desk),
            floor_idx,
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

    fn scene_with(agents: Vec<pixtuoid_core::state::AgentSlot>, cap: usize) -> SceneState {
        let mut s = SceneState::uniform(cap);
        for a in agents {
            s.agents.insert(a.agent_id, a);
        }
        s
    }

    #[test]
    fn snapshot_dual_map_split_assigns_each_party_once() {
        use pixtuoid_scene::maple_world::MapleMapId;

        let mut agents = (0..4)
            .map(|index| active_on(&format!("/snapshot/agent-{index}.jsonl"), 0, index))
            .collect::<Vec<_>>();
        agents.sort_by_key(|agent| agent.agent_id);
        agents[1].parent_id = Some(agents[0].agent_id);
        agents[3].parent_id = Some(agents[2].agent_id);
        let scene = scene_with(agents, 8);

        let mut renderer = MapleRenderer::new();
        renderer.assign_snapshot_scene_across_maps(&scene);

        assert_eq!(
            renderer.maple_world.agents_on(MapleMapId::FreeMarket).len(),
            2
        );
        assert_eq!(
            renderer
                .maple_world
                .agents_on(MapleMapId::ForestTraining)
                .len(),
            2
        );
    }

    #[test]
    fn floating_stems_count_only_the_rendered_floor() {
        // One active on the rendered ground floor versus three on floor 1 must
        // read MODERATE typing,
        // not the BUSY a global count would produce.
        let cap = 16;
        let scene = scene_with(
            vec![
                active_on("/a/f0.jsonl", 0, 0),
                active_on("/a/f1a.jsonl", 1, cap),
                active_on("/a/f1b.jsonl", 1, cap + 1),
                active_on("/a/f1c.jsonl", 1, cap + 2),
            ],
            cap,
        );
        let pack =
            pixtuoid_scene::embedded_pack::load_sprite_pack(None).expect("embedded pack loads");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut renderer = MapleRenderer::new();
        let (handle, rx) = crate::audio::AudioHandle::test_pair();
        renderer.set_audio(handle);
        renderer.render(&scene, &pack, theme, now, 160, 96, FloorMeta::ground());
        let frames = crate::audio::drain_frames(&rx);
        assert!(!frames.is_empty(), "an enabled handle receives frames");
        let stems = frames.last().unwrap().stems;
        let moderate = pixtuoid_scene::audio::stem_levels(
            &pixtuoid_scene::board::StateCounts {
                active: 1,
                waiting: 0,
                idle: 0,
                exiting: 0,
                total: 1,
            },
            0.0,
        );
        assert_eq!(
            stems.typing, moderate.typing,
            "typing level must reflect the RENDERED floor's 1 active, not all 4"
        );
    }

    #[test]
    fn paint_footer_blits_into_the_bottom_band_and_tones_via_the_shared_authority() {
        // The floating footer renders the shared build_footer model into the
        // bottom band, toned through footer_tone_rgb. The pure tier/policy is pinned in
        // scene::footer; this pins the blit region + the tone routing — the
        // phantom-feedback twin of the label/volume blit tests it replaces.
        use pixtuoid_scene::board::{per_floor_counts, scene_stats};
        use pixtuoid_scene::footer::{FooterTone, RungKind};
        let theme = office_test_theme();
        let mut scene = SceneState::new([8; pixtuoid_core::state::MAX_FLOORS]);
        let slot = active_on("/p/a.jsonl", 0, 0);
        scene.agents.insert(slot.agent_id, slot);
        let per_floor = per_floor_counts(&scene);
        let tools = footer_tool_tally(&scene);
        let inputs = FooterInputs {
            counts: scene_stats(&scene),
            per_floor: &per_floor,
            gateway: None,
            floor: None,
            tools: &tools,
            audio_audible: true,
            volume_flash: None,
            source_warning: None,
            keys_stats: FOOTER_KEYS,
            keys_alert: FOOTER_KEYS,
        };
        let (w, h) = (400usize, 160usize);
        let model = build_footer(&inputs, footer_budget(w));
        let mut sb = vec![0u32; w * h];
        paint_footer_into_surface(&mut sb, w, h, &model, theme);
        let changed: Vec<usize> = sb
            .iter()
            .enumerate()
            .filter(|(_, p)| **p != 0)
            .map(|(i, _)| i)
            .collect();
        assert!(!changed.is_empty(), "the footer painted something");
        assert!(
            changed.iter().all(|&i| i / w >= h / 2),
            "the footer stays in the bottom band"
        );
        // The ●A rung tones via the shared authority.
        assert!(
            sb.contains(&pack_xrgb(footer_tone_rgb(
                FooterTone::Rung(RungKind::Active),
                theme
            ))),
            "the ●A rung paints the shared label_active hue"
        );
    }

    #[test]
    fn maple_footer_adds_a_translucent_market_hud_band_without_insetting_the_scene() {
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let scene = SceneState::uniform(8);
        let renderer = MapleRenderer::new();
        let (w, h) = (320usize, 120usize);
        let model = renderer.footer(&scene, footer_budget(w), false, None);
        let ground = 0x0080_90a0;
        let mut surface = vec![ground; w * h];

        paint_footer_into_surface(&mut surface, w, h, &model, theme);

        let font_px = maple_chat_font_px(h);
        let line_height = crate::aa_text::line_height(font_px).max(12);
        let bar_height = (line_height + 10).min(h as i32);
        let bar_top = (h as i32 - MAPLE_CHAT_MARGIN_BOTTOM - bar_height).max(0) as usize;
        assert_eq!(
            surface[(bar_top - 1) * w + w / 2],
            ground,
            "the Maple chat strip stays a bottom overlay and does not repaint the scene above it"
        );
        assert_eq!(
            surface[bar_top * w + MAPLE_CHAT_MARGIN_X as usize],
            pack_xrgb(MAPLE_CHAT_BORDER),
            "the chat strip has the game's dark outlined frame"
        );
        assert!(
            surface.contains(&pack_xrgb(MAPLE_CHAT_CHANNEL)),
            "the left-side agent channel tab is visible"
        );
        assert!(
            surface.contains(&pack_xrgb(MAPLE_CHAT_BUTTON)),
            "the right-side audio controls use Maple-like blue square buttons"
        );
        assert_eq!(
            surface[(h - 1) * w + w / 2],
            ground,
            "the inset bar preserves the scene under its small bottom margin"
        );
    }

    #[test]
    fn maple_footer_translates_exec_and_waiting_into_player_facing_traditional_chinese() {
        use pixtuoid_core::state::ToolKind;
        use pixtuoid_scene::footer::FooterSegment;

        let model = FooterModel {
            segments: vec![
                FooterSegment {
                    text: " 3".into(),
                    tone: FooterTone::Neutral,
                },
                FooterSegment {
                    text: " · ".into(),
                    tone: FooterTone::Neutral,
                },
                FooterSegment {
                    text: "●2 A".into(),
                    tone: FooterTone::Rung(RungKind::Active),
                },
                FooterSegment {
                    text: "◐1 W".into(),
                    tone: FooterTone::Rung(RungKind::Waiting),
                },
                FooterSegment {
                    text: "Exec×2".into(),
                    tone: FooterTone::Tool(ToolKind::Other),
                },
                FooterSegment {
                    text: " ♩ 65% [m]ute [+/-]vol ".into(),
                    tone: FooterTone::Neutral,
                },
            ],
        };

        let summary = maple_footer_summary(&model);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.audio_label.as_deref(), Some("音量 65%"));
        assert_eq!(
            summary
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<Vec<_>>(),
            vec!["等待回覆 1", "執行中 2", "執行指令 2"]
        );
        assert!(
            summary
                .runs
                .iter()
                .all(|run| !run.text.to_ascii_lowercase().contains("waiting")
                    && !run.text.to_ascii_lowercase().contains("exec")),
            "raw monitor tokens never leak into the Maple player-facing strip"
        );
    }

    #[test]
    fn floating_appliance_cues_fire_from_the_sessions_occupancy() {
        // The tracker receives the session's occupied_waypoints plus this
        // frame's waypoint kinds, so a
        // wanderer standing at the printer / vending machine fires the
        // appliance one-shot in the floating window too. Deterministic —
        // fixed agent id + a hand-stepped clock; the loop bound mirrors the
        // scene crate's occupancy sim pin.
        use pixtuoid_scene::audio::OneShot;
        let pack =
            pixtuoid_scene::embedded_pack::load_sprite_pack(None).expect("embedded pack loads");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let now0 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut idle = active_on("/w/wanderer.jsonl", 0, 0);
        idle.state = pixtuoid_core::state::ActivityState::Idle;
        let scene = scene_with(vec![idle], 16);
        let mut renderer = MapleRenderer::new();
        let (handle, rx) = crate::audio::AudioHandle::test_pair();
        renderer.set_audio(handle);
        let mut heard = Vec::new();
        for step in 0..900u64 {
            let now = now0 + std::time::Duration::from_secs(2 * step);
            // 192x160: tall enough that the corridor hosts BOTH appliances
            // (the vending/printer height gates in layout::compute).
            renderer.render(&scene, &pack, theme, now, 192, 160, FloorMeta::ground());
            heard.extend(
                crate::audio::drain_frames(&rx)
                    .into_iter()
                    .flat_map(|f| f.events),
            );
            if heard
                .iter()
                .any(|e| matches!(e, OneShot::PrinterWhir | OneShot::VendingDrop))
            {
                break;
            }
        }
        assert!(
            heard
                .iter()
                .any(|e| matches!(e, OneShot::PrinterWhir | OneShot::VendingDrop)),
            "a wander through the appliance strip must fire a printer/vending cue; heard: {heard:?}"
        );
    }

    #[test]
    fn floating_door_chime_fires_only_for_rendered_floor_arrivals() {
        let cap = 16;
        let pack =
            pixtuoid_scene::embedded_pack::load_sprite_pack(None).expect("embedded pack loads");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let mut now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut renderer = MapleRenderer::new();
        let (handle, rx) = crate::audio::AudioHandle::test_pair();
        renderer.set_audio(handle);

        let mut agents = vec![active_on("/d/f0.jsonl", 0, 0)];
        let scene = scene_with(agents.clone(), cap);
        renderer.render(&scene, &pack, theme, now, 160, 96, FloorMeta::ground());
        crate::audio::drain_frames(&rx); // discard the priming frames

        // an arrival on ANOTHER floor: silent in the ground-floor window
        agents.push(active_on("/d/f1-new.jsonl", 1, cap));
        let scene = scene_with(agents.clone(), cap);
        now += std::time::Duration::from_millis(33);
        renderer.render(&scene, &pack, theme, now, 160, 96, FloorMeta::ground());
        let off_floor: Vec<_> = crate::audio::drain_frames(&rx)
            .into_iter()
            .flat_map(|f| f.events)
            .collect();
        assert!(
            off_floor.is_empty(),
            "a floor-1 walk-in must not chime the ground-floor window: {off_floor:?}"
        );

        // an arrival on the rendered floor chimes
        agents.push(active_on("/d/f0-new.jsonl", 0, 1));
        let scene = scene_with(agents, cap);
        now += std::time::Duration::from_millis(33);
        renderer.render(&scene, &pack, theme, now, 160, 96, FloorMeta::ground());
        let on_floor: Vec<_> = crate::audio::drain_frames(&rx)
            .into_iter()
            .flat_map(|f| f.events)
            .collect();
        assert!(
            on_floor.contains(&pixtuoid_scene::audio::OneShot::DoorChime),
            "a ground-floor walk-in must chime the floating window: {on_floor:?}"
        );
    }

    #[test]
    fn maple_labels_are_empty_before_render_then_follow_a_settled_agent() {
        use pixtuoid_core::source::AgentEvent;
        use pixtuoid_core::{AgentId, Reducer, Transport};
        let pack =
            pixtuoid_scene::embedded_pack::load_sprite_pack(None).expect("embedded pack loads");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut renderer = MapleRenderer::new();
        renderer.set_maple_map(pixtuoid_scene::maple_world::MapleMapId::FreeMarket);

        // One real agent, seeded through the production reducer path.
        let mut scene = SceneState::new([8; pixtuoid_core::state::MAX_FLOORS]);
        let mut reducer = Reducer::new();
        let agent_id = AgentId::from_parts("claude-code", "offscreen-labels-test");
        reducer.apply(
            &mut scene,
            AgentEvent::SessionStart {
                agent_id,
                source: "claude-code".to_string(),
                session_id: "offscreen-labels-test".to_string(),
                cwd: std::path::PathBuf::from("/home/user/demo-project"),
                parent_id: None,
            },
            now,
            Transport::Jsonl,
        );
        reducer.apply(
            &mut scene,
            AgentEvent::ActivityStart {
                agent_id,
                tool_use_id: Some("render-maple-label".to_owned()),
                detail: None,
            },
            now,
            Transport::Jsonl,
        );
        renderer.assign_snapshot_scene_to_map(
            &scene,
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket,
        );

        // No frame rendered yet means no captured Maple viewport.
        assert!(renderer.labels(&scene, now).is_empty());
        // Let the production entry route settle, then require the Maple market
        // overlay to attach exactly one card inside its captured viewport.
        let settled = now + std::time::Duration::from_millis(10_000);
        renderer.render(&scene, &pack, theme, settled, 640, 240, FloorMeta::ground());
        let labels = renderer.labels(&scene, settled);
        assert_eq!(labels.len(), 1, "one seeded agent → one name badge");
        let anchor = labels[0].anchor_px;
        assert!(
            (0..640).contains(&(anchor.x as i32)) && (0..240).contains(&(anchor.y as i32)),
            "badge anchor {anchor:?} lands inside the rendered scene buffer"
        );
    }

    #[test]
    fn maple_renderer_shop_card_attaches_to_a_complete_paperdoll_set() {
        const PACK_TOML: &str = r##"
[pack]
name = "market-paperdoll-overlay-test"
version = "0.0.0"

[palette]
"B" = "#101112"
"H" = "#202122"
"S" = "#303132"
"P" = "#404142"
"X" = "#77aadd"
"A" = "#cc8844"

[animations.standing]
frames = ["standing.sprite"]
frame_ms = 1000

[animations.market_avatar]
frames = [
  "avatar_0.sprite", "avatar_1.sprite", "avatar_2.sprite", "avatar_3.sprite",
  "avatar_4.sprite", "avatar_5.sprite", "avatar_6.sprite", "avatar_7.sprite",
]
frame_ms = 1000

[animations.scene_background]
frames = ["scene_background.sprite"]
frame_ms = 1000
"##;
        let row = std::iter::repeat_n("A", pixtuoid_scene::market::MARKET_AVATAR_WIDTH as usize)
            .collect::<Vec<_>>()
            .join(" ");
        let avatar = format!(
            "@frame 0\n{}\n",
            std::iter::repeat_n(row, pixtuoid_scene::market::MARKET_AVATAR_HEIGHT as usize,)
                .collect::<Vec<_>>()
                .join("\n")
        );
        let avatar_names = [
            "avatar_0.sprite",
            "avatar_1.sprite",
            "avatar_2.sprite",
            "avatar_3.sprite",
            "avatar_4.sprite",
            "avatar_5.sprite",
            "avatar_6.sprite",
            "avatar_7.sprite",
        ];
        let mut sources = vec![
            ("standing.sprite", "@frame 0\nB\n"),
            ("scene_background.sprite", "@frame 0\nX\n"),
        ];
        sources.extend(avatar_names.map(|name| (name, avatar.as_str())));
        let pack = pixtuoid_core::sprite::format::load_pack_from_strings(PACK_TOML, &sources)
            .expect("paperdoll market pack");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let settled = now
            + std::time::Duration::from_millis(u64::from(
                pixtuoid_scene::market::MARKET_MAX_ENTRY_MS,
            ));
        let agent = active_on("/market/paperdoll.jsonl", 0, 0);
        let scene = scene_with(vec![agent], 8);
        let mut renderer = MapleRenderer::new();
        renderer.assign_snapshot_scene_to_map(
            &scene,
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket,
        );

        renderer.render(&scene, &pack, theme, now, 240, 160, FloorMeta::ground());
        assert!(
            renderer.market_player_ids(&scene, now).is_empty(),
            "the stall nameplate stays empty while its merchant is still walking in"
        );

        renderer.render(&scene, &pack, theme, settled, 240, 160, FloorMeta::ground());
        let labels = renderer.labels(&scene, settled);
        let player_ids = renderer.market_player_ids(&scene, settled);
        assert_eq!(labels.len(), 1);
        assert_eq!(player_ids.len(), 1);
        assert_eq!(player_ids[0].text, "素材狐");
        assert_eq!(player_ids[0].anchor_px.x, labels[0].anchor_px.x);
        assert_eq!(
            player_ids[0].anchor_px.y,
            140,
            "the fake player id starts inside the authored stall nameplate, not at the moving foot line"
        );
        assert_eq!(
            labels[0].anchor_px,
            pixtuoid_scene::layout::Point { x: 120, y: 115 },
            "the floating shop card must centre directly over the 32x24 paperdoll head"
        );
    }

    #[test]
    fn maple_renderer_labels_use_the_same_market_slot_as_the_merchant() {
        const PACK_TOML: &str = r##"
[pack]
name = "market-overlay-test"
version = "0.0.0"

[palette]
"B" = "#101112"
"H" = "#202122"
"S" = "#303132"
"P" = "#404142"
"X" = "#77aadd"

[animations.standing]
frames = ["standing.sprite"]
frame_ms = 1000

[animations.scene_background]
frames = ["scene_background.sprite"]
frame_ms = 1000
"##;
        let pack = pixtuoid_core::sprite::format::load_pack_from_strings(
            PACK_TOML,
            &[
                ("standing.sprite", "@frame 0\nB\n"),
                ("scene_background.sprite", "@frame 0\nX\n"),
            ],
        )
        .expect("minimal market pack");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let settled = now
            + std::time::Duration::from_millis(u64::from(
                pixtuoid_scene::market::MARKET_MAX_ENTRY_MS,
            ));
        let mut agent = active_on("/market/merchant.jsonl", 0, 0);
        agent.label = "cx\u{b7}繪製自由市場".into();
        let scene = scene_with(vec![agent], 8);
        let mut renderer = MapleRenderer::new();
        renderer.assign_snapshot_scene_to_map(
            &scene,
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket,
        );

        renderer.render(&scene, &pack, theme, settled, 240, 160, FloorMeta::ground());
        let labels = renderer.labels(&scene, settled);
        assert_eq!(labels.len(), 1);
        assert_eq!(
            labels[0].anchor_px,
            pixtuoid_scene::layout::Point { x: 120, y: 115 },
            "the first shop card must attach to the original procedural actor's Free Market anchor"
        );
        assert_eq!(labels[0].text, "素材狐\u{b7}繪製自由市場");
    }

    #[test]
    fn maple_map_switch_changes_pixels_without_mutating_the_live_scene() {
        const PACK_TOML: &str = r##"
[pack]
name = "two-map-test"
version = "0.0.0"

[palette]
"B" = "#101112"
"M" = "#335577"
"T" = "#77aa44"

[animations.standing]
frames = ["standing.sprite"]
frame_ms = 1000

[animations.scene_background]
frames = ["scene_background.sprite"]
frame_ms = 1000

[animations.training_background]
frames = ["training_background.sprite"]
frame_ms = 1000
"##;
        let pack = pixtuoid_core::sprite::format::load_pack_from_strings(
            PACK_TOML,
            &[
                ("standing.sprite", "@frame 0\nB\n"),
                ("scene_background.sprite", "@frame 0\nM\n"),
                ("training_background.sprite", "@frame 0\nT\n"),
            ],
        )
        .expect("minimal two-map pack");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let scene = SceneState::uniform(8);
        let before = serde_json::to_value(&scene).expect("serialize scene before render");
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut renderer = MapleRenderer::new();
        renderer.set_maple_map(pixtuoid_scene::maple_world::MapleMapId::FreeMarket);

        let market_pixel = renderer
            .render(&scene, &pack, theme, now, 240, 160, FloorMeta::ground())
            .get(0, 0);
        assert_eq!(
            renderer.current_map(),
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket
        );
        renderer.cycle_map();
        let training_pixel = renderer
            .render(&scene, &pack, theme, now, 240, 160, FloorMeta::ground())
            .get(0, 0);

        assert_eq!(
            renderer.current_map(),
            pixtuoid_scene::maple_world::MapleMapId::ForestTraining
        );
        assert_ne!(market_pixel, training_pixel);
        assert!(renderer.market_player_ids(&scene, now).is_empty());
        assert_eq!(
            serde_json::to_value(&scene).expect("serialize scene after render"),
            before,
            "presentation routing is read-only"
        );
    }

    #[test]
    fn maple_dual_view_renders_both_full_scene_plates_in_one_buffer() {
        const PACK_TOML: &str = r##"
[pack]
name = "two-map-dual-test"
version = "0.0.0"

[palette]
"B" = "#101112"
"M" = "#335577"
"T" = "#77aa44"

[animations.standing]
frames = ["standing.sprite"]
frame_ms = 1000

[animations.scene_background]
frames = ["scene_background.sprite"]
frame_ms = 1000

[animations.training_background]
frames = ["training_background.sprite"]
frame_ms = 1000
"##;
        let pack = pixtuoid_core::sprite::format::load_pack_from_strings(
            PACK_TOML,
            &[
                ("standing.sprite", "@frame 0\nB\n"),
                ("scene_background.sprite", "@frame 0\nM\n"),
                ("training_background.sprite", "@frame 0\nT\n"),
            ],
        )
        .expect("minimal two-map pack");
        let theme = pixtuoid_scene::theme::theme_by_name("maple").expect("maple theme exists");
        let scene = SceneState::uniform(8);
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut renderer = MapleRenderer::new();

        let (market_pixel, training_pixel) = {
            let frame = renderer.render(&scene, &pack, theme, now, 480, 160, FloorMeta::ground());
            (frame.get(0, 0), frame.get(479, 0))
        };

        assert_eq!(renderer.view_mode(), MapleViewMode::Dual);
        assert_eq!(
            market_pixel,
            pixtuoid_core::sprite::Rgb {
                r: 0x33,
                g: 0x55,
                b: 0x77
            }
        );
        assert_eq!(
            training_pixel,
            pixtuoid_core::sprite::Rgb {
                r: 0x77,
                g: 0xaa,
                b: 0x44
            }
        );
        assert!(renderer.market_viewport.is_some());
        assert!(renderer.training_viewport.is_some());
    }

    #[test]
    fn maple_tab_cycle_preserves_dual_and_both_single_map_views() {
        let mut renderer = MapleRenderer::new();
        renderer.dual_rendered = true;

        assert_eq!(renderer.view_mode(), MapleViewMode::Dual);
        assert!(renderer.cycle_map());
        assert_eq!(renderer.view_mode(), MapleViewMode::Single);
        assert_eq!(
            renderer.current_map(),
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket
        );
        assert!(renderer.cycle_map());
        assert_eq!(renderer.view_mode(), MapleViewMode::Single);
        assert_eq!(
            renderer.current_map(),
            pixtuoid_scene::maple_world::MapleMapId::ForestTraining
        );
        assert!(renderer.cycle_map());
        assert_eq!(renderer.view_mode(), MapleViewMode::Dual);
    }

    #[test]
    fn map_selector_stays_visible_and_names_dual_and_each_single_map() {
        let mut renderer = MapleRenderer::new();
        renderer.dual_rendered = true;

        assert!(renderer.map_selector_text().unwrap().contains("雙圖"));
        assert!(renderer.cycle_map());
        assert!(renderer.map_selector_text().unwrap().contains("自由市場"));
        assert!(renderer.cycle_map());
        assert!(renderer.map_selector_text().unwrap().contains("訓練場"));
        assert!(renderer.cycle_map());
        assert!(renderer.map_selector_text().unwrap().contains("雙圖"));
    }

    #[test]
    fn compact_dual_fallback_switches_visible_map_on_the_first_tab() {
        let mut renderer = MapleRenderer::new();

        assert_eq!(renderer.view_mode(), MapleViewMode::Dual);
        assert_eq!(
            renderer.current_map(),
            pixtuoid_scene::maple_world::MapleMapId::FreeMarket
        );
        assert!(renderer.cycle_map());
        assert_eq!(renderer.view_mode(), MapleViewMode::Dual);
        assert_eq!(
            renderer.current_map(),
            pixtuoid_scene::maple_world::MapleMapId::ForestTraining
        );
    }

    #[test]
    fn maple_panel_split_refuses_to_shrink_either_reference_plate() {
        let (market, training) = split_map_panels(481, 160).expect("odd widths remain valid");
        assert_eq!(
            market,
            Bounds {
                x: 0,
                y: 0,
                width: 240,
                height: 160
            }
        );
        assert_eq!(
            training,
            Bounds {
                x: 240,
                y: 0,
                width: 241,
                height: 160
            }
        );
        assert!(split_map_panels(479, 160).is_none());
        assert!(split_map_panels(480, 159).is_none());
    }

    #[test]
    fn panel_copy_never_writes_across_its_destination_bounds() {
        let black = pixtuoid_core::sprite::Rgb { r: 0, g: 0, b: 0 };
        let market = pixtuoid_core::sprite::Rgb { r: 1, g: 2, b: 3 };
        let training = pixtuoid_core::sprite::Rgb { r: 4, g: 5, b: 6 };
        let mut destination = RgbBuffer::filled(5, 2, black);
        let left = RgbBuffer::filled(2, 2, market);
        let right = RgbBuffer::filled(3, 2, training);

        copy_rgb_panel(
            &mut destination,
            &left,
            Bounds {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
        );
        copy_rgb_panel(
            &mut destination,
            &right,
            Bounds {
                x: 2,
                y: 0,
                width: 3,
                height: 2,
            },
        );

        for y in 0..2 {
            assert_eq!(destination.get(0, y), market);
            assert_eq!(destination.get(1, y), market);
            assert_eq!(destination.get(2, y), training);
            assert_eq!(destination.get(4, y), training);
        }
    }
}
