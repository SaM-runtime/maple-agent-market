use std::time::SystemTime;

use crate::layout::WALKING_Y_OFF;
use pixtuoid_core::sprite::{Rgb, RgbBuffer};

use super::epoch_ms;
use super::palette::{blend_pixel, blend_rgb};
use crate::layout::Point;
use crate::theme::Theme;

pub(super) fn paint_screen_glow(
    buf: &mut RgbBuffer,
    desk_x: u16,
    desk_y: u16,
    now: SystemTime,
    tint: Rgb,
    theme: &Theme,
) {
    let frame_lit = theme.effects.monitor_frame_lit;
    let glow = tint;
    let white = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };
    let glow_bright = blend_rgb(tint, white, 0.4);
    let scanline = blend_rgb(tint, white, 0.7);
    let put = |buf: &mut RgbBuffer, dx: u16, dy: u16, c: Rgb| {
        buf.put_checked(desk_x + dx, desk_y + dy, c);
    };
    for dx in 3..=10 {
        put(buf, dx, 0, frame_lit);
    }
    for dx in 4..=9 {
        put(buf, dx, 1, glow_bright);
        put(buf, dx, 2, glow);
    }
    for dx in 4..=9 {
        put(buf, dx, 3, frame_lit);
    }
    // Screen scanline advances one column per this interval.
    const SCANLINE_STEP_MS: u64 = 120;
    let elapsed_ms = epoch_ms(now);
    let phase = (elapsed_ms / SCANLINE_STEP_MS) as u16 + desk_x;
    let scan_col = 4 + (phase % 6);
    put(buf, scan_col, 1, scanline);
    put(buf, scan_col, 2, scanline);
}

pub(super) fn paint_sleep_z(
    buf: &mut RgbBuffer,
    head_anchor: Point,
    now: SystemTime,
    seed: u64,
    theme: &Theme,
) {
    let z_color = theme.effects.sleep_z;
    // One z drifts up from just above the head — brightest at the head, fading
    // to nothing as it climbs. The height-coupled fade (`1.0 - t`) is what keeps
    // it from reading as a solid mark parked over the sprite: it's only briefly
    // visible near the head, then dissolves. RISE_MS is the visible rise+fade
    // span; a short REST_MS gap separates one z from the next.
    const RISE_MS: u64 = 2000;
    const REST_MS: u64 = 400;
    const CYCLE_MS: u64 = RISE_MS + REST_MS;
    const MAX_RISE: u16 = 4;
    const FADE_IN_MS: f32 = 150.0;
    const PEAK_ALPHA: f32 = 0.9;
    let phase_ms = epoch_ms(now).wrapping_add(seed % CYCLE_MS) % CYCLE_MS;
    if phase_ms >= RISE_MS {
        return;
    }
    let t = phase_ms as f32 / RISE_MS as f32;
    // Quick ramp-in over the first FADE_IN_MS avoids a hard pop when a fresh z
    // spawns at the head; the `1.0 - t` term then fades it out as it rises.
    let fade_in = (phase_ms as f32 / FADE_IN_MS).min(1.0);
    let alpha = PEAK_ALPHA * fade_in * (1.0 - t);
    if alpha < 0.06 {
        return;
    }
    let rise = (t * MAX_RISE as f32) as u16;
    let z_x = head_anchor.x + 5;
    let z_y = head_anchor.y.saturating_sub(rise + 3);
    const GLYPH: &[(u16, u16)] = &[(0, 0), (1, 0), (1, 1), (0, 2), (1, 2)];
    for (dx, dy) in GLYPH {
        blend_pixel(buf, z_x + dx, z_y + dy, z_color, alpha);
    }
}

pub(super) fn paint_coffee_steam(buf: &mut RgbBuffer, base: Point, now: SystemTime, theme: &Theme) {
    let steam = theme.effects.coffee_steam;
    // Each steam plume fades over one full cycle; 3 plumes staggered by cycle/3.
    const STEAM_CYCLE_MS: u64 = 1800;
    let elapsed_ms = epoch_ms(now);
    for offset in 0..3u64 {
        let phase = (elapsed_ms + offset * (STEAM_CYCLE_MS / 3)) % STEAM_CYCLE_MS;
        let rise = (phase / 140) as u16;
        let alpha = 1.0 - phase as f32 / STEAM_CYCLE_MS as f32;
        if alpha < 0.15 {
            continue;
        }
        let wiggle = if (phase / 200).is_multiple_of(2) {
            0
        } else {
            1
        };
        let px = base.x + wiggle;
        let py = base.y.saturating_sub(rise + 2);
        blend_pixel(buf, px, py, steam, alpha * 0.55);
    }
}

