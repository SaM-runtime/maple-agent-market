//! The `winit` + `softbuffer` window for Maple Agent Market.
//!
//! `FloatingApp` is the `ApplicationHandler`: on `Resumed` it creates ONE frameless,
//! user-selectable normal/always-on-top window + a `softbuffer` surface; it renders the latest `watch`ed scene
//! to a DOWNSCALED Maple `RgbBuffer` via [`MapleRenderer`] (~window/SCALE) then
//! nearest-neighbor upscales it into the surface (CPU, `0x00RRGGBB`) so the pixel-art
//! scene stays chunky/legible instead of 1:1-tiny. Redraw is event-driven (a
//! `FloatingEvent::SceneChanged` from the pipeline
//! bridge) plus a ~30fps animation tick WHILE agents OR a live gateway daemon (the OpenClaw
//! lobster mascot in `scene.daemons`) are present (motion is time-driven); with no agents and
//! every daemon Down it drops to a slow ~1fps ambient tick (keeping the time-driven
//! clock/weather/lightning/day-night/pet alive without the 30fps cost), never fully idle.
//! Platform glue — codecov-ignored like `driver.rs`; the testable seams are
//! `floating::offscreen` (render), `floating::geometry` (the window/monitor rect math
//! pulled out of here: off-screen-recovery overlap + the corner-resize hit-test), and
//! `floating::cadence` (the animation throttle — both FPS constants live there).

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use pixtuoid_core::sprite::format::Pack;
use pixtuoid_core::state::{DaemonLiveness, SceneState, MAX_FLOORS};
use tokio::sync::watch;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::{CursorIcon, Window, WindowId};

use super::offscreen::MapleRenderer;
use crate::config::{self, FloatingConfig};
use pixtuoid_scene::floor::FloorMeta;
use pixtuoid_scene::theme::Theme;

/// Wake reasons delivered to the winit loop from the background tokio pipeline.
#[derive(Debug)]
pub(crate) enum FloatingEvent {
    /// The reducer published a new scene — repaint.
    SceneChanged,
    /// The install-local Maple Atelier helper reached a terminal state.
    CharacterCatalogMutationFinished(super::maple_atelier::CatalogOperationCompletion),
}

/// The floating window app: window + surface (created lazily on `Resumed`), the Maple
/// renderer (owns cross-frame caches), the live scene receiver, and the per-floor desk
/// capacity atomics it keeps in sync with the rendered scene.
pub(crate) struct FloatingApp {
    cfg: FloatingConfig,
    theme: &'static Theme,
    pack: Pack,
    /// Explicit pack selected by CLI/config at process start.  `None` may
    /// resolve through other fallback rules, so it is deliberately ineligible
    /// for install-local hot import.
    active_pack_dir: Option<PathBuf>,
    config_path: PathBuf,
    renderer: MapleRenderer,
    /// Reused packed-XRGB copy of the downscaled scene. Keeping this allocation
    /// for the window lifetime avoids allocating roughly 300 KiB per frame at
    /// the default large dual-map geometry.
    xrgb_frame: Vec<u32>,
    /// The mute/volume persistence protocol. The renderer holds its own handle clone, handed over
    /// once in `new` (`renderer.set_audio(audio_ctl.handle().clone())`); the
    /// shared-Arc handle stays live across a lazy respawn, so there is no
    /// per-spawn re-sync. Flash is VOLUME-only now (was
    /// every gesture): a mute toggle shows no transient overlay until a footer
    /// lands to display it.
    audio_ctl: crate::audio::AudioController,
    scene_rx: watch::Receiver<Arc<SceneState>>,
    floor_caps: Arc<[AtomicUsize; MAX_FLOORS]>,
    /// The buffer size the capacity atomics were last synced for — capacity only changes
    /// with the window size, so re-sync only on a size change (not every frame).
    last_caps_size: Option<(u16, u16)>,
    /// Latest cursor position (physical px) — for the corner resize hit-test on click.
    cursor: PhysicalPosition<f64>,
    /// Cursor point captured by a manual frameless-window drag. Programmatic
    /// movement deliberately avoids Windows' title-bar Snap/maximize gesture.
    drag_grab: Option<PhysicalPosition<f64>>,
    /// Logical task-label preference from the child-only launcher environment.
    /// The redraw multiplies this by winit's live DPI scale factor.
    label_scale: f32,
    /// User-selectable mini/small/medium/large logical window target.
    size_preset: super::geometry::FloatingSizePreset,
    /// Whether the native-surface character roster/showcase panel is open.
    character_panel_open: bool,
    /// Zero-based page in the dynamic character catalog.
    character_panel_page: usize,
    /// Marker-owned per-user Windows startup file. `None` on unsupported hosts
    /// or when the Startup folder cannot be resolved safely.
    startup: Option<super::startup::StartupRegistration>,
    /// A failed toggle remains visible as a compact error state until the next
    /// successful attempt; details also go to the local trace log.
    startup_error: bool,
    /// Short feedback from a character-panel action (for example full market).
    character_notice: Option<(String, Instant)>,
    /// Imported appearance awaiting a second click on its delete button.
    character_delete_confirmation: Option<usize>,
    /// A single catalog mutation is allowed at a time so two helpers cannot race while
    /// atomically rebuilding the same install-local catalog pack.
    character_catalog_mutation: Option<CharacterCatalogMutation>,
    /// Background import workers wake the native event loop through this
    /// proxy; the UI thread never waits for network or sprite conversion.
    event_proxy: EventLoopProxy<FloatingEvent>,
    /// The animation-tick deadline — see [`super::cadence`] for why the redraw
    /// REQUEST (not just the wait) has to be gated on it.
    clock: super::cadence::FrameClock,
    window: Option<Rc<Window>>,
    // softbuffer's `Context` must outlive the `Surface` it spawned, so keep both.
    context: Option<softbuffer::Context<Rc<Window>>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
}

/// Pointer band along every frameless edge that starts the native resize loop.
const RESIZE_BORDER_PX: f64 = 8.0;
const CHARACTER_NOTICE_TTL: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterCatalogMutation {
    Import,
    Remove,
}

