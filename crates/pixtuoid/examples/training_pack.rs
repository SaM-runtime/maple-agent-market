//! Build the local-only MapleStory Classic Henesys Hunting Ground I pack.
//!
//! The input directory is populated from exact public web/API image endpoints;
//! this tool does not read or unpack a local game client. MapleStory artwork is
//! owned by NEXON and neither the inputs nor generated sprites may be committed
//! or redistributed with the public source project.
//!
//! Usage:
//! `cargo run -p pixtuoid --example training_pack -- <hhg1-source> <pack-dir> <preview.png>`

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};

const WIDTH: u32 = 720;
const HEIGHT: u32 = 480;
const SCENE_HEIGHT: u32 = 444;
const ALPHA_CUTOFF: u8 = 32;

// This crop turns the source map's three long platform tops (approximately
// y=569, 809 and 1049) into the existing 720x480 simulation lanes without
// redrawing or inventing platforms.
const FOREGROUND_CROP_X: u32 = 280;
const FOREGROUND_CROP_Y: u32 = 362;
const FOREGROUND_CROP_W: u32 = 1200;
const FOREGROUND_CROP_H: u32 = 740;

const SLIME_MOVE_ORIGINS: [(u32, u32); 7] = [
    (29, 51),
    (31, 53),
    (27, 71),
    (27, 71),
    (27, 94),
    (28, 71),
    (27, 46),
];
const SLIME_DIE_ORIGINS: [(u32, u32); 4] = [(26, 60), (30, 59), (26, 54), (14, 40)];
const GREEN_MUSHROOM_MOVE_ORIGINS: [(u32, u32); 4] = [(27, 52), (27, 51), (27, 52), (27, 51)];
const GREEN_MUSHROOM_DIE_ORIGINS: [(u32, u32); 4] = [(28, 49), (27, 48), (28, 46), (29, 46)];
const PORTAL_ORIGINS: [(u32, u32); 8] = [
    (43, 173),
    (43, 173),
    (44, 193),
    (43, 193),
    (42, 209),
    (42, 209),
    (41, 248),
    (42, 246),
];

struct NormalizedSequence {
    source_subdir: &'static str,
    origins: &'static [(u32, u32)],
    canvas_size: (u32, u32),
    canvas_anchor: (u32, u32),
    output_prefix: &'static str,
}

const CLASSIC_SEQUENCES: [NormalizedSequence; 5] = [
    NormalizedSequence {
        source_subdir: "mobs/slime/move",
        origins: &SLIME_MOVE_ORIGINS,
        canvas_size: (96, 112),
        canvas_anchor: (48, 104),
        output_prefix: "training_monster_slime",
    },
    NormalizedSequence {
        source_subdir: "mobs/slime/die1",
        origins: &SLIME_DIE_ORIGINS,
        canvas_size: (96, 112),
        canvas_anchor: (48, 104),
        output_prefix: "training_monster_slime_die",
    },
    NormalizedSequence {
        source_subdir: "mobs/green-mushroom/move",
        origins: &GREEN_MUSHROOM_MOVE_ORIGINS,
        canvas_size: (72, 64),
        canvas_anchor: (36, 58),
        output_prefix: "training_monster_green_mushroom",
    },
    NormalizedSequence {
        source_subdir: "mobs/green-mushroom/die1",
        origins: &GREEN_MUSHROOM_DIE_ORIGINS,
        canvas_size: (72, 64),
        canvas_anchor: (36, 58),
        output_prefix: "training_monster_green_mushroom_die",
    },
    NormalizedSequence {
        source_subdir: "portal/pv",
        origins: &PORTAL_ORIGINS,
        canvas_size: (96, 264),
        canvas_anchor: (48, 256),
        output_prefix: "training_portal",
    },
];

fn main() -> Result<()> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        bail!("usage: training_pack <hhg1-source> <pack-dir> <preview.png>");
    }
    let source_root = PathBuf::from(&args[0]);
    let pack_dir = PathBuf::from(&args[1]);
    let preview = PathBuf::from(&args[2]);
    fs::create_dir_all(&pack_dir).with_context(|| format!("creating {}", pack_dir.display()))?;
    if let Some(parent) = preview.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let palette = read_palette(&pack_dir.join("pack.toml"))?;
    let background = compose_background(&source_root)?;
    background
        .save(&preview)
        .with_context(|| format!("writing {}", preview.display()))?;
    write_sprite(
        &pack_dir.join("training_background.sprite"),
        &background,
        &palette,
        "MapleStory Classic GMS v62 map 104040000; local-only NEXON derivative",
    )?;

    for sequence in &CLASSIC_SEQUENCES {
        write_normalized_sequence(&source_root, &pack_dir, &palette, sequence)?;
    }

    let removed = remove_deprecated_sunnyland_files(&pack_dir)?;
    println!("preview={}", preview.display());
    println!("pack={}", pack_dir.display());
    println!("removed_deprecated_sunnyland_files={removed}");
    Ok(())
}

