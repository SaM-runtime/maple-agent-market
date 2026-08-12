//! Sprite pack loader.
//!
//! Tries the user-config path first (XDG-style) so power users can drop in a
//! custom pack without recompiling. Falls back to the embedded default pack
//! (compile-time `include_str!`) so the binary ships standalone.
//!
//! ## Custom pack layout
//!
//! Drop a directory at `${XDG_CONFIG_HOME:-~/.config}/pixtuoid/sprites/`
//! containing `pack.toml` + each `.sprite` file referenced from the TOML.
//! See `crates/pixtuoid-scene/sprites/default/` for the canonical example.
//!
//! ## Sharp edge — palette RGB uniqueness
//!
//! The per-agent recolor (`recolor_frame` in `pixel_painter::palette`)
//! substitutes the H/S/B palette colors by RGB equality. If a custom pack
//! reuses the same RGB for two palette keys, the recolor pass will substitute
//! both, producing visual artifacts. Each palette key MUST map to a unique
//! RGB triple.

use std::path::PathBuf;

use anyhow::Result;
use pixtuoid_core::sprite::format::{
    load_pack, load_pack_from_strings, validate_pack_animations, Pack, ValidationReport,
};

/// Resolve the user's sprite-pack directory if XDG settings point at one.
/// Returns the directory only when `pack.toml` exists inside it — otherwise
/// the caller falls back to the embedded pack.
fn xdg_pack_dir() -> Option<PathBuf> {
    let base = xdg_config_base(
        std::env::var_os("XDG_CONFIG_HOME"),
        pixtuoid_core::platform::user_home_opt().map(PathBuf::from),
    )?;
    let dir = base.join("pixtuoid").join("sprites");
    if dir.join("pack.toml").is_file() {
        Some(dir)
    } else {
        None
    }
}

/// Resolve the XDG config base: the env value when set to a NON-EMPTY path, else
/// `<home>/.config`. Per the XDG basedir spec, an EMPTY **or RELATIVE**
/// `XDG_CONFIG_HOME` is invalid and counts as unset — `is_absolute()` rejects
/// both. Without it a `Some("")`/`Some("rel")` skips the fallback and yields a
/// CWD-RELATIVE `pixtuoid/sprites` path, silently loading an untrusted pack from
/// the launch directory while ignoring the user's real `~/.config`. Pure (the env
/// value is passed in) so the precedence is unit-testable without mutating env.
/// Mirrors the binary's `install::io::nonempty_abs_env` `is_absolute()` rule,
/// kept inline per the per-crate-copy convention (scene can't depend on the
/// binary; core's `platform::nonempty` is `pub(crate)`).
fn xdg_config_base(xdg: Option<std::ffi::OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    xdg.filter(|v| std::path::Path::new(v).is_absolute())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".config")))
}

/// Warn when a supplied optional animation is incomplete. Missing animations
/// are valid because every public Maple layer has a programmatic fallback.
fn warn_pack_validation_gaps(pack: &Pack, origin: &str) -> ValidationReport {
    let report = validate_pack_animations(pack);
    for (name, min, got) in &report.insufficient_frames {
        tracing::warn!(
            origin,
            animation = %name,
            min,
            got,
            "custom Maple pack animation has too few frames — the built-in fallback remains active"
        );
    }
    report
}

/// Load the character sprite pack: the compiled-in default pack, with an
/// optional `--pack-dir` custom pack merged over it.
pub fn load_sprite_pack(pack_dir: Option<PathBuf>) -> Result<Pack> {
    let base = load_embedded_pack()?;

    if let Some(dir) = pack_dir {
        let mut custom = load_pack(&dir).map_err(|e| {
            anyhow::anyhow!("failed to load sprite pack from {}: {e}", dir.display())
        })?;
        tracing::info!(path = %dir.display(), "loaded sprite pack from --pack-dir");
        custom.merge_from(&base);
        warn_pack_validation_gaps(&custom, "--pack-dir");
        return Ok(custom);
    }
    if let Some(dir) = xdg_pack_dir() {
        match load_pack(&dir) {
            Ok(mut p) => {
                tracing::info!(path = %dir.display(), "loaded user sprite pack");
                p.merge_from(&base);
                warn_pack_validation_gaps(&p, "xdg");
                return Ok(p);
            }
            Err(e) => {
                tracing::warn!(
                    path = %dir.display(),
                    error = %e,
                    "user sprite pack failed to load; falling back to embedded default"
                );
            }
        }
    }
    Ok(base)
}