struct CharacterCatalogReload {
    pack: Pack,
    characters: crate::config::CharacterConfig,
    available_count: usize,
    last_page: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CharacterCatalogReloadOutcome {
    available_count: usize,
    last_page: usize,
}

fn prepare_character_catalog_reload(
    pack: Pack,
    app_config: &config::AppConfig,
) -> CharacterCatalogReload {
    let available_count = pixtuoid_scene::market::market_avatar_count(&pack);
    let characters = config::resolve_characters_for_count(app_config, available_count);
    let last_page = available_count
        .saturating_sub(1)
        .div_euclid(super::offscreen::CHARACTER_PANEL_PAGE_SIZE);
    CharacterCatalogReload {
        pack,
        characters,
        available_count,
        last_page,
    }
}

/// Atomically switch the two in-memory consumers that must agree on catalog
/// identity: the painter/preview pack and the renderer's character roster.
/// Keeping this as one testable seam prevents a successful disk import from
/// updating the C-panel names while leaving live actor resolution at the old
/// count (or vice versa).
fn install_character_catalog_reload(
    active_pack: &mut Pack,
    renderer: &mut MapleRenderer,
    new_pack: Pack,
    app_config: &config::AppConfig,
    now: SystemTime,
) -> CharacterCatalogReloadOutcome {
    let reload = prepare_character_catalog_reload(new_pack, app_config);
    let outcome = CharacterCatalogReloadOutcome {
        available_count: reload.available_count,
        last_page: reload.last_page,
    };
    *active_pack = reload.pack;
    renderer.configure_characters(reload.characters, now);
    outcome
}

fn compact_character_panel_name(name: String) -> String {
    const MAX_CHARS: usize = 9;
    if name.chars().count() <= MAX_CHARS {
        return name;
    }
    let mut compact = name.chars().take(MAX_CHARS - 1).collect::<String>();
    compact.push('…');
    compact
}

/// Arm one imported character for deletion. Only a second click on the same
/// row confirms; clicking another row moves the confirmation target instead.
fn advance_delete_confirmation(current: Option<usize>, clicked: usize) -> (Option<usize>, bool) {
    (Some(clicked), current == Some(clicked))
}

fn remap_slots_after_removal(slots: &[usize], removed: usize) -> Vec<usize> {
    slots
        .iter()
        .copied()
        .filter_map(|slot| match slot.cmp(&removed) {
            std::cmp::Ordering::Less => Some(slot),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(slot - 1),
        })
        .collect()
}

fn agent_slots_after_removal(
    selected: &[usize],
    removed: usize,
    available_count: usize,
) -> Vec<usize> {
    let mut slots = remap_slots_after_removal(selected, removed);
    if slots.is_empty() && available_count > 0 {
        slots.push(removed.min(available_count - 1));
    }
    slots
}

fn catalog_count_matches_removal(prior: usize, available: usize, removed: usize) -> bool {
    removed < prior && available.saturating_add(1) == prior
}

impl FloatingApp {
    #[allow(clippy::too_many_arguments)] // flat construction inputs; bundling adds no clarity
    pub(crate) fn new(
        cfg: FloatingConfig,
        theme: &'static Theme,
        pack: Pack,
        active_pack_dir: Option<PathBuf>,
        config_path: PathBuf,
        scene_rx: watch::Receiver<Arc<SceneState>>,
        floor_caps: Arc<[AtomicUsize; MAX_FLOORS]>,
        audio: crate::config::AudioConfig,
        characters: crate::config::CharacterConfig,
        event_proxy: EventLoopProxy<FloatingEvent>,
    ) -> Self {
        // The controller OWNS the device thread (boot-spawn here, Drop-teardown)
        // — see AudioController. Built here, after floating::run's fallible boot
        // steps (pack / runtime / event-loop `?`), so a boot failure means no
        // thread ever existed, and every later exit drops `app` → the join runs.
        let audio_ctl = crate::audio::AudioController::new(audio, config_path.clone());
        let label_scale_value = std::env::var(super::offscreen::LABEL_SCALE_ENV).ok();
        let label_scale = super::offscreen::parse_label_scale(label_scale_value.as_deref());
        let size_preset =
            super::geometry::FloatingSizePreset::from_logical_size(cfg.width, cfg.height);
        let mut renderer = MapleRenderer::new();
        renderer.set_audio(audio_ctl.handle().clone());
        renderer.configure_characters(characters, SystemTime::now());
        let startup = match super::startup::StartupRegistration::discover() {
            Ok(registration) => registration,
            Err(error) => {
                tracing::warn!(
                    "maple-agent-market: Windows startup registration is unavailable: {error}"
                );
                None
            }
        };
        Self {
            cfg,
            theme,
            pack,
            active_pack_dir,
            config_path,
            renderer,
            xrgb_frame: Vec::new(),
            audio_ctl,
            scene_rx,
            floor_caps,
            last_caps_size: None,
            cursor: PhysicalPosition::new(0.0, 0.0),
            drag_grab: None,
            label_scale,
            size_preset,
            character_panel_open: false,
            character_panel_page: 0,
            startup,
            startup_error: false,
            character_notice: None,
            character_delete_confirmation: None,
            character_catalog_mutation: None,
            event_proxy,
            clock: super::cadence::FrameClock::new(Instant::now()),
            window: None,
            context: None,
            surface: None,
        }
    }

    /// Persist the current window geometry into `[floating]` (best-effort — a save error
    /// must not block quitting). Size is stored LOGICAL (HiDPI-stable); position PHYSICAL.
    fn persist_geometry(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let logical = window.inner_size().to_logical::<f64>(window.scale_factor());
        let pos = window.outer_position().ok();
        if let Err(e) = config::save_floating(
            &self.config_path,
            logical.width.round() as u32,
            logical.height.round() as u32,
            pos.map(|p| p.x),
            pos.map(|p| p.y),
        ) {
            tracing::warn!("maple-agent-market: could not persist window geometry: {e}");
        }
    }

    /// Exit through the same persistence path for both the OS close request and
    /// the frameless window's hidden Escape gesture.
    fn request_close(&self, event_loop: &ActiveEventLoop) {
        self.persist_geometry();
        event_loop.exit();
    }

    fn size_selector_text(&self) -> Option<String> {
        self.renderer.map_selector_text()?;
        Some(format!("大小：{} [Z]", self.size_preset.title_zh_tw()))
    }

    fn topmost_selector_text(&self) -> String {
        let state = if self.cfg.always_on_top { "開" } else { "關" };
        format!("置頂：{state} [T]")
    }

    fn character_selector_text(&self) -> String {
        let agents = self.renderer.selected_character_slots().len();
        let available = self.renderer.available_character_count();
        let market = self.renderer.selected_market_showcase_slots().len();
        let training = self.renderer.selected_training_showcase_slots().len();
        format!("角色：{agents}/{available}｜逛{market}｜練{training} [C]")
    }

    fn startup_selector_text(&self) -> Option<String> {
        self.startup.as_ref().map(|registration| {
            if self.startup_error {
                "開機啟動：錯誤 [A]".to_owned()
            } else {
                let state = if registration.is_enabled() {
                    "開"
                } else {
                    "關"
                };
                format!("開機啟動：{state} [A]")
            }
        })
    }

