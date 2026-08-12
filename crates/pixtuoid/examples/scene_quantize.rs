use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use image::imageops::FilterType;

const DEFAULT_OUT_W: u32 = 240;
const DEFAULT_OUT_H: u32 = 160;
const KEYS: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'I', 'J', 'T', 'U', 'W', 'X', 'Y', 'Z', 'h',
    'i', 'p', 's', 'v', '!', '$', '%', '&', '(', ')', '*', '+', '-', '/', ':', ';', '<', '=', '>',
    '?', '[', ']', '^', '_', '{', '}', '|', '~', '@',
];

#[derive(Clone, Copy, Debug)]
struct Bucket {
    rgb: [f64; 3],
    count: u64,
}

fn distance_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dr = a[0] - b[0];
    let dg = a[1] - b[1];
    let db = a[2] - b[2];
    dr * dr * 0.9 + dg * dg * 1.2 + db * db * 0.8
}

fn nearest(rgb: [f64; 3], centroids: &[[f64; 3]]) -> usize {
    centroids
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            distance_sq(rgb, **a)
                .partial_cmp(&distance_sq(rgb, **b))
                .expect("finite RGB distances")
        })
        .map(|(index, _)| index)
        .expect("at least one centroid")
}

fn initial_centroids(buckets: &[Bucket], count: usize) -> Vec<[f64; 3]> {
    let mut out = vec![buckets[0].rgb];
    while out.len() < count {
        let next = buckets
            .iter()
            .max_by(|a, b| {
                let score = |bucket: &Bucket| {
                    let novelty = out
                        .iter()
                        .map(|center| distance_sq(bucket.rgb, *center))
                        .fold(f64::INFINITY, f64::min);
                    novelty * (bucket.count as f64).sqrt()
                };
                score(a)
                    .partial_cmp(&score(b))
                    .expect("finite centroid scores")
            })
            .expect("non-empty histogram");
        out.push(next.rgb);
    }
    out
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(
        args.next()
            .context("usage: scene_quantize <input.png> <output.sprite> [width height]")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: scene_quantize <input.png> <output.sprite> [width height]")?,
    );
    let width = args.next();
    let height = args.next();
    let (out_w, out_h) = match (width, height) {
        (None, None) => (DEFAULT_OUT_W, DEFAULT_OUT_H),
        (Some(width), Some(height)) => (
            width
                .to_string_lossy()
                .parse::<u32>()
                .context("width must be a positive integer")?,
            height
                .to_string_lossy()
                .parse::<u32>()
                .context("height must be a positive integer")?,
        ),
        _ => bail!("width and height must be supplied together"),
    };
    if out_w == 0 || out_h == 0 || args.next().is_some() {
        bail!("usage: scene_quantize <input.png> <output.sprite> [width height]");
    }

    let image = image::open(&input)
        .with_context(|| format!("decoding {}", input.display()))?
        .resize_exact(out_w, out_h, FilterType::Lanczos3)
        .to_rgb8();
    let pixels: Vec<[f64; 3]> = image
        .pixels()
        .map(|pixel| [pixel[0] as f64, pixel[1] as f64, pixel[2] as f64])
        .collect();

    let mut histogram: HashMap<[u8; 3], ([u64; 3], u64)> = HashMap::new();
    for rgb in &pixels {
        let key = [rgb[0] as u8 >> 3, rgb[1] as u8 >> 3, rgb[2] as u8 >> 3];
        let entry = histogram.entry(key).or_insert(([0; 3], 0));
        entry.0[0] += rgb[0] as u64;
        entry.0[1] += rgb[1] as u64;
        entry.0[2] += rgb[2] as u64;
        entry.1 += 1;
    }
    let mut buckets: Vec<Bucket> = histogram
        .into_values()
        .map(|(sum, count)| Bucket {
            rgb: [
                sum[0] as f64 / count as f64,
                sum[1] as f64 / count as f64,
                sum[2] as f64 / count as f64,
            ],
            count,
        })
        .collect();
    buckets.sort_by_key(|bucket| std::cmp::Reverse(bucket.count));

    let mut centroids = initial_centroids(&buckets, KEYS.len());
    for _ in 0..12 {
        let mut sums = vec![[0.0; 3]; centroids.len()];
        let mut counts = vec![0u64; centroids.len()];
        for rgb in &pixels {
            let index = nearest(*rgb, &centroids);
            sums[index][0] += rgb[0];
            sums[index][1] += rgb[1];
            sums[index][2] += rgb[2];
            counts[index] += 1;
        }
        for (index, center) in centroids.iter_mut().enumerate() {
            if counts[index] != 0 {
                center[0] = sums[index][0] / counts[index] as f64;
                center[1] = sums[index][1] / counts[index] as f64;
                center[2] = sums[index][2] / counts[index] as f64;
            }
        }
    }

    let mut palette: Vec<[u8; 3]> = centroids
        .iter()
        .map(|center| {
            [
                center[0].round() as u8,
                center[1].round() as u8,
                center[2].round() as u8,
            ]
        })
        .collect();
    let recolor_keys = [
        [0x4a, 0x2a, 0x1e],
        [0xf4, 0xc7, 0x9a],
        [0x3f, 0x7d, 0x45],
        [0x31, 0x4a, 0x6e],
    ];
    for color in &mut palette {
        if recolor_keys.contains(color) {
            color[2] = color[2].saturating_add(1);
        }
    }

    let source_name = input
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("scene.png");
    let mut sprite = format!(
        "# Generated from {source_name}; {out_w}x{out_h}, 48-color deterministic k-means.\n@frame 0\n"
    );
    for row in image.rows() {
        for (x, pixel) in row.enumerate() {
            if x != 0 {
                sprite.push(' ');
            }
            let rgb = [pixel[0] as f64, pixel[1] as f64, pixel[2] as f64];
            sprite.push(KEYS[nearest(rgb, &centroids)]);
        }
        sprite.push('\n');
    }
    fs::write(&output, sprite).with_context(|| format!("writing {}", output.display()))?;

    let palette_output = output.with_extension("palette.toml");
    let mut palette_toml = String::from("# Add these scene-only colors to [palette].\n");
    for (key, color) in KEYS.iter().zip(&palette) {
        palette_toml.push_str(&format!(
            "\"{key}\" = \"#{:02x}{:02x}{:02x}\"\n",
            color[0], color[1], color[2]
        ));
    }
    fs::write(&palette_output, palette_toml)
        .with_context(|| format!("writing {}", palette_output.display()))?;

    println!("sprite={}", output.display());
    println!("palette={}", palette_output.display());
    Ok(())
}
