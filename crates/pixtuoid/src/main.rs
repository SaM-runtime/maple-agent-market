mod crash;
mod logging;
mod sources_cli;

use anyhow::Result;
use clap::Parser;
use pixtuoid::cli::{AssetsAction, Cli, Cmd, SourceArgs, SourcesAction};
use pixtuoid::{assets, config, doctor, floating, install, runtime, sources, validate};

fn main() -> Result<()> {
    crash::install_crash_hook();
    let (log_level, cmd) = Cli::parse().cmd_or_default();

    // The typed LogLevel's as_str is exactly the old free-string levels, so
    // every filter built in logging::init is unchanged — the enum only moved
    // typo rejection to the clap seam (a typo used to parse as a bogus
    // EnvFilter TARGET directive that silently filtered everything off,
    // #157 class).
    let log_level: &'static str = log_level.as_str();
    let window_active = matches!(&cmd, Cmd::Floating { .. });
    logging::init(window_active, log_level);

    match cmd {
        Cmd::Floating { source } => {
            let rc = build_run_config(source)?;
            floating::run(rc)
        }
        Cmd::Assets { root, action } => {
            let root = root.unwrap_or_else(assets::default_assets_root);
            match action {
                AssetsAction::List { json } => {
                    let entries = assets::list_assets(&root)?;
                    assets::print_asset_list(&entries, json)
                }
                AssetsAction::Import {
                    source,
                    id,
                    force,
                    json,
                } => {
                    let result =
                        assets::import_local_pack(&root, &source, &id, install_mode(force))?;
                    assets::print_asset_result("imported locally", &result, json)
                }
                AssetsAction::Verify { id, expected, json } => {
                    let result = assets::verify_installed(&root, &id, expected.as_deref())?;
                    assets::print_asset_result("verified", &result, json)
                }
            }
        }
        Cmd::ValidatePack { pack_dir } => validate::validate_pack(&pack_dir),
        Cmd::Doctor => doctor::run(&logging::log_file_path()).map(|report| print!("{report}")),
        Cmd::Sources { action: None, json } => sources_cli::run_sources_list(json),
        Cmd::Sources {
            action: Some(SourcesAction::Set { ids }),
            json,
        } => sources_cli::run_sources_set(&ids, json),
        Cmd::Connect { ids, json } => sources_cli::run_change(&ids, json, |c, i| {
            sources::connect(c, i).map(|_| sources::ChangeOutcome::Connected)
        }),
        // A folded hook-removal failure is a PARTIAL failure (the flag IS
        // disconnected, but hooks remain) — surface it AND signal it via a
        // non-zero exit (run_change treats an Err op as failed), so a $?-checking
        // script isn't told a clean "disconnected". The shared phrase keeps
        // every source-management surface aligned.
        Cmd::Disconnect { ids, json } => {
            sources_cli::run_change(&ids, json, |c, i| match sources::disconnect(c, i)? {
                sources::DisconnectOutcome::HookRemovalFailed(e) => Err(anyhow::anyhow!(
                    "{}: {e}",
                    sources::HOOK_REMOVAL_FAILED_PHRASE
                )),
                _ => Ok(sources::ChangeOutcome::Disconnected),
            })
        }
        Cmd::Setup { yes } => sources_cli::run_setup(yes),
    }
}

/// Convert the CLI replacement flag into the asset manager's explicit policy.
fn install_mode(force: bool) -> assets::InstallMode {
    if force {
        assets::InstallMode::ReplaceManaged
    } else {
        assets::InstallMode::RefuseExisting
    }
}

/// Resolve the floating-window startup inputs from CLI args and local config.
fn build_run_config(source: SourceArgs) -> Result<runtime::RunConfig> {
    let SourceArgs {
        socket,
        projects_root,
        codex_sessions_root,
        pack_dir,
    } = source;
    let cfg_path = config::config_path();
    let mut cfg_warnings = Vec::new();
    let cfg = config::load(&cfg_path, &mut cfg_warnings);
    let theme = pixtuoid_scene::theme::theme_by_name("maple")
        .expect("the built-in maple theme must stay registered");
    let pack_dir = config::resolve_pack_dir(&cfg, pack_dir);
    // The connected-source set the Maple scene gates actors on: explicit `[sources]`
    // true flags only (absent = disconnected; the install-state migrate
    // inference was dropped in 0.12.0).
    let connected = config::resolve_connected(&cfg);
    for w in &cfg_warnings {
        eprintln!("⚠ maple-agent-market: {w}");
    }
    warn_broken_installs(&connected);
    Ok(runtime::RunConfig {
        socket,
        projects_root,
        codex_sessions_root,
        pack_dir,
        config_path: cfg_path,
        theme,
        connected,
        audio: config::resolve_audio(&cfg),
    })
}

/// Boot preflight (#309): warn (stderr) when a CONNECTED source's hooks are
/// installed but structurally BROKEN — it would render zero sprites with no other
/// hint, so a user who has not run `doctor` still learns.
/// Routed through the SHARED `doctor::diagnose` rollup (empty log = skip the drift
/// scan; warns on broken installs only) so startup can't drift from the CLI
/// report. Iterates TARGETS, not the source registry: only an
/// install-bearing source can be install-BROKEN.
fn warn_broken_installs(connected: &std::collections::HashSet<String>) {
    for &t in install::TARGETS {
        if !connected.contains(t.core_source) {
            continue;
        }
        let diag = doctor::diagnose(t.core_source, "", None);
        if diag.is_broken() {
            let issues = diag
                .install
                .as_ref()
                .map(|v| v.issues.join("; "))
                .unwrap_or_default();
            eprintln!(
                "⚠ maple-agent-market: {} hooks are installed but BROKEN: {issues} — \
                 run `maple-agent-market connect {}` to repair them",
                t.core_source, t.core_source
            );
        }
    }
}
