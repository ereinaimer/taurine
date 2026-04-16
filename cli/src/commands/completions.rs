use crate::{Cli, ShellCompletionAction};
use clap::CommandFactory;
use clap_complete::shells::{Bash, Elvish, Fish, PowerShell, Zsh};
use clap_complete::{Generator, generate};
use colored::Colorize;
use std::fs;
use std::io;
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::process::Command;

pub(crate) fn handle_completion(action: &ShellCompletionAction) -> taurine_core::error::Result<()> {
    let mut cmd = Cli::command();

    match action {
        ShellCompletionAction::Bash => print_completion(Bash, &mut cmd),
        ShellCompletionAction::Elvish => print_completion(Elvish, &mut cmd),
        ShellCompletionAction::Fish => print_completion(Fish, &mut cmd),
        ShellCompletionAction::Powershell => print_completion(PowerShell, &mut cmd),
        ShellCompletionAction::Zsh => print_completion(Zsh, &mut cmd),
        ShellCompletionAction::Install => install_completion(&mut cmd),
        ShellCompletionAction::Uninstall => uninstall_completion(),
    }

    Ok(())
}

fn print_completion<G: Generator>(generator: G, cmd: &mut clap::Command) {
    generate(generator, cmd, "taurine", &mut io::stdout());
}

fn install_completion(cmd: &mut clap::Command) {
    if cfg!(target_os = "windows") {
        install_windows(cmd);
    } else {
        install_unix(cmd);
    }
}

fn uninstall_completion() {
    if cfg!(target_os = "windows") {
        uninstall_windows();
    } else {
        uninstall_unix();
    }
}

fn taurine_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join(taurine_core::constants::APP_NAME_SLUG))
}

#[cfg(target_os = "windows")]
fn install_windows(cmd: &mut clap::Command) {
    let shell = std::env::var("SHELL").unwrap_or_default();
    let is_git_bash = shell.ends_with("bash");
    let is_powershell = std::env::var("PSModulePath").is_ok();

    if is_powershell && !is_git_bash {
        if let Some(policy) = powershell_execution_policy()
            && matches!(policy.as_str(), "Restricted" | "AllSigned")
        {
            eprintln!(
                "\nPowerShell execution policy is too restrictive ({policy})\n\
                Completion scripts cannot be sourced.\n\n\
                Fix:\n\
                Run this once in PowerShell:\n\n\
                Set-ExecutionPolicy -Scope CurrentUser -ExecutionPolicy RemoteSigned\n"
            );
            return;
        }

        let profile_paths = get_powershell_profiles();
        if profile_paths.is_empty() {
            println!("Could not detect any PowerShell profile.");
            return;
        }

        let Some(config_dir) = taurine_config_dir() else {
            eprintln!("Failed to determine Taurine config directory.");
            return;
        };

        let completions_dir = config_dir.join("Completions");
        if let Err(error) = fs::create_dir_all(&completions_dir) {
            eprintln!("Failed to create completions directory: {error}");
            return;
        }

        let ps_file = completions_dir.join("_taurine.ps1");
        let mut file = match fs::File::create(&ps_file) {
            Ok(file) => file,
            Err(error) => {
                eprintln!("Failed to create completion file: {error}");
                return;
            }
        };

        generate(PowerShell, cmd, "taurine", &mut file);
        println!("Installed PowerShell completion to: {}", ps_file.display());

        for profile_path in profile_paths {
            if !profile_path.exists() {
                if let Some(parent) = profile_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(&profile_path, "");
            }

            let profile_content = fs::read_to_string(&profile_path).unwrap_or_default();
            let source_line = format!(". \"{}\"", ps_file.display());

            if !profile_content.contains(&source_line) {
                use std::io::Write;

                let mut file = match fs::OpenOptions::new().append(true).open(&profile_path) {
                    Ok(file) => file,
                    Err(error) => {
                        eprintln!(
                            "Failed to open PowerShell profile {}: {error}",
                            profile_path.display()
                        );
                        continue;
                    }
                };

                if let Err(error) = writeln!(file, "\n{source_line}") {
                    eprintln!(
                        "Failed to append to profile {}: {error}",
                        profile_path.display()
                    );
                } else {
                    println!(
                        "Added sourcing line to PowerShell profile: {}",
                        profile_path.display().to_string().cyan()
                    );
                }
            } else {
                println!(
                    "PowerShell profile already sources this file: {}",
                    profile_path.display().to_string().cyan()
                );
            }
        }

        println!("Restart PowerShell to activate completions!");
    } else if is_git_bash {
        let Some(home) = dirs::home_dir() else {
            eprintln!("Failed to determine the home directory for Git Bash completions.");
            return;
        };

        let completions_dir = home.join(".bash_completions");
        let _ = fs::create_dir_all(&completions_dir);

        let bash_file = completions_dir.join("taurine.bash");
        match fs::File::create(&bash_file) {
            Ok(mut file) => {
                generate(Bash, cmd, "taurine", &mut file);
                println!("Installed Bash completion to: {}", bash_file.display());
                println!(
                    "To enable it, ensure this line is sourced in your ~/.bashrc or ~/.bash_profile:"
                );
                println!("{}", format!("source \"{}\"", bash_file.display()).green());
            }
            Err(error) => eprintln!("Failed to create completion file: {error}"),
        }
    } else {
        println!(
            "Could not detect a supported shell (PowerShell or Git Bash) for automatic installation."
        );
        println!("Generate a script manually with 'taurine completions <shell> > <file>'.");
    }
}

