//! Original side-scrolling forest backdrop for the Maple Adventure theme.
//!
//! This deliberately replaces the office shell at the existing background
//! seam. Agent simulation, placement, sprites, overlays, and the later
//! y-sorted pass remain untouched. The painter uses only procedural geometry
//! and an original palette; no game-client artwork is embedded here.

use std::time::{SystemTime, UNIX_EPOCH};

use pixtuoid_core::sprite::{Rgb, RgbBuffer};

use super::super::palette::blend_rgb;
use super::TimeOfDayLook;

const SKY_TOP_DAY: Rgb = Rgb {
    r: 102,
    g: 184,
    b: 214,
};
const SKY_HORIZON_DAY: Rgb = Rgb {
    r: 201,
    g: 226,
    b: 185,
};
const HILL_FAR_DAY: Rgb = Rgb {
    r: 113,
    g: 169,
    b: 131,
};
const HILL_NEAR_DAY: Rgb = Rgb {
    r: 69,
    g: 137,
    b: 91,
};
const FOREST_DARK_DAY: Rgb = Rgb {
    r: 42,
    g: 105,
    b: 70,
};
const GRASS_LIGHT_DAY: Rgb = Rgb {
    r: 151,
    g: 205,
    b: 82,
};
const GRASS_DAY: Rgb = Rgb {
    r: 88,
    g: 157,
    b: 61,
};
const GRASS_SHADOW_DAY: Rgb = Rgb {
    r: 48,
    g: 112,
    b: 54,
};
const DIRT_DAY: Rgb = Rgb {
    r: 135,
    g: 91,
    b: 52,
};
const DIRT_DARK_DAY: Rgb = Rgb {
    r: 81,
    g: 55,
    b: 43,
};
const WOOD_DAY: Rgb = Rgb {
    r: 126,
    g: 77,
    b: 40,
};
const WOOD_LIGHT_DAY: Rgb = Rgb {
    r: 184,
    g: 124,
    b: 65,
};
const WOOD_DARK_DAY: Rgb = Rgb {
    r: 76,
    g: 46,
    b: 35,
};
const LEAF_DAY: Rgb = Rgb {
    r: 65,
    g: 139,
    b: 62,
};
const LEAF_LIGHT_DAY: Rgb = Rgb {
    r: 105,
    g: 177,
    b: 70,
};
const WINDOW_GLOW_DAY: Rgb = Rgb {
    r: 255,
    g: 218,
    b: 112,
};

const NIGHT: Rgb = Rgb {
    r: 20,
    g: 30,
    b: 55,
};
const NIGHT_GREEN: Rgb = Rgb {
    r: 21,
    g: 55,
    b: 53,
};
const NIGHT_DIRT: Rgb = Rgb {
    r: 55,
    g: 42,
    b: 47,
};

#[derive(Clone, Copy)]
struct ForestPalette {
    sky_top: Rgb,
    sky_horizon: Rgb,
    hill_far: Rgb,
    hill_near: Rgb,
    forest_dark: Rgb,
    grass_light: Rgb,
    grass: Rgb,
    grass_shadow: Rgb,
    dirt: Rgb,
    dirt_dark: Rgb,
    wood: Rgb,
    wood_light: Rgb,
    wood_dark: Rgb,
    leaf: Rgb,
    leaf_light: Rgb,
    window_glow: Rgb,
}

impl ForestPalette {
    fn for_darkness(darkness: f32) -> Self {
        // Keep enough chroma at night for tiny sprites and labels to remain
        // readable; the shared post-pass still contributes the final dim.
        let d = (darkness * 0.72).clamp(0.0, 0.72);
        let green = |day| blend_rgb(day, NIGHT_GREEN, d);
        Self {
            sky_top: blend_rgb(SKY_TOP_DAY, NIGHT, d),
            sky_horizon: blend_rgb(SKY_HORIZON_DAY, NIGHT, d),
            hill_far: green(HILL_FAR_DAY),
            hill_near: green(HILL_NEAR_DAY),
            forest_dark: green(FOREST_DARK_DAY),
            grass_light: green(GRASS_LIGHT_DAY),
            grass: green(GRASS_DAY),
            grass_shadow: green(GRASS_SHADOW_DAY),
            dirt: blend_rgb(DIRT_DAY, NIGHT_DIRT, d),
            dirt_dark: blend_rgb(DIRT_DARK_DAY, NIGHT_DIRT, d),
            wood: blend_rgb(WOOD_DAY, NIGHT_DIRT, d),
            wood_light: blend_rgb(WOOD_LIGHT_DAY, NIGHT_DIRT, d),
            wood_dark: blend_rgb(WOOD_DARK_DAY, NIGHT_DIRT, d),
            leaf: green(LEAF_DAY),
            leaf_light: green(LEAF_LIGHT_DAY),
            // A lit treehouse stays warm after dusk instead of being pulled
            // toward navy with the environment.
            window_glow: blend_rgb(
                WINDOW_GLOW_DAY,
                Rgb {
                    r: 255,
                    g: 179,
                    b: 72,
                },
                darkness * 0.2,
            ),
        }
    }
}

