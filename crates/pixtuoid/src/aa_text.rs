//! Shared anti-aliased text rasterizer (Monaspace Neon) for the binary's pixel
//! surfaces — the floating window's name badges + wall board (`floating/`) and
//! the snapshot example's cell text + `--proof` panel.
//!
//! Kept BINARY-side on purpose: `pixtuoid-scene` (which also compiles to wasm for
//! the web hero) stays font-dep-free — no `ab_glyph`, no embedded font, no wasm
//! bundle bloat (the web hero renders text as a crisp DOM overlay instead of
//! baking it).
//!
//! Monaspace Neon (GitHub Next, OFL) remains the brand mono and the one embedded
//! face across the project. It covers the office's full symbol vocabulary; the
//! `office_symbol_vocabulary_is_fully_covered` test keeps that guarantee. A
//! floating name badge may additionally use an opt-in external fallback font
//! for glyphs Monaspace lacks (for example Traditional Chinese task titles).
//! The fallback never changes wall-board, footer, snapshot-cell, or proof text.
//!
//! Surface-agnostic: [`draw_text_at`] hands each lit pixel's coverage to a
//! `put(x, y, coverage)` closure, so every caller applies its own pixel-format
//! blend (`RgbImage`/`RgbaImage` in the snapshot example, `u32` XRGB in the
//! floating window) — all through [`blend_channel`], the one blend curve.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use ab_glyph::{point, Font, FontArc, FontRef, GlyphId, PxScale, ScaleFont};

/// The bundled Monaspace Neon SemiBold (OFL 1.1 — the user-picked weight for the small-size pixel surfaces) — the ONE AA face every
/// rasterized text surface shares. License text in `fonts/OFL-Monaspace.txt`.
const FONT_BYTES: &[u8] = include_bytes!("../fonts/MonaspaceNeon-SemiBold.otf");

static FONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::try_from_slice(FONT_BYTES).expect("bundled Monaspace Neon OTF must parse")
});

const LABEL_FALLBACK_FONT_ENV: &str = "PIXTUOID_CJK_FONT";

static LABEL_FALLBACK_FONT: LazyLock<Option<FontArc>> = LazyLock::new(discover_label_fallback_font);

fn load_label_fallback_font(path: impl AsRef<Path>) -> Option<FontArc> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| FontArc::try_from_vec(bytes).ok())
}

/// Resolve the first locally available Traditional-Chinese face.  The launcher
/// override remains authoritative, while the adjacent install font and the
/// operating system's normal UI font keep direct executable/example runs from
/// silently falling back to `.notdef` squares.
fn discover_label_fallback_font() -> Option<FontArc> {
    label_fallback_font_candidates()
        .into_iter()
        .find_map(load_label_fallback_font)
}

fn label_fallback_font_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os(LABEL_FALLBACK_FONT_ENV) {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            candidates.push(
                bin_dir
                    .join("..")
                    .join("fonts")
                    .join("NotoSansTC-SemiBold.ttf"),
            );
        }
    }
    candidates.push(PathBuf::from("fonts").join("NotoSansTC-SemiBold.ttf"));

    #[cfg(windows)]
    {
        let windows_dir = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let font_dir = windows_dir.join("Fonts");
        // Prefer the bold Traditional-Chinese UI face; the regular face is a
        // safe fallback on stripped-down Windows installations.
        candidates.push(font_dir.join("msjhbd.ttc"));
        candidates.push(font_dir.join("msjh.ttc"));
        candidates.push(font_dir.join("mingliu.ttc"));
    }

    #[cfg(target_os = "linux")]
    {
        candidates.push(PathBuf::from(
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ));
        candidates.push(PathBuf::from(
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ));
    }

    candidates
}

/// Linear per-channel coverage blend of `fg` over `bg` — THE one blend curve
/// every AA-text surface composites with (snapshot PNG `mix_rgb`, proof-panel
/// `blend_px`, floating `blend_xrgb` all wrap this per their pixel type), so a
/// future curve change (e.g. gamma-correct blending) lands once, never drifts.
/// `cov` is clamped here so callers don't each re-clamp.
pub fn blend_channel(bg: u8, fg: u8, cov: f32) -> u8 {
    let a = cov.clamp(0.0, 1.0);
    (bg as f32 + (fg as f32 - bg as f32) * a).round() as u8
}

/// Whether the face covers `ch` with a real glyph (not `.notdef`). Callers with
/// a non-text fallback (the snapshot cell rasterizer's centered block) gate on
/// this so an uncovered decorative symbol renders as the fallback, never tofu.
pub fn has_glyph(ch: char) -> bool {
    FONT.glyph_id(ch) != GlyphId(0)
}

/// Sum of the face's per-glyph pixel-scaled advances at size `px` — the width
/// function for wrapping / right-flush. Summing real advances (not `chars × one
/// advance`) stays correct even for a future proportional face.
pub fn text_width(s: &str, px: f32) -> i32 {
    let sf = FONT.as_scaled(PxScale::from(px));
    s.chars()
        .map(|c| sf.h_advance(sf.glyph_id(c)))
        .sum::<f32>()
        .round() as i32
}