#[cfg(not(target_os = "windows"))]
fn install_windows(_cmd: &mut clap::Command) {}

#[cfg(not(target_os = "windows"))]
fn install_unix(cmd: &mut clap::Command) {
    let shell = std::env::var("SHELL").unwrap_or_default();

    let Some(config_dir) = taurine_config_dir() else {
        eprintln!("Failed to determine Taurine config directory.");
        return;
    };

    let completions_dir = config_dir.join("completions");
    if let Err(error) = fs::create_dir_all(&completions_dir) {
        eprintln!("Failed to create completions directory: {error}");
        return;
    }

    if shell.contains("zsh") {
        install_zsh(cmd, &completions_dir);
    } else if shell.contains("fish") {
        install_fish(cmd);
    } else {
        install_bash(cmd, &completions_dir);
    }
}

#[cfg(target_os = "windows")]
fn install_unix(_cmd: &mut clap::Command) {}

#[cfg(not(target_os = "windows"))]
fn install_bash(cmd: &mut clap::Command, completions_dir: &std::path::Path) {
    let bash_file = completions_dir.join("taurine.bash");

    match fs::File::create(&bash_file) {
        Ok(mut file) => {
            generate(Bash, cmd, "taurine", &mut file);
            println!("Installed Bash completion to: {}", bash_file.display());

            if let Some(home) = dirs::home_dir() {
                let rc_file = if cfg!(target_os = "macos") {
                    home.join(".bash_profile")
                } else {
                    home.join(".bashrc")
                };

                append_to_rc_file(&rc_file, &format!("source \"{}\"", bash_file.display()));
            }
        }
        Err(error) => eprintln!("Failed to create completion file: {error}"),
    }
}

#[cfg(not(target_os = "windows"))]
fn install_zsh(cmd: &mut clap::Command, completions_dir: &std::path::Path) {
    let zsh_file = completions_dir.join("_taurine");

    match fs::File::create(&zsh_file) {
        Ok(mut file) => {
            generate(Zsh, cmd, "taurine", &mut file);
            println!("Installed Zsh completion to: {}", zsh_file.display());

            if let Some(home) = dirs::home_dir() {
                let rc_file = home.join(".zshrc");
                let line = format!(
                    "fpath=(\"{}\" $fpath)\nautoload -U compinit; compinit",
                    completions_dir.display()
                );
                append_to_rc_file(&rc_file, &line);
            }
        }
        Err(error) => eprintln!("Failed to create completion file: {error}"),
    }
}