pub(super) fn paint_walking_dust(
    buf: &mut RgbBuffer,
    walker_anchor: Point,
    frame_idx: usize,
    theme: &Theme,
) {
    let dust = theme.effects.walking_dust;
    let foot_y = walker_anchor.y + WALKING_Y_OFF;
    let foot_x = walker_anchor.x + if frame_idx == 0 { 6 } else { 1 };
    blend_pixel(buf, foot_x, foot_y, dust, 0.45);
}

/// Floating heart particles for the "pet the cat" interaction.
/// 4 hearts, staggered 150ms apart, each rising 6px over 1550ms and
/// fading via alpha blend toward the background. Last heart starts at
/// 450ms so all 4 complete within PET_DURATION_MS (2000ms).
pub(super) fn paint_pet_hearts(buf: &mut RgbBuffer, cat_pos: Point, elapsed_ms: u64) {
    const STAGGER_MS: u64 = 150;
    const HEART_LIFE_MS: u64 = 1550;
    let heart_color = Rgb {
        r: 255,
        g: 100,
        b: 100,
    };
    for i in 0..4u64 {
        let stagger = i * STAGGER_MS;
        if elapsed_ms < stagger {
            continue;
        }
        let local_ms = elapsed_ms - stagger;
        if local_ms >= HEART_LIFE_MS {
            continue;
        }
        let t = local_ms as f32 / HEART_LIFE_MS as f32;
        let rise = (t * 6.0) as u16;
        let alpha = 1.0 - t;
        if alpha < 0.05 {
            continue;
        }
        // Spread hearts horizontally: offsets -3, -1, +1, +3
        let dx: i16 = (i as i16) * 2 - 3;
        let hx = (cat_pos.x as i32 + dx as i32).max(0) as u16;
        let hy = cat_pos.y.saturating_sub(4 + rise);
        // 2x2 pixel heart
        for dy in 0..2u16 {
            for ddx in 0..2u16 {
                blend_pixel(buf, hx + ddx, hy + dy, heart_color, alpha * 0.8);
            }
        }
    }
}

pub(super) fn paint_waiting_bubble(buf: &mut RgbBuffer, anchor: Point, theme: &Theme) {
    paint_waiting_bubble_scaled(buf, anchor, theme, 1);
}

pub(super) fn paint_waiting_bubble_scaled(
    buf: &mut RgbBuffer,
    anchor: Point,
    theme: &Theme,
    scale: u16,
) {
    let fg = theme.effects.waiting_bubble;
    const GLYPH: &[&[u8]] = &[b".YYY.", b"...Y.", b"..Y..", b"..Y.."];
    let scale = scale.max(1);
    let bx = anchor.x.saturating_add(scale);
    let by = anchor.y.saturating_sub(5u16.saturating_mul(scale));
    for (dy, row) in GLYPH.iter().enumerate() {
        for (dx, byte) in row.iter().enumerate() {
            if *byte != b'Y' {
                continue;
            }
            let px = bx.saturating_add((dx as u16).saturating_mul(scale));
            let py = by.saturating_add((dy as u16).saturating_mul(scale));
            for sy in 0..scale {
                for sx in 0..scale {
                    let out_x = px.saturating_add(sx);
                    let out_y = py.saturating_add(sy);
                    if out_x < buf.width() && out_y < buf.height() {
                        buf.put(out_x, out_y, fg);
                    }
                }
            }
        }
    }
}

const MARKET_SCROLL_SUCCESS_CORE: Rgb = Rgb {
    r: 0xff,
    g: 0xfa,
    b: 0xe0,
};
const MARKET_SCROLL_SUCCESS_WARM: Rgb = Rgb {
    r: 0xff,
    g: 0xe5,
    b: 0x72,
};
const MARKET_SCROLL_SUCCESS_EDGE: Rgb = Rgb {
    r: 0xf3,
    g: 0xb7,
    b: 0x35,
};

const SCROLL_FLASH_SMALL: &[&[u8]] = &[b"..W..", b"..W..", b"WWYWW", b"..W..", b"..W.."];
const SCROLL_FLASH_MEDIUM: &[&[u8]] = &[
    b"...W...", b"..WWW..", b"...W...", b"WYGGGYW", b"...W...", b"..WWW..", b"...W...",
];
// The old-game success frame reads as four white petals opening behind the
// character, with only a small warm transfer core. Keeping the arms separated
// stops this from turning into a modern circular aura or a generic explosion.
const SCROLL_FLASH_LARGE: &[&[u8]] = &[
    b".....W.....",
    b"....WWW....",
    b".....W.....",
    b"..W..Y..W..",
    b".WWWYGYWWW.",
    b"WYYGGWGGYYW",
    b".WWWYGYWWW.",
    b"..W..Y..W..",
    b".....W.....",
    b"....WWW....",
    b".....W.....",
];
const SCROLL_GLINT: &[&[u8]] = &[b"W", b"Y"];

