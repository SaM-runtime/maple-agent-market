//! Content-addressed local sprite-pack management.
//!
//! The manager copies only a pack the user explicitly supplies. It does not fetch
//! game/CDN/API/streaming content and never uploads an imported pack.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use pixtuoid_core::sprite::format::{load_pack, validate_pack_animations};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Manifest stored beside every managed pack.
pub const ASSET_MANIFEST_FILE: &str = "ASSET-MANIFEST.json";

const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// One immutable file entry in a managed pack.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetFileRecord {
    /// Portable slash-separated relative path.
    pub path: String,
    /// Exact byte length.
    pub bytes: u64,
    /// Lower-case SHA-256 of the file content.
    pub sha256: String,
}

/// Provenance and complete hash inventory for one managed pack.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetManifest {
    /// Manifest format version.
    pub schema_version: u32,
    /// Stable local pack id.
    pub id: String,
    /// Human-readable pack name.
    pub display_name: String,
    /// Pack or application version.
    pub version: String,
    /// Distribution classification from the import manifest.
    pub distribution: String,
    /// SPDX licence id when the pack has a verified public grant.
    pub license: Option<String>,
    /// Non-secret provenance summary or source URL.
    pub source: String,
    /// Digest of the sorted file inventory.
    pub fingerprint_sha256: String,
    /// Every managed file except this manifest itself.
    pub files: Vec<AssetFileRecord>,
}

/// Result returned after a successful install or verification.
#[derive(Clone, Debug, Serialize)]
pub struct AssetResult {
    /// Installed pack id.
    pub id: String,
    /// Managed pack directory.
    pub path: PathBuf,
    /// Pack-level fingerprint collaborators compare.
    pub fingerprint_sha256: String,
    /// Number of content files covered by the fingerprint.
    pub file_count: usize,
}

/// Catalog row returned by `assets list`.
#[derive(Clone, Debug, Serialize)]
pub struct AssetListEntry {
    /// Stable pack id.
    pub id: String,
    /// Human-readable name.
    pub display_name: String,
    /// Pack or application version.
    pub version: String,
    /// Redistribution classification from the manifest.
    pub distribution: String,
    /// Whether a managed directory exists locally.
    pub installed: bool,
    /// `Some(true)` for a verified install, `Some(false)` for a damaged one.
    pub verified: Option<bool>,
    /// Content fingerprint (built-in catalog or installed manifest).
    pub fingerprint_sha256: Option<String>,
    /// Installed path when present.
    pub path: Option<PathBuf>,
    /// Verification error for a damaged local install.
    pub error: Option<String>,
}

/// Policy for handling an existing managed pack directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallMode {
    /// Refuse to overwrite any existing directory.
    RefuseExisting,
    /// Replace only a directory carrying this manager's matching ownership marker.
    ReplaceManaged,
}

struct PackMetadata<'a> {
    id: &'a str,
    display_name: &'a str,
    version: &'a str,
    distribution: &'a str,
    license: Option<&'a str>,
    source: &'a str,
}

struct InstallRequest<'a> {
    root: &'a Path,
    metadata: PackMetadata<'a>,
    files: Vec<(String, Vec<u8>)>,
    mode: InstallMode,
}

struct StagingDir {
    path: PathBuf,
    keep: bool,
}

impl StagingDir {
    fn new(root: &Path, id: &str) -> Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = root.join(format!(".staging-{id}-{}-{nonce}", std::process::id()));
        fs::create_dir(&path)
            .with_context(|| format!("failed to create staging directory {}", path.display()))?;
        Ok(Self { path, keep: false })
    }

    fn commit(mut self) {
        self.keep = true;
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        if !self.keep && self.path.is_dir() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Default managed pack root beside the application config file.
pub fn default_assets_root() -> PathBuf {
    crate::config::config_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("packs")
}

/// List every managed local installation.
pub fn list_assets(root: &Path) -> Result<Vec<AssetListEntry>> {
    local_catalog_entries(root)
}

fn local_catalog_entries(root: &Path) -> Result<Vec<AssetListEntry>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    ensure_real_directory(root, "managed asset root")?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        entries.push(local_catalog_entry(root, path, name));
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(entries)
}

fn local_catalog_entry(root: &Path, path: PathBuf, id: String) -> AssetListEntry {
    let manifest = read_manifest(&path);
    match manifest {
        Ok(manifest) => catalog_entry_from_manifest(root, path, id, manifest),
        Err(error) => invalid_catalog_entry(path, id, "Invalid manifest", error),
    }
}

