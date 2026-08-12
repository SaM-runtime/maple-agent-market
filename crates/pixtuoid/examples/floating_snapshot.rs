//! Render one frame of Maple Agent Market to PNG. It drives the same
//! `floating::offscreen::MapleRenderer` and label overlay as the live window,
//! so the result is byte-faithful to the public procedural renderer.
//!
//! Usage:
//!   cargo run --release --example floating_snapshot -- <out.png> [WxH] [--agents N] [--all-active] [--pack-dir <path>] [--map market|training|both] [--native] [--label-px N] [--advance-ms N] [--entry-ms N] [--exit-ms N] [--command-ms N] [--complete-ms N]
//! e.g. `... -- /tmp/floating.png 720x480 --agents 6` (Retina default), `... -- /tmp/f.png 360x240`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use image::{Rgb as ImgRgb, RgbImage};
use pixtuoid::floating::offscreen::{
    paint_map_labels_into_surface_with_font_px,
    paint_map_labels_into_surface_with_font_px_in_panel, paint_map_selector_into_surface,
    paint_market_player_ids_into_surface,
    paint_market_player_ids_into_surface_with_font_px_in_panel, paint_size_selector_into_surface,
    MapleRenderer,
};
use pixtuoid_core::source::{AgentEvent, Transport};
use pixtuoid_core::state::{ActivityState, SceneState, ToolKind};
use pixtuoid_core::{AgentId, AgentSlot, GlobalDeskIndex, Reducer};
use pixtuoid_scene::floor::FloorMeta;
use pixtuoid_scene::maple_world::MapleMapId;
use pixtuoid_scene::theme::theme_by_name;

/// A few Codex task-title merchants at slots 0..n with varied states so the
/// snapshot exercises Traditional Chinese shop cards and every activity tone.
#[derive(Clone, Copy)]
struct DemoAgentOptions {
    entry_age: Duration,
    exit_age: Option<Duration>,
    command_age: Option<Duration>,
    completion_age: Option<Duration>,
    all_active: bool,
}