/// Test-only default-pack loader: takes the crate's `TEST_ENV_LOCK` around the
/// `XDG_CONFIG_HOME` read inside [`load_sprite_pack`], so an env-READING pack
/// load can't race the env-MUTATING test
/// (`load_sprite_pack_resolves_then_falls_back_via_xdg`) under plain
/// `cargo test` — one test binary, many threads (nextest's per-process
/// isolation masks the race). Every unit test resolving the default pack must
/// come through here, never a bare `load_sprite_pack(None)`.
#[cfg(test)]
pub(crate) fn test_default_pack() -> Pack {
    test_fixture_pack("fixture.sprite")
}

fn load_embedded_pack() -> Result<Pack> {
    // The public clean-clone pack is deliberately metadata-only. All visible
    // Maple elements have an original procedural painter; local packs may add
    // optional animation overrides at runtime.
    load_pack_from_strings(include_str!("../sprites/default/pack.toml"), &[])
}

/// The default pack with a 10px-wide `standing` frame (robot packs go up to 10)
/// so the pack-resolved `char_w` differs from the bundled 8-wide `CHARACTER_SPRITE_W`
/// — the only way to drive `sim_step`/`resolve_characters` occupancy + anchors
/// end-to-end at a non-default width (#609). Reuses the FULL default sprite set
/// so `resolve_characters` still finds every pose; only `standing.sprite` is swapped.
#[cfg(test)]
pub(crate) fn test_wide_pack() -> Pack {
    // No TEST_ENV_LOCK: unlike test_default_pack, this builds via the pure
    // load_pack_from_strings and never reads XDG_CONFIG_HOME.
    // The bundled 8x12 standing pose padded to 10 wide with transparent columns
    // (same palette keys). char_w = this frame's width = 10.
    const WIDE_STANDING: &str = "\
@frame 0
. . n H H H H n . .
. n H H H H H H n .
. H H S S S S H H .
. H S e S S e S H .
. . S S S m S S . .
. . n S S S S n . .
. . B B B B B B . .
. B B B B B B B B .
. S B B B B B B S .
. . P P P P P P . .
. . P P P P P P . .
. . P . . . . P . .
";
    let toml = test_fixture_pack_toml("wide.sprite");
    load_pack_from_strings(
        &toml,
        &[
            ("fixture.sprite", TEST_FIXTURE_SPRITE),
            ("wide.sprite", WIDE_STANDING),
        ],
    )
    .expect("wide test pack loads")
}

#[cfg(test)]
const TEST_FIXTURE_SPRITE: &str = "\
@frame 0
. . H H H H . .
. H H H H H H .
. H S S S S H .
. S e S S e S .
. . S S S S . .
. . B B B B . .
. B B B B B B .
. . P P P P . .
. . P . . P . .
. . P . . P . .
. . n . . n . .
. . . . . . . .
@frame 1
. . H H H H . .
. H H H H H H .
. H S S S S H .
. S e S S e S .
. . S S S S . .
. . B B B B . .
. B B B B B B .
. . P P P P . .
. P P . . P . .
. P . . . P . .
. n . . . n . .
. . . . . . . .
@frame 2
. . H H H H . .
. H H H H H H .
. H S S S S H .
. S e S S e S .
. . S S S S . .
. . B B B B . .
. B B B B B B .
. . P P P P . .
. . P . . P P .
. . P . . . P .
. . n . . . n .
. . . . . . . .
@frame 3
. . H H H H . .
. H H H H H H .
. H S S S S H .
. S e S S e S .
. . S S S S . .
. . B B B B . .
. B B B B B B .
. . P P P P . .
. . P P P P . .
. . P . . P . .
. . n . . n . .
. . . . . . . .
";

#[cfg(test)]
fn test_fixture_pack(standing: &str) -> Pack {
    let toml = test_fixture_pack_toml(standing);
    load_pack_from_strings(&toml, &[("fixture.sprite", TEST_FIXTURE_SPRITE)])
        .expect("test compatibility pack loads")
}