fn scaled_advance<F: Font>(font: &F, ch: char, scale: PxScale) -> f32 {
    let sf = font.as_scaled(scale);
    sf.h_advance(sf.glyph_id(ch))
}

fn label_uses_fallback(ch: char, fallback: Option<&FontArc>) -> bool {
    FONT.glyph_id(ch) == GlyphId(0) && fallback.is_some_and(|font| font.glyph_id(ch) != GlyphId(0))
}

fn label_text_width_with_fallback(s: &str, px: f32, fallback: Option<&FontArc>) -> i32 {
    let scale = PxScale::from(px);
    s.chars()
        .map(|ch| {
            if label_uses_fallback(ch, fallback) {
                scaled_advance(fallback.expect("fallback glyph was checked"), ch, scale)
            } else {
                scaled_advance(&*FONT, ch, scale)
            }
        })
        .sum::<f32>()
        .round() as i32
}

/// Width of a floating name badge. Monaspace supplies every covered glyph;
/// only missing glyphs may use the configured external fallback face.
pub fn label_text_width(s: &str, px: f32) -> i32 {
    label_text_width_with_fallback(s, px, LABEL_FALLBACK_FONT.as_ref())
}

/// The face's line height (ascent − descent + line gap) at size `px` — the row
/// advance for stacking multiple text lines (the wall board's 3 rows).
pub fn line_height(px: f32) -> i32 {
    let sf = FONT.as_scaled(PxScale::from(px));
    (sf.ascent() - sf.descent() + sf.line_gap()).round() as i32
}

/// Rasterize `s` in the AA face at pixel size `px`, top-left at `(x, top_y)`,
/// calling `put(px_x, px_y, coverage)` for every lit pixel (`coverage` ∈ `[0,1]` is
/// the AA grayscale strength). Backend-agnostic — the caller composites into its
/// own surface. Returns the total advance width (so a caller placing a cursor /
/// second run doesn't recompute via [`text_width`]).
pub fn draw_text_at(
    s: &str,
    x: i32,
    top_y: i32,
    px: f32,
    mut put: impl FnMut(i32, i32, f32),
) -> i32 {
    let scale = PxScale::from(px);
    let sf = FONT.as_scaled(scale);
    let baseline_y = top_y as f32 + sf.ascent();
    let mut cursor_x = x as f32;
    for ch in s.chars() {
        let gid = sf.glyph_id(ch);
        let glyph = gid.with_scale_and_position(scale, point(cursor_x, baseline_y));
        if let Some(outlined) = FONT.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            let (ox, oy) = (bounds.min.x.round() as i32, bounds.min.y.round() as i32);
            outlined.draw(|gx, gy, coverage| {
                put(ox + gx as i32, oy + gy as i32, coverage);
            });
        }
        cursor_x += sf.h_advance(gid);
    }
    (cursor_x - x as f32).round() as i32
}

fn draw_scaled_glyph<F: Font>(
    font: &F,
    ch: char,
    scale: PxScale,
    cursor_x: f32,
    baseline_y: f32,
    put: &mut impl FnMut(i32, i32, f32),
) -> f32 {
    let sf = font.as_scaled(scale);
    let gid = sf.glyph_id(ch);
    let glyph = gid.with_scale_and_position(scale, point(cursor_x, baseline_y));
    if let Some(outlined) = font.outline_glyph(glyph) {
        let bounds = outlined.px_bounds();
        let (ox, oy) = (bounds.min.x.round() as i32, bounds.min.y.round() as i32);
        outlined.draw(|gx, gy, coverage| {
            put(ox + gx as i32, oy + gy as i32, coverage);
        });
    }
    sf.h_advance(gid)
}

fn draw_label_text_with_fallback(
    s: &str,
    x: i32,
    top_y: i32,
    px: f32,
    fallback: Option<&FontArc>,
    mut put: impl FnMut(i32, i32, f32),
) -> i32 {
    let scale = PxScale::from(px);
    let primary_ascent = FONT.as_scaled(scale).ascent();
    let fallback_ascent = fallback
        .map(|font| font.as_scaled(scale).ascent())
        .unwrap_or(primary_ascent);
    let baseline_y = top_y as f32 + primary_ascent.max(fallback_ascent);
    let mut cursor_x = x as f32;

    for ch in s.chars() {
        cursor_x += if label_uses_fallback(ch, fallback) {
            draw_scaled_glyph(
                fallback.expect("fallback glyph was checked"),
                ch,
                scale,
                cursor_x,
                baseline_y,
                &mut put,
            )
        } else {
            draw_scaled_glyph(&*FONT, ch, scale, cursor_x, baseline_y, &mut put)
        };
    }
    (cursor_x - x as f32).round() as i32
}