/// Paint a complete platform-forest shell in place of the office wall, city
/// windows, and carpet. Coordinates derive from the current buffer so the
/// composition degrades safely at compact TUI/floating sizes.
pub(super) fn paint_forest_scene(
    buf: &mut RgbBuffer,
    buf_w: u16,
    buf_h: u16,
    now: SystemTime,
    look: &TimeOfDayLook,
    top_wall_h: u16,
    altitude: f32,
) {
    let w = buf_w.min(buf.width());
    let h = buf_h.min(buf.height());
    if w == 0 || h == 0 {
        return;
    }

    let palette = ForestPalette::for_darkness(look.darkness);
    let band_h = top_wall_h.min(h.saturating_sub(1));
    let play_h = h.saturating_sub(band_h);
    let ground_y = band_h
        .saturating_add(play_h.saturating_mul(3) / 4)
        .min(h.saturating_sub(1));
    let horizon_y = band_h
        .saturating_add(play_h / 6)
        .min(ground_y.saturating_sub(1));

    paint_sky(buf, w, h, ground_y, palette);
    paint_hills(buf, w, ground_y, horizon_y, altitude, palette);
    paint_far_forest(buf, w, ground_y, horizon_y, palette);

    // Three readable traversal bands establish a side-scrolling composition
    // while leaving the existing simulation coordinates untouched.
    let upper_y = band_h
        .saturating_add(play_h / 3)
        .min(ground_y.saturating_sub(6));
    let middle_y = band_h
        .saturating_add(play_h.saturating_mul(7) / 12)
        .min(ground_y.saturating_sub(3));
    paint_platform(buf, 2, w.saturating_mul(2) / 5, upper_y, palette);
    paint_platform(
        buf,
        w.saturating_mul(7) / 20,
        w.saturating_mul(4) / 5,
        middle_y,
        palette,
    );
    paint_ground(buf, w, h, ground_y, palette);
    paint_treehouse(buf, w, h, upper_y, middle_y, ground_y, palette);
    paint_fireflies(buf, w, ground_y, now, palette.window_glow);
}

/// Wood-and-leaf backing for the shared status-board text overlay. Keeping the
/// original panel bounds preserves overlay alignment while removing its neon
/// office vocabulary.
pub(super) fn paint_quest_board(buf: &mut RgbBuffer, x: u16, y: u16, w: u16, h: u16) {
    const FRAME: Rgb = Rgb {
        r: 184,
        g: 124,
        b: 65,
    };
    const FRAME_DARK: Rgb = Rgb {
        r: 93,
        g: 57,
        b: 37,
    };
    const INSET: Rgb = Rgb {
        r: 34,
        g: 49,
        b: 39,
    };
    const LEAF: Rgb = Rgb {
        r: 96,
        g: 158,
        b: 65,
    };

    for dy in 0..h {
        for dx in 0..w {
            let border = dx == 0 || dy == 0 || dx + 1 == w || dy + 1 == h;
            put(
                buf,
                x as i32 + dx as i32,
                y as i32 + dy as i32,
                if border {
                    if (dx + dy) % 4 == 2 {
                        FRAME_DARK
                    } else {
                        FRAME
                    }
                } else {
                    INSET
                },
            );
        }
    }
    put(buf, x as i32 - 1, y as i32, LEAF);
    put(buf, x as i32, y as i32 - 1, LEAF);
    put(buf, x as i32 + w as i32, y as i32 + h as i32 - 2, LEAF);
}

/// Seven-pixel hanging wayfinder that occupies the former clock seam. Its
/// needle advances slowly so the object stays alive without suggesting an
/// office wall clock.
pub(super) fn paint_wayfinder(buf: &mut RgbBuffer, x: u16, y: u16, now: SystemTime) {
    const RIM: Rgb = Rgb {
        r: 178,
        g: 116,
        b: 61,
    };
    const FACE: Rgb = Rgb {
        r: 229,
        g: 204,
        b: 134,
    };
    const NEEDLE: Rgb = Rgb {
        r: 172,
        g: 65,
        b: 48,
    };
    const LEAF: Rgb = Rgb {
        r: 86,
        g: 153,
        b: 65,
    };

    let rows: &[&[u8]] = &[
        b"..RRR..", b".RFFFR.", b"RFFFFFR", b"RFFFFFR", b"RFFFFFR", b".RFFFR.", b"..RRR..",
    ];
    for (dy, row) in rows.iter().enumerate() {
        for (dx, cell) in row.iter().enumerate() {
            let color = match cell {
                b'R' => RIM,
                b'F' => FACE,
                _ => continue,
            };
            put(buf, x as i32 + dx as i32, y as i32 + dy as i32, color);
        }
    }

    let phase = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / 8 % 8;
    const NEEDLE_DIRS: [(i32, i32); 8] = [
        (0, -2),
        (1, -1),
        (2, 0),
        (1, 1),
        (0, 2),
        (-1, 1),
        (-2, 0),
        (-1, -1),
    ];
    let (dx, dy) = NEEDLE_DIRS[phase as usize];
    put(buf, x as i32 + 3, y as i32 + 3, NEEDLE);
    put(buf, x as i32 + 3 + dx, y as i32 + 3 + dy, NEEDLE);
    put(buf, x as i32 + 5, y as i32, LEAF);
    put(buf, x as i32 + 6, y as i32 - 1, LEAF);
}

