// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

use rusqlite::Connection;
use taurine_core::db::crud::{
    InvocationType, NewEntry, create_entry, delete_alias, find_parent_by_invocation, get_trigger,
    increment_usage_count_by_id, list_active_voice_invocations, threshold_for_phrase,
    validate_voice_phrase,
};
use taurine_core::db::crud::{record_voice_dictation_usage, record_voice_trigger_usage};
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};
use taurine_core::voice::{
    GateDecision, GateWitnesses, VoiceDictionary, evaluate_gate, format_transcript,
    get_model_entry, get_system_ram_gb, resolve_model_alias,
};

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("failed to open in-memory sqlite db");
    taurine_core::db::init::migrate::run_migrations(&conn).expect("run_migrations failed");
    taurine_core::db::init::seed::ensure_defaults(&conn).expect("ensure_defaults failed");
    conn
}

fn voice_entry(
    content: &str,
    action_type: &str,
    phrase: &str,
    require_confirmation: bool,
) -> NewEntry {
    let (interpreter, behavior) = if action_type == "script" {
        (Some(ScriptInterpreter::Bash), Some(ScriptBehavior::Silent))
    } else {
        (None, None)
    };
    NewEntry {
        name: String::new(),
        description: None,
        content: content.to_string(),
        action_type: action_type.to_string(),
        target_os: "all".to_string(),
        only_apps: None,
        except_apps: None,
        tags_json: "[]".to_string(),
        auto_case: false,
        interpreter,
        behavior,
        invocations: vec![(
            InvocationType::Voice,
            phrase.to_string(),
            require_confirmation,
        )],
    }
}

fn active_voice_invocation(
    conn: &Connection,
    phrase: &str,
) -> taurine_core::db::crud::ResolvedInvocation {
    list_active_voice_invocations(conn)
        .unwrap()
        .into_iter()
        .find(|inv| inv.invocation == phrase)
        .unwrap_or_else(|| panic!("voice invocation '{phrase}' should be active"))
}

#[test]
fn test_voice_models_catalog_and_ram_tier() {
    let unified = get_model_entry("parakeet-unified-en-0.6b").expect("unified must be in catalog");
    assert_eq!(unified.id, "parakeet-unified-en-0.6b");
    assert!(!unified.is_archive);
    assert!(unified.remote_files.is_some());
    let light = get_model_entry("parakeet-tdt-ctc-110m").expect("110m must be in catalog");
    assert_eq!(light.id, "parakeet-tdt-ctc-110m");
    assert!(light.is_archive);
    assert_eq!(get_model_entry("silero_vad_v6"), None);
    assert_eq!(get_model_entry("kws-zipformer-zh-en-3M"), None);
    assert_eq!(get_model_entry("whisper-small-en"), None);
    assert_eq!(get_model_entry("parakeet-tdt-0.6b-v3"), None);
    assert_eq!(get_model_entry("moonshine-base-en"), None);
    assert_eq!(get_model_entry("moonshine-tiny-en"), None);
    // Strict canonical names: shorthand aliases are rejected.
    assert_eq!(get_model_entry("110m"), None);
    assert_eq!(get_model_entry("unified"), None);
    assert_eq!(get_model_entry("best"), None);
    assert_eq!(get_model_entry("fast"), None);
    assert_eq!(
        resolve_model_alias("parakeet-unified-en-0.6b"),
        "parakeet-unified-en-0.6b"
    );
    assert_eq!(
        resolve_model_alias("parakeet-tdt-ctc-110m"),
        "parakeet-tdt-ctc-110m"
    );
    assert!(get_system_ram_gb() >= 4);
}

#[test]
fn test_voice_dictionary_pipeline() {
    let dict = VoiceDictionary::from_csv("Taurine, PostgreSQL, Kubernetes, API, GraphQL");

    // Exact match case transformation
    assert_eq!(
        dict.apply("welcome to taurine and postgresql"),
        "welcome to Taurine and PostgreSQL"
    );

    // Fuzzy matching for slight phonetic/STT misspellings
    assert_eq!(
        dict.apply("deploy the app to kubernets cluster"),
        "deploy the app to Kubernetes cluster"
    );

    // Multibyte Unicode punctuation handling
    assert_eq!(dict.apply("—taurine—"), "—Taurine—");
    assert_eq!(dict.apply("“graphql”"), "“GraphQL”");
    assert_eq!(dict.apply("(api)"), "(API)");

    // Empty dictionary pass-through
    let empty_dict = VoiceDictionary::from_csv("");
    assert_eq!(empty_dict.apply("some raw words"), "some raw words");
}

#[test]
fn test_voice_formatting_pipeline() {
    // Spoken punctuation
    let raw = "hello world period how are you today question mark new line i'm doing fine exclamation point";
    let formatted = format_transcript(raw);
    assert_eq!(
        formatted,
        "Hello world. How are you today?\nI'm doing fine!"
    );

    // Disfluencies / filler words removal
    let with_fillers = "um so uh we are ah going to win period";
    assert_eq!(format_transcript(with_fillers), "So we are going to win.");

    // Personal pronouns and contractions capitalization
    let pronouns = "when i arrived i've seen what i'll do and i'd like it";
    assert_eq!(
        format_transcript(pronouns),
        "When I arrived I've seen what I'll do and I'd like it"
    );

    // Spacing normalization
    let messy_spaces = "test   ,  another  .  word  ?";
    assert_eq!(format_transcript(messy_spaces), "Test, another. Word?");
}

