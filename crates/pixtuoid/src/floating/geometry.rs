//! Pure window/monitor geometry for the floating desktop window.
//!
//! These are the only pieces of branching logic in `window.rs`'s `winit` handler that don't
//! need a live `ActiveEventLoop` / cursor to exercise. Extracted here so they're unit-testable
//! (and counted by coverage) while the surrounding event-loop glue stays codecov-ignored — the
//! same split as `offscreen.rs` (testable render seam) vs `window.rs` (platform glue).

use winit::window::{ResizeDirection, WindowLevel};

/// User-facing logical window-size steps. Single-map presets keep the classic
/// 3:2 game viewport; the mini dual-map plate keeps each authored map at the
/// established 240×160 floor while larger presets preserve the existing sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FloatingSizePreset {
    Mini,
    Small,
    Medium,
    Large,
}

impl FloatingSizePreset {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Mini => Self::Small,
            Self::Small => Self::Medium,
            Self::Medium => Self::Large,
            Self::Large => Self::Mini,
        }
    }

    pub(crate) const fn title_zh_tw(self) -> &'static str {
        match self {
            Self::Mini => "迷你",
            Self::Small => "小",
            Self::Medium => "中",
            Self::Large => "大",
        }
    }

    pub(crate) const fn logical_size(self, dual_map: bool) -> (u32, u32) {
        match (self, dual_map) {
            (Self::Mini, false) => (240, 160),
            (Self::Mini, true) => (480, 160),
            (Self::Small, false) => (360, 240),
            (Self::Medium, false) => (540, 360),
            (Self::Large, false) => (720, 480),
            (Self::Small, true) => (960, 320),
            (Self::Medium, true) => (1_200, 400),
            (Self::Large, true) => (1_440, 480),
        }
    }

    pub(crate) fn from_logical_size(width: u32, height: u32) -> Self {
        let dual_map = width >= height.saturating_mul(2);
        [Self::Mini, Self::Small, Self::Medium, Self::Large]
            .into_iter()
            .min_by_key(|preset| {
                logical_size_distance((width, height), preset.logical_size(dual_map))
            })
            .expect("the authored size preset list is non-empty")
    }
}

fn logical_size_distance(actual: (u32, u32), preset: (u32, u32)) -> u64 {
    let width_delta = u64::from(actual.0.abs_diff(preset.0));
    let height_delta = u64::from(actual.1.abs_diff(preset.1));
    width_delta * width_delta + height_delta * height_delta
}

/// Map the persisted boolean to winit's explicit stacking contract. Disabled
/// means a normal window: it remains visible on the desktop but can sit behind
/// the user's work instead of being forced above every application.
pub(crate) const fn window_level(always_on_top: bool) -> WindowLevel {
    if always_on_top {
        WindowLevel::AlwaysOnTop
    } else {
        WindowLevel::Normal
    }
}

/// Uniformly fit a logical target inside a fraction of the current monitor.
/// Never enlarges the target and keeps its aspect ratio, so choosing a preset
/// cannot push a frameless window beyond a smaller laptop display.
pub(crate) fn fit_logical_size(
    target: (u32, u32),
    monitor: (u32, u32),
    max_fraction: f64,
) -> (u32, u32) {
    let fraction = if max_fraction.is_finite() {
        max_fraction.clamp(0.1, 1.0)
    } else {
        0.9
    };
    let available_w = f64::from(monitor.0) * fraction;
    let available_h = f64::from(monitor.1) * fraction;
    let scale = 1.0_f64
        .min(available_w / f64::from(target.0.max(1)))
        .min(available_h / f64::from(target.1.max(1)));
    (
        (f64::from(target.0) * scale).round().max(1.0) as u32,
        (f64::from(target.1) * scale).round().max(1.0) as u32,
    )
}

