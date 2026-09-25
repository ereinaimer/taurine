use crate::args::{AddArgs, AddSubcommand};
use crate::commands::add::{existing_entry_defaults, resolve_display};
use crate::commands::validate::format_trigger_log;
use std::fs;
use std::path::PathBuf;
use taurine_core::db::crud::{
    AddOutcome, InvocationType, NewEntry, TriggerAliasRow, TriggerType, audit_script_payload_tags,
    upsert_entry_full,
};
use taurine_core::db::init;
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

/// Maximum invocations (word + typed) per add command (§0.14).
const MAX_INVOCATIONS: usize = 20;

pub fn execute_args(args: AddArgs, json: bool) -> taurine_core::error::Result<()> {
    let AddSubcommand::Script {
        positional,
        hotkey,
        regex,
        voice,
        yes,
        file,
        lang,
        mode,
        os,
        include_apps,
        exclude_apps,
        tag,
        name,
        description,
        auto_case,
    } = args
        .sub
        .expect("add dispatch routes to script only when subcommand is present");

    let typed_total = hotkey.len() + regex.len() + voice.len();
    let (words, content) = if let Some(ref path) = file {
        if positional.is_empty() && typed_total == 0 {
            let diag = taurine_core::diagnostic::Diagnostic::problem("Missing trigger for script")
                .help("Specify a trigger word or hotkey for the script:")
                .example("taurine add script :run -f ./myscript.sh")
                .example("taurine add script :greet \"echo hello\"");
            return Err(taurine_core::error::Error::Config(diag.render()));
        }
        // With --file every positional is a word trigger; content comes from the file.
        (positional, read_script_file(path)?)
    } else {
        match positional.len() {
            0 => {
                let diag =
                    taurine_core::diagnostic::Diagnostic::problem("Missing trigger for script")
                        .help("Specify a trigger word or hotkey for the script:")
                        .example("taurine add script :run -f ./myscript.sh")
                        .example("taurine add script :greet \"echo hello\"");
                return Err(taurine_core::error::Error::Config(diag.render()));
            }
            1 if typed_total == 0 => {
                let diag = taurine_core::diagnostic::Diagnostic::problem("no trigger specified")
                    .help("Specify at least one trigger plus the script content:")
                    .example("taurine add script :greet \"echo hello\"");
                return Err(taurine_core::error::Error::Config(diag.render()));
            }
            n => (positional[..n - 1].to_vec(), positional[n - 1].clone()),
        }
    };

    let words: Vec<String> = if auto_case {
        words.into_iter().map(|w| w.to_lowercase()).collect()
    } else {
        words
    };
    let mut invocations: Vec<(InvocationType, String, bool)> = Vec::new();
    invocations.extend(words.into_iter().map(|w| (InvocationType::Word, w, false)));
    invocations.extend(
        hotkey
            .into_iter()
            .map(|h| (InvocationType::Hotkey, h, false)),
    );
    invocations.extend(regex.into_iter().map(|r| (InvocationType::Regex, r, false)));
    // Voice aliases on a script entry confirm before execution unless -y/--yes.
    invocations.extend(voice.into_iter().map(|v| (InvocationType::Voice, v, !yes)));
    if invocations.len() > MAX_INVOCATIONS {
        return Err(taurine_core::Error::Config(format!(
            "Too many invocations ({}); maximum is {} per add",
            invocations.len(),
            MAX_INVOCATIONS
        )));
    }

    let conn = init::setup()?;
    let os = os
        .to_db_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| taurine_core::db::get_current_os_db_string().to_string());

    let interpreter = resolve_interpreter(lang.map(Into::into), file.as_deref(), &content)?;
    let behavior: ScriptBehavior = mode.into();

    let audit_type = if invocations
        .iter()
        .any(|(t, _, _)| *t == InvocationType::Regex)
    {
        TriggerType::Regex
    } else {
        TriggerType::Word
    };
    audit_script_payload_tags(&content, audit_type)?;

    // R2: reuse the existing entry's name/description/tags for absent flags.
    let (reuse_name, reuse_description, reuse_tags) = existing_entry_defaults(
        &conn,
        &invocations,
        &os,
        include_apps.as_deref(),
        exclude_apps.as_deref(),
    )?;
    let name = name.unwrap_or(reuse_name);
    let description = description.or(reuse_description);
    let tags_json = match tag {
        Some(tags) => {
            serde_json::to_string(&tags).map_err(|e| taurine_core::Error::Config(e.to_string()))?
        }
        None => reuse_tags,
    };

    let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
    if !settings.scripts_enabled {
        tracing::warn!(
            "Warning: Global script execution is currently disabled. This script trigger will not trigger until `scripts_enabled` is set to true."
        );
    }

    let outcome = upsert_entry_full(
        &conn,
        NewEntry {
            name: name.clone(),
            description,
            content,
            action_type: "script".to_string(),
            target_os: os.clone(),
            only_apps: include_apps.clone(),
            except_apps: exclude_apps.clone(),
            tags_json,
            auto_case,
            interpreter: Some(interpreter),
            behavior: Some(behavior),
            invocations: invocations.clone(),
        },
    )?;

    let (display, aliases) = resolve_display(&conn, &name, &invocations);
    report_outcome(
        outcome,
        &display,
        &aliases,
        (behavior, interpreter),
        &os,
        include_apps.as_deref(),
        exclude_apps.as_deref(),
        json,
    );
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub fn execute(
    trigger: String,
    use_hotkey: bool,
    content: Option<String>,
    file_path: Option<PathBuf>,
    lang: Option<ScriptInterpreter>,
    mode: ScriptBehavior,
    os: String,
    include_apps: Option<String>,
    exclude_apps: Option<String>,
) -> taurine_core::error::Result<()> {
    let (words, content) = if let Some(ref path) = file_path {
        (vec![trigger], read_script_file(path)?)
    } else if let Some(text) = content {
        (vec![trigger], text)
    } else {
        let diag = taurine_core::diagnostic::Diagnostic::problem(
            "Neither script content nor script file provided",
        )
        .help("Provide inline script content or specify a script file using -f / --file:")
        .example("taurine add script :run -f ./myscript.sh");
        return Err(taurine_core::error::Error::Service(diag.render()));
    };

    let invocation_type = if use_hotkey {
        InvocationType::Hotkey
    } else {
        InvocationType::Word
    };
    let conn = init::setup()?;
    let invocations = vec![(invocation_type, words.into_iter().next().unwrap(), false)];
    let interpreter = resolve_interpreter(lang, file_path.as_deref(), &content)?;
    audit_script_payload_tags(
        &content,
        if use_hotkey {
            TriggerType::Hotkey
        } else {
            TriggerType::Word
        },
    )?;
    let (name, description, tags_json) = existing_entry_defaults(
        &conn,
        &invocations,
        &os,
        include_apps.as_deref(),
        exclude_apps.as_deref(),
    )?;

    let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
    if !settings.scripts_enabled {
        tracing::warn!(
            "Warning: Global script execution is currently disabled. This script trigger will not trigger until `scripts_enabled` is set to true."
        );
    }

    let outcome = upsert_entry_full(
        &conn,
        NewEntry {
            name: name.clone(),
            description,
            content,
            action_type: "script".to_string(),
            target_os: os.clone(),
            only_apps: include_apps.clone(),
            except_apps: exclude_apps.clone(),
            tags_json,
            auto_case: false,
            interpreter: Some(interpreter),
            behavior: Some(mode),
            invocations: invocations.clone(),
        },
    )?;

    let (display, aliases) = resolve_display(&conn, &name, &invocations);
    report_outcome(
        outcome,
        &display,
        &aliases,
        (mode, interpreter),
        &os,
        include_apps.as_deref(),
        exclude_apps.as_deref(),
        false,
    );
    Ok(())
}

