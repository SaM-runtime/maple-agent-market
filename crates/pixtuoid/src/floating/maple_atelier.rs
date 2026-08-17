//! Safe, install-local launch boundary for the Maple Atelier importer.
//!
//! The native window never receives a pasted URL: the PowerShell helper reads
//! the clipboard itself.  This module only resolves the fixed helper under the
//! installed product root and starts it without a shell command string.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::Value;
use winit::event_loop::EventLoopProxy;

use super::window::FloatingEvent;

const IMPORT_HELPER_FILE: &str = "Import-MapleAtelierCharacter.ps1";

/// The outcome of asking the local helper to begin an import. `Started` only
/// means the helper process was created; parsing/download completion is owned
/// by the helper and deliberately is not inferred here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CatalogOperationLaunch {
    Started,
    HelperUnavailable,
    CatalogNotActive,
    SpawnFailed(String),
}

/// Terminal result delivered back to the native window after the hidden
/// helper exits.  Keeping this protocol typed lets the UI distinguish a user
/// cancellation from an operational failure and reload the rebuilt catalog
/// without polling files or asking the user to restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CatalogOperationCompletion {
    Imported {
        title: String,
        added: bool,
        catalog_pack: PathBuf,
    },
    Removed {
        title: String,
        removed_appearance_index: usize,
        catalog_pack: PathBuf,
    },
    Cancelled,
    Failed {
        message: String,
    },
}

fn candidate_install_roots(executable: &Path) -> io::Result<Vec<PathBuf>> {
    let exe_dir = executable.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Maple Agent Market executable has no parent directory",
        )
    })?;
    let exe_dir = exe_dir.canonicalize()?;
    let directory_name = exe_dir.file_name().and_then(|name| name.to_str());
    if directory_name
        .is_some_and(|name| name.eq_ignore_ascii_case("bin") || name.eq_ignore_ascii_case("app"))
    {
        return exe_dir
            .parent()
            .map(|root| vec![root.to_path_buf()])
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "install root is missing"));
    }

    let is_cargo_profile = directory_name.is_some_and(|name| {
        name.eq_ignore_ascii_case("debug") || name.eq_ignore_ascii_case("release")
    });
    let target_dir = exe_dir.parent();
    let is_cargo_target = target_dir
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().starts_with("target"));
    if is_cargo_profile && is_cargo_target {
        return target_dir
            .and_then(Path::parent)
            .map(|root| vec![root.to_path_buf()])
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "source root is missing"));
    }

    Ok(vec![exe_dir])
}

/// Resolve the fixed helper in an installed, flat-bundle, or Cargo layout.
///
/// Canonical paths make the boundary explicit: a missing helper, a redirected
/// `tools` directory, or a helper symlink escaping the install root is rejected
/// before the process boundary is reached.
pub(crate) fn resolve_import_helper(executable: &Path) -> io::Result<PathBuf> {
    for install_root in candidate_install_roots(executable)? {
        let tools_candidate = install_root.join("tools");
        if !tools_candidate.exists() {
            continue;
        }
        let tools = tools_candidate.canonicalize()?;
        if !tools.starts_with(&install_root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Maple Atelier tools directory escapes the install root",
            ));
        }
        let helper_candidate = tools.join(IMPORT_HELPER_FILE);
        if !helper_candidate.exists() {
            continue;
        }
        let helper = helper_candidate.canonicalize()?;
        if !helper.starts_with(&tools)
            || helper
                .file_name()
                .is_none_or(|name| name != IMPORT_HELPER_FILE)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Maple Atelier import helper is not the expected install-local file",
            ));
        }
        if !helper.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Maple Atelier import helper is not a file",
            ));
        }
        return Ok(helper);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Maple Atelier import helper is unavailable in every supported layout",
    ))
}

