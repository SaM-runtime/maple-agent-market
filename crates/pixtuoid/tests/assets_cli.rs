//! Local-only content-addressed asset-manager contracts.

use std::path::Path;

use pixtuoid::assets::{
    import_local_pack, verify_installed, AssetManifest, InstallMode, ASSET_MANIFEST_FILE,
};

fn write_test_pack(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("frame.sprite"), "@frame 0\nB\n").unwrap();
    std::fs::write(
        dir.join("pack.toml"),
        r##"
[pack]
name = "collaborator test pack"
version = "1"

[palette]
"." = "transparent"
B = "#4488cc"

[animations.seated]
frames = ["frame.sprite"]
frame_ms = 500
[animations.typing]
frames = ["frame.sprite", "frame.sprite"]
frame_ms = 200
[animations.standing]
frames = ["frame.sprite"]
frame_ms = 500
[animations.walking]
frames = ["frame.sprite", "frame.sprite"]
frame_ms = 200
[animations.walking_back]
frames = ["frame.sprite", "frame.sprite"]
frame_ms = 200
[animations.seated_sleeping]
frames = ["frame.sprite"]
frame_ms = 500
[animations.seated_sleeping_alt]
frames = ["frame.sprite"]
frame_ms = 500
[animations.holding_coffee]
frames = ["frame.sprite"]
frame_ms = 500
[animations.back_couch]
frames = ["frame.sprite"]
frame_ms = 500
"##,
    )
    .unwrap();
}

#[test]
fn local_import_is_fingerprinted_and_detects_tampering() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    let root = tmp.path().join("managed");
    write_test_pack(&source);
    std::fs::write(source.join("not-part-of-pack.txt"), "do not copy").unwrap();

    let imported =
        import_local_pack(&root, &source, "team-pack", InstallMode::RefuseExisting).unwrap();
    assert_eq!(imported.file_count, 2);
    assert!(!imported.path.join("not-part-of-pack.txt").exists());

    let manifest: AssetManifest =
        serde_json::from_slice(&std::fs::read(imported.path.join(ASSET_MANIFEST_FILE)).unwrap())
            .unwrap();
    assert_eq!(manifest.distribution, "local-only");
    assert_eq!(manifest.license, None);
    assert!(!manifest
        .source
        .contains(&source.to_string_lossy().to_string()));

    verify_installed(&root, "team-pack", Some(&imported.fingerprint_sha256)).unwrap();
    std::fs::write(imported.path.join("frame.sprite"), "tampered").unwrap();
    let error = verify_installed(&root, "team-pack", None).unwrap_err();
    assert!(error.to_string().contains("frame.sprite"));
}

#[test]
fn force_never_replaces_an_unmanaged_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    let root = tmp.path().join("managed");
    let destination = root.join("team-pack");
    write_test_pack(&source);
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("keep-me.txt"), "user data").unwrap();

    let error =
        import_local_pack(&root, &source, "team-pack", InstallMode::ReplaceManaged).unwrap_err();
    assert!(error.to_string().contains("refusing to replace unmanaged"));
    assert_eq!(
        std::fs::read_to_string(destination.join("keep-me.txt")).unwrap(),
        "user data"
    );
}

#[test]
fn real_cli_lists_imports_and_verifies_local_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("packs");
    let source = tmp.path().join("friend-authored-pack");
    let binary = env!("CARGO_BIN_EXE_maple-agent-market");
    write_test_pack(&source);

    let empty = std::process::Command::new(binary)
        .args(["assets", "--root"])
        .arg(&root)
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert!(empty.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&empty.stdout).unwrap(),
        serde_json::json!([])
    );

    let imported = std::process::Command::new(binary)
        .args(["assets", "--root"])
        .arg(&root)
        .arg("import")
        .arg(&source)
        .args(["--id", "team-pack", "--json"])
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&imported.stdout).unwrap();
    let fingerprint = result["fingerprint_sha256"].as_str().unwrap();

    let verified = std::process::Command::new(binary)
        .args(["assets", "--root"])
        .arg(&root)
        .args(["verify", "team-pack", "--expect", fingerprint, "--json"])
        .output()
        .unwrap();
    assert!(verified.status.success());

    let listed = std::process::Command::new(binary)
        .args(["assets", "--root"])
        .arg(&root)
        .args(["list", "--json"])
        .output()
        .unwrap();
    let catalog: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(catalog[0]["id"], "team-pack");
    assert_eq!(catalog[0]["distribution"], "local-only");
    assert_eq!(catalog[0]["verified"], true);
}