fn compose_background(root: &Path) -> Result<RgbaImage> {
    let solid = open_rgba(&root.join("map/background/grassySoil-0.png"))?;
    let fill = *solid
        .get_pixel_checked(0, 0)
        .context("grassySoil background 0 is empty")?;
    let mut out = RgbaImage::from_pixel(WIDTH, HEIGHT, fill);

    // Exact grassySoil layers, arranged in the same far-to-near order as the
    // game map. The foreground crop below is the real rendered map, not a
    // hand-built imitation.
    let clouds = resize_to_width(
        &open_rgba(&root.join("map/background/grassySoil-1.png"))?,
        WIDTH,
    );
    imageops::overlay(&mut out, &clouds, 0, 0);

    let distant = resize_to_width(
        &open_rgba(&root.join("map/background/grassySoil-2.png"))?,
        780,
    );
    imageops::overlay(&mut out, &distant, -30, 224);

    let far_tree = imageops::resize(
        &open_rgba(&root.join("map/background/grassySoil-3.png"))?,
        180,
        153,
        FilterType::Nearest,
    );
    imageops::overlay(&mut out, &far_tree, 478, 160);

    let meadow = resize_to_width(
        &open_rgba(&root.join("map/background/grassySoil-4.png"))?,
        WIDTH,
    );
    imageops::overlay(&mut out, &meadow, 0, 263);

    let foreground = open_rgba(&root.join("map/foreground.png"))?;
    if foreground.width() < FOREGROUND_CROP_X + FOREGROUND_CROP_W
        || foreground.height() < FOREGROUND_CROP_Y + FOREGROUND_CROP_H
    {
        bail!(
            "HHG1 foreground is too small: {}x{}",
            foreground.width(),
            foreground.height()
        );
    }
    let crop = imageops::crop_imm(
        &foreground,
        FOREGROUND_CROP_X,
        FOREGROUND_CROP_Y,
        FOREGROUND_CROP_W,
        FOREGROUND_CROP_H,
    )
    .to_image();
    let scaled = imageops::resize(&crop, WIDTH, SCENE_HEIGHT, FilterType::Nearest);
    imageops::overlay(&mut out, &scaled, 0, 0);
    Ok(out)
}

fn open_rgba(path: &Path) -> Result<RgbaImage> {
    image::open(path)
        .with_context(|| format!("opening {}", path.display()))
        .map(|image| image.to_rgba8())
}

fn resize_to_width(source: &RgbaImage, width: u32) -> RgbaImage {
    let height = ((u64::from(source.height()) * u64::from(width) + u64::from(source.width()) / 2)
        / u64::from(source.width())) as u32;
    imageops::resize(source, width, height.max(1), FilterType::Nearest)
}

#[cfg(test)]
fn map_source_y_to_output(source_y: u32) -> u32 {
    let local = source_y.saturating_sub(FOREGROUND_CROP_Y);
    ((f64::from(local) * f64::from(SCENE_HEIGHT)) / f64::from(FOREGROUND_CROP_H)).round() as u32
}

fn normalize_frame(
    source: &RgbaImage,
    source_origin: (u32, u32),
    canvas_size: (u32, u32),
    canvas_anchor: (u32, u32),
) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(canvas_size.0, canvas_size.1, Rgba([0, 0, 0, 0]));
    let x = i64::from(canvas_anchor.0) - i64::from(source_origin.0);
    let y = i64::from(canvas_anchor.1) - i64::from(source_origin.1);
    imageops::overlay(&mut canvas, source, x, y);
    canvas
}

fn write_normalized_sequence(
    source_root: &Path,
    pack_dir: &Path,
    palette: &[(char, [u8; 3])],
    sequence: &NormalizedSequence,
) -> Result<()> {
    let source_dir = source_root.join(sequence.source_subdir);
    for (index, origin) in sequence.origins.iter().copied().enumerate() {
        let source_path = source_dir.join(format!("{index}.png"));
        let source = open_rgba(&source_path)?;
        let normalized = normalize_frame(
            &source,
            origin,
            sequence.canvas_size,
            sequence.canvas_anchor,
        );
        write_sprite(
            &pack_dir.join(format!("{}_{index}.sprite", sequence.output_prefix)),
            &normalized,
            palette,
            &format!(
                "{}; origin={},{}; local-only NEXON derivative",
                source_path.display(),
                origin.0,
                origin.1
            ),
        )?;
    }
    Ok(())
}