/// Keep canonical/verbatim paths for containment checks, but do not pass a
/// `\\?\` path to Windows PowerShell 5.1's `-File` argument. PowerShell can
/// execute the file yet leave `$PSScriptRoot` unusable for that spelling,
/// which makes the install-local helper fail before it can read the clipboard.
fn powershell_file_argument(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        const VERBATIM_PREFIX: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
        const VERBATIM_UNC_PREFIX: &[u16] = &[
            b'\\' as u16,
            b'\\' as u16,
            b'?' as u16,
            b'\\' as u16,
            b'U' as u16,
            b'N' as u16,
            b'C' as u16,
            b'\\' as u16,
        ];

        let wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if let Some(remainder) = wide.strip_prefix(VERBATIM_UNC_PREFIX) {
            let mut normalized = Vec::with_capacity(remainder.len() + 2);
            normalized.extend_from_slice(&[b'\\' as u16, b'\\' as u16]);
            normalized.extend_from_slice(remainder);
            return PathBuf::from(OsString::from_wide(&normalized));
        }
        if let Some(remainder) = wide.strip_prefix(VERBATIM_PREFIX) {
            return PathBuf::from(OsString::from_wide(remainder));
        }
    }

    path.to_path_buf()
}

/// Resolve the one catalog directory the no-argument UI helper is allowed to
/// rebuild.  The helper's JSON is intentionally not trusted as a filesystem
/// path; the native side derives and canonicalizes the expected install-local
/// destination independently.
fn resolve_catalog_pack(executable: &Path) -> io::Result<PathBuf> {
    for install_root in candidate_install_roots(executable)? {
        let candidate = install_root
            .join("private-assets")
            .join("skins")
            .join("catalog-pack");
        if !candidate.exists() {
            continue;
        }
        let catalog_pack = candidate.canonicalize()?;
        if !catalog_pack.starts_with(&install_root) || !catalog_pack.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Maple Atelier catalog pack escapes the install root",
            ));
        }
        return Ok(catalog_pack);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Maple Atelier catalog pack is unavailable in every supported layout",
    ))
}

/// Verify that a hot import would rebuild the pack currently shown by the
/// native window.  This prevents a session launched with a team/custom
/// `--pack-dir` from silently switching to the install-local catalog after the
/// helper finishes.
fn resolve_active_catalog_pack(executable: &Path, active_pack: &Path) -> io::Result<PathBuf> {
    let catalog_pack = resolve_catalog_pack(executable)?;
    let active_pack = active_pack.canonicalize()?;
    if active_pack != catalog_pack {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Maple Atelier hot import requires the install-local catalog pack to be active",
        ));
    }
    Ok(catalog_pack)
}

fn last_json_value(bytes: &[u8]) -> Option<Value> {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
}

fn compact_helper_message(message: &str) -> String {
    let compact = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let mut bounded = chars.by_ref().take(120).collect::<String>();
    if chars.next().is_some() {
        bounded.push('…');
    }
    bounded
}

fn completion_from_streams(
    exit_code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
    catalog_pack: PathBuf,
) -> CatalogOperationCompletion {
    if exit_code == Some(0) {
        if let Some(value) = last_json_value(stdout) {
            let ok = value.get("ok").and_then(Value::as_bool) == Some(true);
            let title = value
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty());
            if ok {
                if let Some(title) = title {
                    if value.get("mode").and_then(Value::as_str) == Some("remove")
                        && value.get("removed").and_then(Value::as_bool) == Some(true)
                    {
                        if let Some(removed_appearance_index) = value
                            .get("removedAppearanceIndex")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                        {
                            return CatalogOperationCompletion::Removed {
                                title: title.to_owned(),
                                removed_appearance_index,
                                catalog_pack,
                            };
                        }
                    }
                    return CatalogOperationCompletion::Imported {
                        title: title.to_owned(),
                        added: value.get("added").and_then(Value::as_bool).unwrap_or(false),
                        catalog_pack,
                    };
                }
            }
        }
        return CatalogOperationCompletion::Failed {
            message: "角色管理工具已結束，但沒有回傳可用資料。".to_owned(),
        };
    }

    if exit_code == Some(3) {
        return CatalogOperationCompletion::Cancelled;
    }

    let message = last_json_value(stderr)
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(compact_helper_message)
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| match exit_code {
            Some(code) => format!("角色管理工具執行失敗（代碼 {code}）。"),
            None => "角色管理工具意外中止。".to_owned(),
        });
    CatalogOperationCompletion::Failed { message }
}

