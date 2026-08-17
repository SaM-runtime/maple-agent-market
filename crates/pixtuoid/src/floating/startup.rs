//! Per-user Windows startup registration for the floating Maple window.
//!
//! The registration is one owned `.cmd` file in the current user's Startup
//! folder. It replays the already-running executable, arguments and the small
//! allow-list of local runtime environment variables, so a private sprite pack
//! remains selected after sign-in. A marker prevents us from overwriting or
//! removing an unrelated file with the same name.

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

const STARTUP_FILE_NAME: &str = "Maple Agent Market.cmd";
const MANAGED_MARKER: &str = "Maple Agent Market managed autostart v1";
const REPLAY_ENV_KEYS: [&str; 9] = [
    "XDG_CONFIG_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
    "PIXTUOID_LOG",
    "PIXTUOID_CODEX_SESSION_INDEX",
    "PIXTUOID_CJK_FONT",
    "PIXTUOID_LABEL_SCALE",
    "PIXTUOID_FLOATING_SCALE",
    "PIXTUOID_SOCKET",
];

/// One exact, marker-owned startup file and the content it may create.
#[derive(Debug, Clone)]
pub(crate) struct StartupRegistration {
    path: PathBuf,
    script: String,
}

impl StartupRegistration {
    /// Discover the current user's Windows Startup folder and capture the
    /// executable invocation that successfully opened this process.
    pub(crate) fn discover() -> Result<Option<Self>> {
        #[cfg(not(windows))]
        {
            Ok(None)
        }
        #[cfg(windows)]
        {
            let appdata = std::env::var_os("APPDATA")
                .context("APPDATA is unavailable; Windows startup cannot be configured")?;
            let startup_dir = PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Startup");
            let executable = std::env::current_exe()
                .context("resolving the current Maple Agent Market executable")?;
            let args = std::env::args_os().skip(1).collect();
            let environment = REPLAY_ENV_KEYS
                .into_iter()
                .filter_map(|key| std::env::var_os(key).map(|value| (OsString::from(key), value)))
                .collect();
            Self::from_parts(&startup_dir, executable, args, environment).map(Some)
        }
    }

    fn from_parts(
        startup_dir: &Path,
        executable: PathBuf,
        args: Vec<OsString>,
        environment: Vec<(OsString, OsString)>,
    ) -> Result<Self> {
        let path = startup_dir.join(STARTUP_FILE_NAME);
        let script = render_script(&executable, &args, &environment)?;
        Ok(Self { path, script })
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    fn script_body(&self) -> &str {
        &self.script
    }

    /// True only for the file this app owns. A same-name foreign file is not
    /// reported as enabled and will be protected by [`Self::toggle`].
    pub(crate) fn is_enabled(&self) -> bool {
        managed_file(&self.path).unwrap_or(false)
    }

    /// Toggle startup and return the new enabled state.
    pub(crate) fn toggle(&self) -> Result<bool> {
        if self.path.exists() {
            if !managed_file(&self.path)? {
                bail!(
                    "refusing to replace an unmanaged startup file: {}",
                    self.path.display()
                );
            }
            fs::remove_file(&self.path)
                .with_context(|| format!("removing {}", self.path.display()))?;
            return Ok(false);
        }

        let parent = self
            .path
            .parent()
            .context("startup file has no parent directory")?;
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        let temp = parent.join(format!(".{STARTUP_FILE_NAME}.{}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .with_context(|| format!("creating {}", temp.display()))?;
        let write_result = (|| -> Result<()> {
            file.write_all(self.script.as_bytes())
                .context("writing the managed startup command")?;
            file.sync_all()
                .context("syncing the managed startup command")?;
            drop(file);
            fs::rename(&temp, &self.path).with_context(|| {
                format!(
                    "installing startup command {} -> {}",
                    temp.display(),
                    self.path.display()
                )
            })?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        write_result?;
        Ok(true)
    }
}

fn managed_file(path: &Path) -> Result<bool> {
    let body = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(body
        .lines()
        .take(4)
        .any(|line| line.contains(MANAGED_MARKER)))
}

fn render_script(
    executable: &Path,
    args: &[OsString],
    environment: &[(OsString, OsString)],
) -> Result<String> {
    let executable_quoted = quoted_batch_value(executable.as_os_str())?;
    let mut script =
        format!("@echo off\r\nrem {MANAGED_MARKER}\r\nsetlocal DisableDelayedExpansion\r\n");
    for (key, value) in environment {
        let key = key.to_string_lossy();
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            bail!("unsafe startup environment key: {key}");
        }
        script.push_str(&format!(
            "set \"{key}={}\"\r\n",
            batch_value(value.as_os_str())?
        ));
    }
    let workdir = executable
        .parent()
        .and_then(Path::parent)
        .or_else(|| executable.parent())
        .unwrap_or_else(|| Path::new("."));
    script.push_str(&format!(
        "cd /d {}\r\n",
        quoted_batch_value(workdir.as_os_str())?
    ));
    script.push_str(&format!("start \"\" {executable_quoted}"));
    for arg in args {
        script.push(' ');
        script.push_str(&quoted_batch_value(arg)?);
    }
    script.push_str("\r\nendlocal\r\nexit /b 0\r\n");
    Ok(script)
}

fn quoted_batch_value(value: &OsStr) -> Result<String> {
    Ok(format!("\"{}\"", batch_value(value)?))
}

fn batch_value(value: &OsStr) -> Result<String> {
    let value = value.to_string_lossy();
    if value.contains(['\r', '\n', '"']) {
        bail!("startup values may not contain quotes or newlines");
    }
    Ok(value.replace('^', "^^").replace('%', "%%"))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    #[test]
    fn managed_startup_script_replays_the_current_executable_args_and_local_environment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable = temp
            .path()
            .join("Maple Agent Market")
            .join("bin")
            .join("maple-agent-market.exe");
        let registration = StartupRegistration::from_parts(
            temp.path(),
            executable.clone(),
            vec![
                OsString::from("floating"),
                OsString::from("--pack-dir"),
                OsString::from("C:\\Maple Agent Market\\private-assets\\skins\\active-pack"),
            ],
            vec![
                (
                    "XDG_CONFIG_HOME".into(),
                    "C:\\Maple Agent Market\\config".into(),
                ),
                ("PIXTUOID_LABEL_SCALE".into(), "1.25".into()),
            ],
        )
        .expect("valid registration");

        let script = registration.script_body();
        assert!(script.contains("Maple Agent Market managed autostart v1"));
        assert!(script.contains("set \"XDG_CONFIG_HOME=C:\\Maple Agent Market\\config\""));
        assert!(script.contains(&format!("\"{}\"", executable.display())));
        assert!(script.contains(
            "\"--pack-dir\" \"C:\\Maple Agent Market\\private-assets\\skins\\active-pack\""
        ));
    }

    #[test]
    fn toggle_creates_and_removes_only_the_managed_startup_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registration = StartupRegistration::from_parts(
            temp.path(),
            temp.path().join("maple-agent-market.exe"),
            vec![OsString::from("floating")],
            Vec::new(),
        )
        .expect("valid registration");

        assert!(!registration.is_enabled());
        assert!(registration.toggle().expect("enable"));
        assert!(registration.is_enabled());
        assert!(!registration.toggle().expect("disable"));
        assert!(!registration.path().exists());

        std::fs::write(
            registration.path(),
            "@echo off\r\nrem someone else's file\r\n",
        )
        .expect("foreign file");
        assert!(
            registration.toggle().is_err(),
            "foreign startup file is never overwritten"
        );
    }
}
