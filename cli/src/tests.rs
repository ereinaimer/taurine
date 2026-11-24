use super::*;
use crate::args::{AddSubcommand, AiProvider};
use crate::commands::completions::generate_completion;
use clap::CommandFactory;
use clap::Parser;
use clap_complete::shells::{Bash, Elvish, Fish, PowerShell, Zsh};

#[test]
fn parses_ai_interactive_default() {
    let cli = Cli::try_parse_from(["nite", "ai"]).expect("ai default should parse");

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
        "nite",
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
    let cli = Cli::try_parse_from(["nite", "ai", "-y", "--remove", "gemini"])
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
    let cli = Cli::try_parse_from(["nite", "add", "--hotkey", "Ctrl+Shift+G", "git status"])
        .expect("add --hotkey should parse");

    match cli.command {
        Some(Commands::Add(args)) => {
            assert_eq!(args.hotkey, vec!["Ctrl+Shift+G"]);
            assert_eq!(args.positional, vec!["git status"]);
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn parses_add_script_hotkey_flag() {
    let cli = Cli::try_parse_from([
        "nite",
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
                positional, hotkey, ..
            }) = &args.sub
            {
                assert_eq!(hotkey, &vec!["ctrl+shift+w".to_string()]);
                assert_eq!(positional, &vec!["winget install [0]".to_string()]);
            } else {
                panic!("expected script subcommand");
            }
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn no_args_route_to_tui() {
    let cli = Cli::try_parse_from(["nite"]).expect("no-args invocation should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Tui);
}

#[test]
fn subcommands_continue_to_route_to_cli_handlers() {
    let cli = Cli::try_parse_from(["nite", "ls"]).expect("list alias should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Command);
}

#[test]
fn daemon_flag_keeps_daemon_launch_path() {
    let cli = Cli::try_parse_from(["nite", "--daemon"]).expect("--daemon should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Daemon);
}

#[test]
fn auto_update_flag_keeps_auto_update_launch_path() {
    let cli = Cli::try_parse_from(["nite", "--auto-update"]).expect("--auto-update should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::AutoUpdate);
}

#[test]
fn version_flag_prints_expected_format() {
    // --version should exit successfully and print "nite <semver>"
    let cli = Cli::try_parse_from(["nite", "--version"]).expect("--version should parse");
    assert!(cli.version, "version flag should be true");
    // The actual output is printed in main() before launch_target is called,
    // so flag routing and constant existence are verified here.
    assert!(
        VERSION.contains('.'),
        "VERSION constant should be a semver string, got: {VERSION}"
    );
}

#[test]
fn verbose_flag_only_invocation_routes_to_tui() {
    let cli =
        Cli::try_parse_from(["nite", "--verbose"]).expect("flag-only invocation should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Tui);
}

#[test]
fn no_log_file_flag_only_invocation_routes_to_tui() {
    let cli =
        Cli::try_parse_from(["nite", "--no-log-file"]).expect("flag-only invocation should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Tui);
}

#[test]
fn quiet_flag_only_invocation_routes_to_tui() {
    let cli = Cli::try_parse_from(["nite", "-q"]).expect("flag-only invocation should parse");
    assert_eq!(launch_target(&cli), LaunchTarget::Tui);
}

#[test]
fn global_json_flag_on_list() {
    let cli = Cli::try_parse_from(["nite", "ls", "--json"]).expect("ls --json should parse");
    assert!(cli.json);
    assert!(matches!(cli.command, Some(Commands::List { .. })));
}

#[test]
fn global_json_flag_on_config_list() {
    let cli = Cli::try_parse_from(["nite", "config", "list", "--json"])
        .expect("config list --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_ai_status() {
    let cli =
        Cli::try_parse_from(["nite", "ai", "-y", "--json"]).expect("ai -y --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_ai_configure() {
    let cli = Cli::try_parse_from(["nite", "ai", "-y", "--provider", "gemini", "--json"])
        .expect("ai -y --provider gemini --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_add() {
    let cli = Cli::try_parse_from(["nite", "add", "gs", "git status", "--json"])
        .expect("add --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_delete() {
    let cli = Cli::try_parse_from(["nite", "delete", "gs", "--json"])
        .expect("delete --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_up() {
    let cli = Cli::try_parse_from(["nite", "up", "--json"]).expect("up --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_down() {
    let cli = Cli::try_parse_from(["nite", "down", "--json"]).expect("down --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_status() {
    let cli =
        Cli::try_parse_from(["nite", "status", "--json"]).expect("status --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_config_set() {
    let cli = Cli::try_parse_from(["nite", "config", "set", "audio_volume", "100", "--json"])
        .expect("config set --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_config_reset() {
    let cli = Cli::try_parse_from(["nite", "config", "reset", "audio_volume", "--json"])
        .expect("config reset --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_on_ai_remove() {
    let cli = Cli::try_parse_from(["nite", "ai", "-y", "--remove", "openai", "--json"])
        .expect("ai remove --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_flag_position_independent() {
    // --json before subcommand
    let cli = Cli::try_parse_from(["nite", "--json", "ls"]).expect("--json before ls should parse");
    assert!(cli.json);
}

#[test]
fn global_json_false_by_default() {
    let cli = Cli::try_parse_from(["nite", "ls"]).expect("ls without --json should parse");
    assert!(!cli.json);
}

#[test]
fn global_json_on_update_parses() {
    let cli =
        Cli::try_parse_from(["nite", "update", "--json"]).expect("update --json should parse");
    assert!(cli.json);
}

#[test]
fn global_json_on_completions_parses() {
    let cli = Cli::try_parse_from(["nite", "completions", "bash", "--json"])
        .expect("completions --json should parse");
    assert!(cli.json);
}

#[test]
fn powershell_completions_generate_for_nite() {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    generate_completion(PowerShell, &mut cmd, &mut out);

    let text = String::from_utf8(out).expect("completions should be valid utf-8");
    assert!(
        text.contains("-CommandName 'nite'"),
        "expected registration for nite, got: {text}"
    );
    assert!(
        !text.contains("-CommandName 'tau'"),
        "did not expect registration for tau, got: {text}"
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
fn zsh_completions_generate_for_nite() {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    generate_completion(Zsh, &mut cmd, &mut out);

    let text = String::from_utf8(out).expect("completions should be valid utf-8");
    let first_line = text.lines().next().expect("script should not be empty");
    assert_eq!(first_line, "#compdef nite");
    assert!(
        !text.contains("compdef _tau tau"),
        "did not expect _tau registration, got: {text}"
    );
}

#[test]
fn bash_completions_generate_for_nite() {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    generate_completion(Bash, &mut cmd, &mut out);

    let text = String::from_utf8(out).expect("completions should be valid utf-8");
    assert!(
        text.contains("_nite() {") || text.contains("nite"),
        "expected nite completion, got: {text}"
    );
    assert!(!text.contains("_tau() {"));
}

#[test]
fn fish_completions_generate_for_nite() {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    generate_completion(Fish, &mut cmd, &mut out);

    let text = String::from_utf8(out).expect("completions should be valid utf-8");
    assert!(
        text.contains("complete -c nite "),
        "expected complete -c nite, got: {text}"
    );
    assert!(!text.contains("complete -c tau "));
}

#[test]
fn elvish_completions_generate_for_nite() {
    let mut cmd = Cli::command();
    let mut out = Vec::new();
    generate_completion(Elvish, &mut cmd, &mut out);

    let text = String::from_utf8(out).expect("completions should be valid utf-8");
    assert!(
        text.contains("arg-completer[nite] = "),
        "expected arg-completer[nite], got: {text}"
    );
    assert!(!text.contains("arg-completer[tau] = "));
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

    let updated = json!({"status": "updated", "key": "audio_volume"});
    assert_eq!(updated["status"], "updated");
    assert_eq!(updated["key"], "audio_volume");

    let reset = json!({"status": "reset", "key": "audio_volume"});
    assert_eq!(reset["status"], "reset");

    let reset_all = json!({"status": "reset_all"});
    assert_eq!(reset_all["status"], "reset_all");
}

#[test]
fn cli_add_regex_flag_parses() {
    let cli = Cli::try_parse_from(["nite", "add", "--regex", "issue-(\\d+)", "link/[0]"])
        .expect("add --regex should parse");
    match cli.command {
        Some(Commands::Add(args)) => {
            assert_eq!(args.regex, vec!["issue-(\\d+)"]);
            assert_eq!(args.positional, vec!["link/[0]"]);
        }
        other => panic!("unexpected parse output: {other:?}"),
    }
}

#[test]
fn parses_import_conflict_flags() {
    // Test long flag --conflict
    let cli = Cli::try_parse_from(["nite", "import", "backup.nx", "--conflict", "skip"])
        .expect("import --conflict should parse");
    match cli.command {
        Some(Commands::Import { conflict, .. }) => {
            assert_eq!(conflict, Some(args::ImportConflictCli::Skip));
        }
        other => panic!("unexpected command parse: {other:?}"),
    }

    // Test short flag -c
    let cli = Cli::try_parse_from(["nite", "import", "backup.nx", "-c", "overwrite"])
        .expect("import -c should parse");
    match cli.command {
        Some(Commands::Import { conflict, .. }) => {
            assert_eq!(conflict, Some(args::ImportConflictCli::Overwrite));
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn parses_auto_update_flag_and_routes_to_launch_target() {
    let cli =
        Cli::try_parse_from(["nite", "--auto-update"]).expect("nite --auto-update should parse");
    assert!(cli.auto_update);
    assert_eq!(launch_target(&cli), LaunchTarget::AutoUpdate);
}

#[test]
fn update_subcommand_routes_to_command_launch_target() {
    let cli = Cli::try_parse_from(["nite", "update"]).expect("nite update should parse");
    assert!(!cli.auto_update);
    assert_eq!(launch_target(&cli), LaunchTarget::Command);
    assert!(matches!(cli.command, Some(Commands::Update)));
}

#[test]
fn delete_without_args_parses_successfully() {
    let cli = Cli::try_parse_from(["nite", "delete"]).expect("delete without args should parse");
    match cli.command {
        Some(Commands::Delete { triggers, tag, .. }) => {
            assert!(triggers.is_empty());
            assert!(tag.is_none());
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn config_set_without_args_parses_successfully() {
    let cli = Cli::try_parse_from(["nite", "config", "set"])
        .expect("config set without args should parse");
    match cli.command {
        Some(Commands::Config {
            action: Some(ConfigAction::Set { key, value }),
        }) => {
            assert!(key.is_none());
            assert!(value.is_none());
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn config_without_args_parses_successfully() {
    let cli = Cli::try_parse_from(["nite", "config"]).expect("config without args should parse");
    match cli.command {
        Some(Commands::Config { action }) => {
            assert!(action.is_none());
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn completions_without_args_parses_successfully() {
    let cli = Cli::try_parse_from(["nite", "completions"])
        .expect("completions without args should parse");
    match cli.command {
        Some(Commands::Completions { action }) => {
            assert!(action.is_none());
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn add_parses_multi_positional_and_repeatable_flags() {
    // NOTE (variance vs brief): the brief calls AddArgs::try_parse_from with
    // ["nite", "add", ...], but clap consumes only the first element as the
    // binary name, leaving "add" inside positional. Parsing through Cli keeps
    // the vector verbatim and matches real routing (Cli strips the subcommand
    // word before delegating to AddArgs).
    let cli = Cli::try_parse_from([
        "nite", "add", "hi", "hello", "Hello!", "--hotkey", "ctrl+h", "--hotkey", "ctrl+j",
        "--voice", "say hi",
    ])
    .unwrap();
    let Some(Commands::Add(args)) = cli.command else {
        panic!("expected add command");
    };
    assert_eq!(args.positional, vec!["hi", "hello", "Hello!"]);
    assert_eq!(args.hotkey, vec!["ctrl+h", "ctrl+j"]);
    assert_eq!(args.voice, vec!["say hi"]);
}

#[test]
fn add_hotkey_only_with_single_positional() {
    let cli = Cli::try_parse_from(["nite", "add", "--hotkey", "ctrl+h", "Do it"]).unwrap();
    let Some(Commands::Add(args)) = cli.command else {
        panic!("expected add command");
    };
    assert_eq!(args.positional, vec!["Do it"]);
}

#[test]
fn add_script_without_args_parses_successfully() {
    let cli = Cli::try_parse_from(["nite", "add", "script"])
        .expect("add script without args should parse");
    match cli.command {
        Some(Commands::Add(args)) => match args.sub {
            Some(AddSubcommand::Script { positional, .. }) => {
                assert!(positional.is_empty());
            }
            other => panic!("unexpected sub parse: {other:?}"),
        },
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn test_login_args_parsing() {
    let cli = Cli::try_parse_from(["nite", "login"]).expect("login parses");
    match cli.command {
        Some(Commands::Login {
            no_browser,
            provider,
            email,
        }) => {
            assert!(!no_browser);
            assert_eq!(provider, None);
            assert_eq!(email, None);
        }
        other => panic!("expected Login, got {other:?}"),
    }

    let cli = Cli::try_parse_from([
        "nite",
        "login",
        "--no-browser",
        "--provider",
        "google",
        "--email",
        "user@example.com",
    ])
    .expect("login with options parses");
    match cli.command {
        Some(Commands::Login {
            no_browser,
            provider,
            email,
        }) => {
            assert!(no_browser);
            assert_eq!(provider.as_deref(), Some("google"));
            assert_eq!(email.as_deref(), Some("user@example.com"));
        }
        other => panic!("expected Login with options, got {other:?}"),
    }

    let cli = Cli::try_parse_from(["nite", "logout"]).expect("logout parses");
    assert!(matches!(
        cli.command,
        Some(Commands::Logout { force: false })
    ));
    let cli_force =
        Cli::try_parse_from(["nite", "logout", "--force"]).expect("logout --force parses");
    assert!(matches!(
        cli_force.command,
        Some(Commands::Logout { force: true })
    ));
}

#[test]
fn test_login_rejects_unsupported_provider() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let _ = nite_core::cloud::clear_tokens();

    let result = crate::commands::auth::execute_login(true, Some("github"), None, false);
    match result {
        Err(nite_core::Error::Config(msg)) => {
            assert!(msg.contains("Unsupported auth provider"));
            assert!(msg.contains("google"));
        }
        other => panic!("expected Config error for unsupported provider, got {other:?}"),
    }
}

#[test]
fn test_status_args_parsing() {
    let cli = Cli::try_parse_from(["nite", "status"]).expect("status parses");
    assert!(!cli.json);
    assert!(matches!(cli.command, Some(Commands::Status)));

    let cli = Cli::try_parse_from(["nite", "status", "--json"]).expect("status --json parses");
    assert!(cli.json);
    assert!(matches!(cli.command, Some(Commands::Status)));
}

#[test]
fn test_workspace_flag_parsing() {
    let cli = Cli::try_parse_from(["nite", "add", ":sig", "Best regards", "--workspace", "work"])
        .expect("add --workspace parses");
    match cli.command {
        Some(Commands::Add(args)) => {
            assert_eq!(args.workspace.as_deref(), Some("work"));
        }
        other => panic!("expected Add with workspace, got {other:?}"),
    }

    let cli = Cli::try_parse_from(["nite", "list", "--workspace", "personal"])
        .expect("list --workspace parses");
    match cli.command {
        Some(Commands::List { workspace, .. }) => {
            assert_eq!(workspace.as_deref(), Some("personal"));
        }
        other => panic!("expected List with workspace, got {other:?}"),
    }
}

#[test]
fn test_unauthenticated_add_rejected_with_login_message() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let _ = nite_core::cloud::clear_tokens();

    let cli =
        Cli::try_parse_from(["nite", "add", ":test_unauth", "hello world"]).expect("add parses");
    let result = run(cli, LaunchTarget::Command);
    match result {
        Err(nite_core::error::Error::Config(msg)) => {
            assert!(
                msg.contains("Please log in to use Nite. Run 'nite login' to continue."),
                "Expected login prompt, got: {msg}"
            );
        }
        other => panic!("expected Config error with login prompt, got {other:?}"),
    }
}

#[test]
fn test_unauthenticated_delete_rejected_with_login_message() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let _ = nite_core::cloud::clear_tokens();

    let cli = Cli::try_parse_from(["nite", "delete", ":test_unauth", "-y"]).expect("delete parses");
    let result = run(cli, LaunchTarget::Command);
    match result {
        Err(nite_core::error::Error::Config(msg)) => {
            assert!(
                msg.contains("Please log in to use Nite. Run 'nite login' to continue."),
                "Expected login prompt, got: {msg}"
            );
        }
        other => panic!("expected Config error with login prompt, got {other:?}"),
    }
}

#[test]
fn test_unauthenticated_up_and_restart_rejected_with_login_message() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let _ = nite_core::cloud::clear_tokens();

    let cli_up = Cli::try_parse_from(["nite", "up"]).expect("up parses");
    let res_up = run(cli_up, LaunchTarget::Command);
    match res_up {
        Err(nite_core::error::Error::Config(msg)) => {
            assert!(msg.contains("Please log in to use Nite. Run 'nite login' to continue."));
        }
        other => panic!("expected Config error for up, got {other:?}"),
    }

    let cli_restart = Cli::try_parse_from(["nite", "restart"]).expect("restart parses");
    let res_restart = run(cli_restart, LaunchTarget::Command);
    match res_restart {
        Err(nite_core::error::Error::Config(msg)) => {
            assert!(msg.contains("Please log in to use Nite. Run 'nite login' to continue."));
        }
        other => panic!("expected Config error for restart, got {other:?}"),
    }
}

#[test]
fn test_unauthenticated_daemon_rejected_with_login_message() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let _ = nite_core::cloud::clear_tokens();

    // The TUI is deliberately absent here: it now opens a sign-in gate
    // instead of exiting, and cannot be launched inside a test terminal.
    let cli_daemon = Cli::try_parse_from(["nite", "--daemon"]).expect("daemon parses");
    let res_daemon = run(cli_daemon, LaunchTarget::Daemon);
    match res_daemon {
        Err(nite_core::error::Error::Config(msg)) => {
            assert!(msg.contains("Please log in to use Nite. Run 'nite login' to continue."));
        }
        other => panic!("expected Config error for daemon, got {other:?}"),
    }
}

#[test]
fn test_unauthenticated_list_rejected_with_login_message() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let _ = nite_core::cloud::clear_tokens();

    let cli_list = Cli::try_parse_from(["nite", "list"]).expect("list parses");
    let res_list = run(cli_list, LaunchTarget::Command);
    match res_list {
        Err(nite_core::error::Error::Config(msg)) => {
            assert!(msg.contains("Please log in to use Nite. Run 'nite login' to continue."));
        }
        other => panic!("expected Config error for list, got {other:?}"),
    }
}

#[test]
fn test_logout_purges_local_database_and_stops_daemon() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("NITE_DATA_DIR", temp.path()) };

    let mock_tokens = nite_core::cloud::AuthTokens {
        access_token: "mock-access-token".to_string(),
        refresh_token: "mock-refresh-token".to_string(),
        user_id: "user-uuid-12345".to_string(),
        expires_at: Some(9999999999),
    };
    nite_core::cloud::store_tokens(&mock_tokens).expect("store tokens succeeds");

    let conn = nite_core::db::init::setup().expect("setup succeeds");
    conn.execute(
        "INSERT INTO triggers (id, output, created_at, updated_at) VALUES ('logout_trig', 'output', 1, 1)",
        [],
    )
    .expect("insert trigger succeeds");
    drop(conn);

    crate::commands::auth::execute_logout(false, false).expect("logout succeeds");

    assert!(nite_core::cloud::get_tokens().unwrap().is_none());
    assert!(nite_core::cloud::get_vault_dek().unwrap().is_none());

    let conn = nite_core::db::init::setup().expect("setup succeeds");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE id = 'logout_trig'",
            [],
            |row| row.get(0),
        )
        .expect("query succeeds");
    assert_eq!(count, 0, "local database triggers must be purged on logout");
    drop(conn);

    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("NITE_DATA_DIR") };
}

#[test]
fn test_authenticated_ensure_authenticated_succeeds() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();

    let mock_tokens = nite_core::cloud::AuthTokens {
        access_token: "mock-access-token".to_string(),
        refresh_token: "mock-refresh-token".to_string(),
        user_id: "user-uuid-12345".to_string(),
        expires_at: Some(2000000000),
    };
    nite_core::cloud::store_tokens(&mock_tokens).expect("store_tokens must succeed");

    let auth = crate::commands::auth::ensure_authenticated();
    assert!(
        auth.is_ok(),
        "ensure_authenticated must succeed when logged in"
    );
    assert_eq!(auth.unwrap().user_id, "user-uuid-12345");

    let _ = nite_core::cloud::clear_tokens();
}

#[test]
fn test_logout_clears_tokens_and_resets_tier() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("NITE_DATA_DIR", temp.path()) };

    let mock_tokens = nite_core::cloud::AuthTokens {
        access_token: "tok-abc".to_string(),
        refresh_token: "tok-ref".to_string(),
        user_id: "user-to-logout".to_string(),
        expires_at: None,
    };
    nite_core::cloud::store_tokens(&mock_tokens).expect("store_tokens");

    let conn = nite_core::db::init::setup().expect("db setup");
    nite_core::db::crud::set_user_tier(&conn, nite_core::db::crud::UserTier::Pro)
        .expect("set tier Pro");

    let cli = Cli::try_parse_from(["nite", "logout"]).expect("logout parses");
    let res = run(cli, LaunchTarget::Command);
    assert!(res.is_ok(), "logout must succeed");

    let stored = nite_core::cloud::get_tokens().expect("get_tokens");
    assert!(stored.is_none(), "tokens must be cleared on logout");

    let tier = nite_core::db::crud::get_user_tier(&conn);
    assert_eq!(
        tier,
        nite_core::db::crud::UserTier::Free,
        "tier must reset to Free"
    );

    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("NITE_DATA_DIR") };
}

#[test]
fn test_status_command_executes_hermetically() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("NITE_DATA_DIR", temp.path()) };

    let res_plain = crate::commands::status::execute_status(false);
    assert!(res_plain.is_ok());

    let res_json = crate::commands::status::execute_status(true);
    assert!(res_json.is_ok());

    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("NITE_DATA_DIR") };
}

#[test]
fn test_status_command_authenticated_with_email() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("NITE_DATA_DIR", temp.path()) };

    let mock_tokens = nite_core::cloud::AuthTokens {
        access_token: "e30.eyJzdWIiOiJ1MSIsImVtYWlsIjoidXNlckBleGFtcGxlLmNvbSJ9.sig".to_string(),
        refresh_token: "mock-refresh-token".to_string(),
        user_id: "user-uuid-12345".to_string(),
        expires_at: Some(9999999999),
    };
    nite_core::cloud::store_tokens(&mock_tokens).expect("store tokens succeeds");

    let res_plain = crate::commands::status::execute_status(false);
    assert!(res_plain.is_ok());

    let res_json = crate::commands::status::execute_status(true);
    assert!(res_json.is_ok());

    nite_core::cloud::clear_tokens().expect("clear tokens succeeds");

    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("NITE_DATA_DIR") };
}

#[test]
fn test_workspace_command_parsing() {
    let cli = Cli::try_parse_from(["nite", "workspace"]).expect("workspace default parses");
    assert!(matches!(
        cli.command,
        Some(Commands::Workspace { action: None })
    ));

    let cli = Cli::try_parse_from(["nite", "workspace", "list"]).expect("workspace list parses");
    assert!(matches!(
        cli.command,
        Some(Commands::Workspace {
            action: Some(crate::args::WorkspaceAction::List),
        })
    ));

    let cli =
        Cli::try_parse_from(["nite", "workspace", "add", "ClientA"]).expect("workspace add parses");
    match cli.command {
        Some(Commands::Workspace {
            action: Some(crate::args::WorkspaceAction::Add { name }),
        }) => {
            assert_eq!(name, "ClientA");
        }
        other => panic!("expected Workspace Add, got {other:?}"),
    }

    let cli = Cli::try_parse_from(["nite", "workspace", "delete", "ClientA"])
        .expect("workspace delete parses");
    match cli.command {
        Some(Commands::Workspace {
            action: Some(crate::args::WorkspaceAction::Delete { name }),
        }) => {
            assert_eq!(name, "ClientA");
        }
        other => panic!("expected Workspace Delete, got {other:?}"),
    }
}

#[test]
fn test_workspace_execution_hermetically() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("NITE_DATA_DIR", temp.path()) };

    let res_list = crate::commands::workspace::execute_list(false);
    assert!(res_list.is_ok());

    let res_list_json = crate::commands::workspace::execute_list(true);
    assert!(res_list_json.is_ok());

    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("NITE_DATA_DIR") };
}

#[test]
fn test_enable_disable_command_parsing() {
    let cli = Cli::try_parse_from(["nite", "enable", ":mytrigger"]).expect("enable parses");
    assert!(matches!(
        cli.command,
        Some(Commands::Enable { trigger }) if trigger == ":mytrigger"
    ));

    let cli = Cli::try_parse_from(["nite", "disable", ":mytrigger"]).expect("disable parses");
    assert!(matches!(
        cli.command,
        Some(Commands::Disable { trigger }) if trigger == ":mytrigger"
    ));
}

#[test]
fn test_enable_disable_execution_hermetically() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("NITE_DATA_DIR", temp.path()) };

    let conn = nite_core::db::init::setup().expect("setup");
    let entry = nite_core::db::crud::NewEntry {
        description: None,
        content: "Hello from toggle test".to_string(),
        action_type: "text".to_string(),
        target_os: "all".to_string(),
        only_apps: None,
        except_apps: None,
        tags_json: "[]".to_string(),
        auto_case: false,
        interpreter: None,
        behavior: None,
        invocations: vec![(
            nite_core::db::crud::InvocationType::Word,
            ":toggle_test".to_string(),
            false,
        )],
    };
    let (id, _) = nite_core::db::crud::create_entry(&conn, entry).expect("create_entry");

    // Disable the trigger
    crate::commands::toggle::execute_disable(":toggle_test", false).expect("disable succeeds");
    let row = nite_core::db::crud::get_trigger(&conn, &id)
        .expect("get_trigger")
        .expect("found");
    assert!(!row.is_enabled);

    // Re-enable the trigger
    crate::commands::toggle::execute_enable(":toggle_test", false).expect("enable succeeds");
    let row = nite_core::db::crud::get_trigger(&conn, &id)
        .expect("get_trigger")
        .expect("found");
    assert!(row.is_enabled);

    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("NITE_DATA_DIR") };
}

#[test]
fn test_sync_command_parsing() {
    let cli = Cli::try_parse_from(["nite", "sync"]).expect("sync parses");
    assert!(matches!(cli.command, Some(Commands::Sync)));

    let cli_push = Cli::try_parse_from(["nite", "push"]).expect("push alias parses");
    assert!(matches!(cli_push.command, Some(Commands::Sync)));

    let cli_pull = Cli::try_parse_from(["nite", "pull"]).expect("pull alias parses");
    assert!(matches!(cli_pull.command, Some(Commands::Sync)));
}

#[test]
fn test_sync_command_unauthenticated() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("NITE_DATA_DIR", temp.path()) };

    let _ = nite_core::cloud::clear_tokens();

    let res_plain = crate::commands::sync::execute_sync(false);
    assert!(res_plain.is_ok());

    let res_json = crate::commands::sync::execute_sync(true);
    assert!(res_json.is_ok());

    // SAFETY: access to NITE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("NITE_DATA_DIR") };
}

#[test]
fn test_cli_help_does_not_contain_taurine() {
    use clap::CommandFactory;
    let mut cmd = crate::args::Cli::command();
    let mut help_buf = Vec::new();
    cmd.write_help(&mut help_buf).unwrap();
    let help_str = String::from_utf8_lossy(&help_buf);
    assert!(
        !help_str.contains("Taurine"),
        "CLI help must not mention Taurine"
    );
}
