use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "maple-agent-market",
    version,
    about = "Maple-inspired agent orchestration market, based on Pixtuoid"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,

    /// Log verbosity. The floating window logs warn+ to a file
    /// (~/.cache/pixtuoid/log, or $PIXTUOID_LOG /
    /// $XDG_STATE_HOME/pixtuoid/log); debug/trace raise the file's
    /// verbosity. Foreground CLI commands log to stderr at this level.
    /// ($RUST_LOG remains the escape hatch for full directive syntax.)
    #[arg(long, global = true, value_enum, default_value = "info")]
    pub log_level: LogLevel,
}

/// Source inputs for the floating window's source → reducer → renderer pipeline.
#[derive(Debug, clap::Args)]
pub struct SourceArgs {
    /// Empty/whitespace is rejected: an explicit override deserves a loud
    /// answer, and bind("") would strand a relative `.lock` in the CWD.
    #[arg(long, value_parser = parse_nonempty_path, value_hint = clap::ValueHint::FilePath)]
    pub socket: Option<PathBuf>,
    #[arg(long, value_hint = clap::ValueHint::DirPath)]
    pub projects_root: Option<PathBuf>,
    /// Override the Codex sessions root (default ~/.codex/sessions).
    /// Point at a temp dir to replay fixtures into an isolated window.
    #[arg(long, value_hint = clap::ValueHint::DirPath)]
    pub codex_sessions_root: Option<PathBuf>,
    #[arg(long, value_hint = clap::ValueHint::DirPath)]
    pub pack_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Open the live Maple Agent Market desktop window (the default command).
    Floating {
        #[command(flatten)]
        source: SourceArgs,
    },
    /// Import, list, and verify content-addressed local sprite packs.
    Assets {
        /// Managed pack root. Defaults to the application's config directory.
        #[arg(long, global = true, value_hint = clap::ValueHint::DirPath)]
        root: Option<PathBuf>,
        #[command(subcommand)]
        action: AssetsAction,
    },
    /// Validate a custom sprite pack directory.
    ValidatePack {
        /// Path to the pack directory (must contain pack.toml).
        #[arg(value_hint = clap::ValueHint::DirPath)]
        pack_dir: PathBuf,
    },
    /// Diagnose source health: connection, hooks, and decode drift recorded in
    /// the log. Read-only.
    Doctor,
    /// List agent sources + their connection state, or apply a set. The
    /// scriptable source-management surface for the floating window.
    Sources {
        #[command(subcommand)]
        action: Option<SourcesAction>,
        /// Emit machine-readable JSON instead of a table. `global` so it's
        /// honored both before and after `set` (`sources --json set …` ==
        /// `sources set … --json`) — the natural automation form.
        #[arg(long, global = true)]
        json: bool,
    },
    /// Connect one or more sources: install their hooks + persist the choice.
    /// A running visualizer reflects it on its next launch.
    Connect {
        /// Source ids, e.g. `claude-code codex`.
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Disconnect one or more sources: remove their hooks + persist the choice.
    Disconnect {
        /// Source ids, e.g. `codex cursor`.
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Detect installed agent CLIs and optionally connect them. Without `--yes`
    /// this is a dry run; writing another tool's hook config remains opt-in.
    Setup {
        /// Apply: actually connect the detected CLIs (install their hooks). Without
        /// it, `setup` only previews the detected set.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum SourcesAction {
    /// Make the connected set EXACTLY these ids (declarative — everything else
    /// disconnects). `--json` is
    /// the global flag on `sources` (works before or after `set`).
    Set {
        #[arg(required = true)]
        ids: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AssetsAction {
    /// List locally imported packs.
    List {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Import a local sprite pack without publishing or uploading it.
    Import {
        /// Directory containing pack.toml and the referenced sprite files.
        #[arg(value_hint = clap::ValueHint::DirPath)]
        source: PathBuf,
        /// Stable local id used by list and verify.
        #[arg(long)]
        id: String,
        /// Replace an existing managed installation of the same id.
        #[arg(long, default_value_t = false)]
        force: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Verify every managed file and print the pack fingerprint.
    Verify {
        /// Installed pack id.
        id: String,
        /// Fail unless the computed pack fingerprint equals this SHA-256.
        #[arg(long = "expect")]
        expected: Option<String>,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

/// `--log-level` values. A typed enum (not a free `String`) so a typo like
/// `dbug` is a hard clap parse error instead of silently parsing as an
/// `EnvFilter` TARGET directive (`dbug=trace`) that filters everything off —
/// the #157 silent-diagnostics class. `$RUST_LOG` stays the escape hatch for
/// full tracing directive syntax (targets, per-module levels).
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// The tracing level token — exactly the strings the old free-form
    /// `--log-level` accepted, so every `EnvFilter` built from it is
    /// unchanged; the enum only moved typo rejection to the clap seam.
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

impl Cli {
    pub fn cmd_or_default(self) -> (LogLevel, Cmd) {
        let level = self.log_level;
        let cmd = self.cmd.unwrap_or(Cmd::Floating {
            source: SourceArgs {
                socket: None,
                projects_root: None,
                codex_sessions_root: None,
                pack_dir: None,
            },
        });
        (level, cmd)
    }
}

fn parse_nonempty_path(s: &str) -> Result<PathBuf, String> {
    if s.trim().is_empty() {
        Err("must not be empty — pass a path, or drop the flag for the default".into())
    } else {
        Ok(PathBuf::from(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_command_uses_the_side_project_brand() {
        use clap::CommandFactory;
        let command = Cli::command();
        assert_eq!(command.get_name(), "maple-agent-market");
        assert_eq!(
            command.get_about().map(ToString::to_string).as_deref(),
            Some("Maple-inspired agent orchestration market, based on Pixtuoid")
        );
    }

    #[test]
    fn empty_socket_is_a_hard_parse_error() {
        // An explicit override deserves a loud answer: bind("") would die
        // late with an opaque error and strand a relative `.lock` in the CWD.
        for v in ["", "   "] {
            let err =
                Cli::try_parse_from(["maple-agent-market", "floating", "--socket", v]).unwrap_err();
            assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
        }
    }

    #[test]
    fn log_level_typo_is_a_hard_parse_error() {
        // A free String silently parsed a typo like `dbug` as an EnvFilter
        // TARGET directive (everything filtered off — the #157 class); the
        // ValueEnum makes it a loud clap error at the seam.
        let err = Cli::try_parse_from(["pixtuoid", "--log-level", "dbug"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn log_level_valid_values_map_to_the_same_filter_tokens_as_before() {
        // The enum's as_str must be exactly the strings the old free-form
        // --log-level accepted, and each must build a valid EnvFilter — the
        // typed seam may not change any accepted filter.
        for (raw, lvl) in [
            ("error", LogLevel::Error),
            ("warn", LogLevel::Warn),
            ("info", LogLevel::Info),
            ("debug", LogLevel::Debug),
            ("trace", LogLevel::Trace),
        ] {
            let cli = Cli::try_parse_from(["pixtuoid", "--log-level", raw]).unwrap();
            assert_eq!(cli.log_level, lvl);
            assert_eq!(lvl.as_str(), raw, "filter token must be unchanged");
            assert!(
                tracing_subscriber::EnvFilter::try_new(lvl.as_str()).is_ok(),
                "{raw} must parse as an EnvFilter level"
            );
        }
    }

    #[test]
    fn cmd_or_default_returns_floating_when_no_subcommand() {
        let cli = Cli {
            cmd: None,
            log_level: LogLevel::Info,
        };
        let (level, cmd) = cli.cmd_or_default();
        assert_eq!(level, LogLevel::Info);
        assert!(matches!(cmd, Cmd::Floating { .. }));
    }

    #[test]
    fn cmd_or_default_preserves_explicit_subcommand() {
        let cli = Cli {
            cmd: Some(Cmd::ValidatePack {
                pack_dir: PathBuf::from("/some/pack"),
            }),
            log_level: LogLevel::Debug,
        };
        let (level, cmd) = cli.cmd_or_default();
        assert_eq!(level, LogLevel::Debug);
        assert!(matches!(cmd, Cmd::ValidatePack { .. }));
    }

    #[test]
    fn floating_subcommand_parses_with_shared_run_flags() {
        let cli =
            Cli::try_parse_from(["pixtuoid", "floating", "--projects-root", "/tmp/p"]).unwrap();
        assert!(matches!(
            cli.cmd,
            Some(Cmd::Floating {
                source: SourceArgs {
                    projects_root: Some(_),
                    ..
                },
            })
        ));
    }

    #[test]
    fn floating_subcommand_rejects_empty_socket() {
        // The shared `parse_nonempty_path` guard applies to floating's --socket too.
        assert!(Cli::try_parse_from(["pixtuoid", "floating", "--socket", "  "]).is_err());
    }

    #[test]
    fn sources_list_parses_with_json() {
        let cli = Cli::try_parse_from(["pixtuoid", "sources", "--json"]).unwrap();
        assert!(matches!(
            cli.cmd,
            Some(Cmd::Sources {
                action: None,
                json: true
            })
        ));
    }

    #[test]
    fn sources_set_parses_its_ids() {
        let cli = Cli::try_parse_from(["pixtuoid", "sources", "set", "codex", "cursor"]).unwrap();
        match cli.cmd {
            Some(Cmd::Sources {
                action: Some(SourcesAction::Set { ids }),
                ..
            }) => assert_eq!(ids, vec!["codex".to_string(), "cursor".to_string()]),
            other => panic!("expected sources set, got {other:?}"),
        }
    }

    #[test]
    fn sources_json_is_global_across_the_set_subcommand() {
        // --json must bind the same flag whether it precedes OR follows `set`
        // (the natural automation form) — the global-arg fix.
        for args in [
            ["pixtuoid", "sources", "--json", "set", "codex"],
            ["pixtuoid", "sources", "set", "codex", "--json"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(
                matches!(
                    cli.cmd,
                    Some(Cmd::Sources {
                        action: Some(SourcesAction::Set { .. }),
                        json: true
                    })
                ),
                "args {args:?} must parse json=true"
            );
        }
    }

    #[test]
    fn connect_requires_at_least_one_id() {
        // `required = true` on the ids vec — a bare `connect` is a parse error.
        assert!(Cli::try_parse_from(["pixtuoid", "connect"]).is_err());
    }

    #[test]
    fn connect_and_disconnect_parse_multiple_ids() {
        let c = Cli::try_parse_from(["pixtuoid", "connect", "claude-code", "codex"]).unwrap();
        assert!(matches!(c.cmd, Some(Cmd::Connect { .. })));
        let d = Cli::try_parse_from(["pixtuoid", "disconnect", "codex", "--json"]).unwrap();
        assert!(matches!(d.cmd, Some(Cmd::Disconnect { json: true, .. })));
    }

    #[test]
    fn setup_defaults_to_dry_run_and_takes_yes() {
        let dry = Cli::try_parse_from(["pixtuoid", "setup"]).unwrap();
        assert!(matches!(dry.cmd, Some(Cmd::Setup { yes: false })));
        let apply = Cli::try_parse_from(["pixtuoid", "setup", "--yes"]).unwrap();
        assert!(matches!(apply.cmd, Some(Cmd::Setup { yes: true })));
    }

    #[test]
    fn assets_subcommands_parse_the_collaboration_contract() {
        let list = Cli::try_parse_from(["pixtuoid", "assets", "list", "--json"]).unwrap();
        assert!(matches!(
            list.cmd,
            Some(Cmd::Assets {
                action: AssetsAction::List { json: true, .. },
                ..
            })
        ));

        let import = Cli::try_parse_from([
            "pixtuoid",
            "assets",
            "import",
            "C:/my-pack",
            "--id",
            "team-pack",
        ])
        .unwrap();
        assert!(matches!(
            import.cmd,
            Some(Cmd::Assets {
                action: AssetsAction::Import { id, .. },
                ..
            }) if id == "team-pack"
        ));

        let verify = Cli::try_parse_from([
            "pixtuoid",
            "assets",
            "verify",
            "team-pack",
            "--expect",
            "0123456789abcdef",
        ])
        .unwrap();
        assert!(matches!(
            verify.cmd,
            Some(Cmd::Assets {
                action: AssetsAction::Verify {
                    expected: Some(_),
                    ..
                },
                ..
            })
        ));
    }
}
