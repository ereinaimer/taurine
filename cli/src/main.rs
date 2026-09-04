// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.
pub mod args;
pub mod commands;
pub mod platform;

use args::{Cli, Commands, ConfigAction, LaunchTarget, VERSION};
use clap::Parser;
use tracing::{error, info};

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    if cli.version {
        println!("taurine {}", VERSION);
        return std::process::ExitCode::SUCCESS;
    }

    let launch_target = launch_target(&cli);

    // Suppress console tracing for interactive import/export/ai commands
    // to prevent tracing output from interleaving with prompts and TUI overlays.
    let is_interactive_command = matches!(
        &cli.command,
        Some(Commands::Import { .. })
            | Some(Commands::Export { .. })
            | Some(Commands::Ai { yes: false, .. })
    );
    let quiet = cli.quiet || (is_interactive_command && cli.verbose == 0);

    let component = if cli.daemon {
        taurine_core::logs::LogComponent::Daemon
    } else {
        taurine_core::logs::LogComponent::Cli
    };

    let _guard = taurine_core::logs::init_tracing_for_app(
        cli.verbose,
        quiet,
        cli.no_log_file,
        cli.no_color,
        cli.show_log_prefixes,
        component,
        launch_target == LaunchTarget::Tui || cli.json,
    );

    // Install a panic hook that:
    // 1) writes structured diagnostics into tracing + daily log file
    // 2) prints the human-friendly color-eyre report.
    let (panic_hook, _eyre_hook) = color_eyre::config::HookBuilder::new().into_hooks();
    let color_eyre_panic = panic_hook.into_panic_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        taurine_core::logs::handle_panic_info(panic_info);
        color_eyre_panic(panic_info);
    }));

    if let Err(e) = run(cli, launch_target) {
        error!("Error: {}", e);
        return std::process::ExitCode::from(1);
    }

    std::process::ExitCode::SUCCESS
}

fn run(cli: Cli, launch_target: LaunchTarget) -> taurine_core::error::Result<()> {
    let json = cli.json;
    match launch_target {
        LaunchTarget::Daemon => {
            info!("Initializing Taurine v{VERSION}");

            // Execute the startup sequence (database init, seed, etc.)
            taurine_daemon::start()?;
            info!("Taurine service has been stopped cleanly.");
        }
        LaunchTarget::AutoUpdate => {
            let _ = commands::update::run_auto_update();
            return Ok(());
        }
        LaunchTarget::Tui => return taurine_tui::run(),
        LaunchTarget::Command => {}
    }

    match cli.command {
        #[cfg(target_os = "linux")]
        Some(Commands::Setup) => {
            taurine_core::service::linux_setup()?;
        }
        Some(Commands::Up) => commands::service::execute_up(json)?,
        Some(Commands::Restart) => commands::service::execute_restart(json)?,
        Some(Commands::Down) => commands::service::execute_down(json)?,
        Some(Commands::Status) => commands::service::execute_status(json)?,
        Some(Commands::Update) => commands::update::execute()?,
        Some(Commands::Add(args)) => commands::add::execute_args(*args, json)?,
        Some(Commands::Delete { triggers, tag, yes }) => {
            commands::delete::execute(triggers, tag, yes, json)?;
        }
        Some(Commands::List {
            sort,
            asc,
            desc,
            tag,
        }) => {
            commands::list::execute(sort, asc, desc, json, tag)?;
        }
        Some(Commands::Export { path, plain, yes }) => {
            commands::export::execute(path, plain, yes)?;
        }
        Some(Commands::Import {
            path,
            conflict,
            yes,
        }) => {
            commands::import::execute(path, conflict, yes)?;
        }
        Some(Commands::Config { action }) => match action {
            ConfigAction::Set { key, value } => commands::config::execute_set(key, value, json)?,
            ConfigAction::List => commands::config::execute_list(json)?,
            ConfigAction::Reset { key, all } => {
                if all {
                    commands::config::execute_reset_all(json)?;
                } else if let Some(k) = key {
                    commands::config::execute_reset(k, json)?;
                } else {
                    error!("error: provide a key to reset or use --all to reset everything");
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Ai {
            yes,
            provider,
            key,
            model,
            endpoint,
            remove,
            remove_all,
        }) => {
            commands::ai::execute(commands::ai::AiCommandArgs {
                yes,
                provider: provider.map(Into::into),
                key,
                model,
                endpoint,
                remove: remove.map(Into::into),
                remove_all,
                json,
            })?;
        }
        Some(Commands::Completions { action }) => {
            commands::completions::handle_completion(&action)?;
        }
        None => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            cmd.print_help()?;
        }
    }

    Ok(())
}

fn launch_target(cli: &Cli) -> LaunchTarget {
    if cli.daemon {
        LaunchTarget::Daemon
    } else if cli.auto_update {
        LaunchTarget::AutoUpdate
    } else if cli.command.is_none() {
        LaunchTarget::Tui
    } else {
        LaunchTarget::Command
    }
}

#[cfg(test)]
mod tests;
