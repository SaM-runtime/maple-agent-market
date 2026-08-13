use std::path::{Path, PathBuf};

use super::*;

#[test]
fn audio_defaults_to_muted_and_clamps_volume() {
    let cfg = AppConfig::default();
    let audio = resolve_audio(&cfg);
    assert!(audio.muted);
    assert_eq!(audio.volume, 1.0);

    for (raw, expected) in [(-0.5, 0.0), (0.4, 0.4), (1.5, 1.0)] {
        let cfg = AppConfig {
            audio: Some(AudioConfigRaw {
                muted: Some(false),
                volume: Some(raw),
                ..Default::default()
            }),
            ..Default::default()
        };
        let audio = resolve_audio(&cfg);
        assert!(!audio.muted);
        assert!((audio.volume - expected).abs() < f32::EPSILON);
    }
}

#[test]
fn local_bgm_path_round_trips() {
    let cfg: AppConfig =
        toml::from_str("[audio]\nmuted = false\nvolume = 0.35\nbgm-path = 'C:/Music/market.mp3'\n")
            .expect("valid config");
    assert_eq!(
        resolve_audio(&cfg).bgm_path.as_deref(),
        Some(Path::new("C:/Music/market.mp3"))
    );
}

#[test]
fn floating_defaults_and_clamps_to_legible_bounds() {
    let defaults = resolve_floating(&AppConfig::default());
    assert_eq!(defaults.width, FLOATING_DEFAULT_W);
    assert_eq!(defaults.height, FLOATING_DEFAULT_H);
    assert_eq!(defaults.opacity, 1.0);

    let cfg = AppConfig {
        floating: Some(FloatingConfigRaw {
            width: Some(1),
            height: Some(2),
            x: Some(-20),
            y: Some(30),
            opacity: Some(0.01),
        }),
        ..Default::default()
    };
    let resolved = resolve_floating(&cfg);
    assert_eq!(resolved.width, FLOATING_MIN_W);
    assert_eq!(resolved.height, FLOATING_MIN_H);
    assert_eq!(resolved.x, Some(-20));
    assert_eq!(resolved.y, Some(30));
    assert_eq!(resolved.opacity, FLOATING_MIN_OPACITY);
}

#[test]
fn cli_pack_dir_wins_over_config() {
    let cfg = AppConfig {
        pack_dir: Some("configured-pack".into()),
        ..Default::default()
    };
    assert_eq!(
        resolve_pack_dir(&cfg, Some(PathBuf::from("cli-pack"))),
        Some(PathBuf::from("cli-pack"))
    );
    assert_eq!(
        resolve_pack_dir(&cfg, None),
        Some(PathBuf::from("configured-pack"))
    );
}

#[test]
fn missing_config_loads_as_empty_without_warning() {
    let dir = tempfile::tempdir().unwrap();
    let mut warnings = Vec::new();
    let cfg = load(&dir.path().join("missing.toml"), &mut warnings);
    assert!(cfg.pack_dir.is_none());
    assert!(cfg.sources.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn malformed_config_falls_back_and_reports_one_warning() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[audio\n").unwrap();
    let mut warnings = Vec::new();
    let cfg = load(&path, &mut warnings);
    assert!(cfg.audio.is_none());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("malformed config"));
}

#[test]
fn legacy_office_keys_are_ignored_instead_of_reactivated() {
    let cfg: AppConfig = toml::from_str(
        "theme = 'maple'\nmax-desks = 8\nlast-seen-version = '0.15'\n\
         [[pets]]\nkind = 'cat'\n\
         [audio]\nmuted = true\n",
    )
    .expect("unknown legacy keys are tolerated");
    assert!(resolve_audio(&cfg).muted);
    assert!(cfg.pack_dir.is_none());
}

#[test]
fn source_flags_resolve_only_registered_true_entries() {
    let cfg: AppConfig =
        toml::from_str("[sources]\ncodex = true\nclaude-code = false\nnot-a-source = true\n")
            .unwrap();
    let connected = resolve_connected(&cfg);
    assert!(connected.contains("codex"));
    assert!(!connected.contains("claude-code"));
    assert!(!connected.contains("not-a-source"));
}

#[test]
fn source_save_and_remove_preserve_comments_and_unknown_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "# hand tuned\nfuture-key = 7\n").unwrap();

    save_source_connected(&path, "codex", true).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("# hand tuned"));
    assert!(written.contains("future-key = 7"));
    assert!(written.contains("codex = true"));

    remove_source_connected(&path, "codex").unwrap();
    let removed = std::fs::read_to_string(&path).unwrap();
    assert!(!removed.contains("[sources]"));
    assert!(removed.contains("future-key = 7"));
}

#[test]
fn audio_updates_preserve_siblings_and_comments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "# user file\nfuture-key = 9\n[audio]\nvolume = 0.4\n",
    )
    .unwrap();

    save_audio_muted(&path, false).unwrap();
    save_audio_volume(&path, 0.75).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("# user file"));
    assert!(written.contains("future-key = 9"));
    assert!(written.contains("muted = false"));
    assert!(written.contains("volume = 0.75"));
}

#[test]
fn malformed_existing_config_is_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let malformed = "[audio\n";
    std::fs::write(&path, malformed).unwrap();
    assert!(save_audio_muted(&path, false).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), malformed);
}

#[test]
fn floating_save_sets_size_and_clears_unavailable_position() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[floating]\nwidth = 360\nheight = 240\nx = 10\ny = 20\n",
    )
    .unwrap();
    save_floating(&path, 720, 480, None, None).unwrap();
    let cfg = load(&path, &mut Vec::new());
    let floating = resolve_floating(&cfg);
    assert_eq!((floating.width, floating.height), (720, 480));
    assert_eq!(floating.x, None);
    assert_eq!(floating.y, None);
}

#[test]
fn floating_save_can_persist_negative_monitor_coordinates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    save_floating(&path, 640, 360, Some(-1920), Some(-120)).unwrap();
    let floating = resolve_floating(&load(&path, &mut Vec::new()));
    assert_eq!(floating.x, Some(-1920));
    assert_eq!(floating.y, Some(-120));
}

#[test]
fn config_round_trip_contains_only_current_product_sections() {
    let cfg = AppConfig {
        pack_dir: Some("C:/packs/team".into()),
        sources: [("codex".to_owned(), true)].into_iter().collect(),
        floating: Some(FloatingConfigRaw {
            width: Some(720),
            height: Some(480),
            ..Default::default()
        }),
        audio: Some(AudioConfigRaw {
            muted: Some(true),
            volume: Some(0.5),
            ..Default::default()
        }),
    };
    let encoded = toml::to_string(&cfg).unwrap();
    assert!(encoded.contains("pack-dir"));
    assert!(encoded.contains("[sources]"));
    assert!(encoded.contains("[floating]"));
    assert!(encoded.contains("[audio]"));
    for removed in ["theme", "max-desks", "last-seen-version", "[[pets]]"] {
        assert!(!encoded.contains(removed), "removed key leaked: {removed}");
    }
}