fn catalog_entry_from_manifest(
    root: &Path,
    path: PathBuf,
    id: String,
    manifest: AssetManifest,
) -> AssetListEntry {
    match verify_installed(root, &id, None) {
        Ok(result) => AssetListEntry {
            id,
            display_name: manifest.display_name,
            version: manifest.version,
            distribution: manifest.distribution,
            installed: true,
            verified: Some(true),
            fingerprint_sha256: Some(result.fingerprint_sha256),
            path: Some(path),
            error: None,
        },
        Err(error) => AssetListEntry {
            id,
            display_name: manifest.display_name,
            version: manifest.version,
            distribution: manifest.distribution,
            installed: true,
            verified: Some(false),
            fingerprint_sha256: Some(manifest.fingerprint_sha256),
            path: Some(path),
            error: Some(format!("{error:#}")),
        },
    }
}

fn invalid_catalog_entry(
    path: PathBuf,
    id: String,
    display_name: &str,
    error: anyhow::Error,
) -> AssetListEntry {
    AssetListEntry {
        id,
        display_name: display_name.to_string(),
        version: "unknown".to_string(),
        distribution: "unknown".to_string(),
        installed: true,
        verified: Some(false),
        fingerprint_sha256: None,
        path: Some(path),
        error: Some(format!("{error:#}")),
    }
}

/// Print a catalog without exposing raw control characters from local manifests.
pub fn print_asset_list(entries: &[AssetListEntry], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(entries)?);
        return Ok(());
    }
    println!("ID\tDISTRIBUTION\tSTATUS\tFINGERPRINT");
    for entry in entries {
        let status = match (entry.installed, entry.verified) {
            (false, _) => "available",
            (true, Some(true)) => "installed+verified",
            (true, Some(false)) => "installed+invalid",
            (true, None) => "installed",
        };
        let fingerprint = entry
            .fingerprint_sha256
            .as_deref()
            .map(crate::strip_control_chars)
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{}\t{}\t{}\t{}",
            crate::strip_control_chars(&entry.id),
            crate::strip_control_chars(&entry.distribution),
            status,
            fingerprint
        );
        if let Some(error) = &entry.error {
            println!("  error: {}", crate::strip_control_chars(error));
        }
    }
    Ok(())
}

/// Print an install/import/verify result in plain text or stable JSON.
pub fn print_asset_result(verb: &str, result: &AssetResult, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(result)?);
        return Ok(());
    }
    println!("{} asset pack {}", verb, result.id);
    println!(
        "path: {}",
        crate::strip_control_chars(&result.path.display().to_string())
    );
    println!("files: {}", result.file_count);
    println!("fingerprint: {}", result.fingerprint_sha256);
    println!(
        "run: maple-agent-market floating --pack-dir \"{}\"",
        crate::strip_control_chars(&result.path.display().to_string())
    );
    Ok(())
}

/// Copy a user-selected sprite pack into managed local storage.
///
/// Only `pack.toml` and files referenced by `[animations.*].frames` are copied.
/// The generated manifest deliberately marks the result `local-only`; importing
/// a file does not assert or grant redistribution rights.
pub fn import_local_pack(
    root: &Path,
    source_dir: &Path,
    id: &str,
    mode: InstallMode,
) -> Result<AssetResult> {
    validate_id(id)?;
    ensure_real_directory(source_dir, "source pack")?;
    let pack_toml_path = source_dir.join("pack.toml");
    ensure_real_file(&pack_toml_path, "pack.toml")?;
    let pack_toml = fs::read(&pack_toml_path)
        .with_context(|| format!("failed to read {}", pack_toml_path.display()))?;
    let referenced = referenced_pack_files(source_dir, &pack_toml)?;
    let pack = validated_source_pack(source_dir)?;
    let files = read_import_files(source_dir, pack_toml, referenced)?;
    let display_name = crate::strip_control_chars(&pack.name);
    let version = crate::strip_control_chars(&pack.version);

    install_files(InstallRequest {
        root,
        metadata: PackMetadata {
            id,
            display_name: &display_name,
            version: &version,
            distribution: "local-only",
            license: None,
            source: "user-supplied local import; redistribution rights not asserted",
        },
        files,
        mode,
    })
}

fn referenced_pack_files(source_dir: &Path, pack_toml: &[u8]) -> Result<BTreeSet<String>> {
    let text = std::str::from_utf8(pack_toml).context("pack.toml must be UTF-8")?;
    let parsed: toml::Value = toml::from_str(text).context("pack.toml is not valid TOML")?;
    let animations = parsed
        .get("animations")
        .and_then(toml::Value::as_table)
        .context("pack.toml must contain an [animations] table")?;
    let mut referenced = BTreeSet::new();
    for (animation, value) in animations {
        collect_animation_frames(source_dir, animation, value, &mut referenced)?;
    }
    Ok(referenced)
}