/// Compatibility for the pre-alias call shape (single trigger + explicit
/// type); forwards to `execute`. Kept so existing callers keep compiling.
#[allow(clippy::too_many_arguments)]
pub fn execute_with_trigger_type(
    trigger: String,
    trigger_type: TriggerType,
    content: Option<String>,
    file_path: Option<PathBuf>,
    lang: Option<ScriptInterpreter>,
    mode: ScriptBehavior,
    os: String,
    include_apps: Option<String>,
    exclude_apps: Option<String>,
    _tags: Option<Vec<String>>,
    _name: Option<String>,
    _description: Option<String>,
    auto_case: bool,
    _json: bool,
) -> taurine_core::error::Result<()> {
    let trigger = if auto_case && !matches!(trigger_type, TriggerType::Regex) {
        trigger.to_lowercase()
    } else {
        trigger
    };
    execute(
        trigger,
        matches!(trigger_type, TriggerType::Hotkey),
        content,
        file_path,
        lang,
        mode,
        os,
        include_apps,
        exclude_apps,
    )
}

fn read_script_file(path: &PathBuf) -> taurine_core::error::Result<String> {
    if !path.exists() {
        let diag = taurine_core::diagnostic::Diagnostic::problem(format!(
            "Script file does not exist: {}",
            path.display()
        ))
        .help("Verify that the file path is correct and accessible.")
        .example("taurine add script -f ./scripts/deploy.sh :deploy");
        return Err(taurine_core::error::Error::NotFound(diag.render()));
    }
    fs::read_to_string(path).map_err(|e| {
        taurine_core::error::Error::Service(format!("Failed to read script file: {}", e))
    })
}

