//! Frameless Maple Agent Market desktop window with user-selectable stacking.
//!
//! A binary-only front-end on the shared engine: the runtime pipeline produces
//! `SceneState`, and this module presents each frame as a full-resolution
//! [`offscreen::MapleRenderer`] `RgbBuffer` blitted into a `winit` +
//! `softbuffer` window instead of half-block terminal cells. `pixtuoid-core` stays
//! window-free (invariant #1) — all windowing lives here.

mod cadence;
mod characters;
mod geometry;
mod input;
mod maple_atelier;
pub mod offscreen;
mod startup;
mod window;

use anyhow::{Context, Result};
use pixtuoid_core::source::claude_code::ClaudeCodeSource;
use winit::event_loop::EventLoop;

use crate::config;
use crate::runtime::{ConnectedSources, RunConfig};
use window::{FloatingApp, FloatingEvent};

/// Open the floating window and drive it until the user closes it.
///
/// `winit`'s event loop must own the main thread, so the source pipeline runs on a
/// background tokio runtime (spawned, NEVER `block_on` — that would stall the window),
/// and scene changes reach the loop via an `EventLoopProxy`. BLOCKS until the window
/// closes; the runtime + source handles are held alive across the call.
pub fn run(cfg: RunConfig) -> Result<()> {
    let RunConfig {
        socket,
        projects_root,
        codex_sessions_root,
        pack_dir,
        theme,
        connected,
        config_path,
        audio,
        ..
    } = cfg;
    let app_config = config::load(&config_path, &mut Vec::new());
    let floating_cfg = config::resolve_floating(&app_config);
    let active_pack_dir = pack_dir.clone();
    let pack = pixtuoid_scene::embedded_pack::load_sprite_pack(pack_dir)
        .context("loading the sprite pack for the floating window")?;
    let character_cfg = config::resolve_characters_for_count(
        &app_config,
        pixtuoid_scene::market::market_avatar_count(&pack),
    );

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the floating tokio runtime")?;
    // Enter the runtime on the main thread so the source set's internal `tokio::spawn`s
    // (presence watch, source manager) have a runtime context. We never `block_on` here.
    let _guard = rt.enter();

    // The shared runtime pipeline owns all source and reducer wiring. The
    // rt.enter() guard above provides the ambient runtime its spawns land on;
    // only window-specific pieces stay here.
    let connected = ConnectedSources::new(connected);
    let socket_path = socket.unwrap_or_else(ClaudeCodeSource::default_socket_path);
    // Boot capacity comes from the same scaled geometry as the first redraw so
    // a session arriving during startup cannot be assigned outside the scene.
    let boot_caps = offscreen::boot_capacities_for_window(floating_cfg.width, floating_cfg.height);
    // The source tasks live on `rt` (kept alive to the end of `run`);
    // `_source_handles` is an inert anchor (see Pipeline's doc).
    let crate::runtime::pipeline::Pipeline {
        scene_rx,
        health_rx,
        floor_caps,
        _source_handles,
    } = crate::runtime::pipeline::spawn_pipeline(
        socket_path,
        projects_root,
        codex_sessions_root,
        connected,
        boot_caps,
    );

    // --- the window event loop (main thread) ---
    let mut builder = EventLoop::<FloatingEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        // Accessory: no Dock icon, doesn't steal focus — an ambient companion.
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = builder
        .build()
        .context("building the floating event loop")?;
    let proxy = event_loop.create_proxy();

    // Bridge: a new scene → a repaint. Breaks cleanly when the window closes
    // (`send_event` → `EventLoopClosed`) or the reducer drops its sender — never unwraps.
    {
        let mut scene_rx = scene_rx.clone();
        let scene_proxy = proxy.clone();
        rt.spawn(async move {
            while scene_rx.changed().await.is_ok() {
                if scene_proxy.send_event(FloatingEvent::SceneChanged).is_err() {
                    break;
                }
            }
        });
    }
    // Source deaths are LOGGED here (the office partially freezes). The floating
    // footer exists now but doesn't thread this health channel yet — it passes
    // `source_warning: None` (the seam is ready; see `offscreen::footer`).
    // Deduped by count exactly like headless_loop's consumer of the
    // SAME channel: the watch value is a grow-only Vec, so logging the whole
    // borrow on every change re-warns all prior deaths (N deaths → N(N+1)/2
    // lines, reading as repeated crashes in log forensics).
    {
        let mut health_rx = health_rx;
        rt.spawn(async move {
            let mut deaths_seen = 0usize;
            while health_rx.changed().await.is_ok() {
                let deaths = health_rx.borrow_and_update().clone();
                for death in crate::runtime::unseen_deaths(&deaths, &mut deaths_seen) {
                    tracing::warn!("maple-agent-market: source exited: {death:?}");
                }
            }
        });
    }

    // FloatingApp OWNS the audio device thread via its AudioController (boot-spawn
    // iff unmuted, Drop-teardown). Constructed HERE, after every fallible `?` boot
    // step (pack load, runtime + event-loop build), so a boot failure means no
    // thread ever existed — and once `app` exists, its Drop joins the device
    // thread on EVERY exit (run_app returning normally OR a window-creation
    // failure), no manual shutdown call. This is what fixes the "music keeps
    // playing after quit / killall coreaudiod" bug at the root, structurally.
    let mut app = FloatingApp::new(
        floating_cfg,
        theme,
        pack,
        active_pack_dir,
        config_path,
        scene_rx,
        floor_caps,
        audio,
        character_cfg,
        proxy,
    );
    event_loop
        .run_app(&mut app)
        .context("running the floating window event loop")
    // `app` drops here → AudioController Drop → device thread joined before we
    // return (and the process exits).
}