fn collect_animation_frames(
    source_dir: &Path,
    animation: &str,
    value: &toml::Value,
    referenced: &mut BTreeSet<String>,
) -> Result<()> {
    let frames = value
        .get("frames")
        .and_then(toml::Value::as_array)
        .with_context(|| format!("animation {animation:?} must contain a frames array"))?;
    for frame in frames {
        let raw = frame
            .as_str()
            .with_context(|| format!("animation {animation:?} has a non-string frame"))?;
        ensure_source_file(source_dir, &safe_relative_path(raw)?, raw)?;
        referenced.insert(raw.to_string());
    }
    Ok(())
}

fn validated_source_pack(source_dir: &Path) -> Result<pixtuoid_core::sprite::format::Pack> {
    let pack = load_pack(source_dir)
        .with_context(|| format!("failed to load source pack {}", source_dir.display()))?;
    let report = validate_pack_animations(&pack);
    if report.has_errors() {
        bail!(
            "source pack has incomplete supplied animations: insufficient_frames={:?}",
            report.insufficient_frames
        );
    }
    Ok(pack)
}

fn read_import_files(
    source_dir: &Path,
    pack_toml: Vec<u8>,
    referenced: BTreeSet<String>,
) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::with_capacity(1 + referenced.len());
    files.push(("pack.toml".to_string(), pack_toml));
    for portable in referenced {
        let relative = safe_relative_path(&portable)?;
        ensure_source_file(source_dir, &relative, &portable)?;
        let bytes = fs::read(source_dir.join(relative))?;
        files.push((portable, bytes));
    }
    Ok(files)
}

/// Verify an installed pack and optionally require a known team fingerprint.
pub fn verify_installed(
    root: &Path,
    id: &str,
    expected_fingerprint: Option<&str>,
) -> Result<AssetResult> {
    validate_id(id)?;
    let dir = root.join(id);
    ensure_real_directory(&dir, "managed pack")?;
    let manifest = read_manifest(&dir)?;
    validate_manifest_identity(&manifest, id, &dir)?;
    let fingerprint = verify_manifest_files(&dir, &manifest)?;
    verify_expected_fingerprint(&fingerprint, expected_fingerprint)?;
    Ok(AssetResult {
        id: id.to_string(),
        path: dir,
        fingerprint_sha256: fingerprint,
        file_count: manifest.files.len(),
    })
}

fn read_manifest(dir: &Path) -> Result<AssetManifest> {
    let manifest_path = dir.join(ASSET_MANIFEST_FILE);
    ensure_real_file(&manifest_path, ASSET_MANIFEST_FILE)?;
    let bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid {}", manifest_path.display()))
}

fn validate_manifest_identity(manifest: &AssetManifest, id: &str, dir: &Path) -> Result<()> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        bail!(
            "unsupported asset manifest schema {} in {}/{}",
            manifest.schema_version,
            dir.display(),
            ASSET_MANIFEST_FILE
        );
    }
    if manifest.id != id {
        bail!(
            "asset manifest id {:?} does not match directory id {:?}",
            manifest.id,
            id
        );
    }
    Ok(())
}

fn verify_manifest_files(dir: &Path, manifest: &AssetManifest) -> Result<String> {
    let mut declared = BTreeSet::new();
    let mut records = manifest.files.clone();
    records.sort_by(|a, b| a.path.cmp(&b.path));
    for record in &records {
        if !declared.insert(record.path.clone()) {
            bail!("duplicate asset manifest path {:?}", record.path);
        }
        verify_file_record(dir, record)?;
    }
    let actual_files = collect_regular_files(dir)?;
    if actual_files != declared {
        let missing: Vec<_> = declared.difference(&actual_files).cloned().collect();
        let extra: Vec<_> = actual_files.difference(&declared).cloned().collect();
        bail!("managed file inventory mismatch; missing={missing:?}, extra={extra:?}");
    }
    let fingerprint = inventory_fingerprint(&records);
    if fingerprint != manifest.fingerprint_sha256 {
        bail!(
            "asset manifest fingerprint mismatch: expected {}, got {}",
            manifest.fingerprint_sha256,
            fingerprint
        );
    }
    Ok(fingerprint)
}

