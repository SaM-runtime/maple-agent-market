//! Content-addressed asset-manager contracts exercised through its public
//! filesystem seam. Network access and user config are intentionally absent.

use pixtuoid::assets::{
    import_local_pack, install_public_classic, verify_installed, AssetManifest, InstallMode,
    ASSET_MANIFEST_FILE, PUBLIC_CLASSIC_ID,
};
use pixtuoid::init_pack::init_pack;

#[test]
fn public_classic_installs_verifies_and_detects_tampering() {
    let tmp = tempfile::tempdir().unwrap();

    let installed = install_public_classic(tmp.path(), InstallMode::RefuseExisting).unwrap();
    assert_eq!(installed.id, PUBLIC_CLASSIC_ID);
    assert_eq!(installed.file_count, 49);
    assert_eq!(installed.fingerprint_sha256.len(), 64);
    assert!(installed.path.join("pack.toml").is_file());
    assert!(installed.path.join("ASSET-MANIFEST.json").is_file());

    let verified = verify_installed(tmp.path(), PUBLIC_CLASSIC_ID, None).unwrap();
    assert_eq!(verified.fingerprint_sha256, installed.fingerprint_sha256);
    assert_eq!(verified.file_count, installed.file_count);

    let sprite_path = installed.path.join("standing.sprite");
    let mut tampered = std::fs::read(&sprite_path).unwrap();
    tampered[0] ^= 1;
    std::fs::write(&sprite_path, tampered).unwrap();
    let err = verify_installed(tmp.path(), PUBLIC_CLASSIC_ID, None).unwrap_err();
    assert!(
        err.to_string().contains("standing.sprite") && err.to_string().contains("SHA-256 mismatch"),
        "unexpected tamper error: {err:#}"
    );

    let repaired = install_public_classic(tmp.path(), InstallMode::ReplaceManaged).unwrap();
    assert_eq!(repaired.fingerprint_sha256, installed.fingerprint_sha256);
    verify_installed(tmp.path(), PUBLIC_CLASSIC_ID, None).unwrap();
}

#[test]
fn force_never_replaces_an_unmanaged_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let destination = tmp.path().join(PUBLIC_CLASSIC_ID);
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("keep-me.txt"), "user data").unwrap();

    let err = install_public_classic(tmp.path(), InstallMode::ReplaceManaged).unwrap_err();
    assert!(err.to_string().contains("refusing to replace unmanaged"));
    assert_eq!(
        std::fs::read_to_string(destination.join("keep-me.txt")).unwrap(),
        "user data"
    );
}

#[test]
fn local_import_copies_only_pack_files_and_supports_team_fingerprint_checks() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    let root = tmp.path().join("managed");
    init_pack(&source, false).unwrap();
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
    assert!(
        !manifest
            .source
            .contains(&source.to_string_lossy().to_string()),
        "a shareable manifest must not leak the importer's absolute path"
    );

    verify_installed(
        &root,
        "team-pack",
        Some(&imported.fingerprint_sha256.to_ascii_uppercase()),
    )
    .unwrap();
    let err = verify_installed(
        &root,
        "team-pack",
        Some("0000000000000000000000000000000000000000000000000000000000000000"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("team fingerprint mismatch"));
}

#[test]
fn real_cli_lists_installs_and_verifies_public_classic() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("packs");
    let binary = env!("CARGO_BIN_EXE_pixtuoid");

    let list = std::process::Command::new(binary)
        .args(["assets", "--root"])
        .arg(&root)
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    let catalog: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(catalog[0]["id"], PUBLIC_CLASSIC_ID);
    assert_eq!(catalog[0]["installed"], false);

    let install = std::process::Command::new(binary)
        .args(["assets", "--root"])
        .arg(&root)
        .args(["install", PUBLIC_CLASSIC_ID, "--json"])
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let installed: serde_json::Value = serde_json::from_slice(&install.stdout).unwrap();
    let fingerprint = installed["fingerprint_sha256"].as_str().unwrap();
    assert_eq!(fingerprint.len(), 64);

    let verify = std::process::Command::new(binary)
        .args(["assets", "--root"])
        .arg(&root)
        .args([
            "verify",
            PUBLIC_CLASSIC_ID,
            "--expect",
            fingerprint,
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        verify.status.success(),
        "{}",
        String::from_utf8_lossy(&verify.stderr)
    );
    let verified: serde_json::Value = serde_json::from_slice(&verify.stdout).unwrap();
    assert_eq!(verified["fingerprint_sha256"], fingerprint);

    let source = tmp.path().join("friend-authored-pack");
    init_pack(&source, false).unwrap();
    let import = std::process::Command::new(binary)
        .args(["assets", "--root"])
        .arg(&root)
        .arg("import")
        .arg(&source)
        .args(["--id", "team-pack", "--json"])
        .output()
        .unwrap();
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );

    let list = std::process::Command::new(binary)
        .args(["assets", "--root"])
        .arg(&root)
        .args(["list", "--json"])
        .output()
        .unwrap();
    let catalog: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    let team = catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "team-pack")
        .unwrap();
    assert_eq!(team["distribution"], "local-only");
    assert_eq!(team["verified"], true);
}