#[cfg(not(target_os = "windows"))]
fn install_fish(cmd: &mut clap::Command) {
    let Some(home) = dirs::home_dir() else {
        eprintln!("Failed to determine the home directory for Fish completions.");
        return;
    };

    let completions_dir = home.join(".config/fish/completions");
    if let Err(error) = fs::create_dir_all(&completions_dir) {
        eprintln!("Failed to create fish completions directory: {error}");
        return;
    }

    let fish_file = completions_dir.join("taurine.fish");
    match fs::File::create(&fish_file) {
        Ok(mut file) => {
            generate(Fish, cmd, "taurine", &mut file);
            println!("Installed Fish completion to: {}", fish_file.display());
        }
        Err(error) => eprintln!("Failed to create completion file: {error}"),
    }
}

#[cfg(not(target_os = "windows"))]
fn append_to_rc_file(path: &std::path::Path, content: &str) {
    if !path.exists() {
        match fs::File::create(path) {
            Ok(mut file) => {
                use std::io::Write;

                if let Err(error) = writeln!(file, "\n{content}") {
                    eprintln!("Failed to write to {}: {error}", path.display());
                } else {
                    println!(
                        "Added sourcing line to: {}",
                        path.display().to_string().cyan()
                    );
                }
            }
            Err(error) => eprintln!("Failed to create {}: {error}", path.display()),
        }
        return;
    }

    match fs::read_to_string(path) {
        Ok(existing_content) => {
            let fpath_prefix = if content.contains("fpath") {
                content.split_once(' ').map(|(prefix, _)| prefix)
            } else {
                None
            };

            let already_exists = existing_content.contains(content)
                || fpath_prefix.is_some_and(|prefix| existing_content.contains(prefix));

            if already_exists {
                println!(
                    "File {} already contains the sourcing line.",
                    path.display().to_string().cyan()
                );
                return;
            }

            use std::io::Write;

            let mut file = match fs::OpenOptions::new().write(true).append(true).open(path) {
                Ok(file) => file,
                Err(error) => {
                    eprintln!("Failed to open {} for appending: {error}", path.display());
                    return;
                }
            };

            if let Err(error) = writeln!(file, "\n{content}") {
                eprintln!("Failed to append to {}: {error}", path.display());
            } else {
                println!(
                    "Added sourcing line to: {}",
                    path.display().to_string().cyan()
                );
            }
        }
        Err(error) => eprintln!("Failed to read {}: {error}", path.display()),
    }
}