fn verify_file_record(dir: &Path, record: &AssetFileRecord) -> Result<()> {
    let path = dir.join(safe_relative_path(&record.path)?);
    ensure_real_file(&path, &record.path)?;
    let bytes = fs::read(&path)
        .with_context(|| format!("failed to read managed file {}", path.display()))?;
    if bytes.len() as u64 != record.bytes {
        bail!(
            "{} byte length mismatch: expected {}, got {}",
            record.path,
            record.bytes,
            bytes.len()
        );
    }
    let actual = sha256_hex(&bytes);
    if actual != record.sha256 {
        bail!(
            "{} SHA-256 mismatch: expected {}, got {}",
            record.path,
            record.sha256,
            actual
        );
    }
    Ok(())
}

fn verify_expected_fingerprint(fingerprint: &str, expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        let expected = normalized_sha256(expected)?;
        if fingerprint != expected {
            bail!(
                "team fingerprint mismatch: expected {}, got {}",
                expected,
                fingerprint
            );
        }
    }
    Ok(())
}

fn install_files(mut request: InstallRequest<'_>) -> Result<AssetResult> {
    let root = request.root;
    let id = request.metadata.id;
    validate_id(id)?;
    ensure_managed_root(root)?;
    let destination = root.join(id);
    if destination.exists() {
        if request.mode == InstallMode::RefuseExisting {
            bail!(
                "managed asset pack {} already exists (use --force to replace it)",
                destination.display()
            );
        }
        ensure_owned_destination(&destination, id).with_context(|| {
            format!(
                "refusing to replace unmanaged or damaged directory {}",
                destination.display()
            )
        })?;
    }

    request.files.sort_by(|a, b| a.0.cmp(&b.0));
    let staging = StagingDir::new(root, id)?;
    let records = write_pack_files(&staging.path, &request.files)?;
    let fingerprint = inventory_fingerprint(&records);
    write_pack_manifest(&staging.path, &request.metadata, records, &fingerprint)?;
    verify_staged_directory(&staging.path, id)?;
    activate_staging(root, staging, &destination, id)?;
    Ok(AssetResult {
        id: id.to_string(),
        path: destination,
        fingerprint_sha256: fingerprint,
        file_count: request.files.len(),
    })
}

fn write_pack_files(dir: &Path, files: &[(String, Vec<u8>)]) -> Result<Vec<AssetFileRecord>> {
    let mut records = Vec::with_capacity(files.len());
    for (portable, bytes) in files {
        let path = dir.join(safe_relative_path(portable)?);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)
            .with_context(|| format!("failed to write managed asset {}", path.display()))?;
        records.push(AssetFileRecord {
            path: portable.clone(),
            bytes: bytes.len() as u64,
            sha256: sha256_hex(bytes),
        });
    }
    Ok(records)
}

fn write_pack_manifest(
    dir: &Path,
    metadata: &PackMetadata<'_>,
    files: Vec<AssetFileRecord>,
    fingerprint: &str,
) -> Result<()> {
    let manifest = AssetManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        id: metadata.id.to_string(),
        display_name: metadata.display_name.to_string(),
        version: metadata.version.to_string(),
        distribution: metadata.distribution.to_string(),
        license: metadata.license.map(str::to_string),
        source: metadata.source.to_string(),
        fingerprint_sha256: fingerprint.to_string(),
        files,
    };
    let mut bytes = serde_json::to_vec_pretty(&manifest)?;
    bytes.push(b'\n');
    fs::write(dir.join(ASSET_MANIFEST_FILE), bytes)?;
    Ok(())
}

fn activate_staging(root: &Path, staging: StagingDir, destination: &Path, id: &str) -> Result<()> {
    if destination.exists() {
        replace_existing_pack(root, staging, destination, id)
    } else {
        fs::rename(&staging.path, destination)
            .with_context(|| format!("failed to install managed pack {}", destination.display()))?;
        staging.commit();
        Ok(())
    }
}

fn replace_existing_pack(
    root: &Path,
    staging: StagingDir,
    destination: &Path,
    id: &str,
) -> Result<()> {
    let backup = replacement_backup_path(root, id);
    fs::rename(destination, &backup)
        .with_context(|| format!("failed to stage replacement of {}", destination.display()))?;
    if let Err(error) = fs::rename(&staging.path, destination) {
        let _ = fs::rename(&backup, destination);
        return Err(error)
            .with_context(|| format!("failed to install managed pack {}", destination.display()));
    }
    staging.commit();
    remove_replaced_backup(&backup);
    Ok(())
}

fn replacement_backup_path(root: &Path, id: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    root.join(format!(".backup-{id}-{nonce}"))
}

fn remove_replaced_backup(backup: &Path) {
    if let Err(error) = fs::remove_dir_all(backup) {
        tracing::warn!(
            path = %backup.display(),
            error = %error,
            "installed asset pack but could not remove its verified old backup"
        );
    }
}