fn remove_deprecated_sunnyland_files(pack_dir: &Path) -> Result<usize> {
    let mut names = Vec::new();
    names.extend((0..8).map(|index| format!("training_monster_ant_{index}.sprite")));
    names.extend((0..4).map(|index| format!("training_monster_grasshopper_{index}.sprite")));
    names.extend((0..4).map(|index| format!("training_enemy_death_{index}.sprite")));
    names.extend((0..3).map(|index| format!("training_drop_acorn_{index}.sprite")));

    let mut removed = 0;
    for name in names {
        let path = pack_dir.join(name);
        if path.try_exists()? {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn read_palette(path: &Path) -> Result<Vec<(char, [u8; 3])>> {
    let source = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let root = toml::from_str::<toml::Table>(&source).context("parsing pack palette")?;
    let table = root
        .get("palette")
        .and_then(toml::Value::as_table)
        .context("pack.toml has no [palette]")?;
    let mut palette = Vec::new();
    for (key, value) in table {
        if key == "." {
            continue;
        }
        let mut chars = key.chars();
        let Some(glyph) = chars.next() else {
            continue;
        };
        if chars.next().is_some() {
            continue;
        }
        let Some(hex) = value.as_str().and_then(|raw| raw.strip_prefix('#')) else {
            continue;
        };
        if hex.len() != 6 {
            continue;
        }
        let color = [
            u8::from_str_radix(&hex[0..2], 16)?,
            u8::from_str_radix(&hex[2..4], 16)?,
            u8::from_str_radix(&hex[4..6], 16)?,
        ];
        palette.push((glyph, color));
    }
    if palette.is_empty() {
        bail!("pack palette has no opaque single-character entries");
    }
    Ok(palette)
}

fn write_sprite(
    output: &Path,
    image: &RgbaImage,
    palette: &[(char, [u8; 3])],
    source: &str,
) -> Result<()> {
    let mut cache = HashMap::<[u8; 3], char>::new();
    let mut sprite = format!(
        "# Generated from {source}; {}x{}; nearest existing local-pack palette.\n@frame 0\n",
        image.width(),
        image.height()
    );
    for y in 0..image.height() {
        for x in 0..image.width() {
            if x != 0 {
                sprite.push(' ');
            }
            let pixel = image.get_pixel(x, y);
            if pixel[3] < ALPHA_CUTOFF {
                sprite.push('.');
                continue;
            }
            let rgb = [pixel[0], pixel[1], pixel[2]];
            let glyph = *cache.entry(rgb).or_insert_with(|| nearest(rgb, palette));
            sprite.push(glyph);
        }
        sprite.push('\n');
    }
    fs::write(output, sprite).with_context(|| format!("writing {}", output.display()))
}

fn nearest(rgb: [u8; 3], palette: &[(char, [u8; 3])]) -> char {
    palette
        .iter()
        .min_by_key(|(_, color)| {
            let dr = i32::from(rgb[0]) - i32::from(color[0]);
            let dg = i32::from(rgb[1]) - i32::from(color[1]);
            let db = i32::from(rgb[2]) - i32::from(color[2]);
            dr * dr + dg * dg + db * db
        })
        .map(|(glyph, _)| *glyph)
        .unwrap_or('?')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hhg1_crop_maps_the_three_game_platforms_to_training_lanes() {
        assert_eq!(map_source_y_to_output(569), 124);
        assert_eq!(map_source_y_to_output(809), 268);
        assert_eq!(map_source_y_to_output(1049), 412);
    }

    #[test]
    fn frame_normalization_uses_the_game_origin_as_the_fixed_foot_anchor() {
        let source = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
        let normalized = normalize_frame(&source, (1, 2), (6, 6), (3, 5));

        assert_eq!(*normalized.get_pixel(2, 3), Rgba([255, 0, 0, 255]));
        assert_eq!(*normalized.get_pixel(3, 4), Rgba([255, 0, 0, 255]));
        assert_eq!(*normalized.get_pixel(3, 5), Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn source_animation_metadata_has_the_expected_classic_frame_counts() {
        assert_eq!(SLIME_MOVE_ORIGINS.len(), 7);
        assert_eq!(SLIME_DIE_ORIGINS.len(), 4);
        assert_eq!(GREEN_MUSHROOM_MOVE_ORIGINS.len(), 4);
        assert_eq!(GREEN_MUSHROOM_DIE_ORIGINS.len(), 4);
        assert_eq!(PORTAL_ORIGINS.len(), 8);
    }
}