fn paint_scaled_pattern(buf: &mut RgbBuffer, centre: Point, scale: u16, pattern: &[&[u8]]) {
    let scale = scale.max(1);
    let width = pattern.iter().map(|row| row.len()).max().unwrap_or(0) as i32;
    let height = pattern.len() as i32;
    let origin_x = i32::from(centre.x) - width * i32::from(scale) / 2;
    let origin_y = i32::from(centre.y) - height * i32::from(scale) / 2;
    for (row, pixels) in pattern.iter().enumerate() {
        for (column, pixel) in pixels.iter().enumerate() {
            let color = match pixel {
                b'W' => MARKET_SCROLL_SUCCESS_CORE,
                b'Y' => MARKET_SCROLL_SUCCESS_WARM,
                b'G' => MARKET_SCROLL_SUCCESS_EDGE,
                _ => continue,
            };
            let cell_x = origin_x + column as i32 * i32::from(scale);
            let cell_y = origin_y + row as i32 * i32::from(scale);
            for sy in 0..scale {
                for sx in 0..scale {
                    let x = cell_x + i32::from(sx);
                    let y = cell_y + i32::from(sy);
                    if x >= 0 && y >= 0 && x < i32::from(buf.width()) && y < i32::from(buf.height())
                    {
                        buf.put(x as u16, y as u16, color);
                    }
                }
            }
        }
    }
}

/// Classic scroll-success transfer flash, reconstructed as programmatic pixel
/// art from observed gameplay rather than copied game assets. It is painted
/// behind the actor: the white petals remain visible around the body while the
/// paperdoll and shop card stay legible on top.
pub(super) fn paint_market_scroll_success(
    buf: &mut RgbBuffer,
    centre: Point,
    elapsed_ms: u64,
    scale: u16,
) {
    let pattern = match elapsed_ms {
        0..=119 => SCROLL_FLASH_SMALL,
        120..=299 => SCROLL_FLASH_MEDIUM,
        300..=519 => SCROLL_FLASH_LARGE,
        520..=719 => SCROLL_FLASH_MEDIUM,
        720..=959 => SCROLL_FLASH_SMALL,
        _ => return,
    };
    let scale = scale.max(1);
    let rise = if elapsed_ms >= 840 {
        scale.saturating_mul(2)
    } else if elapsed_ms >= 720 {
        scale
    } else {
        0
    };
    paint_scaled_pattern(
        buf,
        Point {
            x: centre.x,
            y: centre.y.saturating_sub(rise),
        },
        scale,
        pattern,
    );

    // The transfer flash finishes with two detached pixels drifting upward.
    // Their asymmetry keeps the exit legible as loose petals rather than a
    // modern circular aura, and the whole effect still ends before 960 ms.
    if elapsed_ms >= 720 {
        let (left_lift, right_lift) = if elapsed_ms >= 840 { (6, 5) } else { (4, 3) };
        let spread = if elapsed_ms >= 840 { 5 } else { 4 };
        paint_scaled_pattern(
            buf,
            Point {
                x: centre.x.saturating_sub(scale.saturating_mul(spread)),
                y: centre.y.saturating_sub(scale.saturating_mul(left_lift)),
            },
            scale,
            SCROLL_GLINT,
        );
        paint_scaled_pattern(
            buf,
            Point {
                x: centre.x.saturating_add(scale.saturating_mul(spread)),
                y: centre.y.saturating_sub(scale.saturating_mul(right_lift)),
            },
            scale,
            SCROLL_GLINT,
        );
    }
}

const PUBLIC_SKILL_CORE: Rgb = Rgb {
    r: 0xf8,
    g: 0xfd,
    b: 0xff,
};
const ARCANE_CLAW_LIGHT: Rgb = Rgb {
    r: 0x7d,
    g: 0xe9,
    b: 0xff,
};
const ARCANE_CLAW_EDGE: Rgb = Rgb {
    r: 0x6e,
    g: 0x54,
    b: 0xe8,
};
const HOLY_LIGHT_GOLD: Rgb = Rgb {
    r: 0xff,
    g: 0xdf,
    b: 0x66,
};
const HOLY_LIGHT_EDGE: Rgb = Rgb {
    r: 0x8b,
    g: 0xd9,
    b: 0xff,
};
const DRAGON_PULSE_CORE: Rgb = Rgb {
    r: 0xff,
    g: 0x58,
    b: 0x4f,
};
const DRAGON_PULSE_EDGE: Rgb = Rgb {
    r: 0xb7,
    g: 0x16,
    b: 0x3d,
};
const DRAGON_PULSE_GLOW: Rgb = Rgb {
    r: 0xff,
    g: 0xcf,
    b: 0x72,
};