fn ensure_owned_destination(destination: &Path, id: &str) -> Result<()> {
    ensure_real_directory(destination, "managed pack")?;
    let manifest = read_manifest(destination)?;
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION || manifest.id != id {
        bail!(
            "managed ownership marker does not match id {id:?} and schema {MANIFEST_SCHEMA_VERSION}"
        );
    }
    Ok(())
}

fn verify_staged_directory(dir: &Path, id: &str) -> Result<()> {
    let staging_name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .context("managed staging directory name is not UTF-8")?;
    let expected_prefix = format!(".staging-{id}-");
    if !staging_name.starts_with(&expected_prefix) {
        bail!("unexpected managed staging directory {}", dir.display());
    }
    let manifest = read_manifest(dir)?;
    validate_manifest_identity(&manifest, id, dir)?;
    verify_manifest_files(dir, &manifest)?;
    Ok(())
}

fn ensure_managed_root(root: &Path) -> Result<()> {
    if root.exists() {
        ensure_real_directory(root, "managed asset root")?;
    } else {
        fs::create_dir_all(root)
            .with_context(|| format!("failed to create asset root {}", root.display()))?;
    }
    Ok(())
}

fn ensure_real_directory(path: &Path, label: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("{label} {} does not exist", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("{label} {} must be a real directory", path.display());
    }
    Ok(())
}

fn ensure_real_file(path: &Path, label: &str) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("managed file {label:?} is missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("managed file {label:?} must be a real regular file");
    }
    Ok(())
}

fn ensure_source_file(root: &Path, relative: &Path, label: &str) -> Result<()> {
    let components: Vec<_> = relative.components().collect();
    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("source pack file {label:?} is missing"))?;
        if metadata.file_type().is_symlink() {
            bail!("source pack file {label:?} traverses a symlink");
        }
        let is_last = index + 1 == components.len();
        if (is_last && !metadata.is_file()) || (!is_last && !metadata.is_dir()) {
            bail!("source pack file {label:?} is not a regular in-pack file");
        }
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id.as_bytes()[id.len() - 1].is_ascii_alphanumeric();
    if !valid {
        bail!("invalid asset id {id:?}; use 1-64 lower-case letters, digits, or interior hyphens");
    }
    Ok(())
}

fn safe_relative_path(raw: &str) -> Result<PathBuf> {
    if raw.is_empty() || raw.contains(['\\', ':']) || raw.eq_ignore_ascii_case(ASSET_MANIFEST_FILE)
    {
        bail!("unsafe or reserved asset path {raw:?}");
    }
    let path = Path::new(raw);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("asset path must be a portable relative path: {raw:?}");
    }
    Ok(path.to_path_buf())
}

fn collect_regular_files(dir: &Path) -> Result<BTreeSet<String>> {
    fn walk(root: &Path, current: &Path, out: &mut BTreeSet<String>) -> Result<()> {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                bail!("managed asset tree contains a symlink: {}", path.display());
            }
            if metadata.is_dir() {
                walk(root, &path, out)?;
            } else if metadata.is_file() {
                let relative = path.strip_prefix(root)?;
                let portable = relative
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                if portable != ASSET_MANIFEST_FILE {
                    out.insert(portable);
                }
            } else {
                bail!(
                    "managed asset tree contains a non-regular file: {}",
                    path.display()
                );
            }
        }
        Ok(())
    }

    let mut files = BTreeSet::new();
    walk(dir, dir, &mut files)?;
    Ok(files)
}

fn inventory_fingerprint(records: &[AssetFileRecord]) -> String {
    let mut sorted = records.to_vec();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut hasher = Sha256::new();
    for record in &sorted {
        hasher.update(record.path.as_bytes());
        hasher.update([0]);
        hasher.update(record.bytes.to_le_bytes());
        hasher.update([0]);
        hasher.update(record.sha256.as_bytes());
        hasher.update(b"\n");
    }
    hex_lower(hasher.finalize())
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(Sha256::digest(bytes))
}

fn hex_lower(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn normalized_sha256(raw: &str) -> Result<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("expected fingerprint must be exactly 64 hexadecimal characters");
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_uses_the_standard_abc_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn ids_and_manifest_paths_cannot_escape_the_managed_root() {
        for id in ["", "../pack", "Pack", "-pack", "pack-", "pack_name"] {
            assert!(validate_id(id).is_err(), "id {id:?} must be rejected");
        }
        for path in [
            "../secret",
            "/absolute",
            "C:/absolute",
            "a\\b.sprite",
            "asset-manifest.json",
        ] {
            assert!(
                safe_relative_path(path).is_err(),
                "path {path:?} must be rejected"
            );
        }
    }
}