/// Does the saved window rect `(x, y, w, h)` overlap ANY currently-connected monitor?
///
/// Guards against restoring onto a now-disconnected monitor (the off-screen-unrecoverable
/// case: frameless + always-on-top + no taskbar → no way to drag a fully off-screen window
/// back). All values are physical px (saved position per `persist_geometry`, monitor
/// position/size from winit), so they compare directly; `w`/`h` are the saved LOGICAL dims
/// used here only as an approximate extent — a few px of HiDPI slop is irrelevant for an
/// on/off-screen test. Standard axis-aligned-rect overlap (any non-empty intersection counts;
/// an edge-touching window with a zero-area intersection does NOT). Defensive: an EMPTY
/// monitor iterator (winit reports none) returns `true` so we still honor the saved position
/// rather than second-guessing the OS.
pub(crate) fn window_visible_on_monitors(
    win: (i32, i32, u32, u32),
    monitors: impl IntoIterator<Item = (i32, i32, u32, u32)>,
) -> bool {
    let (wx, wy, ww, wh) = win;
    let (win_l, win_t) = (wx as i64, wy as i64);
    let (win_r, win_b) = (win_l + ww as i64, win_t + wh as i64);
    let mut any_monitor = false;
    for (mx, my, mw, mh) in monitors {
        any_monitor = true;
        let (mon_l, mon_t) = (mx as i64, my as i64);
        let (mon_r, mon_b) = (mon_l + mw as i64, mon_t + mh as i64);
        if win_l < mon_r && win_r > mon_l && win_t < mon_b && win_b > mon_t {
            return true;
        }
    }
    // No monitor overlapped — but if winit reported NONE, don't override the OS.
    !any_monitor
}

/// Resolve a pointer inside the frameless resize border to the native direction.
/// Corners win over straight edges; a point outside the client rect or in its
/// interior returns `None` so the caller can use the same press for window move.
pub(crate) fn resize_direction_at(
    cursor: (f64, f64),
    size: (u32, u32),
    border_px: f64,
) -> Option<ResizeDirection> {
    let (cx, cy) = cursor;
    let (width, height) = (f64::from(size.0), f64::from(size.1));
    if !cx.is_finite() || !cy.is_finite() || cx < 0.0 || cy < 0.0 || cx >= width || cy >= height {
        return None;
    }

    let border = if border_px.is_finite() {
        border_px.max(1.0)
    } else {
        1.0
    };
    let left = cx < border.min(width / 2.0);
    let right = cx >= width - border.min(width / 2.0);
    let top = cy < border.min(height / 2.0);
    let bottom = cy >= height - border.min(height / 2.0);

    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        _ => None,
    }
}