fn populate_demo_agents(
    scene: &mut SceneState,
    now: SystemTime,
    n: usize,
    options: DemoAgentOptions,
) {
    let archetypes: [(&str, ActivityState); 6] = [
        (
            "巡查訓練場怪物",
            ActivityState::Active {
                tool_use_id: Some("tu_a".into()),
                detail: Some("Write: src/foo.rs".into()),
                kind: ToolKind::Edit,
            },
        ),
        ("整理隊伍站位", ActivityState::Idle),
        (
            "等待需求確認",
            ActivityState::Waiting {
                reason: "permission?".into(),
            },
        ),
        (
            "同步角色戰鬥狀態",
            ActivityState::Active {
                tool_use_id: Some("tu_d".into()),
                detail: Some("Bash: cargo test".into()),
                kind: ToolKind::Bash,
            },
        ),
        ("檢查繁中字體", ActivityState::Idle),
        (
            "驗證浮動視窗",
            ActivityState::Active {
                tool_use_id: Some("tu_e".into()),
                detail: Some("Grep: TODO".into()),
                kind: ToolKind::Search,
            },
        ),
    ];
    // Default snapshots back-date well past entry. The lifecycle CLI options
    // deliberately override this for frame-by-frame market-motion QA.
    let created_at = now.checked_sub(options.entry_age).unwrap_or(now);
    let exiting_at = options.exit_age.and_then(|age| now.checked_sub(age));
    let recent = now.checked_sub(Duration::from_secs(3)).unwrap_or(now);
    let mut ids = Vec::with_capacity(n);
    for i in 0..n {
        let (label, archetype_state) = &archetypes[i % archetypes.len()];
        let state = if options.all_active {
            ActivityState::Active {
                tool_use_id: Some(format!("tu_skill_{i}").into()),
                detail: Some("Exec: visual skill QA".into()),
                kind: ToolKind::Bash,
            }
        } else {
            archetype_state.clone()
        };
        let key = format!("{label}-{i}");
        let id = AgentId::from_transcript_path(&format!("/demo/{key}.jsonl"));
        let parent_id = match i {
            1 => ids.first().copied(),
            3 | 4 => ids.get(2).copied(),
            _ => None,
        };
        let state_started_at = if matches!(
            &state,
            ActivityState::Active {
                kind: ToolKind::Bash,
                ..
            }
        ) {
            options
                .command_age
                .and_then(|age| now.checked_sub(age))
                .unwrap_or(created_at)
        } else {
            created_at
        };
        scene.agents.insert(
            id,
            AgentSlot {
                agent_id: id,
                source: Arc::from("claude-code"),
                session_id: Arc::from(format!("demo-{key}-{i:04x}").as_str()),
                cwd: Arc::from(PathBuf::from("/demo").as_path()),
                label: (*label).into(),
                state,
                state_started_at,
                created_at,
                last_event_at: recent,
                exiting_at,
                pending_idle_at: None,
                desk_index: GlobalDeskIndex(i),
                floor_idx: scene.floor_of(GlobalDeskIndex(i)),
                tool_call_count: 0,
                active_ms: 0,
                unknown_cwd: false,
                parent_id,
                pid: None,
                model: None,
                effort: None,
                tokens_used: 0,
                last_usage: None,
            },
        );
        ids.push(id);
    }
    if let (Some(age), Some(agent_id)) = (options.completion_age, ids.first().copied()) {
        let completed_at = now.checked_sub(age).unwrap_or(now);
        Reducer::new().apply(
            scene,
            AgentEvent::TurnComplete { agent_id },
            completed_at,
            Transport::Jsonl,
        );
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let out = args.next().ok_or_else(|| {
        anyhow!("usage: floating_snapshot <out.png> [WxH] [--agents N] [--all-active] [--pack-dir <path>] [--map market|training|both] [--native] [--label-px N] [--advance-ms N] [--entry-ms N] [--exit-ms N] [--command-ms N] [--complete-ms N]")
    })?;

    let mut size = (720u16, 480u16); // Retina default (360x240 logical @2x)
    let mut n_agents = 0usize;
    let mut all_active = false;
    let mut pack_dir: Option<PathBuf> = None;
    let mut entry_age = Duration::from_secs(120);
    let mut exit_age: Option<Duration> = None;
    let mut command_age: Option<Duration> = None;
    let mut completion_age: Option<Duration> = None;
    let mut maple_map = MapleMapId::FreeMarket;
    let mut both_maps = false;
    let mut size_explicit = false;
    let mut native_scale = false;
    let mut label_font_px = 12.0f32;
    let mut advance = Duration::ZERO;
    let rest: Vec<String> = args.collect();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--agents" => {
                n_agents = rest
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--agents needs a value"))?
                    .parse()
                    .context("bad --agents")?;
                i += 2;
            }
            "--all-active" => {
                all_active = true;
                i += 1;
            }
            "--pack-dir" => {
                pack_dir = Some(PathBuf::from(
                    rest.get(i + 1)
                        .ok_or_else(|| anyhow!("--pack-dir needs a value"))?,
                ));
                i += 2;
            }
            "--map" => {
                match rest.get(i + 1).map(String::as_str) {
                    Some("market") => {
                        maple_map = MapleMapId::FreeMarket;
                        both_maps = false;
                    }
                    Some("training") => {
                        maple_map = MapleMapId::ForestTraining;
                        both_maps = false;
                    }
                    Some("both") => both_maps = true,
                    _ => return Err(anyhow!("--map needs market, training, or both")),
                }
                i += 2;
            }
            "--native" => {
                native_scale = true;
                i += 1;
            }
            "--label-px" => {
                label_font_px = rest
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--label-px needs a value"))?
                    .parse()
                    .context("bad --label-px")?;
                if !label_font_px.is_finite() || !(8.0..=32.0).contains(&label_font_px) {
                    return Err(anyhow!("--label-px must be between 8 and 32"));
                }
                i += 2;
            }
            "--advance-ms" => {
                let millis = rest
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--advance-ms needs a value"))?
                    .parse()
                    .context("bad --advance-ms")?;
                advance = Duration::from_millis(millis);
                i += 2;
            }
            "--entry-ms" => {
                let millis = rest
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--entry-ms needs a value"))?
                    .parse()
                    .context("bad --entry-ms")?;
                entry_age = Duration::from_millis(millis);
                i += 2;
            }
            "--exit-ms" => {
                let millis = rest
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--exit-ms needs a value"))?
                    .parse()
                    .context("bad --exit-ms")?;
                exit_age = Some(Duration::from_millis(millis));
                i += 2;
            }
            "--command-ms" => {
                let millis = rest
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--command-ms needs a value"))?
                    .parse()
                    .context("bad --command-ms")?;
                command_age = Some(Duration::from_millis(millis));
                i += 2;
            }
            "--complete-ms" => {
                let millis = rest
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--complete-ms needs a value"))?
                    .parse()
                    .context("bad --complete-ms")?;
                completion_age = Some(Duration::from_millis(millis));
                i += 2;
            }
            s if s.contains('x') => {
                let (w, h) = s.split_once('x').unwrap();
                size = (
                    w.parse().context("bad width")?,
                    h.parse().context("bad height")?,
                );
                size_explicit = true;
                i += 1;
            }
            other => return Err(anyhow!("unexpected arg: {other}")),
        }
    }

    let theme = theme_by_name("maple").expect("the built-in Maple theme must stay registered");
    if both_maps && !size_explicit {
        size = (1_440, 480);
    }
    let pack = pixtuoid_scene::embedded_pack::load_sprite_pack(pack_dir)?;
    let now = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000) + advance;

    // `--agents N` adds demo agents so cards and lifecycle animations can be
    // captured without a live Codex session.
    let mut scene = SceneState::uniform(64);
    populate_demo_agents(
        &mut scene,
        now,
        n_agents,
        DemoAgentOptions {
            entry_age,
            exit_age,
            command_age,
            completion_age,
            all_active,
        },
    );
    let mut renderer = MapleRenderer::new();
    if both_maps {
        renderer.set_maple_dual_view();
        renderer.assign_snapshot_scene_across_maps(&scene);
    } else {
        renderer.set_maple_map(maple_map);
        renderer.assign_snapshot_scene_to_map(&scene, maple_map);
    }
    // Mirror floating::window EXACTLY: render the Maple scene at window/SCALE, nearest-neighbor
    // upscale into a `u32` surface, then blit the name badges — so the PNG is byte-faithful.
    let (win_w, win_h) = (size.0 as u32, size.1 as u32);
    let scale = if native_scale {
        1
    } else {
        pixtuoid::floating::offscreen::maple_scale(win_h)
    };
    let ow = (win_w / scale).max(1).min(u16::MAX as u32) as u16;
    let oh = (win_h / scale).max(1).min(u16::MAX as u32) as u16;
    let buf = renderer.render(&scene, &pack, theme, now, ow, oh, FloorMeta::ground());
    let (bw, bh) = (buf.width() as u32, buf.height() as u32);

    let (ww, wh) = (win_w as usize, win_h as usize);
    let mut sb: Vec<u32> = vec![0; ww * wh];
    for wy in 0..win_h {
        let oy = (wy / scale).min(bh - 1);
        for wx in 0..win_w {
            let ox = (wx / scale).min(bw - 1);
            let p = buf.as_slice()[(oy * bw + ox) as usize];
            sb[wy as usize * ww + wx as usize] =
                (p.r as u32) << 16 | (p.g as u32) << 8 | p.b as u32;
        }
    }
    let map_batches = renderer.maple_overlay_batches(&scene, now);
    if map_batches.is_empty() {
        let labels = renderer.labels(&scene, now);
        paint_map_labels_into_surface_with_font_px(
            &mut sb,
            ww,
            wh,
            &labels,
            scale as i32,
            label_font_px,
            theme,
            maple_map,
        );
        let player_ids = renderer.market_player_ids(&scene, now);
        paint_market_player_ids_into_surface(&mut sb, ww, wh, &player_ids, scale as i32, theme);
    }
    for batch in map_batches {
        let panel_left = usize::from(batch.viewport.x).saturating_mul(scale as usize);
        let buffer_right = usize::from(batch.viewport.x.saturating_add(batch.viewport.width));
        let panel_width = if buffer_right >= usize::from(ow) {
            ww.saturating_sub(panel_left)
        } else {
            usize::from(batch.viewport.width).saturating_mul(scale as usize)
        };
        paint_map_labels_into_surface_with_font_px_in_panel(
            &mut sb,
            ww,
            wh,
            &batch.labels,
            scale as i32,
            label_font_px,
            theme,
            batch.map,
            panel_left,
            panel_width,
        );
        if batch.map == MapleMapId::FreeMarket {
            paint_market_player_ids_into_surface_with_font_px_in_panel(
                &mut sb,
                ww,
                wh,
                &batch.player_ids,
                scale as i32,
                label_font_px,
                theme,
                panel_left,
                panel_width,
            );
        }
    }
    let board = renderer.board(&scene, now);
    pixtuoid::floating::offscreen::paint_wall_board_into_surface(
        &mut sb,
        ww,
        wh,
        &board,
        scale as i32,
        theme,
    );
    if let Some(selector) = renderer.map_selector_text() {
        paint_map_selector_into_surface(&mut sb, ww, wh, &selector, label_font_px);
        paint_size_selector_into_surface(&mut sb, ww, wh, "大小：中 [Z]", label_font_px);
    }
    // The status footer band (full TUI parity) — audible so the ♩ suffix shows;
    // no transient flash in a static snapshot.
    let budget = pixtuoid::floating::offscreen::footer_budget(ww);
    let footer = renderer.footer(&scene, budget, true, None);
    pixtuoid::floating::offscreen::paint_footer_into_surface(&mut sb, ww, wh, &footer, theme);

    let mut img = RgbImage::new(win_w, win_h);
    for wy in 0..win_h {
        for wx in 0..win_w {
            let px = sb[wy as usize * ww + wx as usize];
            img.put_pixel(
                wx,
                wy,
                ImgRgb([(px >> 16) as u8, (px >> 8) as u8, px as u8]),
            );
        }
    }
    img.save(&out).with_context(|| format!("writing {out}"))?;
    eprintln!("wrote {out} ({win_w}x{win_h}, Maple buffer {bw}x{bh} @{scale}x, {n_agents} agents)");
    Ok(())
}