    fn toggle_startup(&mut self) {
        let Some(registration) = &self.startup else {
            return;
        };
        match registration.toggle() {
            Ok(_) => self.startup_error = false,
            Err(error) => {
                self.startup_error = true;
                tracing::warn!(
                    "maple-agent-market: could not toggle Windows startup registration: {error}"
                );
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn persist_character_choices(&self) {
        if let Err(error) = config::save_character_choices_for_count(
            &self.config_path,
            config::CharacterChoices {
                enabled_slots: self.renderer.selected_character_slots(),
                market_showcase_slots: &self.renderer.selected_market_showcase_slots(),
                training_showcase_slots: &self.renderer.selected_training_showcase_slots(),
                available_count: self.renderer.available_character_count(),
            },
        ) {
            tracing::warn!("maple-agent-market: could not persist character choices: {error}");
        }
    }

    fn toggle_character_panel(&mut self) {
        self.character_panel_open = !self.character_panel_open;
        if self.character_panel_open {
            self.character_panel_page = 0;
        }
        self.character_delete_confirmation = None;
        self.character_notice = None;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn close_character_panel(&mut self) {
        self.character_panel_open = false;
        self.character_delete_confirmation = None;
        self.character_notice = None;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn character_name(&self, slot: usize) -> String {
        self.pack
            .character_names()
            .get(slot)
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| pixtuoid_scene::characters::fallback_character_name_zh_tw(slot))
    }

    fn character_panel_model(&self, scene: &SceneState) -> super::offscreen::CharacterPanelModel {
        let real_market_agents = self.renderer.real_market_agent_count(scene);
        let real_training_agents = self.renderer.real_training_agent_count(scene);
        let selected = self.renderer.selected_character_slots();
        let rows = (0..self.renderer.available_character_count())
            .map(|slot| {
                let name = compact_character_panel_name(self.character_name(slot));
                let market_showcase = match self.renderer.market_showcase_visible_state(slot) {
                    Some(true) => super::offscreen::CharacterShowcaseState::Present,
                    Some(false) => super::offscreen::CharacterShowcaseState::Leaving,
                    None => super::offscreen::CharacterShowcaseState::Absent,
                };
                let training_showcase = match self.renderer.training_showcase_visible_state(slot) {
                    Some(true) => super::offscreen::CharacterShowcaseState::Present,
                    Some(false) => super::offscreen::CharacterShowcaseState::Leaving,
                    None => super::offscreen::CharacterShowcaseState::Absent,
                };
                super::offscreen::CharacterPanelRow {
                    name,
                    appearance_index: slot,
                    agent_enabled: selected.binary_search(&slot).is_ok(),
                    deletable: slot >= pixtuoid_scene::characters::CHARACTER_SLOT_COUNT,
                    delete_confirmation: self.character_delete_confirmation == Some(slot),
                    market_showcase,
                    training_showcase,
                }
            })
            .collect::<Vec<_>>();
        let market_showcase_count = rows
            .iter()
            .filter(|row| row.market_showcase != super::offscreen::CharacterShowcaseState::Absent)
            .count();
        let training_showcase_count = rows
            .iter()
            .filter(|row| row.training_showcase != super::offscreen::CharacterShowcaseState::Absent)
            .count();
        let notice = if let Some(mutation) = self.character_catalog_mutation {
            Some(match mutation {
                CharacterCatalogMutation::Import => {
                    "角色匯入進行中；完成後清單會自動更新".to_owned()
                }
                CharacterCatalogMutation::Remove => {
                    "正在刪除自訂角色；完成後清單會自動更新".to_owned()
                }
            })
        } else {
            self.character_notice
                .as_ref()
                .and_then(|(text, at)| (at.elapsed() < CHARACTER_NOTICE_TTL).then(|| text.clone()))
        };
        super::offscreen::CharacterPanelModel {
            real_market_agents,
            real_training_agents,
            market_showcase_count,
            training_showcase_count,
            rows,
            page: self.character_panel_page,
            notice,
        }
    }

    fn change_character_panel_page(&mut self, delta: isize) {
        let page_count = self
            .renderer
            .available_character_count()
            .div_ceil(super::offscreen::CHARACTER_PANEL_PAGE_SIZE)
            .max(1);
        self.character_panel_page = self
            .character_panel_page
            .saturating_add_signed(delta)
            .min(page_count - 1);
        self.character_delete_confirmation = None;
        self.character_notice = None;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn request_character_removal(&mut self, slot: usize) {
        if self.character_catalog_mutation.is_some() {
            self.character_notice = Some((
                "已有一個角色清單操作正在執行；請等待完成".to_owned(),
                Instant::now(),
            ));
            return;
        }
        let title = self.character_name(slot);
        let (next_confirmation, confirmed) =
            advance_delete_confirmation(self.character_delete_confirmation, slot);
        self.character_delete_confirmation = next_confirmation;
        if !confirmed {
            self.character_notice =
                Some((format!("再按一次「刪除」確認移除：{title}"), Instant::now()));
            return;
        }
        let launched = std::env::current_exe().map(|executable| {
            super::maple_atelier::start_remove_from(
                &executable,
                self.active_pack_dir.as_deref(),
                slot,
                self.event_proxy.clone(),
            )
        });
        self.apply_character_removal_launch(launched);
    }

    fn apply_character_removal_launch(
        &mut self,
        launched: std::io::Result<super::maple_atelier::CatalogOperationLaunch>,
    ) {
        use super::maple_atelier::CatalogOperationLaunch;
        let failure_notice = match launched {
            Ok(CatalogOperationLaunch::Started) => {
                self.character_catalog_mutation = Some(CharacterCatalogMutation::Remove);
                self.character_notice = None;
                return;
            }
            Ok(CatalogOperationLaunch::HelperUnavailable) => {
                "找不到角色管理工具或 catalog；請確認安裝完整".to_owned()
            }
            Ok(CatalogOperationLaunch::CatalogNotActive) => {
                "刪除角色只適用於目前載入的本機角色 catalog".to_owned()
            }
            Ok(CatalogOperationLaunch::SpawnFailed(error)) => {
                tracing::warn!("maple-agent-market: could not start character removal: {error}");
                "無法啟動角色刪除工具；請查看本機記錄".to_owned()
            }
            Err(error) => {
                tracing::warn!(
                    "maple-agent-market: could not resolve current executable for character removal: {error}"
                );
                "無法定位程式位置；角色沒有刪除".to_owned()
            }
        };
        self.character_delete_confirmation = None;
        self.character_notice = Some((failure_notice, Instant::now()));
    }

    fn toggle_always_on_top(&mut self) {
        self.cfg.always_on_top = !self.cfg.always_on_top;
        if let Some(window) = &self.window {
            window.set_window_level(super::geometry::window_level(self.cfg.always_on_top));
            window.request_redraw();
        }
        if let Err(error) =
            config::save_floating_always_on_top(&self.config_path, self.cfg.always_on_top)
        {
            tracing::warn!(
                "maple-agent-market: could not persist always-on-top preference: {error}"
            );
        }
    }

    fn finish_imported_character(&mut self, title: String, added: bool, catalog_pack: PathBuf) {
        let pack = match pixtuoid_scene::embedded_pack::load_sprite_pack(Some(catalog_pack.clone()))
        {
            Ok(pack) => pack,
            Err(error) => {
                tracing::warn!(
                    path = %catalog_pack.display(),
                    "maple-agent-market: imported catalog could not be reloaded: {error}"
                );
                self.character_notice = Some((
                    "角色已匯入，但即時載入失敗；重新開啟程式後可使用".to_owned(),
                    Instant::now(),
                ));
                return;
            }
        };
        let app_config = config::load(&self.config_path, &mut Vec::new());
        let reload = install_character_catalog_reload(
            &mut self.pack,
            &mut self.renderer,
            pack,
            &app_config,
            SystemTime::now(),
        );
        if reload.available_count == 0 {
            self.character_notice = Some((
                "角色已匯入，但新的 catalog 沒有可用角色".to_owned(),
                Instant::now(),
            ));
            return;
        }
        self.character_panel_page = reload.last_page;
        self.character_panel_open = true;
        let verb = if added { "已匯入" } else { "已存在" };
        self.character_notice = Some((format!("{verb}：{title}｜清單已即時更新"), Instant::now()));
    }

    fn character_config_after_removal(
        &self,
        removed: usize,
        available_count: usize,
    ) -> config::CharacterConfig {
        config::CharacterConfig {
            agent_roster: pixtuoid_scene::characters::CharacterRoster::new_with_count(
                agent_slots_after_removal(
                    self.renderer.selected_character_slots(),
                    removed,
                    available_count,
                ),
                available_count,
            ),
            showcase_slots: remap_slots_after_removal(
                &self.renderer.selected_market_showcase_slots(),
                removed,
            ),
            training_showcase_slots: remap_slots_after_removal(
                &self.renderer.selected_training_showcase_slots(),
                removed,
            ),
        }
    }

    fn finish_removed_character(&mut self, title: String, removed: usize, catalog_pack: PathBuf) {
        let pack = match pixtuoid_scene::embedded_pack::load_sprite_pack(Some(catalog_pack.clone()))
        {
            Ok(pack) => pack,
            Err(error) => {
                tracing::warn!(
                    path = %catalog_pack.display(),
                    "maple-agent-market: removed catalog could not be reloaded: {error}"
                );
                self.character_notice = Some((
                    "角色已刪除，但即時載入失敗；重新開啟程式後會更新".to_owned(),
                    Instant::now(),
                ));
                return;
            }
        };
        let prior_count = self.renderer.available_character_count();
        let available_count = pixtuoid_scene::market::market_avatar_count(&pack);
        if !catalog_count_matches_removal(prior_count, available_count, removed) {
            self.character_notice = Some((
                "角色素材已刪除，但 catalog 數量不一致；請重新開啟程式".to_owned(),
                Instant::now(),
            ));
            return;
        }
        let characters = self.character_config_after_removal(removed, available_count);
        self.pack = pack;
        self.renderer
            .configure_characters(characters, SystemTime::now());
        let last_page = available_count
            .saturating_sub(1)
            .div_euclid(super::offscreen::CHARACTER_PANEL_PAGE_SIZE);
        self.character_panel_page = self.character_panel_page.min(last_page);
        self.character_panel_open = true;
        self.persist_character_choices();
        self.character_notice = Some((format!("已刪除：{title}｜清單已即時更新"), Instant::now()));
    }

    fn finish_character_catalog_mutation(
        &mut self,
        completion: super::maple_atelier::CatalogOperationCompletion,
    ) {
        use super::maple_atelier::CatalogOperationCompletion;
        self.character_catalog_mutation = None;
        self.character_delete_confirmation = None;
        match completion {
            CatalogOperationCompletion::Imported {
                title,
                added,
                catalog_pack,
            } => self.finish_imported_character(title, added, catalog_pack),
            CatalogOperationCompletion::Removed {
                title,
                removed_appearance_index,
                catalog_pack,
            } => self.finish_removed_character(title, removed_appearance_index, catalog_pack),
            CatalogOperationCompletion::Cancelled => {
                self.character_notice = Some((
                    "已取消新增角色；本機素材沒有變更".to_owned(),
                    Instant::now(),
                ));
            }
            CatalogOperationCompletion::Failed { message } => {
                tracing::warn!("maple-agent-market: character catalog operation failed: {message}");
                self.character_notice =
                    Some((format!("角色清單操作失敗：{message}"), Instant::now()));
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn request_preset_size(&self, window: &Window) {
        let target = self
            .size_preset
            .logical_size(self.renderer.prefers_dual_map());
        let fitted = window.current_monitor().map_or(target, |monitor| {
            let logical = monitor.size().to_logical::<f64>(window.scale_factor());
            super::geometry::fit_logical_size(
                target,
                (
                    logical.width.round().max(1.0) as u32,
                    logical.height.round().max(1.0) as u32,
                ),
                0.9,
            )
        });
        let _ =
            window.request_inner_size(LogicalSize::new(f64::from(fitted.0), f64::from(fitted.1)));
    }

    fn cycle_size_preset(&mut self) -> bool {
        if self.renderer.map_selector_text().is_none() {
            return false;
        }
        self.size_preset = self.size_preset.next();
        if let Some(window) = self.window.clone() {
            self.request_preset_size(&window);
            window.request_redraw();
        }
        true
    }

    fn cycle_map_view(&mut self) {
        let was_dual = self.renderer.prefers_dual_map();
        if !self.renderer.cycle_map() {
            return;
        }
        if let Some(window) = self.window.clone() {
            if was_dual != self.renderer.prefers_dual_map() {
                self.request_preset_size(&window);
            }
            window.request_redraw();
        }
    }

    fn choose_map_view(&mut self, selection: super::input::MapViewSelection) {
        if !self.renderer.select_map_view(selection) {
            return;
        }
        if let Some(window) = self.window.clone() {
            self.request_preset_size(&window);
            window.request_redraw();
        }
    }

    /// Render at native resolution by default. An explicit low-resolution
    /// scale still uses nearest-neighbor expansion for constrained machines.
    fn redraw(&mut self) {
        // Clone the Rc to release the `self.window` borrow before touching `self.surface`.
        let Some(window) = self.window.clone() else {
            return;
        };
        let size = window.inner_size();
        let (win_w, win_h) = (size.width, size.height);
        let (Some(nw), Some(nh)) = (NonZeroU32::new(win_w), NonZeroU32::new(win_h)) else {
            return; // a 0-area window: nothing to draw
        };
        // Audio state for the footer's ♩ suffix + the expiry-driven debounced
        // volume persist, both owned by the controller — resolved BEFORE the
        // surface borrow below. `audio_audible` requires a live handle, an
        // unmuted controller and a nonzero level, so a dead device shows no
        // phantom ♩. `volume_flash` drives the transient `♩ N%` beat.
        let audio_now = Instant::now();
        self.audio_ctl.tick(audio_now);
        let audio_audible = self.audio_ctl.handle().is_audible();
        let volume_flash = self.audio_ctl.volume_flash(audio_now);
        // The ONE window-to-buffer projection helper is shared with the boot
        // seed so native and explicit low-resolution modes cannot drift.
        let (scale, buf_w, buf_h) = super::offscreen::window_buffer_geometry(win_w, win_h);
        // Keep reducer capacity in lockstep with the scene rendered at this buffer size.
        if self.last_caps_size != Some((buf_w, buf_h)) {
            sync_floor_caps(&self.floor_caps, buf_w, buf_h);
            self.last_caps_size = Some((buf_w, buf_h));
        }
        // Arc clone releases the watch borrow before the (mutable) renderer borrow.
        let scene = self.scene_rx.borrow().clone();
        let floor_meta = FloorMeta::ground();
        let maple_frame = self.renderer.render(
            &scene,
            &self.pack,
            self.theme,
            SystemTime::now(),
            buf_w,
            buf_h,
            floor_meta,
        );
        // Collect scene pixels (release the `self.renderer` borrow) as `0x00RRGGBB`.
        let (ow, oh) = (maple_frame.width() as usize, maple_frame.height() as usize);
        self.xrgb_frame.resize(ow.saturating_mul(oh), 0);
        for (packed, pixel) in self.xrgb_frame.iter_mut().zip(maple_frame.as_slice()) {
            *packed = super::offscreen::pack_xrgb(*pixel);
        }
        let map_selector = self.renderer.map_selector_text();
        let size_selector = map_selector
            .as_ref()
            .map(|_| format!("大小：{} [Z]", self.size_preset.title_zh_tw()));
        let topmost_selector = super::offscreen::topmost_selector_visible(win_h as usize)
            .then(|| self.topmost_selector_text());
        let character_selector = super::offscreen::character_selector_visible(win_h as usize)
            .then(|| self.character_selector_text());
        let startup_selector = super::offscreen::startup_selector_visible(win_h as usize)
            .then(|| self.startup_selector_text())
            .flatten();
        let escape_hint =
            super::offscreen::escape_hint_visible(win_h as usize).then_some("ESC：關閉程式");
        let character_panel = self
            .character_panel_open
            .then(|| self.character_panel_model(&scene));

        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        if surface.resize(nw, nh).is_err() {
            return;
        }
        let Ok(mut sb) = surface.buffer_mut() else {
            return;
        };
        // Nearest-neighbour upscale the reusable packed frame into the window.
        // The helper writes contiguous pixel runs + repeated rows instead of an
        // integer division for every native-surface pixel.
        let (win_w, win_h, scale) = (win_w as usize, win_h as usize, scale as usize);
        if ow == 0 || oh == 0 || sb.len() < win_w * win_h {
            return; // nothing rendered / a transient resize race — skip this frame
        }
        if !super::offscreen::upscale_xrgb_nearest(
            &self.xrgb_frame,
            ow,
            oh,
            &mut sb,
            win_w,
            win_h,
            scale,
        ) {
            return;
        }
        // Name badges + the neon wall board, drawn POST-upscale at native surface res
        // (crisp anti-aliased Monaspace Neon) using the same map state the scene
        // pass just used. Badges are a fixed caption height; the board scales with the panel.
        let overlay_now = SystemTime::now();
        let label_font_px =
            super::offscreen::label_font_px(window.scale_factor(), self.label_scale);
        let map_batches = self.renderer.maple_overlay_batches(&scene, overlay_now);
        if map_batches.is_empty() {
            let labels = self.renderer.labels(&scene, overlay_now);
            super::offscreen::paint_map_labels_into_surface_with_font_px(
                &mut sb,
                win_w,
                win_h,
                &labels,
                scale as i32,
                label_font_px,
                self.theme,
                self.renderer.current_map(),
            );
        } else {
            for batch in map_batches {
                let panel_left = usize::from(batch.viewport.x).saturating_mul(scale);
                let buffer_right =
                    usize::from(batch.viewport.x.saturating_add(batch.viewport.width));
                let panel_width = if buffer_right >= ow {
                    win_w.saturating_sub(panel_left)
                } else {
                    usize::from(batch.viewport.width).saturating_mul(scale)
                };
                super::offscreen::paint_map_labels_into_surface_with_font_px_in_panel(
                    &mut sb,
                    win_w,
                    win_h,
                    &batch.labels,
                    scale as i32,
                    label_font_px,
                    self.theme,
                    batch.map,
                    panel_left,
                    panel_width,
                );
                if batch.map == pixtuoid_scene::maple_world::MapleMapId::FreeMarket {
                    super::offscreen::paint_market_player_ids_into_surface_with_font_px_in_panel(
                        &mut sb,
                        win_w,
                        win_h,
                        &batch.player_ids,
                        scale as i32,
                        label_font_px,
                        self.theme,
                        panel_left,
                        panel_width,
                    );
                }
            }
        }
        let board = self.renderer.board(&scene, overlay_now);
        super::offscreen::paint_wall_board_into_surface(
            &mut sb,
            win_w,
            win_h,
            &board,
            scale as i32,
            self.theme,
        );
        if let Some(selector) = map_selector {
            super::offscreen::paint_map_selector_into_surface(
                &mut sb,
                win_w,
                win_h,
                &selector,
                label_font_px,
            );
            if let Some(size_selector) = size_selector {
                super::offscreen::paint_size_selector_into_surface(
                    &mut sb,
                    win_w,
                    win_h,
                    &size_selector,
                    label_font_px,
                );
            }
            if let Some(topmost_selector) = topmost_selector {
                super::offscreen::paint_topmost_selector_into_surface(
                    &mut sb,
                    win_w,
                    win_h,
                    &topmost_selector,
                    label_font_px,
                );
            }
            if let Some(character_selector) = character_selector {
                super::offscreen::paint_character_selector_into_surface(
                    &mut sb,
                    win_w,
                    win_h,
                    &character_selector,
                    label_font_px,
                );
            }
            if let Some(startup_selector) = startup_selector {
                super::offscreen::paint_startup_selector_into_surface(
                    &mut sb,
                    win_w,
                    win_h,
                    &startup_selector,
                    label_font_px,
                );
            }
            if let Some(escape_hint) = escape_hint {
                super::offscreen::paint_escape_hint_into_surface(
                    &mut sb,
                    win_w,
                    win_h,
                    escape_hint,
                    label_font_px,
                );
            }
        }
        if let Some(character_panel) = character_panel {
            super::offscreen::paint_character_panel_into_surface(
                &mut sb,
                win_w,
                win_h,
                &character_panel,
                &self.pack,
                label_font_px,
            );
        }
        // The status footer is a bottom-overlay band that carries
        // the ♩/♩N% audio suffix the standalone volume flash used to, plus the
        // scene stats/rungs/tools/gateway. `win_w`/`win_h` are usize surface dims.
        let budget = super::offscreen::footer_budget(win_w);
        let footer = self
            .renderer
            .footer(&scene, budget, audio_audible, volume_flash);
        super::offscreen::paint_footer_into_surface(&mut sb, win_w, win_h, &footer, self.theme);
        window.pre_present_notify();
        let _ = sb.present();
    }
}

/// Sync the per-floor capacity atomics to the scene layout at `buf_w`×`buf_h`.
/// `store` (not `fetch_max`) lets a window shrink lower its capacity immediately.
fn sync_floor_caps(floor_caps: &[AtomicUsize; MAX_FLOORS], buf_w: u16, buf_h: u16) {
    for (floor_idx, cap) in floor_caps.iter().enumerate() {
        let seed = pixtuoid_scene::floor::floor_seed(floor_idx);
        let capacity = pixtuoid_scene::floor::floor_capacity(buf_w, buf_h, seed);
        cap.store(capacity, Ordering::Relaxed);
    }
}

/// Does the saved window rect `(x, y, w, h)` overlap ANY currently-connected monitor?
/// Thin winit binding over the pure [`super::geometry::window_visible_on_monitors`] (the
/// overlap logic + empty-list guard is unit-tested there; this just pulls the monitor rects).
fn position_on_a_monitor(event_loop: &ActiveEventLoop, x: i32, y: i32, w: u32, h: u32) -> bool {
    super::geometry::window_visible_on_monitors(
        (x, y, w, h),
        event_loop.available_monitors().map(|m| {
            let (pos, size) = (m.position(), m.size());
            (pos.x, pos.y, size.width, size.height)
        }),
    )
}

impl ApplicationHandler<FloatingEvent> for FloatingApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // already created — a re-resume must not spawn a second window
        }
        let mut attrs = Window::default_attributes()
            .with_title(crate::PRODUCT_NAME)
            .with_decorations(false)
            .with_resizable(true)
            .with_window_level(super::geometry::window_level(self.cfg.always_on_top))
            .with_inner_size(LogicalSize::new(
                self.cfg.width as f64,
                self.cfg.height as f64,
            ))
            .with_min_inner_size(LogicalSize::new(
                config::FLOATING_MIN_W as f64,
                config::FLOATING_MIN_H as f64,
            ));
        // Restore the saved position (physical px) ONLY if it still lands on a currently
        // connected monitor; else let the OS place it. A window last closed on a now-
        // disconnected monitor would otherwise restore fully off-screen and be
        // unrecoverable (frameless + no taskbar, especially while topmost → no way to drag it back).
        if let (Some(x), Some(y)) = (self.cfg.x, self.cfg.y) {
            if position_on_a_monitor(event_loop, x, y, self.cfg.width, self.cfg.height) {
                attrs = attrs.with_position(PhysicalPosition::new(x, y));
            }
        }
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs = attrs.with_has_shadow(true).with_titlebar_hidden(true);
        }
        #[cfg(target_os = "windows")]
        {
            // No taskbar button — it's an ambient overlay, not a primary window.
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_skip_taskbar(true);
        }
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                tracing::error!("maple-agent-market: failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };
        let context = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("maple-agent-market: failed to create softbuffer context: {e}");
                event_loop.exit();
                return;
            }
        };
        let surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("maple-agent-market: failed to create softbuffer surface: {e}");
                event_loop.exit();
                return;
            }
        };
        // `cfg.opacity` is parsed + clamped but NOT applied in v1: winit 0.30 exposes no
        // per-window opacity, and softbuffer writes opaque XRGB (no alpha). Honest no-op —
        // real translucency needs a native shim or a wgpu surface (deferred, see spec §11).
        window.request_redraw();
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: FloatingEvent) {
        match event {
            FloatingEvent::SceneChanged => {
                if let Some(window) = self.window.clone() {
                    window.request_redraw();
                }
            }
            FloatingEvent::CharacterCatalogMutationFinished(completion) => {
                self.finish_character_catalog_mutation(completion);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                // Geometry MUST persist HERE — the window is gone once `run_app`
                // returns. The audio persist + device stop instead ride
                // `AudioController::drop` when `app` drops post-`run_app` (#752).
                self.request_close(event_loop);
            }
            // `is_synthetic: false`: winit fabricates a Pressed for every key
            // physically held when the window GAINS FOCUS (X11 + Windows). A
            // muted user holding `+`/`m` who clicks in would otherwise be
            // spuriously unmuted AND have it persisted (volume-up is the
            // un-mute gesture). Ignore the release replay after focus gain.
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed => {
                if super::input::close_requested(&event.logical_key, event.repeat) {
                    self.request_close(event_loop);
                    return;
                }
                if super::input::map_switch_requested(&event.logical_key, event.repeat) {
                    self.cycle_map_view();
                    return;
                }
                if super::input::size_switch_requested(&event.logical_key, event.repeat) {
                    self.cycle_size_preset();
                    return;
                }
                if super::input::topmost_toggle_requested(&event.logical_key, event.repeat) {
                    self.toggle_always_on_top();
                    return;
                }
                if super::input::character_panel_toggle_requested(&event.logical_key, event.repeat)
                {
                    self.toggle_character_panel();
                    return;
                }
                if self.character_panel_open {
                    if let Some(delta) =
                        super::input::character_panel_page_delta(&event.logical_key, event.repeat)
                    {
                        self.change_character_panel_page(delta);
                        return;
                    }
                }
                if super::input::startup_toggle_requested(&event.logical_key, event.repeat) {
                    self.toggle_startup();
                    return;
                }
                if let Some(selection) =
                    super::input::map_view_selection(&event.logical_key, event.repeat)
                {
                    self.choose_map_view(selection);
                    return;
                }
                if let Some(action) = super::input::audio_action(&event.logical_key, event.repeat) {
                    // floating has no [p]ause; effective mute == muted. The
                    // controller persists mute NOW + debounces the volume + arms
                    // the (volume-only) readout.
                    self.audio_ctl
                        .apply(action, Instant::now(), crate::audio::respawn);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(size) => {
                if let Some(window) = &self.window {
                    let logical = size.to_logical::<f64>(window.scale_factor());
                    self.size_preset = super::geometry::FloatingSizePreset::from_logical_size(
                        logical.width.round().max(1.0) as u32,
                        logical.height.round().max(1.0) as u32,
                    );
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position;
                if let (Some(grab), Some(window)) = (self.drag_grab, &self.window) {
                    if let Ok(outer) = window.outer_position() {
                        let target = super::geometry::manual_drag_position(
                            (outer.x, outer.y),
                            (position.x, position.y),
                            (grab.x, grab.y),
                        );
                        window.set_outer_position(PhysicalPosition::new(target.0, target.1));
                    }
                } else if let Some(window) = &self.window {
                    let size = window.inner_size();
                    let cursor = super::geometry::resize_direction_at(
                        (position.x, position.y),
                        (size.width, size.height),
                        RESIZE_BORDER_PX,
                    )
                    .map_or(CursorIcon::Default, CursorIcon::from);
                    window.set_cursor(cursor);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // Frameless: a left-press starts a programmatic move, EXCEPT along
                // any edge/corner, which enters the matching native resize loop.
                // Native move is intentionally avoided because Windows Snap can
                // maximize the window while it crosses a monitor edge.
                if let Some(window) = self.window.clone() {
                    let size = window.inner_size();
                    let font_px =
                        super::offscreen::label_font_px(window.scale_factor(), self.label_scale);
                    if self.character_panel_open {
                        if let Some(action) = super::offscreen::character_panel_hit_test(
                            (self.cursor.x, self.cursor.y),
                            font_px,
                            size.width as usize,
                            size.height as usize,
                            self.character_panel_page,
                            self.renderer.available_character_count(),
                        ) {
                            let scene = self.scene_rx.borrow().clone();
                            match action {
                                super::offscreen::CharacterPanelAction::ClosePanel => {
                                    self.close_character_panel();
                                }
                                super::offscreen::CharacterPanelAction::StartMapleAtelierClipboardImport => {
                                    if self.character_catalog_mutation.is_some() {
                                        self.character_notice = Some((
                                            "已有一個角色清單操作正在執行；請等待完成".to_owned(),
                                            Instant::now(),
                                        ));
                                    } else {
                                        self.character_delete_confirmation = None;
                                        let launched = std::env::current_exe().map(|executable| {
                                            super::maple_atelier::start_import_from(
                                                &executable,
                                                self.active_pack_dir.as_deref(),
                                                self.event_proxy.clone(),
                                            )
                                        });
                                        match launched {
                                            Ok(super::maple_atelier::CatalogOperationLaunch::Started) => {
                                                self.character_catalog_mutation =
                                                    Some(CharacterCatalogMutation::Import);
                                                self.character_notice = None;
                                            }
                                            Ok(super::maple_atelier::CatalogOperationLaunch::HelperUnavailable) => {
                                                self.character_notice = Some((
                                                    "找不到 Maple Atelier 匯入工具或 catalog；請確認安裝完整".to_owned(),
                                                    Instant::now(),
                                                ));
                                            }
                                            Ok(super::maple_atelier::CatalogOperationLaunch::CatalogNotActive) => {
                                                self.character_notice = Some((
                                                    "新增角色只適用於目前載入的本機角色 catalog".to_owned(),
                                                    Instant::now(),
                                                ));
                                            }
                                            Ok(super::maple_atelier::CatalogOperationLaunch::SpawnFailed(_))
                                            | Err(_) => {
                                                self.character_notice = Some((
                                                    "無法啟動 Maple Atelier 匯入工具；請查看本機記錄".to_owned(),
                                                    Instant::now(),
                                                ));
                                            }
                                        }
                                    }
                                }
                                super::offscreen::CharacterPanelAction::ToggleAgentSlot(slot) => {
                                    self.character_delete_confirmation = None;
                                    if !self.renderer.toggle_character_slot(slot) {
                                        self.character_notice = Some((
                                            "至少要保留 1 個 Agent 外觀".to_owned(),
                                            Instant::now(),
                                        ));
                                    } else {
                                        self.character_notice = None;
                                        self.persist_character_choices();
                                    }
                                }
                                super::offscreen::CharacterPanelAction::RequestDeleteSlot(slot) => {
                                    self.request_character_removal(slot);
                                }
                                super::offscreen::CharacterPanelAction::ToggleMarketShowcaseSlot(
                                    slot,
                                ) => {
                                    use super::characters::ShowcaseToggle;
                                    match self.renderer.toggle_market_showcase_slot(
                                        &scene,
                                        slot,
                                        SystemTime::now(),
                                    ) {
                                        ShowcaseToggle::Entered => {
                                            self.character_notice = Some((
                                                "展示角色正從自由市場傳點進場".to_owned(),
                                                Instant::now(),
                                            ));
                                            self.persist_character_choices();
                                        }
                                        ShowcaseToggle::Leaving => {
                                            self.character_notice = Some((
                                                "展示角色正收攤並走向傳點".to_owned(),
                                                Instant::now(),
                                            ));
                                            self.persist_character_choices();
                                        }
                                        ShowcaseToggle::Full => {
                                            self.character_notice = Some((
                                                "自由市場已滿；真 Agent 保留優先座位".to_owned(),
                                                Instant::now(),
                                            ));
                                        }
                                        ShowcaseToggle::Invalid => {}
                                    }
                                }
                                super::offscreen::CharacterPanelAction::ToggleTrainingShowcaseSlot(
                                    slot,
                                ) => {
                                    use super::characters::ShowcaseToggle;
                                    match self.renderer.toggle_training_showcase_slot(
                                        &scene,
                                        slot,
                                        SystemTime::now(),
                                    ) {
                                        ShowcaseToggle::Entered => {
                                            self.character_notice = Some((
                                                "練功角色正從訓練場傳點進場".to_owned(),
                                                Instant::now(),
                                            ));
                                            self.persist_character_choices();
                                        }
                                        ShowcaseToggle::Leaving => {
                                            self.character_notice = Some((
                                                "練功角色正沿平台／繩索返回傳點".to_owned(),
                                                Instant::now(),
                                            ));
                                            self.persist_character_choices();
                                        }
                                        ShowcaseToggle::Full => {
                                            self.character_notice = Some((
                                                "訓練場已滿；真 Agent 保留優先位置".to_owned(),
                                                Instant::now(),
                                            ));
                                        }
                                        ShowcaseToggle::Invalid => {}
                                    }
                                }
                                super::offscreen::CharacterPanelAction::PreviousPage => {
                                    self.change_character_panel_page(-1);
                                }
                                super::offscreen::CharacterPanelAction::NextPage => {
                                    self.change_character_panel_page(1);
                                }
                            }
                            self.drag_grab = None;
                            window.request_redraw();
                            return;
                        }
                        // Clicking outside the modal does not drag the window;
                        // use C, the compact tab, or the panel's X to close.
                        self.drag_grab = None;
                        return;
                    }
                    let selector_clicked = self.renderer.map_selector_text().is_some_and(|text| {
                        super::offscreen::map_selector_hit_test(
                            (self.cursor.x, self.cursor.y),
                            &text,
                            font_px,
                            size.width as usize,
                            size.height as usize,
                        )
                    });
                    let size_selector_clicked = self.size_selector_text().is_some_and(|text| {
                        super::offscreen::size_selector_hit_test(
                            (self.cursor.x, self.cursor.y),
                            &text,
                            font_px,
                            size.width as usize,
                            size.height as usize,
                        )
                    });
                    let topmost_selector_clicked =
                        super::offscreen::topmost_selector_visible(size.height as usize) && {
                            let topmost_text = self.topmost_selector_text();
                            super::offscreen::topmost_selector_hit_test(
                                (self.cursor.x, self.cursor.y),
                                &topmost_text,
                                font_px,
                                size.width as usize,
                                size.height as usize,
                            )
                        };
                    let character_selector_clicked =
                        super::offscreen::character_selector_visible(size.height as usize)
                            && super::offscreen::character_selector_hit_test(
                                (self.cursor.x, self.cursor.y),
                                &self.character_selector_text(),
                                font_px,
                                size.width as usize,
                                size.height as usize,
                            );
                    let startup_selector_clicked =
                        super::offscreen::startup_selector_visible(size.height as usize)
                            && self.startup_selector_text().is_some_and(|text| {
                                super::offscreen::startup_selector_hit_test(
                                    (self.cursor.x, self.cursor.y),
                                    &text,
                                    font_px,
                                    size.width as usize,
                                    size.height as usize,
                                )
                            });
                    if startup_selector_clicked {
                        self.drag_grab = None;
                        self.toggle_startup();
                        return;
                    }
                    if character_selector_clicked {
                        self.drag_grab = None;
                        self.toggle_character_panel();
                        return;
                    }
                    if topmost_selector_clicked {
                        self.drag_grab = None;
                        self.toggle_always_on_top();
                        return;
                    }
                    if size_selector_clicked {
                        self.drag_grab = None;
                        self.cycle_size_preset();
                        return;
                    }
                    if selector_clicked {
                        self.drag_grab = None;
                        self.cycle_map_view();
                        return;
                    }
                    let resize_direction = super::geometry::resize_direction_at(
                        (self.cursor.x, self.cursor.y),
                        (size.width, size.height),
                        RESIZE_BORDER_PX,
                    );
                    if let Some(direction) = resize_direction {
                        self.drag_grab = None;
                        let _ = window.drag_resize_window(direction);
                    } else {
                        self.drag_grab = Some(self.cursor);
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            }
            | WindowEvent::Focused(false) => self.drag_grab = None,
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Agents animate continuously (walk/breathe — time-driven), so tick ~30fps WHILE
        // any agent is present. When the scene is EMPTY we don't go fully idle: the
        // time-driven AMBIENT layer (clock hands, weather cycle, lightning, day/night
        // lighting, the wandering pet) still advances, so a 0fps idle would freeze it and
        // an empty window would look dead/broken. Drop to a slow ~1fps ambient tick
        // instead — enough to keep the scene alive while preserving the CPU-saving intent
        // (nowhere near the 30fps agents-present path). A LIVE gateway daemon (the OpenClaw
        // lobster) lives in `daemons`, not `agents`, and is a time-driven WANDERING mascot
        // — not slow ambient decor — so it keeps the 30fps path unless every daemon is Down
        // (a Down daemon is gone/leaving within MASCOT_LEAVE_MS, not a sustained wanderer, so
        // it stays on the ambient tick — same brief terminal transition as before this change).
        let scene = self.scene_rx.borrow();
        let scene_idle = super::cadence::uses_ambient_tick(
            !scene.agents.is_empty(),
            scene
                .daemons()
                .all(|(_, _, daemon)| daemon.liveness == DaemonLiveness::Down),
            self.renderer.showcase_needs_active_animation(),
        );
        drop(scene);
        // The redraw REQUEST rides the same deadline as the wait: requesting one
        // unconditionally here leaves winit a pending redraw, so `WaitUntil` never
        // sleeps and both cadences collapse to max-rate (see `super::cadence`).
        let (paint, deadline) = self.clock.poll(Instant::now(), scene_idle);
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        if paint {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_a_custom_character_requires_two_clicks_on_the_same_row() {
        assert_eq!(advance_delete_confirmation(None, 12), (Some(12), false));
        assert_eq!(
            advance_delete_confirmation(Some(12), 13),
            (Some(13), false),
            "choosing another row arms that row instead of deleting the first"
        );
        assert_eq!(
            advance_delete_confirmation(Some(13), 13),
            (Some(13), true),
            "only a repeated click confirms the destructive action"
        );
    }

    #[test]
    fn catalog_removal_preserves_other_character_choices_by_shifting_later_slots() {
        assert_eq!(
            remap_slots_after_removal(&[1, 8, 12, 15], 12),
            vec![1, 8, 14]
        );
        assert!(remap_slots_after_removal(&[12], 12).is_empty());
        assert_eq!(
            agent_slots_after_removal(&[12], 12, 15),
            vec![12],
            "removing the only Agent appearance selects the nearest remaining slot"
        );
    }

    #[test]
    fn imported_catalog_selects_the_page_containing_its_newest_character() {
        const PACK_TOML: &str = r##"
[pack]
name = "hot-reload-test"
version = "0.0.0"

[palette]
"x" = "#ffffff"

[characters]
names = ["一", "二", "三", "四", "五", "六", "七", "八", "剛匯入"]

[animations.market_avatar_hires]
frames = ["avatars.sprite"]
frame_ms = 1000
"##;
        let row = std::iter::repeat_n(
            "x",
            pixtuoid_scene::market::MARKET_AVATAR_HIRES_WIDTH as usize,
        )
        .collect::<Vec<_>>()
        .join(" ");
        let avatars = (0..9)
            .map(|frame| {
                let pixels = std::iter::repeat_n(
                    row.as_str(),
                    pixtuoid_scene::market::MARKET_AVATAR_HIRES_HEIGHT as usize,
                )
                .collect::<Vec<_>>()
                .join("\n");
                format!("@frame {frame}\n{pixels}\n")
            })
            .collect::<String>();
        let pack = pixtuoid_core::sprite::format::load_pack_from_strings(
            PACK_TOML,
            &[("avatars.sprite", avatars.as_str())],
        )
        .expect("nine-character test pack");

        let mut active_pack = pixtuoid_core::sprite::format::load_pack_from_strings(
            r##"[pack]
name = "before-reload"
version = "0.0.0"
[palette]
"x" = "#ffffff"

[animations]
"##,
            &[],
        )
        .expect("pre-import pack");
        let mut renderer = MapleRenderer::new();

        let reload = install_character_catalog_reload(
            &mut active_pack,
            &mut renderer,
            pack,
            &config::AppConfig::default(),
            SystemTime::UNIX_EPOCH,
        );

        assert_eq!(reload.available_count, 9);
        assert_eq!(reload.last_page, 1);
        assert_eq!(active_pack.character_names().last().unwrap(), "剛匯入");
        assert_eq!(renderer.available_character_count(), 9);
    }
}