fn paint_sky(buf: &mut RgbBuffer, w: u16, h: u16, ground_y: u16, p: ForestPalette) {
    let denom = ground_y.max(1) as f32;
    for y in 0..h {
        let t = (y.min(ground_y) as f32 / denom).clamp(0.0, 1.0);
        let color = blend_rgb(p.sky_top, p.sky_horizon, t);
        for x in 0..w {
            buf.put(x, y, color);
        }
    }
}

fn paint_hills(
    buf: &mut RgbBuffer,
    w: u16,
    ground_y: u16,
    horizon_y: u16,
    altitude: f32,
    p: ForestPalette,
) {
    let parallax = (altitude * 2.0).round().clamp(-4.0, 4.0) as i32;
    paint_hill_layer(
        buf,
        w,
        ground_y,
        horizon_y as i32 + 4 - parallax,
        31,
        7,
        p.hill_far,
    );
    paint_hill_layer(
        buf,
        w,
        ground_y,
        horizon_y as i32 + 8 - parallax,
        23,
        5,
        p.hill_near,
    );
}

fn paint_hill_layer(
    buf: &mut RgbBuffer,
    w: u16,
    ground_y: u16,
    base_y: i32,
    period: u16,
    relief: i32,
    color: Rgb,
) {
    for x in 0..w {
        let phase = x % period;
        let half = period / 2;
        let triangle = if phase <= half { phase } else { period - phase };
        let ridge = base_y - (triangle as i32 * relief / half.max(1) as i32);
        for y in ridge.max(0)..ground_y as i32 {
            put(buf, x as i32, y, color);
        }
    }
}

fn paint_far_forest(buf: &mut RgbBuffer, w: u16, ground_y: u16, horizon_y: u16, p: ForestPalette) {
    let mut x = 3u16;
    while x < w {
        let hash = (x as u32).wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let trunk_h = 7 + (hash % 7) as i32;
        let base = (ground_y.saturating_sub(2)) as i32;
        let top = (horizon_y as i32 + 2).min(base - trunk_h);
        fill_rect(
            buf,
            x as i32,
            top,
            2,
            (base - top).max(1) as u16,
            p.forest_dark,
        );
        fill_circle(buf, x as i32, top, 4, p.hill_near);
        fill_circle(buf, x as i32 + 3, top + 2, 3, p.hill_near);
        x = x.saturating_add(10 + (hash % 4) as u16);
    }
}

fn paint_platform(buf: &mut RgbBuffer, x0: u16, x1: u16, y: u16, p: ForestPalette) {
    let x1 = x1.min(buf.width());
    if x0 >= x1 || y >= buf.height() {
        return;
    }
    for x in x0..x1 {
        buf.put(x, y, p.grass_light);
        if y + 1 < buf.height() {
            buf.put(x, y + 1, p.grass);
        }
        let depth = 4 + ((x.wrapping_mul(7) + y) % 3);
        if y + 2 < buf.height() {
            buf.put(x, y + 2, p.grass_shadow);
        }
        for dy in 3..depth {
            if y + dy < buf.height() {
                let color = if (x + dy) % 5 == 0 {
                    p.dirt_dark
                } else {
                    p.dirt
                };
                buf.put(x, y + dy, color);
            }
        }
        if x % 9 == 2 {
            put(buf, x as i32, y as i32 - 1, p.grass_light);
        }
    }
}

fn paint_ground(buf: &mut RgbBuffer, w: u16, h: u16, y: u16, p: ForestPalette) {
    for x in 0..w {
        buf.put(x, y, p.grass_light);
        if y + 1 < h {
            buf.put(x, y + 1, p.grass);
        }
        if y + 2 < h {
            buf.put(x, y + 2, p.grass_shadow);
        }
        for py in y.saturating_add(3)..h {
            let strata = ((x as u32 * 13 + py as u32 * 7) % 17) < 3;
            buf.put(x, py, if strata { p.dirt_dark } else { p.dirt });
        }
        if x % 11 == 4 {
            put(buf, x as i32, y as i32 - 1, p.grass_light);
        }
    }
}