fn wait_for_helper(
    child: Child,
    catalog_pack: PathBuf,
    event_proxy: EventLoopProxy<FloatingEvent>,
) {
    let completion = match child.wait_with_output() {
        Ok(output) => completion_from_streams(
            output.status.code(),
            &output.stdout,
            &output.stderr,
            catalog_pack,
        ),
        Err(error) => CatalogOperationCompletion::Failed {
            message: format!("無法等待角色管理工具完成：{error}"),
        },
    };
    let _ = event_proxy.send_event(FloatingEvent::CharacterCatalogMutationFinished(completion));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogOperation {
    ImportFromClipboard,
    RemoveAppearance(usize),
}

fn catalog_operation_command(helper: &Path, operation: CatalogOperation) -> Command {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-WindowStyle",
        "Hidden",
        "-File",
    ]);
    command.arg(powershell_file_argument(helper));
    if let CatalogOperation::RemoveAppearance(appearance_index) = operation {
        command.arg("-RemoveAppearanceIndex");
        command.arg(appearance_index.to_string());
    }
    // The C panel owns terminal feedback; the naming form remains available
    // for a raw render URL without adding a second completion dialog.
    command.arg("-NoCompletionDialog");
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

fn spawn_catalog_worker(
    mut command: Command,
    catalog_pack: PathBuf,
    event_proxy: EventLoopProxy<FloatingEvent>,
) -> CatalogOperationLaunch {
    let (child_tx, child_rx) = std::sync::mpsc::sync_channel::<Child>(1);
    let worker = std::thread::Builder::new()
        .name("maple-atelier-catalog".to_owned())
        .spawn(move || {
            if let Ok(child) = child_rx.recv() {
                wait_for_helper(child, catalog_pack, event_proxy);
            }
        });
    if let Err(error) = worker {
        tracing::warn!("maple-agent-market: could not create catalog worker: {error}");
        return CatalogOperationLaunch::SpawnFailed(error.to_string());
    }
    match command.spawn() {
        Ok(child) => match child_tx.send(child) {
            Ok(()) => CatalogOperationLaunch::Started,
            Err(error) => {
                let mut child = error.0;
                let _ = child.kill();
                let message = "角色清單背景工作無法接收 helper process".to_owned();
                tracing::warn!("maple-agent-market: {message}");
                CatalogOperationLaunch::SpawnFailed(message)
            }
        },
        Err(error) => {
            tracing::warn!("maple-agent-market: catalog helper could not start: {error}");
            CatalogOperationLaunch::SpawnFailed(error.to_string())
        }
    }
}

fn start_catalog_operation_from(
    executable: &Path,
    active_pack: Option<&Path>,
    event_proxy: EventLoopProxy<FloatingEvent>,
    operation: CatalogOperation,
) -> CatalogOperationLaunch {
    let Some(active_pack) = active_pack else {
        tracing::warn!(
            "maple-agent-market: Maple Atelier hot import needs an explicit active catalog pack"
        );
        return CatalogOperationLaunch::CatalogNotActive;
    };
    let catalog_pack = match resolve_active_catalog_pack(executable, active_pack) {
        Ok(catalog_pack) => catalog_pack,
        Err(error) => {
            tracing::warn!(
                "maple-agent-market: Maple Atelier catalog is not the active pack: {error}"
            );
            return CatalogOperationLaunch::CatalogNotActive;
        }
    };
    let helper = match resolve_import_helper(executable) {
        Ok(helper) => helper,
        Err(error) => {
            tracing::warn!("maple-agent-market: Maple Atelier import helper unavailable: {error}");
            return CatalogOperationLaunch::HelperUnavailable;
        }
    };
    spawn_catalog_worker(
        catalog_operation_command(&helper, operation),
        catalog_pack,
        event_proxy,
    )
}

/// Start the PowerShell 5.1 helper, which reads its own clipboard input, then
/// deliver its terminal result to the winit loop without blocking rendering.
pub(crate) fn start_import_from(
    executable: &Path,
    active_pack: Option<&Path>,
    event_proxy: EventLoopProxy<FloatingEvent>,
) -> CatalogOperationLaunch {
    start_catalog_operation_from(
        executable,
        active_pack,
        event_proxy,
        CatalogOperation::ImportFromClipboard,
    )
}

