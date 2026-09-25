// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

use rusqlite::Connection;
use taurine_core::db::crud::voice_triggers::{
    add_voice_trigger_full, delete_voice_trigger_by_phrase, get_voice_trigger_by_phrase,
    increment_voice_trigger_usage, list_active_voice_triggers, validate_voice_phrase,
};
use taurine_core::db::crud::{record_voice_dictation_usage, record_voice_trigger_usage};
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

#[test]
fn test_voice_models_catalog_and_ram_tier() {
    let unified = get_model_entry("parakeet-unified-en-0.6b").expect("unified must be in catalog");
    assert_eq!(unified.id, "parakeet-unified-en-0.6b");
    assert!(!unified.is_archive);
    assert!(unified.remote_files.is_some());
    let light = get_model_entry("parakeet-tdt-ctc-110m").expect("110m must be in catalog");
    assert_eq!(light.id, "parakeet-tdt-ctc-110m");
    assert!(light.is_archive);
    let silero = get_model_entry("silero_vad_v6").expect("silero vad in catalog");
    assert_eq!(silero.id, "silero_vad_v6");
    let kws = get_model_entry("kws-zipformer-zh-en-3M").expect("kws in catalog");
    assert_eq!(kws.id, "kws-zipformer-zh-en-3M");
    assert_eq!(get_model_entry("whisper-small-en"), None);
    assert_eq!(get_model_entry("parakeet-tdt-0.6b-v3"), None);
    assert_eq!(get_model_entry("moonshine-base-en"), None);
    assert_eq!(get_model_entry("moonshine-tiny-en"), None);
    assert_eq!(resolve_model_alias("unified"), "parakeet-unified-en-0.6b");
    assert_eq!(resolve_model_alias("110m"), "parakeet-tdt-ctc-110m");
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
    let row = add_voice_trigger_full(
        &conn,
        "deploy production",
        "kubectl apply -f prod.yaml",
        "script",
        "any",
        None,
        None,
        true, // requires confirmation
    )
    .unwrap();

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
    let row_no_confirm = add_voice_trigger_full(
        &conn,
        "paste signature",
        "Best regards,\nAlice",
        "text",
        "any",
        None,
        None,
        false,
    )
    .unwrap();

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

    // 2. Add voice trigger
    let added = add_voice_trigger_full(
        &conn,
        "take screenshot",
        "grim screenshot.png",
        "script",
        "linux",
        Some("foot,alacritty"),
        None,
        false,
    )
    .unwrap();

    assert_eq!(added.spoken_phrase, "take screenshot");
    assert_eq!(added.output, "grim screenshot.png");
    assert_eq!(added.action_type, "script");
    assert_eq!(added.target_os, "linux");
    assert_eq!(added.only_apps.as_deref(), Some("foot,alacritty"));
    assert_eq!(added.usage_count, 0);
    assert!(added.is_enabled);

    // 3. Lookup by phrase
    let fetched = get_voice_trigger_by_phrase(&conn, "Take Screenshot")
        .unwrap()
        .expect("trigger should be found case-insensitively");
    assert_eq!(fetched.id, added.id);

    // 4. List active triggers
    let active = list_active_voice_triggers(&conn).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, added.id);

    // 5. Increment usage count
    increment_voice_trigger_usage(&conn, &added.id).unwrap();
    let updated = get_voice_trigger_by_phrase(&conn, "take screenshot")
        .unwrap()
        .unwrap();
    assert_eq!(updated.usage_count, 1);

    // 6. Record trigger usage log
    record_voice_trigger_usage("take screenshot", 19, Some("foot".to_string()));

    // 7. Record voice dictation log
    record_voice_dictation_usage(42, 210, Some("code".to_string()));

    // 8. Soft delete trigger by phrase
    let deleted = delete_voice_trigger_by_phrase(&conn, "take screenshot").unwrap();
    assert!(deleted);

    let after_delete = get_voice_trigger_by_phrase(&conn, "take screenshot").unwrap();
    assert!(after_delete.is_none());

    let active_after_delete = list_active_voice_triggers(&conn).unwrap();
    assert!(active_after_delete.is_empty());
}