fn blend_training_skill_cell(
    buf: &mut RgbBuffer,
    foot: Point,
    dx: i32,
    dy: i32,
    scale: u16,
    color: Rgb,
    alpha: f32,
) {
    let scale = scale.max(1);
    let origin_x = i32::from(foot.x) + dx * i32::from(scale);
    let origin_y = i32::from(foot.y) + dy * i32::from(scale);
    for sy in 0..scale {
        for sx in 0..scale {
            let x = origin_x + i32::from(sx);
            let y = origin_y + i32::from(sy);
            if x >= 0 && y >= 0 && x < i32::from(buf.width()) && y < i32::from(buf.height()) {
                blend_pixel(buf, x as u16, y as u16, color, alpha.clamp(0.0, 1.0));
            }
        }
    }
}

/// Zero-asset training VFX shipped with the public build. These small,
/// programmatic silhouettes are original to this fork: no game frame or
/// extracted client art is embedded. A pack may layer separately licensed
/// `training_skill_*` art over this baseline through the caller.
pub(super) fn paint_public_training_skill(
    buf: &mut RgbBuffer,
    foot: Point,
    kind: crate::training::TrainingSkillKind,
    frame_index: usize,
    scale: u16,
) {
    let frame = frame_index.min(3);
    match kind {
        crate::training::TrainingSkillKind::MagicClaw => {
            paint_public_arcane_claw(buf, foot, frame, scale)
        }
        crate::training::TrainingSkillKind::HolyLight => {
            paint_public_holy_light(buf, foot, frame, scale)
        }
        crate::training::TrainingSkillKind::DragonPulse => {
            paint_public_dragon_pulse(buf, foot, frame, scale)
        }
    }
}

fn paint_public_arcane_claw(buf: &mut RgbBuffer, foot: Point, frame: usize, scale: u16) {
    let reach = [14, 18, 24, 28][frame];
    let fade = [0.78, 0.88, 0.96, 0.68][frame];
    for slash in 0..2i32 {
        for step in 0..=reach {
            let dx = -2 + step;
            let dy = -24 + slash * 7 + step / 2;
            blend_training_skill_cell(buf, foot, dx, dy, scale, ARCANE_CLAW_EDGE, fade);
            blend_training_skill_cell(buf, foot, dx + 1, dy, scale, ARCANE_CLAW_LIGHT, fade * 0.92);
            blend_training_skill_cell(
                buf,
                foot,
                dx + 1,
                dy + 1,
                scale,
                ARCANE_CLAW_EDGE,
                fade * 0.58,
            );
            if step.rem_euclid(4) == 0 {
                blend_training_skill_cell(
                    buf,
                    foot,
                    dx + 2,
                    dy - 1,
                    scale,
                    PUBLIC_SKILL_CORE,
                    fade * 0.78,
                );
            }
        }
    }
}

fn paint_public_holy_light(buf: &mut RgbBuffer, foot: Point, frame: usize, scale: u16) {
    let height = [28, 38, 48, 42][frame];
    let alpha = [0.76, 0.86, 0.94, 0.64][frame];
    for (column, dx) in [-18i32, -9, 0, 9, 18].into_iter().enumerate() {
        let stagger = ((column + frame) % 3) as i32 * 3;
        let top = -height + stagger;
        let bottom = -5 - stagger / 2;
        for dy in (top..=bottom).step_by(2) {
            let color = if dx == 0 || dy.rem_euclid(6) == 0 {
                PUBLIC_SKILL_CORE
            } else if column.is_multiple_of(2) {
                HOLY_LIGHT_GOLD
            } else {
                HOLY_LIGHT_EDGE
            };
            blend_training_skill_cell(buf, foot, dx, dy, scale, color, alpha);
            if dx == 0 {
                blend_training_skill_cell(
                    buf,
                    foot,
                    dx + 1,
                    dy,
                    scale,
                    HOLY_LIGHT_GOLD,
                    alpha * 0.72,
                );
            }
        }
    }

    let wing = [12, 16, 20, 22][frame];
    for step in 0..=wing {
        let lift = step / 3;
        for direction in [-1i32, 1] {
            blend_training_skill_cell(
                buf,
                foot,
                direction * step,
                -17 - lift,
                scale,
                HOLY_LIGHT_GOLD,
                alpha * 0.76,
            );
        }
    }
}

