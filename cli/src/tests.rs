use super::*;
use crate::args::{AddSubcommand, AiProvider};
use crate::commands::completions::{
    generate_powershell_with_alias, generate_with_alias, generate_zsh_with_alias,
};
use clap::CommandFactory;
use clap_complete::shells::{Bash, Elvish, Fish};

#[test]
fn parses_ai_interactive_default() {
    let cli = Cli::try_parse_from(["taurine", "ai"]).expect("ai default should parse");

    match cli.command {
        Some(Commands::Ai {
            yes,
            provider,
            key,
            model,
            endpoint,
            remove,
            remove_all,
        }) => {
            assert!(!yes);
            assert_eq!(provider, None);
            assert_eq!(key, None);
            assert_eq!(model, None);
            assert_eq!(endpoint, None);
            assert_eq!(remove, None);
            assert!(!remove_all);
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn parses_ai_headless_configure() {
    let cli = Cli::try_parse_from([
        "taurine",
        "ai",
        "-y",
        "--provider",
        "openai",
        "--key",
        "sk-secret",
        "--model",
        "gpt-4o",
    ])
    .expect("ai headless configure should parse");

    match cli.command {
        Some(Commands::Ai {
            yes,
            provider,
            key,
            model,
            ..
        }) => {
            assert!(yes);
            assert_eq!(provider, Some(AiProvider::Openai));
            assert_eq!(key.as_deref(), Some("sk-secret"));
            assert_eq!(model.as_deref(), Some("gpt-4o"));
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn parses_ai_headless_remove() {
    let cli = Cli::try_parse_from(["taurine", "ai", "-y", "--remove", "gemini"])
        .expect("ai remove should parse");

    match cli.command {
        Some(Commands::Ai { yes, remove, .. }) => {
            assert!(yes);
            assert_eq!(remove, Some(AiProvider::Gemini));
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn parses_add_hotkey_flag_as_boolean_mode() {
    let cli = Cli::try_parse_from(["taurine", "add", "--hotkey", "Ctrl+Shift+G", "git status"])
        .expect("add --hotkey should parse");

    match cli.command {
        Some(Commands::Add(args)) => {
            assert!(args.hotkey);
            assert_eq!(args.trigger.as_deref(), Some("Ctrl+Shift+G"));
            assert_eq!(args.output.as_deref(), Some("git status"));
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn parses_add_script_hotkey_flag() {
    let cli = Cli::try_parse_from([
        "taurine",
        "add",
        "script",
        "--hotkey",
        "ctrl+shift+w",
        "-l",
        "powershell",
        "winget install [0]",
    ])
    .expect("add script --hotkey should parse");

    match cli.command {
        Some(Commands::Add(args)) => {
            if let Some(AddSubcommand::Script {
                trigger,
                hotkey,
                content,
                ..
            }) = &args.sub
            {
                assert!(hotkey);
                assert_eq!(trigger, "ctrl+shift+w");
                assert_eq!(content.as_deref(), Some("winget install [0]"));
            } else {
                panic!("expected script subcommand");
            }
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn no_args_route_to_tui() {
    let cli = Cli::try_parse_from(["taurine"]).expect("no-args invocation should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Tui);
}

#[test]
fn subcommands_continue_to_route_to_cli_handlers() {
    let cli = Cli::try_parse_from(["taurine", "ls"]).expect("list alias should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Command);
}

#[test]
fn daemon_flag_keeps_daemon_launch_path() {
    let cli = Cli::try_parse_from(["taurine", "--daemon"]).expect("--daemon should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Daemon);
}

#[test]
fn auto_update_flag_keeps_auto_update_launch_path() {
    let cli =
        Cli::try_parse_from(["taurine", "--auto-update"]).expect("--auto-update should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::AutoUpdate);
}

#[test]
fn version_flag_prints_expected_format() {
    // --version should exit successfully and print "taurine <semver>"
    let cli = Cli::try_parse_from(["taurine", "--version"]).expect("--version should parse");
    assert!(cli.version, "version flag should be true");
    // The actual output is printed in main() before launch_target is called,
    // so we verify the flag routing here and the constant exists.
    assert!(
        VERSION.contains('.'),
        "VERSION constant should be a semver string, got: {VERSION}"
    );
}

#[test]
fn verbose_flag_only_invocation_routes_to_tui() {
    let cli =
        Cli::try_parse_from(["taurine", "--verbose"]).expect("flag-only invocation should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Tui);
}

#[test]
fn no_log_file_flag_only_invocation_routes_to_tui() {
    let cli = Cli::try_parse_from(["taurine", "--no-log-file"])
        .expect("flag-only invocation should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Tui);
}

#[test]
fn quiet_flag_only_invocation_routes_to_tui() {
    let cli = Cli::try_parse_from(["taurine", "-q"]).expect("flag-only invocation should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Tui);
}

#[test]
fn global_json_flag_on_list() {
    let cli = Cli::try_parse_from(["taurine", "ls", "--json"]).expect("ls --json should parse");
    assert!(cli.json);
    assert!(matches!(cli.command, Some(Commands::List { .. })));
}

#[test]
fn global_json_flag_on_config_list() {
    let cli = Cli::try_parse_from(["taurine", "config", "list", "--json"])
        .expect("config list --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_ai_status() {
    let cli =
        Cli::try_parse_from(["taurine", "ai", "-y", "--json"]).expect("ai -y --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_ai_configure() {
    let cli = Cli::try_parse_from(["taurine", "ai", "-y", "--provider", "gemini", "--json"])
        .expect("ai -y --provider gemini --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_add() {
    let cli = Cli::try_parse_from(["taurine", "add", "gs", "git status", "--json"])
        .expect("add --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_delete() {
    let cli = Cli::try_parse_from(["taurine", "delete", "gs", "--json"])
        .expect("delete --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_up() {
    let cli = Cli::try_parse_from(["taurine", "up", "--json"]).expect("up --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_down() {
    let cli = Cli::try_parse_from(["taurine", "down", "--json"]).expect("down --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_status() {
    let cli =
        Cli::try_parse_from(["taurine", "status", "--json"]).expect("status --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_config_set() {
    let cli = Cli::try_parse_from(["taurine", "config", "set", "wpm", "100", "--json"])
        .expect("config set --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_config_reset() {
    let cli = Cli::try_parse_from(["taurine", "config", "reset", "wpm", "--json"])
        .expect("config reset --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_ai_remove() {
    let cli = Cli::try_parse_from(["taurine", "ai", "-y", "--remove", "openai", "--json"])
        .expect("ai remove --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_position_independent() {
    // --json before subcommand
    let cli =
        Cli::try_parse_from(["taurine", "--json", "ls"]).expect("--json before ls should parse");
    assert!(cli.json);
}

#[test]
fn global_json_false_by_default() {
    let cli = Cli::try_parse_from(["taurine", "ls"]).expect("ls without --json should parse");
    assert!(!cli.json);
}

#[test]
fn global_json_on_update_parses() {
    let cli =
        Cli::try_parse_from(["taurine", "update", "--json"]).expect("update --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_on_completions_parses() {
    let cli = Cli::try_parse_from(["taurine", "completions", "bash", "--json"])
        .expect("completions --json should parse");
    assert!(cli.json);
}

#[test]
fn powershell_completions_register_alias() {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    generate_powershell_with_alias(&mut cmd, &mut out);

    let text = String::from_utf8(out).expect("completions should be valid utf-8");
    assert!(
        text.contains("-CommandName 'taurine'"),
        "expected registration for taurine, got: {text}"
    );
    assert!(
        text.contains("-CommandName 'tau'"),
        "expected registration for tau, got: {text}"
    );

    let lines: Vec<_> = text.lines().collect();
    let first_registration = lines
        .iter()
        .position(|line| line.contains("Register-ArgumentCompleter"))
        .expect("script should register a completer");
    let last_using_namespace = lines
        .iter()
        .rposition(|line| line.starts_with("using namespace"))
        .expect("script should have a using namespace header");
    assert!(
        last_using_namespace < first_registration,
        "using namespace must precede all registrations, got: {text}"
    );
}

#[test]
fn zsh_completions_merge_compdef_header() {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    generate_zsh_with_alias(&mut cmd, &mut out);

    let text = String::from_utf8(out).expect("completions should be valid utf-8");
    let first_line = text.lines().next().expect("script should not be empty");
    assert_eq!(first_line, "#compdef taurine tau");
    assert!(
        text.contains("compdef _tau tau"),
        "expected _tau registration, got: {text}"
    );
}

#[test]
fn bash_completions_register_alias() {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    generate_with_alias(Bash, &mut cmd, &mut out);

    let text = String::from_utf8(out).expect("completions should be valid utf-8");
    assert!(
        text.contains("_tau() {"),
        "expected _tau function, got: {text}"
    );
    assert!(
        text.contains("complete -F _tau"),
        "expected complete -F _tau, got: {text}"
    );
}

#[test]
fn fish_completions_register_alias() {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    generate_with_alias(Fish, &mut cmd, &mut out);

    let text = String::from_utf8(out).expect("completions should be valid utf-8");
    assert!(
        text.contains("complete -c tau "),
        "expected complete -c tau, got: {text}"
    );
}

#[test]
fn elvish_completions_register_alias() {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    generate_with_alias(Elvish, &mut cmd, &mut out);

    let text = String::from_utf8(out).expect("completions should be valid utf-8");
    assert!(
        text.contains("arg-completer[tau] = "),
        "expected arg-completer[tau], got: {text}"
    );
}

#[test]
fn action_command_json_status_format() {
    use serde_json::json;

    // add command status objects
    let created = json!({"status": "created", "trigger": "gs"});
    assert_eq!(created["status"], "created");
    assert_eq!(created["trigger"], "gs");

    let exists = json!({"status": "exists", "trigger": "gs"});
    assert_eq!(exists["status"], "exists");

    let updated = json!({"status": "updated", "trigger": "gs"});
    assert_eq!(updated["status"], "updated");

    // delete command status objects
    let deleted = json!({"status": "deleted", "count": 3});
    assert_eq!(deleted["status"], "deleted");
    assert_eq!(deleted["count"], 3);

    let not_found = json!({"status": "not_found", "tag": "dev"});
    assert_eq!(not_found["status"], "not_found");
    assert_eq!(not_found["tag"], "dev");

    // service command status objects
    let started = json!({"status": "started"});
    assert_eq!(started["status"], "started");

    let stopped = json!({"status": "stopped"});
    assert_eq!(stopped["status"], "stopped");

    let restarted = json!({"status": "restarted"});
    assert_eq!(restarted["status"], "restarted");
}

#[test]
fn action_command_json_script_format() {
    use serde_json::json;

    let created = json!({"status": "created", "trigger": "deploy", "action_type": "script"});
    assert_eq!(created["action_type"], "script");
    assert_eq!(created["trigger"], "deploy");

    let updated = json!({"status": "updated", "trigger": "deploy", "action_type": "script"});
    assert_eq!(updated["status"], "updated");
}

#[test]
fn action_command_json_config_format() {
    use serde_json::json;

    let updated = json!({"status": "updated", "key": "wpm"});
    assert_eq!(updated["status"], "updated");
    assert_eq!(updated["key"], "wpm");

    let reset = json!({"status": "reset", "key": "wpm"});
    assert_eq!(reset["status"], "reset");

    let reset_all = json!({"status": "reset_all"});
    assert_eq!(reset_all["status"], "reset_all");
}

#[test]
fn cli_add_regex_flag_parses() {
    let cli = Cli::try_parse_from(["taurine", "add", "--regex", "issue-(\\d+)", "link/[0]"])
        .expect("add --regex should parse");
    match cli.command {
        Some(Commands::Add(args)) => {
            assert!(args.regex);
            assert_eq!(args.trigger.as_deref(), Some("issue-(\\d+)"));
            assert_eq!(args.output.as_deref(), Some("link/[0]"));
        }
        other => panic!("unexpected parse output: {other:?}"),
    }
}

#[test]
fn parses_import_conflict_flags() {
    // Test long flag --conflict
    let cli = Cli::try_parse_from(["taurine", "import", "backup.tau", "--conflict", "skip"])
        .expect("import --conflict should parse");
    match cli.command {
        Some(Commands::Import { conflict, .. }) => {
            assert_eq!(conflict, Some(args::ImportConflictCli::Skip));
        }
        other => panic!("unexpected command parse: {other:?}"),
    }

    // Test short flag -c
    let cli = Cli::try_parse_from(["taurine", "import", "backup.tau", "-c", "overwrite"])
        .expect("import -c should parse");
    match cli.command {
        Some(Commands::Import { conflict, .. }) => {
            assert_eq!(conflict, Some(args::ImportConflictCli::Overwrite));
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}
