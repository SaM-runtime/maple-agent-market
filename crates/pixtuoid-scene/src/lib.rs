//! Window-free render and simulation engine used by Maple Agent Market.
//!
//! The crate owns layout, pose / motion / pathfinding, Maple map routing,
//! procedural pixel painters and optional sprite-pack overrides. It has no
//! terminal or window dependency; winit / softbuffer live in the binary crate.
//!
//! ```
//! use pixtuoid_scene::layout::Bounds;
//! use pixtuoid_scene::market::{market_slots, MARKET_MAX_AGENTS};
//! use pixtuoid_scene::theme::theme_by_name;
//!
//! let theme = theme_by_name("maple").expect("built-in Maple theme");
//! assert_eq!(theme.name, "maple");
//!
//! // Market placement is pure geometry and remains window-independent.
//! let slots = market_slots(
//!     Bounds { x: 0, y: 0, width: 240, height: 160 },
//!     MARKET_MAX_AGENTS,
//! );
//! assert_eq!(slots.len(), MARKET_MAX_AGENTS);
//! ```

// Terminal- and window-free (invariant #1, crate-boundary enforced). The dep
// boundary can't see a raw `println!` (std, no dep); this restriction lint does
// (a hard error under the workspace `-D warnings`). Non-test builds only, so
// test diagnostics may print freely.
#![cfg_attr(not(test), warn(clippy::print_stdout, clippy::print_stderr))]
// The engine crate has ZERO unsafe — lock that in so a future "just this once"
// block can't slip in (raw-pixel hot paths belong behind core's checked
// Grid/RgbBuffer seams). Lives here, not in Cargo.toml: `[lints] workspace =
// true` cannot be combined with a per-crate `[lints.rust]` table.
#![forbid(unsafe_code)]
// Public-API doc gate. Scoped to this PUBLISHED crate (not `[workspace.lints]`)
// because the binary crates' `pub` items aren't a semver surface — the same
// two-crate scope as the semver-checks + api-surface gates. `-D warnings`
// (`just clippy`) promotes it to a hard gate; `#[doc(hidden)] pub` is exempt.
#![warn(missing_docs)]

// Easing curves for the binary's floor-slide/popup animations — in-workspace
// painter plumbing, not a stable engine API.
#[doc(hidden)]
pub mod anim;
#[doc(hidden)] // shared workspace model (the overlay/board pattern), not stable engine API
pub mod audio;
// The neon wall-board MODEL + shared scene-stats tally the three in-workspace
// painters consume — their shared single source of truth, not a stable engine API.
#[doc(hidden)]
pub mod board;
// Burn-tier interpretation (model/effort → hair color, dossier effort row)
// the in-workspace painters + the binary's tooltip consume — shared single
// source of truth, not a stable engine API.
#[doc(hidden)]
pub mod burn;
pub mod chitchat;
// Fixed-slot character roster and per-actor appearance selection shared by
// both Maple maps and the floating character panel.
#[doc(hidden)]
pub mod characters;
pub(crate) mod creatures;
pub mod embedded_pack;
pub mod floor;
// The status-footer MODEL the two in-workspace painters consume — their shared
// single source of truth (like board/overlay), not a stable engine API.
#[doc(hidden)]
pub mod footer;
// Per-agent recolored-sprite cache owned by each painter's `FloorCtx` — an
// in-workspace render internal, not a stable engine API.
#[doc(hidden)]
pub mod frame_cache;
pub mod layout;
// Shared Maple-world map routing for the floating painter.  This presentation
// state deliberately stays outside core's immutable desk/floor assignment.
#[doc(hidden)]
pub mod maple_world;
// Shared free-market slot + overlay authority for in-workspace painters, not a
// stable engine API (same boundary as overlay/board).
#[doc(hidden)]
pub mod market;
pub mod motion;
// The name-badge label MODEL the two in-workspace painters consume — their
// shared single source of truth, not a stable engine API.
#[doc(hidden)]
pub mod overlay;
pub mod pathfind;
/// Office pets — the `Pet`/`PetKind` model and per-floor selection.
pub mod pet;
pub mod physics;
pub mod pixel_painter;
pub mod pose;
/// The color-theme MODEL: the `Theme` role palette and the bundled themes.
pub mod theme;
pub mod token_meter;
// Forest-training placement/motion authority shared by pixels and labels.
#[doc(hidden)]
pub mod training;

/// Test-only mutex serializing tests that mutate process-global environment
/// variables (`XDG_CONFIG_HOME`). The crate's unit tests share one test binary,
/// so two env-mutating tests can otherwise race under plain `cargo test`
/// (nextest isolates per-process, but the `justfile` falls back to `cargo test`
/// when nextest is absent). Lock it for the whole test.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