fn paint_public_dragon_pulse(buf: &mut RgbBuffer, foot: Point, frame: usize, scale: u16) {
    let radius_x = [8i32, 14, 22, 30][frame];
    let radius_y = [5i32, 9, 13, 17][frame];
    let alpha = [0.52, 0.74, 0.92, 0.60][frame];
    let centre_y = -10i32;
    let outer = radius_x * radius_x * radius_y * radius_y;
    let inner_x = (radius_x - 2).max(1);
    let inner_y = (radius_y - 2).max(1);
    let inner = inner_x * inner_x * inner_y * inner_y;
    for dy in -radius_y..=radius_y {
        for dx in -radius_x..=radius_x {
            let outer_distance = dx * dx * radius_y * radius_y + dy * dy * radius_x * radius_x;
            let inner_distance = dx * dx * inner_y * inner_y + dy * dy * inner_x * inner_x;
            if outer_distance > outer || inner_distance < inner {
                continue;
            }
            let color = if (dx + dy).rem_euclid(5) == 0 {
                DRAGON_PULSE_GLOW
            } else if dx.unsigned_abs() > dy.unsigned_abs() {
                DRAGON_PULSE_CORE
            } else {
                DRAGON_PULSE_EDGE
            };
            blend_training_skill_cell(buf, foot, dx, centre_y + dy, scale, color, alpha);
        }
    }

    let spike = 2 + frame as i32;
    for step in 1..=spike {
        for (dx, dy) in [
            (-radius_x - step, centre_y),
            (radius_x + step, centre_y),
            (-radius_x * 3 / 4 - step / 2, centre_y - radius_y - step / 2),
            (radius_x * 3 / 4 + step / 2, centre_y - radius_y - step / 2),
            (-radius_x * 3 / 4 - step / 2, centre_y + radius_y + step / 2),
            (radius_x * 3 / 4 + step / 2, centre_y + radius_y + step / 2),
        ] {
            blend_training_skill_cell(
                buf,
                foot,
                dx,
                dy,
                scale,
                if step.rem_euclid(2) == 0 {
                    DRAGON_PULSE_GLOW
                } else {
                    DRAGON_PULSE_CORE
                },
                alpha * 0.82,
            );
        }
    }
}

const MAPLE_LEVEL_UP_CORE: Rgb = Rgb {
    r: 0xf2,
    g: 0xfe,
    b: 0xff,
};
const MAPLE_LEVEL_UP_CYAN: Rgb = Rgb {
    r: 0x3d,
    g: 0xd9,
    b: 0xff,
};
const MAPLE_LEVEL_UP_BLUE: Rgb = Rgb {
    r: 0x00,
    g: 0x7c,
    b: 0xff,
};
const MAPLE_LEVEL_UP_EDGE: Rgb = Rgb {
    r: 0x00,
    g: 0x38,
    b: 0xc8,
};

fn blend_level_up_cell(
    buf: &mut RgbBuffer,
    foot: Point,
    dx: i32,
    rise: u16,
    scale: u16,
    color: Rgb,
    alpha: f32,
) {
    let scale = scale.max(1);
    let origin_x = i32::from(foot.x) + dx * i32::from(scale);
    let origin_y = i32::from(foot.y) - i32::from(rise.saturating_mul(scale));
    for sy in 0..scale {
        for sx in 0..scale {
            let x = origin_x + i32::from(sx);
            let y = origin_y + i32::from(sy);
            if x >= 0 && y >= 0 && x < i32::from(buf.width()) && y < i32::from(buf.height()) {
                blend_pixel(buf, x as u16, y as u16, color, alpha.clamp(0.0, 1.0));
            }
        }
    }
}

