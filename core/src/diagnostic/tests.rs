use super::Diagnostic;
use super::matcher::find_best_match;

#[test]
fn test_matcher_substring() {
    let candidates = ["audio_theme", "wpm"];
    assert_eq!(find_best_match("theme", &candidates), Some("audio_theme"));
}

#[test]
fn test_matcher_prefix() {
    let candidates = ["python", "bash"];
    assert_eq!(find_best_match("pyt", &candidates), Some("python"));
}

#[test]
fn test_matcher_typo_and_transposition() {
    let candidates = ["shift", "alt", "ctrl", "groq"];

    // Typo / Deletion: "shft" -> "shift"
    assert_eq!(find_best_match("shft", &candidates), Some("shift"));

    // Transposition: "atl" -> "alt"
    assert_eq!(find_best_match("atl", &candidates), Some("alt"));

    // Transposition: "ctlr" -> "ctrl"
    assert_eq!(find_best_match("ctlr", &candidates), Some("ctrl"));

    // Substitution: "grok" -> "groq"
    assert_eq!(find_best_match("grok", &candidates), Some("groq"));
}

#[test]
fn test_matcher_non_matches() {
    let candidates = ["python", "bash", "ruby"];
    assert_eq!(find_best_match("xyz", &candidates), None);
    assert_eq!(find_best_match("completely_unrelated", &candidates), None);
    assert_eq!(find_best_match("", &candidates), None);
    assert_eq!(find_best_match("   ", &candidates), None);
    assert_eq!(find_best_match("test", &[]), None);
}

#[test]
fn test_diagnostic_problem_only() {
    let diag = Diagnostic::problem("Unknown error occurred");
    assert_eq!(diag.render(), "Unknown error occurred");
}

#[test]
fn test_diagnostic_with_suggest() {
    let diag =
        Diagnostic::problem("Unknown modifier: shft").suggest("shft", &["shift", "alt", "ctrl"]);
    let rendered = diag.render();
    assert!(rendered.contains("Unknown modifier: shft"));
    assert!(rendered.contains("Did you mean shift?"));
    // Verify conversational style: no robotic double or single quotes
    assert!(!rendered.contains('"'));
    assert!(!rendered.contains('\''));
}

#[test]
fn test_diagnostic_without_suggest_match() {
    let diag = Diagnostic::problem("Unknown setting: xyz").suggest("xyz", &["audio_theme", "wpm"]);
    let rendered = diag.render();
    assert_eq!(rendered, "Unknown setting: xyz");
    assert!(!rendered.contains("Did you mean"));
}

#[test]
fn test_diagnostic_full_rendering() {
    let diag = Diagnostic::problem("Unknown setting: theme")
        .suggest("theme", &["audio_theme", "wpm"])
        .help("Run taurine config list to see all available settings.")
        .options("Valid settings", &["audio_theme", "wpm", "sound"])
        .example("taurine config set audio_theme typewriter");

    let rendered = diag.render();

    let expected = "\
Unknown setting: theme

Did you mean audio_theme?

Run taurine config list to see all available settings.

Valid settings: audio_theme, wpm, sound

Example:
  taurine config set audio_theme typewriter";

    assert_eq!(rendered, expected);
    assert!(!rendered.contains('"'));
    assert!(!rendered.contains('\''));
}

#[test]
fn test_diagnostic_multiple_examples() {
    let diag = Diagnostic::problem("Missing trigger or output")
        .help("Provide both trigger phrase and output text.")
        .example("taurine add :hello Hello World")
        .example("taurine add :shrug ¯\\_(ツ)_/¯");

    let rendered = diag.render();

    let expected = "\
Missing trigger or output

Provide both trigger phrase and output text.

Examples:
  taurine add :hello Hello World
  taurine add :shrug ¯\\_(ツ)_/¯";

    assert_eq!(rendered, expected);
}
