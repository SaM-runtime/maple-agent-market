fn is_newer_version(current: &str, last_seen: &str) -> bool {
    parse_semver(current)
        .zip(parse_semver(last_seen))
        .is_some_and(|(current, last_seen)| current > last_seen)
}

fn is_valid_version(value: &str) -> bool {
    parse_semver(value).is_some()
}

pub(crate) struct BootDecision {
    pub(crate) should_show_popup: bool,
    pub(crate) should_persist: bool,
}

/// 決定啟動時是否顯示版本訊息，以及是否更新 `last-seen-version`。
pub(crate) fn boot_decision(current_ver: &str, last_seen: Option<&str>) -> BootDecision {
    let last_seen_parseable = last_seen.is_some_and(is_valid_version);
    let should_show_popup = match last_seen {
        Some(last) if last_seen_parseable => {
            is_newer_version(current_ver, last) && release_notes(current_ver).is_some()
        }
        _ => false,
    };
    let should_persist = should_show_popup || last_seen.is_none() || !last_seen_parseable;
    BootDecision {
        should_show_popup,
        should_persist,
    }
}

/// 將 `major.minor.patch[-prerelease]` 轉成可比較的簡化 semver tuple。
fn parse_semver(version: &str) -> Option<(u64, u64, u64, u8)> {
    let mut parts = version.splitn(3, '.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().unwrap_or("0");
    let (patch, is_release) = match patch.split_once('-') {
        Some((number, _)) => (number.parse().ok()?, 0),
        None => (patch.parse().ok()?, 1),
    };
    Some((major, minor, patch, is_release))
}

/// Maple Agent Market 的 in-app 更新摘要。
pub(crate) fn release_notes(version: &str) -> Option<&'static [&'static str]> {
    match version {
        "0.16.0" => Some(&[
            "Maple Agent Market：自由市場與訓練場可並排或單獨顯示",
            "agent 會依執行、等待、閒置與離場狀態呈現不同動作",
            "主 task、子代理、商店字卡、角色 ID 與訊息列更容易辨識",
            "floating 視窗支援拖曳、自由縮放、尺寸預設與快捷關閉",
            "可載入本機 sprite pack 與 BGM；公開版不包含遊戲素材",
        ]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_handles_release_and_prerelease() {
        assert!(is_newer_version("0.2.0", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.2.0"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
        assert!(is_newer_version("0.5.0", "0.5.0-rc1"));
        assert!(!is_newer_version("0.5.0-rc1", "0.5.0"));
    }

    #[test]
    fn malformed_versions_are_rejected() {
        for malformed in ["", "v0.16.0", "not-semver"] {
            assert!(!is_valid_version(malformed), "accepted {malformed:?}");
        }
        assert!(is_valid_version("0.16"));
    }

    #[test]
    fn current_version_has_release_notes() {
        let current = env!("CARGO_PKG_VERSION");
        let notes = release_notes(current)
            .unwrap_or_else(|| panic!("release_notes({current:?}) is missing"));
        assert!(!notes.is_empty());
    }

    #[test]
    fn unknown_version_has_no_release_notes() {
        assert!(release_notes("9.9.9").is_none());
    }

    #[test]
    fn boot_decision_recovers_invalid_or_missing_state() {
        for last_seen in [None, Some("v0.15.0")] {
            let decision = boot_decision("0.16.0", last_seen);
            assert!(!decision.should_show_popup);
            assert!(decision.should_persist);
        }
    }

    #[test]
    fn boot_decision_shows_current_upgrade_once() {
        let upgrade = boot_decision("0.16.0", Some("0.15.0"));
        assert!(upgrade.should_show_popup);
        assert!(upgrade.should_persist);

        let same = boot_decision("0.16.0", Some("0.16.0"));
        assert!(!same.should_show_popup);
        assert!(!same.should_persist);
    }

    #[test]
    fn path_dependency_versions_track_workspace_version() {
        let assert_tracks = |manifest: &str, name: &str| {
            let mut checked = 0;
            for line in manifest.lines() {
                let line = line.trim_start();
                if !(line.contains("path =") && line.contains("version = \"")) {
                    continue;
                }
                let dependency_version = line
                    .split_once("version = \"")
                    .and_then(|(_, rest)| rest.split('"').next())
                    .expect("path dependency version");
                assert_eq!(
                    dependency_version,
                    env!("CARGO_PKG_VERSION"),
                    "{name}: path dependency version drifted"
                );
                checked += 1;
            }
            assert!(checked > 0, "{name}: no versioned path dependency found");
        };

        assert_tracks(include_str!("../Cargo.toml"), "pixtuoid");
        assert_tracks(include_str!("../../pixtuoid-scene/Cargo.toml"), "scene");
    }
}
