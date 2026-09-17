//! agent-sync — sync AI coding agent configs from one canonical Commons.
//!
//! The single testing seam is [`run`]: it takes argv, the process environment,
//! and writers for stdout/stderr, and returns the process exit code. Every
//! integration test drives the tool through this function against a throwaway
//! directory tree, so no test touches the real home directory.

use std::io::Write;

use clap::Parser;

pub mod adopt;
pub mod cli;
pub mod commons;
pub mod config;
pub mod config_edit;
pub mod doctor;
pub mod env;
pub mod family;
pub mod hooks;
pub mod init;
pub mod instructions;
pub mod link;
pub mod lock;
pub mod mcp;
pub mod mcp_cmd;
pub mod mcp_toml;
pub mod registry;
pub mod render;
pub mod report;
pub mod revert;
pub mod status;
pub mod sync;
pub mod target;
pub mod write;

/// Everything is in the state the Commons describes.
pub const EXIT_CLEAN: i32 = 0;
/// The command could not complete.
pub const EXIT_ERROR: i32 = 1;
/// Actionable state exists: something for `sync` to do.
pub const EXIT_ACTIONABLE: i32 = 2;

/// The single seam. `args` is argv including the program name; `vars` is the
/// process environment as key/value pairs.
pub fn run(
    args: &[String],
    vars: &[(String, String)],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let parsed = match cli::Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            use clap::error::ErrorKind;
            // --help and --version are results, not diagnostics.
            let informational = matches!(
                e.kind(),
                ErrorKind::DisplayHelp
                    | ErrorKind::DisplayVersion
                    | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            );
            if informational {
                let _ = write!(out, "{e}");
                return EXIT_CLEAN;
            }
            let _ = write!(err, "{e}");
            return EXIT_ERROR;
        }
    };

    let env = match env::Env::resolve(vars) {
        Ok(env) => env,
        Err(e) => {
            let _ = writeln!(err, "error: {e}");
            return EXIT_ERROR;
        }
    };

    let config = match config::Config::load(env.config_dir()) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "error: {e}");
            return EXIT_ERROR;
        }
    };

    // A rule naming an agent that does not exist is almost always a typo, and
    // silently ignoring it would scope a server to nothing at all.
    for (server, agent) in config.mcp_agent_names() {
        if registry::by_name(agent).is_none() {
            let _ = writeln!(
                err,
                "error: mcp.{server} names `{agent}`, which is not an agent agent-sync knows about"
            );
            return EXIT_ERROR;
        }
    }

    let mut reporter = report::Reporter::new(out, err);

    // A bare invocation is `sync`, the everyday command.
    match parsed.command.unwrap_or(cli::Command::Sync { dry_run: false }) {
        cli::Command::Sync { dry_run } => sync::run(&env, &config, &mut reporter, dry_run),
        cli::Command::Init => init::run(&env, &config, &mut reporter),
        cli::Command::Adopt { path, dry_run } => {
            adopt::run(&env, &config, &mut reporter, &path, dry_run)
        }
        cli::Command::Revert { agent } => revert::run(&env, &config, &mut reporter, &agent),
        cli::Command::Status { json } => status::run(&env, &config, &mut reporter, json),
        cli::Command::Mcp { command } => match command {
            cli::McpCommand::List { json } => mcp_cmd::list(&env, &config, &mut reporter, json),
            cli::McpCommand::Adopt { spec, all } => {
                mcp_cmd::adopt(&env, &config, &mut reporter, spec.as_deref(), all)
            }
            cli::McpCommand::Remove { name } => {
                mcp_cmd::remove(&env, &config, &mut reporter, &name)
            }
            cli::McpCommand::Disable { name } => {
                mcp_cmd::disable(&env, &config, &mut reporter, &name)
            }
            cli::McpCommand::Enable { name } => {
                mcp_cmd::enable(&env, &config, &mut reporter, &name)
            }
        },
        cli::Command::Doctor => doctor::run(&env, &config, &mut reporter),
    }
}