#[cfg(test)]
fn test_fixture_pack_toml(standing: &str) -> String {
    let names = [
        "seated",
        "side_seated",
        "typing",
        "standing",
        "walking",
        "walking_back",
        "walking_coffee",
        "seated_sleeping",
        "seated_sleeping_alt",
        "holding_coffee",
        "back_couch",
        "desk",
        "filing_cabinet",
        "plant",
        "plant_tall",
        "plant_flower",
        "plant_succulent",
        "floor_lamp",
        "door",
        "cat_walk",
        "cat_sit",
        "cat_sleep",
        "dog_walk",
        "dog_sit",
        "dog_sleep",
        "lobster_walk",
        "lobster_rest",
        "meeting_sofa",
        "meeting_screen",
        "pantry",
        "pantry_small",
        "whiteboard",
        "bookshelf",
        "snack_shelf",
        "tv_stand",
        "phone_booth",
        "standing_desk",
        "bulletin_board",
        "exit_sign",
    ];
    let mut out = String::from(
        "[pack]\nname = \"test-compatibility\"\nversion = \"1\"\n\n[palette]\n\
         \".\" = \"transparent\"\nH = \"#443322\"\nS = \"#e8c090\"\n\
         B = \"#4488cc\"\nP = \"#334455\"\ne = \"#202020\"\nn = \"#664422\"\nm = \"#aa6655\"\n",
    );
    for name in names {
        let file = if name == "standing" {
            standing
        } else {
            "fixture.sprite"
        };
        out.push_str(&format!(
            "\n[animations.{name}]\nframes = [\"{file}\"]\nframe_ms = 200\n"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn xdg_config_base_treats_empty_or_relative_as_unset() {
        // XDG spec: an EMPTY or RELATIVE XDG_CONFIG_HOME is invalid → unset, else
        // the pack dir resolves CWD-relative (an untrusted pack from the launch dir).
        for invalid in ["", "   ", "rel/config", "~/config"] {
            assert_eq!(
                xdg_config_base(
                    Some(std::ffi::OsString::from(invalid)),
                    Some(PathBuf::from("/home/u"))
                ),
                Some(PathBuf::from("/home/u/.config")),
                "invalid XDG_CONFIG_HOME {invalid:?} must fall to ~/.config"
            );
        }
    }

    #[test]
    fn xdg_config_base_prefers_a_set_value_over_home() {
        // An ABSOLUTE value wins over home. The absolute form is platform-specific
        // — a leading-slash path is NOT absolute on Windows (no drive prefix).
        let abs = if cfg!(windows) { "C:/xdg" } else { "/xdg" };
        assert_eq!(
            xdg_config_base(
                Some(std::ffi::OsString::from(abs)),
                Some(PathBuf::from("/home/u")),
            ),
            Some(PathBuf::from(abs)),
        );
    }

    #[test]
    fn xdg_config_base_falls_back_to_home_when_absent() {
        assert_eq!(
            xdg_config_base(None, Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/home/u/.config")),
        );
    }

    #[test]
    fn xdg_config_base_is_none_without_xdg_or_home() {
        assert_eq!(
            xdg_config_base(Some(std::ffi::OsString::from("")), None),
            None
        );
        assert_eq!(xdg_config_base(None, None), None);
    }

    fn write_optional_map_pack(dst: &Path) {
        fs::create_dir_all(dst).expect("mkdir pack dir");
        fs::write(
            dst.join("pack.toml"),
            r##"[pack]
name = "test-map-pack"
version = "1"

[palette]
A = "#224466"

[animations.scene_background]
frames = ["frame.sprite"]
frame_ms = 100
"##,
        )
        .expect("write pack.toml");
        fs::write(dst.join("frame.sprite"), "@frame 0\nA\n").expect("write frame");
    }

    #[test]
    fn load_sprite_pack_from_custom_dir_merges_with_embedded() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let pack_dir = tmp.path().join("custom");
        write_optional_map_pack(&pack_dir);

        let pack = load_sprite_pack(Some(pack_dir)).expect("custom pack loads");
        // A local optional plate loads over the metadata-only public pack.
        assert!(
            pack.animation("scene_background").is_some(),
            "custom pack must carry its supplied map plate"
        );
        assert!(pack.animation("desk").is_none());
    }

    /// Counts WARN-level tracing events emitted inside `with_default`.
    #[derive(Clone)]
    struct WarnCounter(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl tracing::Subscriber for WarnCounter {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.level() == &tracing::Level::WARN
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, _: &tracing::Event<'_>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    #[test]
    fn embedded_pack_is_the_maple_agent_market_original_not_the_old_office() {
        let pack = load_sprite_pack(None).expect("embedded pack");
        assert_eq!(pack.name, "Maple Agent Market Original");
        for legacy in [
            "desk",
            "meeting_sofa",
            "meeting_screen",
            "pantry",
            "phone_booth",
            "standing_desk",
        ] {
            assert!(
                pack.animation(legacy).is_none(),
                "the public runtime must not embed Pixtuoid office animation {legacy:?}"
            );
        }
    }

    #[test]
    fn embedded_default_pack_animations_are_all_in_the_registry() {
        // The scene-side half of the registry bridge: every animation the
        // EMBEDDED pack ships must be registry-known, or validate-pack
        // falsely reports it "unused by renderer" (the side_seated drift).
        let pack = load_sprite_pack(None).expect("embedded pack");
        let report = pixtuoid_core::sprite::format::validate_pack_animations(&pack);
        assert!(
            report.unknown.is_empty(),
            "embedded animation missing from the registry: {:?}",
            report.unknown
        );
    }

    #[test]
    fn incomplete_supplied_cycle_loads_with_a_warning() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let pack_dir = tmp.path().join("gappy");
        write_optional_map_pack(&pack_dir);
        let toml_path = pack_dir.join("pack.toml");
        let mut toml = fs::read_to_string(&toml_path).expect("read pack.toml");
        toml.push_str(
            "\n[animations.training_skill_holy_light]\n\
             frames = [\"frame.sprite\", \"frame.sprite\", \"frame.sprite\"]\n\
             frame_ms = 100\n",
        );
        fs::write(&toml_path, toml).expect("write incomplete pack.toml");

        let warns = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let pack = tracing::subscriber::with_default(WarnCounter(warns.clone()), || {
            load_sprite_pack(Some(pack_dir))
        })
        .expect("an incomplete optional cycle still loads");
        assert!(
            pack.animation("training_skill_holy_light").is_some(),
            "the supplied incomplete animation remains inspectable"
        );
        assert!(
            warns.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "load_sprite_pack must warn about the incomplete supplied cycle"
        );
        assert_eq!(
            warn_pack_validation_gaps(&pack, "test").insufficient_frames,
            vec![("training_skill_holy_light".to_string(), 4, 3)]
        );
    }

    #[test]
    fn load_sprite_pack_from_missing_custom_dir_errors() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let missing = tmp.path().join("does-not-exist");
        assert!(
            load_sprite_pack(Some(missing)).is_err(),
            "a nonexistent --pack-dir must surface a load error"
        );
    }

    // The XDG path mutates a process-global env var. The TEST_ENV_LOCK
    // serializes this mutator against the crate's env-READING pack loads —
    // every `test_default_pack()` caller (floor / pixel_painter / the
    // embedded-pack tests below resolve the default pack through the same
    // XDG_CONFIG_HOME read) — so a reader can't observe the temp dirs set
    // here under plain `cargo test` (nextest's per-process isolation masks
    // the race). This test calls `load_sprite_pack` DIRECTLY, not the locked
    // helper: it already holds the (non-reentrant) lock.
    #[test]
    fn load_sprite_pack_resolves_then_falls_back_via_xdg() {
        let _env = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let saved = std::env::var_os("XDG_CONFIG_HOME");

        // (a) Valid XDG pack at $XDG/pixtuoid/sprites/ → loaded.
        let good = tempfile::TempDir::new().expect("tempdir");
        let good_sprites = good.path().join("pixtuoid").join("sprites");
        write_optional_map_pack(&good_sprites);
        std::env::set_var("XDG_CONFIG_HOME", good.path());
        let pack = load_sprite_pack(None).expect("xdg pack loads");
        assert!(
            pack.animation("scene_background").is_some(),
            "the valid XDG pack must be loaded (xdg Ok arm)"
        );

        // (b) Malformed pack.toml at the XDG path → warn + fall back to embedded.
        let bad = tempfile::TempDir::new().expect("tempdir");
        let bad_sprites = bad.path().join("pixtuoid").join("sprites");
        fs::create_dir_all(&bad_sprites).expect("mkdir bad sprites");
        fs::write(bad_sprites.join("pack.toml"), b"this is not valid toml {{{")
            .expect("write malformed pack.toml");
        std::env::set_var("XDG_CONFIG_HOME", bad.path());
        // The malformed pack triggers the Err arm → falls back to embedded (Ok),
        // which is the metadata-only Maple Agent Market pack.
        let fallback = load_sprite_pack(None).expect("malformed pack falls back, never errors");
        assert_eq!(fallback.name, "Maple Agent Market Original");
        assert!(fallback.animation_names().is_empty());

        // Restore env for the rest of the suite.
        match saved {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    // The recolor invariant applies to EVERY palette key, not just the 4
    // recolor targets: recolor_frame matches by RGB equality, so two keys
    // sharing a color are indistinguishable (a recolor — or any future
    // per-key logic — swaps both). Transparent (None) keys are exempt. Caught
    // the e/q = #1a1a1a dup that the B/H/S/P-only check below missed.
    #[test]
    fn embedded_pack_all_palette_keys_are_distinct_rgbs() {
        let pack = test_default_pack();
        let entries: Vec<(char, pixtuoid_core::sprite::Rgb)> = pack
            .palette
            .iter()
            .filter_map(|(k, p)| p.map(|rgb| (k, rgb)))
            .collect();
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                assert_ne!(
                    entries[i].1, entries[j].1,
                    "palette keys {:?} and {:?} share an RGB — recolor_frame can't distinguish them",
                    entries[i].0, entries[j].0
                );
            }
        }
    }

    // recolor_frame (pixel_painter/palette.rs) substitutes agent colors by RGB
    // equality against the base pack's B/H/S/P entries. If any two share an RGB,
    // the recolor pass swaps both and produces artifacts. No validate-pack check
    // enforces it, so this guards the documented uniqueness invariant for the
    // shipped embedded pack.
    #[test]
    fn embedded_pack_recolor_keys_are_distinct_rgbs() {
        let pack = test_default_pack();
        // The single source of truth — same set recolor_frame + the load guard use.
        let keys = pixtuoid_core::sprite::format::RECOLOR_KEYS;
        let rgbs: Vec<_> = keys
            .iter()
            .map(|&k| {
                pack.palette
                    .get(k)
                    .flatten()
                    .unwrap_or_else(|| panic!("embedded pack missing recolor key {k:?}"))
            })
            .collect();
        for i in 0..rgbs.len() {
            for j in (i + 1)..rgbs.len() {
                assert_ne!(
                    rgbs[i], rgbs[j],
                    "recolor keys {:?} and {:?} share an RGB — recolor_frame would swap both",
                    keys[i], keys[j]
                );
            }
        }
    }

    // `layout::CHARACTER_SPRITE_W` is the width every out-of-pixel_painter site
    // (hit-test pin box, decor walk-offset, floating label centering) hard-codes
    // its geometry on, as the width-unknown fallback for the pack's real
    // `frame.width`. If the embedded pack's character sprite ever grows/shrinks,
    // the const must move with it — else the pin box drifts off the painted
    // sprite. `sim.rs` resolves the SAME "standing" reference pose per frame.
    #[test]
    fn character_sprite_w_matches_the_embedded_pack() {
        let pack = test_default_pack();
        let frame = pack
            .animation("standing")
            .and_then(|a| a.frames.first())
            .expect("embedded pack carries a standing pose");
        let (w, h) = (frame.width(), frame.height());
        assert_eq!(
            w,
            crate::layout::CHARACTER_SPRITE_W,
            "embedded 'standing' sprite is {w}px wide but CHARACTER_SPRITE_W is {} — \
             update the const so hit-test/decor/label geometry tracks the pack",
            crate::layout::CHARACTER_SPRITE_W
        );
        // The px sprite is `CHARACTER_SPRITE_H_CELLS` half-block rows tall (2 px per cell); pin
        // the cell const too so the hit-test box height can't drift from the pack.
        assert_eq!(
            h,
            crate::layout::CHARACTER_SPRITE_H_CELLS * 2,
            "embedded 'standing' sprite is {h}px tall but CHARACTER_SPRITE_H_CELLS \
             ({}) implies {}px — update the const so the hit-test box tracks the pack",
            crate::layout::CHARACTER_SPRITE_H_CELLS,
            crate::layout::CHARACTER_SPRITE_H_CELLS * 2
        );
    }
}