#[cfg(target_os = "windows")]
fn uninstall_windows() {
    let Some(config_dir) = taurine_config_dir() else {
        eprintln!("Failed to determine Taurine config directory.");
        return;
    };

    let completions_dir = config_dir.join("Completions");
    let ps_file = completions_dir.join("_taurine.ps1");

    if ps_file.exists() {
        match fs::remove_file(&ps_file) {
            Ok(_) => println!("Removed completion file: {}", ps_file.display()),
            Err(error) => eprintln!("Failed to remove completion file: {error}"),
        }
    } else {
        println!("Completion file not found: {}", ps_file.display());
    }

    if let Some(home) = dirs::home_dir() {
        let bash_file = home.join(".bash_completions").join("taurine.bash");
        if bash_file.exists() {
            match fs::remove_file(&bash_file) {
                Ok(_) => println!("Removed Git Bash completion file: {}", bash_file.display()),
                Err(error) => eprintln!("Failed to remove Git Bash completion file: {error}"),
            }
        }
    }

    let profile_paths = get_powershell_profiles();
    if profile_paths.is_empty() {
        println!("Could not detect any PowerShell profile paths.");
        return;
    }

    for profile_path in profile_paths {
        if !profile_path.exists() {
            continue;
        }

        let source_line = format!(". \"{}\"", ps_file.display());
        match fs::read_to_string(&profile_path) {
            Ok(content) => {
                if content.contains(&source_line) {
                    let new_content = content
                        .lines()
                        .filter(|line| !line.contains(&source_line))
                        .collect::<Vec<_>>()
                        .join("\n");

                    match fs::write(&profile_path, new_content) {
                        Ok(_) => println!(
                            "Removed sourcing line from PowerShell profile: {}",
                            profile_path.display().to_string().cyan()
                        ),
                        Err(error) => eprintln!(
                            "Failed to write to profile {}: {error}",
                            profile_path.display()
                        ),
                    }
                } else {
                    println!(
                        "PowerShell profile does not contain the sourcing line: {}",
                        profile_path.display().to_string().cyan()
                    );
                }
            }
            Err(error) => eprintln!("Failed to read profile {}: {error}", profile_path.display()),
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn uninstall_windows() {}

#[cfg(not(target_os = "windows"))]
fn uninstall_unix() {
    let Some(config_dir) = taurine_config_dir() else {
        eprintln!("Failed to determine Taurine config directory.");
        return;
    };

    let completions_dir = config_dir.join("completions");
    let bash_file = completions_dir.join("taurine.bash");
    if bash_file.exists() {
        let _ = fs::remove_file(&bash_file);
        println!("Removed Bash completions.");
    }

    let zsh_file = completions_dir.join("_taurine");
    if zsh_file.exists() {
        let _ = fs::remove_file(&zsh_file);
        println!("Removed Zsh completions.");
    }

    if let Some(home) = dirs::home_dir() {
        let fish_file = home.join(".config/fish/completions/taurine.fish");
        if fish_file.exists() {
            let _ = fs::remove_file(&fish_file);
            println!("Removed Fish completions.");
        }

        let rc_file = if cfg!(target_os = "macos") {
            home.join(".bash_profile")
        } else {
            home.join(".bashrc")
        };
        remove_line_from_file(&rc_file, &format!("source \"{}\"", bash_file.display()));

        let zshrc = home.join(".zshrc");
        let zsh_line = format!("fpath=(\"{}\" $fpath)", completions_dir.display());
        remove_line_from_file(&zshrc, &zsh_line);
    }
}

#[cfg(target_os = "windows")]
fn uninstall_unix() {}

#[cfg(not(target_os = "windows"))]
fn remove_line_from_file(path: &std::path::Path, partial_content: &str) {
    if !path.exists() {
        return;
    }

    match fs::read_to_string(path) {
        Ok(content) => {
            if content.contains(partial_content) {
                let new_content = content
                    .lines()
                    .filter(|line| !line.contains(partial_content))
                    .collect::<Vec<_>>()
                    .join("\n");

                match fs::write(path, new_content) {
                    Ok(_) => println!(
                        "Removed sourcing line from: {}",
                        path.display().to_string().cyan()
                    ),
                    Err(error) => eprintln!("Failed to write to {}: {error}", path.display()),
                }
            }
        }
        Err(error) => eprintln!("Failed to read {}: {error}", path.display()),
    }
}

#[cfg(target_os = "windows")]
fn powershell_execution_policy() -> Option<String> {
    for exe in ["pwsh", "powershell"] {
        let output = Command::new(exe)
            .args([
                "-NoProfile",
                "-Command",
                "Get-ExecutionPolicy -Scope CurrentUser",
            ])
            .output();

        if let Ok(output) = output
            && output.status.success()
        {
            return Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn get_powershell_profiles() -> Vec<PathBuf> {
    let mut profiles = Vec::new();

    if let Ok(profile) = std::env::var("PROFILE") {
        let path = PathBuf::from(profile);
        if !path.as_os_str().is_empty() {
            profiles.push(path);
        }
    }

    if let Some(docs) = dirs::document_dir() {
        let pwsh_profile = docs
            .join("PowerShell")
            .join("Microsoft.PowerShell_profile.ps1");
        if pwsh_profile.exists() {
            profiles.push(pwsh_profile);
        }

        let ps_profile = docs
            .join("WindowsPowerShell")
            .join("Microsoft.PowerShell_profile.ps1");
        if ps_profile.exists() {
            profiles.push(ps_profile);
        }

        if profiles.is_empty() {
            profiles.push(
                docs.join("PowerShell")
                    .join("Microsoft.PowerShell_profile.ps1"),
            );
        }
    }

    profiles.sort();
    profiles.dedup();
    profiles
}