/// Resolve one programmatic frameless-window drag step in physical pixels.
///
/// Keeping the original in-window grab point under the cursor avoids winit's
/// native title-bar drag loop. On Windows that loop participates in Snap and can
/// maximize a borderless window while it crosses a monitor edge.
pub(crate) fn manual_drag_position(
    outer: (i32, i32),
    cursor: (f64, f64),
    grab: (f64, f64),
) -> (i32, i32) {
    let resolve_axis = |origin: i32, current: f64, grabbed: f64| {
        (f64::from(origin) + (current - grabbed).round())
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    };
    (
        resolve_axis(outer.0, cursor.0, grab.0),
        resolve_axis(outer.1, cursor.1, grab.1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HD: (i32, i32, u32, u32) = (0, 0, 1920, 1080);

    #[test]
    fn overlapping_window_is_visible() {
        assert!(window_visible_on_monitors((100, 100, 800, 600), [HD]));
    }

    #[test]
    fn fully_offscreen_after_a_monitor_disconnect_is_not_visible() {
        // Saved at x=3000 but only the single 1920-wide monitor remains.
        assert!(!window_visible_on_monitors((3000, 0, 800, 600), [HD]));
    }

    #[test]
    fn partial_overlap_counts_as_visible() {
        // Straddles the right edge — still partly on-screen.
        assert!(window_visible_on_monitors((1800, 100, 400, 300), [HD]));
    }

    #[test]
    fn edge_touching_is_not_overlap() {
        // Window's left edge == monitor's right edge → zero-area intersection.
        assert!(!window_visible_on_monitors((1920, 0, 100, 100), [HD]));
    }

    #[test]
    fn lands_on_a_negative_origin_second_monitor() {
        // A monitor left of the primary (negative x); the window sits on it.
        assert!(window_visible_on_monitors(
            (-1500, 100, 400, 300),
            [HD, (-1920, 0, 1920, 1080)],
        ));
    }

    #[test]
    fn empty_monitor_list_honors_the_saved_position() {
        let none: [(i32, i32, u32, u32); 0] = [];
        assert!(window_visible_on_monitors((100, 100, 800, 600), none));
    }

    #[test]
    fn every_frameless_edge_and_corner_maps_to_native_resize_direction() {
        use winit::window::ResizeDirection::*;

        let size = (800, 600);
        for (cursor, expected) in [
            ((2.0, 2.0), NorthWest),
            ((400.0, 2.0), North),
            ((797.0, 2.0), NorthEast),
            ((797.0, 300.0), East),
            ((797.0, 597.0), SouthEast),
            ((400.0, 597.0), South),
            ((2.0, 597.0), SouthWest),
            ((2.0, 300.0), West),
        ] {
            assert_eq!(resize_direction_at(cursor, size, 8.0), Some(expected));
        }
        assert_eq!(resize_direction_at((400.0, 300.0), size, 8.0), None);
        assert_eq!(resize_direction_at((-1.0, 2.0), size, 8.0), None);
    }

    #[test]
    fn topmost_choice_maps_to_normal_window_level_when_disabled() {
        use winit::window::WindowLevel;

        assert_eq!(window_level(true), WindowLevel::AlwaysOnTop);
        assert_eq!(window_level(false), WindowLevel::Normal);
    }

    #[test]
    fn manual_drag_keeps_the_grab_point_under_the_cursor_across_monitor_origins() {
        assert_eq!(
            manual_drag_position((100, 80), (450.0, 260.0), (400.0, 220.0)),
            (150, 120)
        );
        assert_eq!(
            manual_drag_position((-1200, 100), (12.5, 33.5), (52.5, 23.5)),
            (-1240, 110),
            "negative monitor coordinates must remain valid"
        );
    }

    #[test]
    fn size_presets_cycle_and_use_single_or_dual_map_aspect_ratios() {
        assert_eq!(FloatingSizePreset::Mini.logical_size(false), (240, 160));
        assert_eq!(FloatingSizePreset::Mini.logical_size(true), (480, 160));
        assert_eq!(FloatingSizePreset::Small.logical_size(false), (360, 240));
        assert_eq!(
            FloatingSizePreset::Small.logical_size(true),
            (960, 320),
            "two maps need a wide 3:1 plate even at the smallest preset"
        );
        assert_eq!(FloatingSizePreset::Mini.next(), FloatingSizePreset::Small);
        assert_eq!(FloatingSizePreset::Small.next(), FloatingSizePreset::Medium);
        assert_eq!(FloatingSizePreset::Medium.next(), FloatingSizePreset::Large);
        assert_eq!(FloatingSizePreset::Large.next(), FloatingSizePreset::Mini);
    }

    #[test]
    fn preset_inference_round_trips_every_authored_single_and_dual_size() {
        assert_eq!(
            FloatingSizePreset::from_logical_size(160, 96),
            FloatingSizePreset::Mini,
            "a freely dragged ultra-compact window reports the nearest mini preset"
        );
        for dual_map in [false, true] {
            for preset in [
                FloatingSizePreset::Mini,
                FloatingSizePreset::Small,
                FloatingSizePreset::Medium,
                FloatingSizePreset::Large,
            ] {
                let (width, height) = preset.logical_size(dual_map);
                assert_eq!(
                    FloatingSizePreset::from_logical_size(width, height),
                    preset,
                    "{width}x{height} must restore its own selector step"
                );
            }
        }
    }

    #[test]
    fn preset_size_fits_inside_a_monitor_without_changing_aspect_or_enlarging() {
        assert_eq!(fit_logical_size((960, 320), (1920, 1080), 0.9), (960, 320));
        assert_eq!(
            fit_logical_size((1440, 480), (1200, 800), 0.9),
            (1080, 360),
            "the wide window is uniformly reduced to 90% of monitor width"
        );
        assert_eq!(
            fit_logical_size((720, 480), (800, 500), 0.9),
            (675, 450),
            "height can be the limiting axis while 3:2 is preserved"
        );
    }
}