/// Classic-style Maple level-up pillar reconstructed as programmatic pixel
/// art from gameplay references. No game frame is embedded. The effect is
/// actor-centred and painted behind the paperdoll: a narrow white/cyan core,
/// blue outer shaft, paired rising ribbons and detached final glints.
pub(super) fn paint_maple_level_up_pillar(
    buf: &mut RgbBuffer,
    foot: Point,
    elapsed_ms: u64,
    scale: u16,
) {
    if elapsed_ms >= crate::market::MARKET_TURN_COMPLETE_MS {
        return;
    }

    let scale = scale.max(1);
    let open = (elapsed_ms as f32 / 260.0).clamp(0.0, 1.0);
    let tail_fade = if elapsed_ms <= 1_400 {
        1.0
    } else {
        (1.0 - (elapsed_ms - 1_400) as f32 / 800.0).clamp(0.0, 1.0)
    };
    let shaft_fade = if elapsed_ms <= 1_350 {
        1.0
    } else {
        (1.0 - (elapsed_ms - 1_350) as f32 / 500.0).clamp(0.0, 1.0)
    };
    let alpha = open * shaft_fade;
    let height = if elapsed_ms < 420 {
        8 + ((elapsed_ms * 40) / 420) as u16
    } else {
        48
    };

    // The column grows from the actor's feet. Alternating one-pixel edge
    // notches keep it authored/pixel-like instead of reading as a smooth web
    // gradient; the hot centre stays narrow enough that the body is legible.
    for rise in 1..=height {
        let half_width: i32 = if rise < 7 {
            5
        } else if rise > 41 {
            2
        } else {
            4
        };
        for dx in -half_width..=half_width {
            let distance = dx.unsigned_abs();
            let (color, strength) = match distance {
                0 => (MAPLE_LEVEL_UP_CORE, 0.90),
                1 => (MAPLE_LEVEL_UP_CYAN, 0.82),
                2 | 3 => (MAPLE_LEVEL_UP_BLUE, 0.62),
                _ => (MAPLE_LEVEL_UP_EDGE, 0.48),
            };
            if distance == half_width as u32 && (u32::from(rise) + distance).is_multiple_of(3) {
                continue;
            }
            blend_level_up_cell(buf, foot, dx, rise, scale, color, alpha * strength);
        }
    }

    // Symmetric lower ribbons curl toward the shaft as they rise. At the
    // authored middle frame they cross ±8 px around shoulder height — the
    // recognizable wide base of the old blue-column silhouette.
    for step in 0..15u16 {
        let rise = 6 + step;
        let outward = 9i32 - i32::from(step / 4);
        let ribbon_alpha = alpha * (0.82 - step as f32 * 0.018).max(0.40);
        for dx in [-outward, outward] {
            blend_level_up_cell(
                buf,
                foot,
                dx,
                rise,
                scale,
                MAPLE_LEVEL_UP_BLUE,
                ribbon_alpha,
            );
            if step.is_multiple_of(3) {
                blend_level_up_cell(
                    buf,
                    foot,
                    dx.signum() + dx,
                    rise.saturating_add(1),
                    scale,
                    MAPLE_LEVEL_UP_CYAN,
                    ribbon_alpha * 0.72,
                );
            }
        }
    }

    // A broken horizontal foot-ring gives the beam a grounded source without
    // drawing a modern circular aura over the paperdoll.
    for dx in -11i32..=11 {
        if (-3..=3).contains(&dx) || dx.unsigned_abs().is_multiple_of(3) {
            continue;
        }
        blend_level_up_cell(
            buf,
            foot,
            dx,
            2 + (dx.unsigned_abs() % 2) as u16,
            scale,
            MAPLE_LEVEL_UP_CYAN,
            alpha * 0.76,
        );
    }

    // Detached pixels rise after the shaft peaks, providing the same visual
    // stop as an authored final frame. Time remains absolute, so a 1 FPS window
    // skips forward instead of stretching or looping the celebration.
    if elapsed_ms >= 1_150 {
        let drift = ((elapsed_ms - 1_150) / 120) as u16;
        for (dx, base_rise, color) in [
            (-12, 18, MAPLE_LEVEL_UP_CYAN),
            (11, 25, MAPLE_LEVEL_UP_CORE),
            (-7, 34, MAPLE_LEVEL_UP_BLUE),
            (8, 40, MAPLE_LEVEL_UP_CYAN),
        ] {
            blend_level_up_cell(
                buf,
                foot,
                dx,
                base_rise + drift,
                scale,
                color,
                tail_fade * 0.86,
            );
        }
    }
}

/// The Top-tier flame crown (`burn::BurnTier::Top`) — a 2-frame flicker above
/// the sprite's hair, painted AFTER the character blit so it rides every pose
/// (seated/walking/standing) through the one `paint_character_at` seam. The
/// aesthetic is the user-ratified mockup: tips capped ≤2 px above
/// the hair top so the flame never collides with the name-badge row; the
/// asymmetric two-frame flicker is what reads as fire, not a hat. INTEGER
/// phase division before any float (the epoch-ms-as-f32 freeze sharp edge).
/// The flame gradient's deep-ember base — ONE literal shared with the
/// Premium ember-hair recolor (`palette::agent_palette`), so a gradient
/// tweak can't desync the hair from the crown.
pub(crate) const FLAME_DEEP: Rgb = Rgb {
    r: 0xc2,
    g: 0x28,
    b: 0x12,
};