/// Rasterize a floating name badge with the optional external fallback font.
/// All non-label surfaces continue to call [`draw_text_at`] directly.
pub fn draw_label_text_at(
    s: &str,
    x: i32,
    top_y: i32,
    px: f32,
    put: impl FnMut(i32, i32, f32),
) -> i32 {
    draw_label_text_with_fallback(s, x, top_y, px, LABEL_FALLBACK_FONT.as_ref(), put)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_parses_and_metrics_are_positive() {
        assert!(text_width("M", 16.0) > 0, "a glyph has positive advance");
        assert!(line_height(16.0) > 0, "positive line height");
    }

    #[test]
    fn width_grows_with_length_and_size() {
        // More glyphs → wider; bigger size → wider. (Exact N× proportionality
        // isn't asserted — text_width rounds the summed f32 advance ONCE, so
        // round(4·adv) ≠ 4·round(adv) in general.)
        let one = text_width("M", 16.0);
        assert!(one > 0);
        assert!(text_width("MM", 16.0) > one);
        assert!(text_width("MMMM", 16.0) > text_width("MM", 16.0));
        assert!(text_width("M", 32.0) > one, "larger px advances wider");
        // Monospace sanity: 4 M's land within ±1px of 4× one (pure rounding slack).
        assert!((text_width("MMMM", 16.0) - one * 4).abs() <= 1);
    }

    #[test]
    fn draw_emits_partial_coverage_pixels_the_bitmap_font_cannot() {
        // The whole point of the AA path: glyph edges emit intermediate coverage,
        // not the all-or-nothing pixels an 8×8 bitmap font produces.
        let mut lit = 0usize;
        let mut partial = 0usize;
        let advance = draw_text_at("a", 0, 0, 24.0, |_x, _y, cov| {
            assert!((0.0..=1.0).contains(&cov), "coverage in [0,1]: {cov}");
            lit += 1;
            if cov > 0.02 && cov < 0.98 {
                partial += 1;
            }
        });
        assert!(lit > 0, "the glyph lit some pixels");
        assert!(
            partial > 0,
            "AA glyph has anti-aliased (partial-coverage) edges"
        );
        assert!(advance > 0, "returns the advance width");
    }

    #[test]
    fn available_label_fallback_renders_traditional_chinese() {
        let Some(fallback) = discover_label_fallback_font() else {
            return; // public CI images are not required to ship a CJK system face
        };
        assert!(!has_glyph('尋'), "the bundled mono face needs the fallback");
        assert_ne!(fallback.glyph_id('尋'), GlyphId(0));

        let mut lit = 0usize;
        let advance = draw_label_text_with_fallback(
            "cx·尋找",
            0,
            0,
            16.0,
            Some(&fallback),
            |_x, _y, _cov| lit += 1,
        );
        assert!(lit > 0, "the mixed-script label lights pixels");
        assert_eq!(
            advance,
            label_text_width_with_fallback("cx·尋找", 16.0, Some(&fallback))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_direct_runs_find_a_traditional_chinese_face_without_launcher_env() {
        let fallback = discover_label_fallback_font()
            .expect("Windows installs a Traditional-Chinese UI font for direct snapshots");
        assert_ne!(fallback.glyph_id('圖'), GlyphId(0));
        assert_ne!(fallback.glyph_id('訓'), GlyphId(0));
    }

    #[test]
    fn blend_channel_endpoints_midpoint_and_clamp() {
        assert_eq!(blend_channel(0, 200, 0.0), 0);
        assert_eq!(blend_channel(0, 200, 1.0), 200);
        assert_eq!(blend_channel(0, 200, 0.5), 100);
        assert_eq!(blend_channel(0, 200, 2.0), 200, "over-coverage clamps");
        assert_eq!(blend_channel(0, 200, -1.0), 0, "negative clamps");
    }

    #[test]
    fn office_symbol_vocabulary_is_fully_covered() {
        // Every non-ASCII glyph the TUI's text surfaces emit (labels, footer,
        // board, tooltip, dashboard, doctor) must be a REAL Monaspace Neon
        // glyph — the vocabulary is single-face by design (no fallback face).
        // An uncovered glyph would rasterize as the snapshot fallback block
        // and as tofu in floating.
        // HAND-MAINTAINED allowlist (no single machine-readable source of the
        // render vocabulary exists to derive from): adding a glyph to footer.rs /
        // wall_board.rs / theme_picker.rs / version_popup.rs / elevator.rs /
        // board.rs / overlay.rs / tooltip.rs / dashboard.rs means ADDING IT
        // HERE — and it must be Monaspace-covered, or the vocabulary changes.
        for ch in [
            '●', '○', '◐', '◌', '▲', '▼', '▸', '▾', '★', '⬢', '▮', '▯', '↳', '◷', '▤', '↑', '↓',
            '·', '×', '⚠', '…', '⋮', '─', '│', '█', '▓', '▒', '░', '▀', '✓', '└', '├', 'Σ', '♩',
        ] {
            assert!(has_glyph(ch), "Monaspace Neon does not cover {ch:?}");
        }
    }
}