fn resolve_interpreter(
    lang: Option<ScriptInterpreter>,
    path: Option<&std::path::Path>,
    content: &str,
) -> taurine_core::error::Result<ScriptInterpreter> {
    match lang {
        Some(i) => Ok(i),
        None => infer_interpreter(path, content).ok_or_else(|| {
            let diag = taurine_core::diagnostic::Diagnostic::problem(
                "Could not infer script language from content or file extension",
            )
            .help("Specify the interpreter explicitly using --lang:")
            .options(
                "Supported languages",
                &["bash", "powershell", "python", "node", "cmd"],
            )
            .example("taurine add script :py \"print(1)\" --lang python");
            taurine_core::error::Error::Service(diag.render())
        }),
    }
}

/// R2 preload + display lookup live in add.rs (shared, single copy).
#[allow(clippy::too_many_arguments)]
fn report_outcome(
    outcome: AddOutcome,
    display: &str,
    aliases: &[TriggerAliasRow],
    script_info: (ScriptBehavior, ScriptInterpreter),
    os: &str,
    include_apps: Option<&str>,
    exclude_apps: Option<&str>,
    json: bool,
) {
    let invocations: Vec<serde_json::Value> = aliases
        .iter()
        .map(|a| {
            serde_json::json!({"type": a.invocation_type.as_db_str(), "invocation": a.invocation})
        })
        .collect();
    let (action, status) = match outcome {
        AddOutcome::Created => ("Added", "created"),
        AddOutcome::AlreadyExists => ("Trigger already exists for", "exists"),
        AddOutcome::Updated => ("Updated", "updated"),
    };
    let log_msg = format_trigger_log(
        action,
        display,
        Some(script_info),
        os,
        include_apps,
        exclude_apps,
    );
    tracing::info!("{}", log_msg);
    if matches!(outcome, AddOutcome::Created | AddOutcome::Updated) {
        taurine_core::rpc::notify_daemon_reload();
    }
    if json {
        println!(
            "{}",
            serde_json::json!({"status": status, "trigger": display, "action_type": "script", "invocations": invocations})
        );
    }
}

pub(crate) fn infer_interpreter(
    path: Option<&std::path::Path>,
    content: &str,
) -> Option<ScriptInterpreter> {
    taurine_core::engine::shell::infer_interpreter(path, content)
}