/// The flame gradient's yellow tip. `pub(crate)` alongside [`FLAME_DEEP`] so
/// render tests assert the REAL painted colors instead of re-hardcoding them.
pub(crate) const FLAME_TIP: Rgb = Rgb {
    r: 0xff,
    g: 0xd2,
    b: 0x4a,
};

pub(super) fn paint_flame_crown(
    buf: &mut RgbBuffer,
    anchor: Point,
    sprite_w: u16,
    now: SystemTime,
) {
    // Ratified flame palette (deep ember → orange → yellow tip → hot core);
    // the deep base is the shared FLAME_DEEP (also the Premium hair recolor).
    const MID: Rgb = Rgb {
        r: 0xe8,
        g: 0x64,
        b: 0x1f,
    };
    const TIP: Rgb = FLAME_TIP;
    const CORE: Rgb = Rgb {
        r: 0xff,
        g: 0xf3,
        b: 0xa0,
    };
    const FLICKER_MS: u64 = 260;
    let f2 = (epoch_ms(now) / FLICKER_MS) % 2 == 1;

    // Head-center column; the crown hugs the hair's top row (anchor.y) and
    // rises two rows above it. Pattern is (dx from center-left, dy up, color).
    let cx = anchor.x + sprite_w / 2;
    let frame_a: &[(i32, u16, Rgb)] = &[
        // crown row over the hair top
        (-2, 0, MID),
        (-1, 0, MID),
        (0, 0, FLAME_DEEP),
        (1, 0, MID),
        // first rise
        (-2, 1, MID),
        (-1, 1, CORE),
        (0, 1, MID),
        (1, 1, TIP),
        // tips
        (-2, 2, TIP),
        (0, 2, TIP),
    ];
    let frame_b: &[(i32, u16, Rgb)] = &[
        (-2, 0, MID),
        (-1, 0, FLAME_DEEP),
        (0, 0, MID),
        (1, 0, MID),
        (-2, 1, TIP),
        (-1, 1, MID),
        (0, 1, CORE),
        (1, 1, MID),
        (-1, 2, TIP),
        (1, 2, TIP),
    ];
    for &(dx, dy, c) in if f2 { frame_b } else { frame_a } {
        let Some(px) = cx.checked_add_signed(dx as i16) else {
            continue;
        };
        let Some(py) = anchor.y.checked_sub(dy) else {
            continue;
        };
        if px < buf.width() && py < buf.height() {
            buf.put(px, py, c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn theme() -> &'static Theme {
        crate::theme::theme_by_name("normal").expect("normal theme")
    }

    fn render(head: Point, phase_ms: u64) -> RgbBuffer {
        let mut buf = RgbBuffer::filled(64, 64, Rgb { r: 0, g: 0, b: 0 });
        let now = SystemTime::UNIX_EPOCH + Duration::from_millis(phase_ms);
        paint_sleep_z(&mut buf, head, now, 0, theme());
        buf
    }

    fn lum(c: Rgb) -> u32 {
        c.r as u32 + c.g as u32 + c.b as u32
    }

    // Topmost lit pixel in the z's column, if any (kept independent of MAX_RISE).
    fn top_lit(buf: &RgbBuffer, head: Point, bg: Rgb) -> Option<(u16, Rgb)> {
        let zx = head.x + 5;
        (0..head.y).find_map(|y| {
            let p = buf.get(zx, y);
            (p != bg).then_some((y, p))
        })
    }

    #[test]
    fn sleep_z_dims_as_it_rises_then_rests() {
        let head = Point { x: 20, y: 30 };
        let bg = Rgb { r: 0, g: 0, b: 0 };
        let zx = head.x + 5;

        // Just spawned (rise 0 for any MAX_RISE): brightest, at the spawn row.
        let low = render(head, 200);
        let low_px = low.get(zx, head.y - 3);
        assert!(lum(low_px) > 0, "z near the head is visible");

        // Later it has risen AND faded ("higher = blurrier").
        let high = render(head, 1600);
        let (top_y, top_px) = top_lit(&high, head, bg).expect("risen z still visible");
        assert!(top_y < head.y - 3, "z rose above its spawn row");
        assert!(
            lum(top_px) < lum(low_px),
            "a higher z must be dimmer than one at the head"
        );

        // During the rest gap (phase >= RISE_MS) nothing is painted at all.
        let resting = render(head, 2300);
        for y in 0..resting.height() {
            for x in 0..resting.width() {
                assert_eq!(resting.get(x, y), bg, "no z during the rest gap");
            }
        }
    }

    #[test]
    fn market_scroll_success_finishes_with_separated_rising_glints() {
        let bg = Rgb { r: 0, g: 0, b: 0 };
        let core = Rgb {
            r: 0xff,
            g: 0xfa,
            b: 0xe0,
        };
        let centre = Point { x: 24, y: 24 };
        let mut buf = RgbBuffer::filled(48, 48, bg);

        paint_market_scroll_success(&mut buf, centre, 880, 1);

        assert_eq!(
            buf.get(centre.x - 5, centre.y - 7),
            core,
            "the late left glint must separate from and rise above the shrinking core"
        );
        assert_eq!(
            buf.get(centre.x + 5, centre.y - 6),
            core,
            "the late right glint must use a slightly offset game-like petal path"
        );
        assert_eq!(
            buf.get(centre.x - 8, centre.y - 7),
            bg,
            "the separated glints stay compact instead of becoming a wide aura"
        );
    }

    #[test]
    fn maple_level_up_reads_as_a_tall_blue_pillar_and_clips_safely() {
        let bg = Rgb { r: 0, g: 0, b: 0 };
        let foot = Point { x: 36, y: 58 };
        let mut buf = RgbBuffer::filled(72, 64, bg);

        paint_maple_level_up_pillar(&mut buf, foot, 760, 1);

        let blue_pixels = (0..buf.height())
            .flat_map(|y| (0..buf.width()).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let pixel = buf.get(*x, *y);
                pixel != bg && pixel.b > pixel.r && pixel.b >= pixel.g
            })
            .count();
        assert!(blue_pixels > 90, "the effect needs a readable blue body");
        assert!(
            (0..foot.y.saturating_sub(32)).any(|y| buf.get(foot.x, y) != bg),
            "the central light must rise well above the paperdoll"
        );
        assert_ne!(
            buf.get(foot.x.saturating_sub(8), foot.y.saturating_sub(12)),
            bg,
            "the classic silhouette needs a left flare outside the core"
        );
        assert_ne!(
            buf.get(foot.x.saturating_add(8), foot.y.saturating_sub(12)),
            bg,
            "the classic silhouette needs a right flare outside the core"
        );

        let mut clipped = RgbBuffer::filled(32, 24, bg);
        paint_maple_level_up_pillar(&mut clipped, Point { x: 2, y: 8 }, 760, 2);
        assert!(
            clipped.as_slice().iter().any(|pixel| *pixel != bg),
            "an upper-platform actor must retain the on-screen part of the pillar"
        );
    }

    #[test]
    fn every_public_training_skill_is_readable_from_its_first_frame_at_scale_one() {
        let bg = Rgb { r: 0, g: 0, b: 0 };
        let foot = Point { x: 48, y: 72 };
        let mut counts = Vec::new();

        for kind in [
            crate::training::TrainingSkillKind::MagicClaw,
            crate::training::TrainingSkillKind::HolyLight,
            crate::training::TrainingSkillKind::DragonPulse,
        ] {
            let mut buf = RgbBuffer::filled(96, 96, bg);
            paint_public_training_skill(&mut buf, foot, kind, 0, 1);
            let changed = buf.as_slice().iter().filter(|pixel| **pixel != bg).count();
            counts.push((kind, changed));
        }

        assert!(
            counts.iter().all(|(_, changed)| *changed >= 70),
            "each opening silhouette needs at least 70 lit pixels in the small dual-map view; got {counts:?}"
        );
    }

    #[test]
    fn dragon_pulse_is_a_wide_bounded_roar_instead_of_a_card_high_shield() {
        let bg = Rgb { r: 0, g: 0, b: 0 };
        let foot = Point { x: 48, y: 72 };
        let mut buf = RgbBuffer::filled(96, 96, bg);

        paint_public_training_skill(
            &mut buf,
            foot,
            crate::training::TrainingSkillKind::DragonPulse,
            2,
            1,
        );

        let changed = (0..buf.height())
            .flat_map(|y| (0..buf.width()).map(move |x| (x, y)))
            .filter(|(x, y)| buf.get(*x, *y) != bg)
            .collect::<Vec<_>>();
        let min_x = changed.iter().map(|(x, _)| *x).min().unwrap();
        let max_x = changed.iter().map(|(x, _)| *x).max().unwrap();
        let min_y = changed.iter().map(|(_, y)| *y).min().unwrap();
        let max_y = changed.iter().map(|(_, y)| *y).max().unwrap();

        assert!(max_x - min_x >= 48, "the roar needs a wide horizontal wave");
        assert!(
            max_x - min_x > max_y - min_y + 12,
            "a dragon pulse should not regress to a circular shield"
        );
        assert!(
            min_y >= foot.y - 28,
            "the upper edge must stay below the task-card band"
        );
    }
}