/// Start one confirmed install-local catalog removal. The UI only supplies an
/// appearance index; the helper independently rejects the eight built-ins and
/// resolves the exact `user-*` source under the active catalog root.
pub(crate) fn start_remove_from(
    executable: &Path,
    active_pack: Option<&Path>,
    appearance_index: usize,
    event_proxy: EventLoopProxy<FloatingEvent>,
) -> CatalogOperationLaunch {
    start_catalog_operation_from(
        executable,
        active_pack,
        event_proxy,
        CatalogOperation::RemoveAppearance(appearance_index),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolves_the_import_helper_inside_the_install_root_tools_folder() {
        let temp = tempfile::tempdir().expect("temporary install root");
        let app_dir = temp.path().join("app");
        let tools_dir = temp.path().join("tools");
        fs::create_dir_all(&app_dir).expect("app directory");
        fs::create_dir_all(&tools_dir).expect("tools directory");
        let executable = app_dir.join("MapleAgentMarket.exe");
        let helper = tools_dir.join("Import-MapleAtelierCharacter.ps1");
        fs::write(&executable, []).expect("placeholder executable");
        fs::write(&helper, "# helper").expect("placeholder helper");

        assert_eq!(
            resolve_import_helper(&executable).expect("safe helper path"),
            helper.canonicalize().expect("canonical helper")
        );
    }

    #[test]
    fn resolves_the_import_helper_beside_a_flat_public_bundle() {
        let temp = tempfile::tempdir().expect("temporary flat bundle");
        let tools_dir = temp.path().join("tools");
        fs::create_dir_all(&tools_dir).expect("tools directory");
        let executable = temp.path().join("maple-agent-market.exe");
        let helper = tools_dir.join(IMPORT_HELPER_FILE);
        fs::write(&executable, []).expect("placeholder executable");
        fs::write(&helper, "# helper").expect("placeholder helper");

        assert_eq!(
            resolve_import_helper(&executable).expect("flat-bundle helper path"),
            helper.canonicalize().expect("canonical helper")
        );
    }

    #[test]
    fn flat_bundle_never_borrows_a_parent_directories_helper() {
        let temp = tempfile::tempdir().expect("temporary parent directory");
        let bundle = temp.path().join("bundle");
        let parent_tools = temp.path().join("tools");
        fs::create_dir_all(&bundle).expect("bundle directory");
        fs::create_dir_all(&parent_tools).expect("parent tools directory");
        let executable = bundle.join("maple-agent-market.exe");
        let parent_helper = parent_tools.join(IMPORT_HELPER_FILE);
        fs::write(&executable, []).expect("placeholder executable");
        fs::write(&parent_helper, "# unrelated helper").expect("parent helper");

        assert_eq!(
            resolve_import_helper(&executable)
                .expect_err("flat bundles may only use their own tools directory")
                .kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn resolves_the_import_helper_from_a_cargo_target_directory() {
        let temp = tempfile::tempdir().expect("temporary source checkout");
        let app_dir = temp.path().join("target").join("debug");
        let tools_dir = temp.path().join("tools");
        fs::create_dir_all(&app_dir).expect("debug directory");
        fs::create_dir_all(&tools_dir).expect("tools directory");
        let executable = app_dir.join("maple-agent-market.exe");
        let helper = tools_dir.join(IMPORT_HELPER_FILE);
        fs::write(&executable, []).expect("placeholder executable");
        fs::write(&helper, "# helper").expect("placeholder helper");

        assert_eq!(
            resolve_import_helper(&executable).expect("source-checkout helper path"),
            helper.canonicalize().expect("canonical helper")
        );
    }

    #[cfg(windows)]
    #[test]
    fn powershell_receives_a_win32_path_instead_of_a_verbatim_disk_path() {
        let canonical =
            PathBuf::from(r"\\?\C:\Maple Agent Market\tools\Import-MapleAtelierCharacter.ps1");

        assert_eq!(
            powershell_file_argument(&canonical),
            PathBuf::from(r"C:\Maple Agent Market\tools\Import-MapleAtelierCharacter.ps1")
        );
    }

    #[cfg(windows)]
    #[test]
    fn powershell_receives_a_win32_unc_path_instead_of_a_verbatim_unc_path() {
        let canonical = PathBuf::from(
            r"\\?\UNC\server\share\Maple Agent Market\Import-MapleAtelierCharacter.ps1",
        );

        assert_eq!(
            powershell_file_argument(&canonical),
            PathBuf::from(r"\\server\share\Maple Agent Market\Import-MapleAtelierCharacter.ps1")
        );
    }

    #[test]
    fn hot_import_requires_the_install_local_catalog_to_be_the_active_pack() {
        let temp = tempfile::tempdir().expect("temporary install root");
        let app_dir = temp.path().join("bin");
        let catalog_pack = temp
            .path()
            .join("private-assets")
            .join("skins")
            .join("catalog-pack");
        let team_pack = temp.path().join("team-pack");
        fs::create_dir_all(&app_dir).expect("app directory");
        fs::create_dir_all(&catalog_pack).expect("catalog pack");
        fs::create_dir_all(&team_pack).expect("team pack");
        let executable = app_dir.join("maple-agent-market.exe");
        fs::write(&executable, []).expect("placeholder executable");

        assert_eq!(
            resolve_active_catalog_pack(&executable, &catalog_pack)
                .expect("active catalog is accepted"),
            catalog_pack.canonicalize().expect("canonical catalog")
        );
        let error = resolve_active_catalog_pack(&executable, &team_pack)
            .expect_err("a different active pack must not be replaced");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn flat_bundle_hot_import_resolves_its_own_catalog() {
        let temp = tempfile::tempdir().expect("temporary flat bundle");
        let catalog_pack = temp
            .path()
            .join("private-assets")
            .join("skins")
            .join("catalog-pack");
        fs::create_dir_all(&catalog_pack).expect("catalog pack");
        let executable = temp.path().join("maple-agent-market.exe");
        fs::write(&executable, []).expect("placeholder executable");

        assert_eq!(
            resolve_active_catalog_pack(&executable, &catalog_pack)
                .expect("flat-bundle active catalog is accepted"),
            catalog_pack.canonicalize().expect("canonical catalog")
        );
    }

    #[test]
    fn successful_helper_output_requests_an_immediate_catalog_reload() {
        let catalog_pack =
            PathBuf::from(r"C:\Maple Agent Market\private-assets\skins\catalog-pack");
        let completion = completion_from_streams(
            Some(0),
            r#"{"ok":true,"mode":"import","added":true,"title":"USER URL 回歸驗證"}"#.as_bytes(),
            b"",
            catalog_pack.clone(),
        );

        assert_eq!(
            completion,
            CatalogOperationCompletion::Imported {
                title: "USER URL 回歸驗證".to_owned(),
                added: true,
                catalog_pack,
            }
        );
    }

    #[test]
    fn successful_removal_output_requests_an_immediate_catalog_reload() {
        let catalog_pack =
            PathBuf::from(r"C:\Maple Agent Market\private-assets\skins\catalog-pack");
        let completion = completion_from_streams(
            Some(0),
            r#"{"ok":true,"mode":"remove","removed":true,"removedAppearanceIndex":12,"title":"舊造型"}"#.as_bytes(),
            b"",
            catalog_pack.clone(),
        );

        assert_eq!(
            completion,
            CatalogOperationCompletion::Removed {
                title: "舊造型".to_owned(),
                removed_appearance_index: 12,
                catalog_pack,
            }
        );
    }

    #[test]
    fn failed_helper_output_surfaces_its_traditional_chinese_reason() {
        let completion = completion_from_streams(
            Some(5),
            b"",
            r#"{"ok":false,"error":{"code":"SOURCE_UNAVAILABLE","message":"無法下載角色姿勢。"}}"#
                .as_bytes(),
            PathBuf::from("catalog-pack"),
        );

        assert_eq!(
            completion,
            CatalogOperationCompletion::Failed {
                message: "無法下載角色姿勢。".to_owned(),
            }
        );
    }

    #[test]
    fn cancelled_name_prompt_is_not_reported_as_an_import_failure() {
        assert_eq!(
            completion_from_streams(
                Some(3),
                b"",
                r#"{"ok":false,"error":{"code":"IMPORT_CANCELLED","message":"已取消。"}}"#
                    .as_bytes(),
                PathBuf::from("catalog-pack"),
            ),
            CatalogOperationCompletion::Cancelled
        );
    }
}
