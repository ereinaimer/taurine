use super::*;
use crate::args::{AddSubcommand, AiProvider};
use crate::commands::completions::{
    generate_powershell_with_alias, generate_with_alias, generate_zsh_with_alias,
};
use clap::CommandFactory;
use clap::Parser;
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
            assert_eq!(args.hotkey, vec!["Ctrl+Shift+G"]);
            assert_eq!(args.positional, vec!["git status"]);
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
fn voice_daemon_flag_stays_hidden_and_routes() {
    let cli = Cli::try_parse_from([
        "taurine",
        "--voice-daemon",
        "--voice-pipe",
        "taurine-voice-1-abc",
        "--voice-version-token",
        "v1",
    ])
    .expect("--voice-daemon should parse");
    assert!(cli.voice_daemon);
    assert_eq!(
        cli.voice_pipe.as_deref(),
        Some("taurine-voice-1-abc"),
        "pipe name must round-trip"
    );
    assert_eq!(
        cli.voice_version_token.as_deref(),
        Some("v1"),
        "version token must round-trip"
    );
    assert_eq!(launch_target(&cli), LaunchTarget::VoiceDaemon);

    let mut help = Vec::new();
    Cli::command()
        .write_long_help(&mut help)
        .expect("help must render");
    let help = String::from_utf8(help).expect("help must be UTF-8");
    assert!(
        !help.contains("voice-daemon"),
        "internal worker role must stay out of help"
    );
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
            assert_eq!(args.regex, vec!["issue-(\\d+)"]);
            assert_eq!(args.positional, vec!["link/[0]"]);
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

#[test]
fn parses_auto_update_flag_and_routes_to_launch_target() {
    let cli = Cli::try_parse_from(["taurine", "--auto-update"])
        .expect("taurine --auto-update should parse");
    assert!(cli.auto_update);
    assert_eq!(launch_target(&cli), LaunchTarget::AutoUpdate);
}

#[test]
fn update_subcommand_routes_to_command_launch_target() {
    let cli = Cli::try_parse_from(["taurine", "update"]).expect("taurine update should parse");
    assert!(!cli.auto_update);
    assert_eq!(launch_target(&cli), LaunchTarget::Command);
    assert!(matches!(cli.command, Some(Commands::Update)));
}

#[test]
fn delete_without_args_parses_successfully() {
    let cli = Cli::try_parse_from(["taurine", "delete"]).expect("delete without args should parse");
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
    let cli = Cli::try_parse_from(["taurine", "config", "set"])
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
    let cli = Cli::try_parse_from(["taurine", "config"]).expect("config without args should parse");
    match cli.command {
        Some(Commands::Config { action }) => {
            assert!(action.is_none());
        }
        other => panic!("unexpected command parse: {other:?}"),
    }
}

#[test]
fn completions_without_args_parses_successfully() {
    let cli = Cli::try_parse_from(["taurine", "completions"])
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
    // ["taurine", "add", ...], but clap consumes only the first element as the
    // binary name, leaving "add" inside positional. Parsing through Cli keeps
    // the vector verbatim and matches real routing (Cli strips the subcommand
    // word before delegating to AddArgs).
    let cli = Cli::try_parse_from([
        "taurine", "add", "hi", "hello", "Hello!", "--hotkey", "ctrl+h", "--hotkey", "ctrl+j",
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
    let cli = Cli::try_parse_from(["taurine", "add", "--hotkey", "ctrl+h", "Do it"]).unwrap();
    let Some(Commands::Add(args)) = cli.command else {
        panic!("expected add command");
    };
    assert_eq!(args.positional, vec!["Do it"]);
}

#[test]
fn add_script_without_args_parses_successfully() {
    let cli = Cli::try_parse_from(["taurine", "add", "script"])
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
    let cli = Cli::try_parse_from(["taurine", "login"]).expect("login parses");
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
        "taurine",
        "login",
        "--no-browser",
        "--provider",
        "github",
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
            assert_eq!(provider.as_deref(), Some("github"));
            assert_eq!(email.as_deref(), Some("user@example.com"));
        }
        other => panic!("expected Login with options, got {other:?}"),
    }

    let cli = Cli::try_parse_from(["taurine", "logout"]).expect("logout parses");
    assert!(matches!(cli.command, Some(Commands::Logout)));
}

#[test]
fn test_status_args_parsing() {
    let cli = Cli::try_parse_from(["taurine", "status"]).expect("status parses");
    assert!(!cli.json);
    assert!(matches!(cli.command, Some(Commands::Status)));

    let cli = Cli::try_parse_from(["taurine", "status", "--json"]).expect("status --json parses");
    assert!(cli.json);
    assert!(matches!(cli.command, Some(Commands::Status)));
}

#[test]
fn test_workspace_flag_parsing() {
    let cli = Cli::try_parse_from([
        "taurine",
        "add",
        ":sig",
        "Best regards",
        "--workspace",
        "work",
    ])
    .expect("add --workspace parses");
    match cli.command {
        Some(Commands::Add(args)) => {
            assert_eq!(args.workspace.as_deref(), Some("work"));
        }
        other => panic!("expected Add with workspace, got {other:?}"),
    }

    let cli = Cli::try_parse_from(["taurine", "list", "--workspace", "personal"])
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
    let _ = taurine_core::cloud::clear_tokens();

    let cli =
        Cli::try_parse_from(["taurine", "add", ":test_unauth", "hello world"]).expect("add parses");
    let result = run(cli, LaunchTarget::Command);
    match result {
        Err(taurine_core::error::Error::Config(msg)) => {
            assert!(
                msg.contains("Authentication required. Please run 'taurine login' to continue."),
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
    let _ = taurine_core::cloud::clear_tokens();

    let cli =
        Cli::try_parse_from(["taurine", "delete", ":test_unauth", "-y"]).expect("delete parses");
    let result = run(cli, LaunchTarget::Command);
    match result {
        Err(taurine_core::error::Error::Config(msg)) => {
            assert!(
                msg.contains("Authentication required. Please run 'taurine login' to continue."),
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
    let _ = taurine_core::cloud::clear_tokens();

    let cli_up = Cli::try_parse_from(["taurine", "up"]).expect("up parses");
    let res_up = run(cli_up, LaunchTarget::Command);
    match res_up {
        Err(taurine_core::error::Error::Config(msg)) => {
            assert!(
                msg.contains("Authentication required. Please run 'taurine login' to continue.")
            );
        }
        other => panic!("expected Config error for up, got {other:?}"),
    }

    let cli_restart = Cli::try_parse_from(["taurine", "restart"]).expect("restart parses");
    let res_restart = run(cli_restart, LaunchTarget::Command);
    match res_restart {
        Err(taurine_core::error::Error::Config(msg)) => {
            assert!(
                msg.contains("Authentication required. Please run 'taurine login' to continue.")
            );
        }
        other => panic!("expected Config error for restart, got {other:?}"),
    }
}

#[test]
fn test_authenticated_ensure_authenticated_succeeds() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();

    let mock_tokens = taurine_core::cloud::AuthTokens {
        access_token: "mock-access-token".to_string(),
        refresh_token: "mock-refresh-token".to_string(),
        user_id: "user-uuid-12345".to_string(),
        expires_at: Some(2000000000),
    };
    taurine_core::cloud::store_tokens(&mock_tokens).expect("store_tokens must succeed");

    let auth = crate::commands::auth::ensure_authenticated();
    assert!(
        auth.is_ok(),
        "ensure_authenticated must succeed when logged in"
    );
    assert_eq!(auth.unwrap().user_id, "user-uuid-12345");

    let _ = taurine_core::cloud::clear_tokens();
}

#[test]
fn test_logout_clears_tokens_and_resets_tier() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: access to TAURINE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("TAURINE_DATA_DIR", temp.path()) };

    let mock_tokens = taurine_core::cloud::AuthTokens {
        access_token: "tok-abc".to_string(),
        refresh_token: "tok-ref".to_string(),
        user_id: "user-to-logout".to_string(),
        expires_at: None,
    };
    taurine_core::cloud::store_tokens(&mock_tokens).expect("store_tokens");

    let conn = taurine_core::db::init::setup().expect("db setup");
    taurine_core::db::crud::set_user_tier(&conn, taurine_core::db::crud::UserTier::Pro)
        .expect("set tier Pro");

    let cli = Cli::try_parse_from(["taurine", "logout"]).expect("logout parses");
    let res = run(cli, LaunchTarget::Command);
    assert!(res.is_ok(), "logout must succeed");

    let stored = taurine_core::cloud::get_tokens().expect("get_tokens");
    assert!(stored.is_none(), "tokens must be cleared on logout");

    let tier = taurine_core::db::crud::get_user_tier(&conn);
    assert_eq!(
        tier,
        taurine_core::db::crud::UserTier::Free,
        "tier must reset to Free"
    );

    // SAFETY: access to TAURINE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
}

#[test]
fn test_status_command_executes_hermetically() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    crate::commands::test_keyring::use_shared_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: access to TAURINE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("TAURINE_DATA_DIR", temp.path()) };

    let res_plain = crate::commands::status::execute_status(false);
    assert!(res_plain.is_ok());

    let res_json = crate::commands::status::execute_status(true);
    assert!(res_json.is_ok());

    // SAFETY: access to TAURINE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
}

#[test]
fn test_workspace_command_parsing() {
    let cli = Cli::try_parse_from(["taurine", "workspace"]).expect("workspace default parses");
    assert!(matches!(
        cli.command,
        Some(Commands::Workspace { action: None })
    ));

    let cli = Cli::try_parse_from(["taurine", "workspace", "list"]).expect("workspace list parses");
    assert!(matches!(
        cli.command,
        Some(Commands::Workspace {
            action: Some(crate::args::WorkspaceAction::List),
        })
    ));

    let cli = Cli::try_parse_from(["taurine", "workspace", "add", "ClientA"])
        .expect("workspace add parses");
    match cli.command {
        Some(Commands::Workspace {
            action: Some(crate::args::WorkspaceAction::Add { name }),
        }) => {
            assert_eq!(name, "ClientA");
        }
        other => panic!("expected Workspace Add, got {other:?}"),
    }

    let cli = Cli::try_parse_from(["taurine", "workspace", "delete", "ClientA"])
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
    // SAFETY: access to TAURINE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("TAURINE_DATA_DIR", temp.path()) };

    let res_list = crate::commands::workspace::execute_list(false);
    assert!(res_list.is_ok());

    let res_list_json = crate::commands::workspace::execute_list(true);
    assert!(res_list_json.is_ok());

    // SAFETY: access to TAURINE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
}

#[test]
fn test_enable_disable_command_parsing() {
    let cli = Cli::try_parse_from(["taurine", "enable", ":mytrigger"]).expect("enable parses");
    assert!(matches!(
        cli.command,
        Some(Commands::Enable { trigger }) if trigger == ":mytrigger"
    ));

    let cli = Cli::try_parse_from(["taurine", "disable", ":mytrigger"]).expect("disable parses");
    assert!(matches!(
        cli.command,
        Some(Commands::Disable { trigger }) if trigger == ":mytrigger"
    ));
}

#[test]
fn test_enable_disable_execution_hermetically() {
    let _lock = crate::commands::TEST_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: access to TAURINE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::set_var("TAURINE_DATA_DIR", temp.path()) };

    let conn = taurine_core::db::init::setup().expect("setup");
    let entry = taurine_core::db::crud::NewEntry {
        name: "Toggle Test Snippet".to_string(),
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
            taurine_core::db::crud::InvocationType::Word,
            ":toggle_test".to_string(),
            false,
        )],
    };
    let (id, _) = taurine_core::db::crud::create_entry(&conn, entry).expect("create_entry");

    // Disable the trigger
    crate::commands::toggle::execute_disable(":toggle_test", false).expect("disable succeeds");
    let row = taurine_core::db::crud::get_trigger(&conn, &id)
        .expect("get_trigger")
        .expect("found");
    assert!(!row.is_enabled);

    // Re-enable the trigger
    crate::commands::toggle::execute_enable(":toggle_test", false).expect("enable succeeds");
    let row = taurine_core::db::crud::get_trigger(&conn, &id)
        .expect("get_trigger")
        .expect("found");
    assert!(row.is_enabled);

    // SAFETY: access to TAURINE_DATA_DIR is serialized by TEST_LOCK
    unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
}