#[test]
fn test_voice_gate_three_witnesses_decisions() {
    let conn = setup_test_db();
    create_entry(
        &conn,
        voice_entry(
            "kubectl apply -f prod.yaml",
            "script",
            "deploy production",
            true, // requires confirmation
        ),
    )
    .unwrap();
    let row = active_voice_invocation(&conn, "deploy production");

    // 1. All witnesses agree, but requires confirmation -> AskConfirm
    let witnesses_agree = GateWitnesses {
        vad_confidence: 0.95,
        kws_phrase: "deploy production",
        kws_confidence: 0.90,
        verifier_transcript: "deploy production",
    };
    let decision = evaluate_gate(&witnesses_agree, &row);
    assert_eq!(decision, GateDecision::AskConfirm(row.clone()));

    // 2. Immediate fire trigger when confirmation is disabled
    create_entry(
        &conn,
        voice_entry("Best regards,\nAlice", "text", "paste signature", false),
    )
    .unwrap();
    let row_no_confirm = active_voice_invocation(&conn, "paste signature");

    let sig_witnesses = GateWitnesses {
        vad_confidence: 0.92,
        kws_phrase: "paste signature",
        kws_confidence: 0.88,
        verifier_transcript: "paste signature",
    };
    let fire_decision = evaluate_gate(&sig_witnesses, &row_no_confirm);
    assert_eq!(fire_decision, GateDecision::Fire(row_no_confirm));

    // 3. VAD confidence too low -> Drop
    let low_vad = GateWitnesses {
        vad_confidence: 0.40,
        kws_phrase: "deploy production",
        kws_confidence: 0.90,
        verifier_transcript: "deploy production",
    };
    assert!(matches!(
        evaluate_gate(&low_vad, &row),
        GateDecision::Drop { .. }
    ));

    // 4. KWS confidence below trigger threshold -> Drop
    let low_kws = GateWitnesses {
        vad_confidence: 0.90,
        kws_phrase: "deploy production",
        kws_confidence: 0.50,
        verifier_transcript: "deploy production",
    };
    assert!(matches!(
        evaluate_gate(&low_kws, &row),
        GateDecision::Drop { .. }
    ));

    // 5. Verifier transcript diverges -> Drop
    let diverged = GateWitnesses {
        vad_confidence: 0.90,
        kws_phrase: "deploy production",
        kws_confidence: 0.90,
        verifier_transcript: "cancel operations",
    };
    assert!(matches!(
        evaluate_gate(&diverged, &row),
        GateDecision::Drop { .. }
    ));
}

#[test]
fn test_voice_trigger_database_crud_lifecycle() {
    let conn = setup_test_db();

    // 1. Validate phrase rules
    assert!(validate_voice_phrase("").is_err());
    assert!(validate_voice_phrase("type this").is_err());
    assert!(validate_voice_phrase("type this please").is_err());
    assert_eq!(
        validate_voice_phrase("  Hello World  ").unwrap(),
        "hello world"
    );

    // Threshold tiers by phrase length (helper names kept).
    assert_eq!(threshold_for_phrase("my email"), 0.85);
    assert_eq!(threshold_for_phrase("deploy production"), 0.75);
    assert_eq!(
        threshold_for_phrase("this is a much longer voice trigger phrase"),
        0.65
    );

    // 2. Add voice entry with a voice invocation
    let mut script_entry = voice_entry("grim screenshot.png", "script", "take screenshot", false);
    script_entry.target_os = "all".to_string();
    script_entry.only_apps = Some("foot,alacritty".to_string());
    let (parent_id, aliases) = create_entry(&conn, script_entry).unwrap();
    assert_eq!(aliases.len(), 1);
    assert_eq!(aliases[0].invocation, "take screenshot");
    assert_eq!(aliases[0].invocation_type, InvocationType::Voice);

    let added = get_trigger(&conn, &parent_id)
        .unwrap()
        .expect("parent entry should exist");
    assert_eq!(added.action_type, "script");
    assert_eq!(added.target_os, "all");
    assert_eq!(added.only_apps.as_deref(), Some("foot,alacritty"));
    assert_eq!(added.usage_count, 0);
    assert!(added.is_enabled);

    // 3. Lookup parent by phrase (case-insensitive via normalization)
    let fetched_id = find_parent_by_invocation(&conn, InvocationType::Voice, "Take Screenshot")
        .unwrap()
        .expect("entry should be found case-insensitively");
    assert_eq!(fetched_id, parent_id);

    // 4. List active voice invocations
    let active = list_active_voice_invocations(&conn).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].trigger_id, parent_id);

    // 5. Increment usage count
    increment_usage_count_by_id(&conn, &parent_id).unwrap();
    let updated = get_trigger(&conn, &parent_id).unwrap().unwrap();
    assert_eq!(updated.usage_count, 1);

    // 6. Record trigger usage log
    record_voice_trigger_usage("take screenshot", 19, Some("foot".to_string()));

    // 7. Record voice dictation log
    record_voice_dictation_usage(42, 210, Some("code".to_string()));

    // 8. Delete the last alias tombstones the parent entry
    let deleted =
        delete_alias(&conn, &parent_id, InvocationType::Voice, "take screenshot").unwrap();
    assert!(deleted);

    let after_delete =
        find_parent_by_invocation(&conn, InvocationType::Voice, "take screenshot").unwrap();
    assert!(after_delete.is_none());

    let tombstoned = get_trigger(&conn, &parent_id).unwrap().unwrap();
    assert!(tombstoned.is_deleted);

    let active_after_delete = list_active_voice_invocations(&conn).unwrap();
    assert!(active_after_delete.is_empty());
}
