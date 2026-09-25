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

/// Process-local mock OS keystore so global-DB handles never touch the real
/// keystore (reads or first-run key creation). Mirrors the unit-test shared
/// credential: entries from the same pair share one password.
mod mock_keystore {
    use std::collections::HashMap;
    use std::sync::{Mutex, Once, OnceLock};

    type PasswordMap = HashMap<(String, String), Vec<u8>>;

    const FIXED_TEST_DB_KEY_HEX: &[u8] =
        b"4343434343434343434343434343434343434343434343434343434343434343";

    static PASSWORDS: OnceLock<Mutex<PasswordMap>> = OnceLock::new();
    static INSTALL: Once = Once::new();

    fn passwords() -> &'static Mutex<PasswordMap> {
        PASSWORDS.get_or_init(|| {
            let mut map = HashMap::new();
            map.insert(
                ("taurine".to_string(), "db-key".to_string()),
                FIXED_TEST_DB_KEY_HEX.to_vec(),
            );
            Mutex::new(map)
        })
    }

    #[derive(Debug)]
    struct TestCredential {
        service: String,
        user: String,
    }

    impl keyring::credential::CredentialApi for TestCredential {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            passwords()
                .lock()
                .expect("test keystore poisoned")
                .insert((self.service.clone(), self.user.clone()), secret.to_vec());
            Ok(())
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            passwords()
                .lock()
                .expect("test keystore poisoned")
                .get(&(self.service.clone(), self.user.clone()))
                .cloned()
                .ok_or(keyring::Error::NoEntry)
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            passwords()
                .lock()
                .expect("test keystore poisoned")
                .remove(&(self.service.clone(), self.user.clone()))
                .map(|_| ())
                .ok_or(keyring::Error::NoEntry)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            std::fmt::Debug::fmt(self, f)
        }
    }

    #[derive(Debug)]
    struct TestCredentialBuilder;

    impl keyring::credential::CredentialBuilderApi for TestCredentialBuilder {
        fn build(
            &self,
            _target: Option<&str>,
            service: &str,
            user: &str,
        ) -> keyring::Result<Box<keyring::credential::Credential>> {
            Ok(Box::new(TestCredential {
                service: service.to_string(),
                user: user.to_string(),
            }))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn persistence(&self) -> keyring::credential::CredentialPersistence {
            keyring::credential::CredentialPersistence::ProcessOnly
        }
    }

    /// Install once per test binary; entries never reach the OS store.
    pub(super) fn use_mock_keystore() {
        INSTALL.call_once(|| {
            keyring::set_default_credential_builder(Box::new(TestCredentialBuilder));
        });
    }
}

/// Points TAURINE_DATA_DIR at a temp dir for the test body, restoring the
/// previous value on drop, so global-DB stats writes never reach the real DB.
/// Serialized via TEST_LOCK by callers: the env var is process-global.
struct TempDataDir {
    _dir: tempfile::TempDir,
    prev: Option<String>,
}

impl TempDataDir {
    /// Callers must hold `taurine_core::testing::TEST_LOCK` for the whole
    /// isolated section; the env var is process-global.
    fn isolated() -> Self {
        // SAFETY: callers hold TEST_LOCK across the isolated section.
        let dir = tempfile::tempdir().expect("temp data dir");
        let prev = std::env::var("TAURINE_DATA_DIR").ok();
        unsafe { std::env::set_var("TAURINE_DATA_DIR", dir.path()) };
        Self { _dir: dir, prev }
    }
}

impl Drop for TempDataDir {
    fn drop(&mut self) {
        // SAFETY: callers hold TEST_LOCK across the isolated section.
        unsafe {
            match &self.prev {
                Some(prev) => std::env::set_var("TAURINE_DATA_DIR", prev),
                None => std::env::remove_var("TAURINE_DATA_DIR"),
            }
        }
    }
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
    // Hermetic: usage-log writes below go through the global DB handle, so
    // isolate TAURINE_DATA_DIR. The env var is process-global: hold the lock
    // for the whole test.
    let _lock = taurine_core::testing::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _data = TempDataDir::isolated();
    mock_keystore::use_mock_keystore();
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

#[test]
fn test_phonetic_dictionary_pipeline() {
    let dict = VoiceDictionary::from_csv("Taurine");
    // Distinct English words like "torrent" or "thirty" are preserved
    assert_eq!(dict.apply("run torrent now"), "run torrent now");
    assert_eq!(dict.apply("thirty two"), "thirty two");

    // Exact casing is applied
    assert_eq!(dict.apply("run taurine now"), "run Taurine now");

    // Close typos are corrected
    assert_eq!(dict.apply("run taurin now"), "run Taurine now");

    // Exact zero-distance phonetic sound-alike matches are corrected
    assert_eq!(
        dict.apply("Dorin is the best text expander in the world"),
        "Taurine is the best text expander in the world"
    );
}

fn test_voice_invocation(phrase: &str, output: &str) -> taurine_core::db::crud::ResolvedInvocation {
    taurine_core::db::crud::ResolvedInvocation {
        trigger_id: format!("id-{phrase}"),
        invocation: phrase.to_string(),
        invocation_type: taurine_core::db::crud::InvocationType::Voice,
        action: taurine_core::db::crud::TriggerAction::text(output),
        require_confirmation: false,
        strict_threshold: 0.75,
    }
}

#[test]
fn test_exact_static_trigger_takes_precedence_over_parameterized() {
    use taurine_core::voice::rank_voice_invocations;

    let static_trig = test_voice_invocation("say hi", "Hello there!");
    let param_trig = test_voice_invocation("say hi to [person]", "Hello, [person]!");
    let triggers = vec![static_trig.clone(), param_trig.clone()];

    // Saying "say hi" perfectly matches static trigger
    let m = rank_voice_invocations("say hi", &triggers).unwrap();
    assert_eq!(m.trigger.invocation, "say hi");
    assert!(m.args.named.is_empty());

    // Saying "say hi to Bob" matches parameterized trigger
    let m2 = rank_voice_invocations("say hi to Bob", &triggers).unwrap();
    assert_eq!(m2.trigger.invocation, "say hi to [person]");
    assert_eq!(m2.args.named.get("person").unwrap(), "Bob");
}

#[test]
fn test_static_fuzzy_fallback_when_no_parameterized_match() {
    use taurine_core::voice::rank_voice_invocations;

    let static_trig = test_voice_invocation("movies folder", "opened");
    let param_trig = test_voice_invocation("send [msg] to [person]", "To: [person]!");
    let triggers = vec![static_trig.clone(), param_trig.clone()];

    // Near-miss keeps legacy static behavior
    let m = rank_voice_invocations("movies follower", &triggers).unwrap();
    assert_eq!(m.trigger.invocation, "movies folder");
    assert!(m.args.named.is_empty());
}