fn paint_treehouse(
    buf: &mut RgbBuffer,
    w: u16,
    h: u16,
    upper_y: u16,
    middle_y: u16,
    ground_y: u16,
    p: ForestPalette,
) {
    if w < 44 || h < 28 {
        return;
    }

    let tree_x = (w as i32 * 3 / 4).clamp(28, w as i32 - 8);
    let deck_y = (upper_y as i32 + 4).min(middle_y as i32 - 2).max(13);
    let house_x = tree_x - 18;
    let house_y = (deck_y - 10).max(4);

    // Canopy and trunk are behind the house; asymmetric circles avoid a
    // generic cloud silhouette and keep the pixel-art edge lively.
    fill_circle(buf, tree_x - 3, house_y - 3, 8, p.leaf);
    fill_circle(buf, tree_x + 5, house_y - 5, 7, p.leaf);
    fill_circle(buf, tree_x + 10, house_y + 1, 6, p.leaf_light);
    fill_circle(buf, tree_x - 10, house_y + 1, 5, p.leaf_light);
    fill_rect(
        buf,
        tree_x - 3,
        house_y,
        7,
        (ground_y as i32 - house_y + 5).max(1) as u16,
        p.wood,
    );
    fill_rect(
        buf,
        tree_x + 2,
        house_y + 2,
        2,
        (ground_y as i32 - house_y + 2).max(1) as u16,
        p.wood_dark,
    );

    // Branch-supported deck and compact cabin.
    fill_rect(buf, house_x - 4, deck_y, 31, 2, p.wood_dark);
    fill_rect(buf, house_x - 3, deck_y - 1, 29, 1, p.wood_light);
    fill_rect(buf, house_x, house_y, 18, 10, p.wood);
    for y in (house_y + 2..deck_y).step_by(3) {
        fill_rect(buf, house_x, y, 18, 1, p.wood_light);
    }

    // Leafy pitched roof, warm window, and a dark plank door.
    for row in 0..5 {
        fill_rect(
            buf,
            house_x - 2 + row,
            house_y - 5 + row,
            (22 - row * 2) as u16,
            1,
            if row % 2 == 0 { p.leaf_light } else { p.leaf },
        );
    }
    fill_rect(buf, house_x + 4, house_y + 3, 4, 3, p.window_glow);
    put(buf, house_x + 6, house_y + 3, p.wood_dark);
    fill_rect(buf, house_x + 12, house_y + 3, 4, 7, p.wood_dark);
    put(buf, house_x + 13, house_y + 6, p.wood_light);

    // Rope ladder ties the treehouse to the next horizontal traversal band.
    let ladder_x = tree_x + 8;
    let ladder_bottom = (middle_y as i32 + 1).min(h as i32 - 1);
    for y in deck_y + 1..=ladder_bottom {
        put(buf, ladder_x, y, p.wood_light);
        put(buf, ladder_x + 4, y, p.wood_light);
        if (y - deck_y) % 3 == 0 {
            fill_rect(buf, ladder_x, y, 5, 1, p.wood);
        }
    }
}

fn paint_fireflies(buf: &mut RgbBuffer, w: u16, ground_y: u16, now: SystemTime, color: Rgb) {
    if w < 12 || ground_y < 8 {
        return;
    }
    let tick = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        / 450;
    for i in 0..9u64 {
        let seed = i.wrapping_mul(0x9e37_79b9).wrapping_add(tick / 3);
        let x = 4 + (seed.wrapping_mul(17) % (w.saturating_sub(8) as u64)) as u16;
        let y =
            4 + (seed.wrapping_mul(11).rotate_left(7) % (ground_y.saturating_sub(6) as u64)) as u16;
        if !(seed + tick).is_multiple_of(4) {
            buf.put(x, y, color);
        }
    }
}

fn fill_rect(buf: &mut RgbBuffer, x: i32, y: i32, w: u16, h: u16, color: Rgb) {
    for dy in 0..h as i32 {
        for dx in 0..w as i32 {
            put(buf, x + dx, y + dy, color);
        }
    }
}

fn fill_circle(buf: &mut RgbBuffer, cx: i32, cy: i32, radius: i32, color: Rgb) {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius * radius {
                put(buf, cx + dx, cy + dy, color);
            }
        }
    }
}

fn put(buf: &mut RgbBuffer, x: i32, y: i32, color: Rgb) {
    if x >= 0 && y >= 0 && x < buf.width() as i32 && y < buf.height() as i32 {
        buf.put(x as u16, y as u16, color);
    }
}
