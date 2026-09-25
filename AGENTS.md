# Project Overview & Tech Stack
- **Backend:** Rust.
- **Knowledge Graph:** Uses `graphify` for deterministic AST-based dependency tracking.

# Architectural Guidelines
- **Separation of Concerns:** Keep files modular. Do not create monolithic, 1,000-line behemoth files. When a file grows too large, proactively suggest extracting logic into a new, dedicated module.
- **Rust Standards:** Adhere strictly to idiomatic Rust. Run `cargo fmt` and ensure there are zero `cargo clippy` warnings before finalizing any backend code. 
- **Unsafe Code Documentation:** All raw Win32 calls, Obj-C messaging, and other unsafe blocks/functions must have a `// SAFETY:` comment directly preceding them explaining exactly why the operations within the unsafe boundary are memory-safe and valid.

# Operational Boundaries
- **NEVER** commit any files to version control yourself.
- **NEVER** push to any remote branches.
- The user will handle all git operations manually.
- **Scope Containment:** Do not modify code formatting or perform refactors outside the immediate scope of the requested task.
- **Plan Before Executing:** For significant architectural changes or refactors spanning multiple files, output a brief execution plan and wait for user approval before modifying files.
- **Changelog Maintenance:** Whenever a user-facing change is implemented, you must modify `CHANGELOG.md`. You are writing for the public end-user, not a developer. 
  - **DO document:** New features, changes to user workflows/configurations, breaking changes, and bug fixes (translate technical root causes into the symptom the user experienced).
  - **DO NOT document:** Internal code refactoring, CI/CD pipeline updates, GitHub Actions, dependency/crate swaps, or test data modifications. If a implementation only affects internal changes, do not touch the changelog.
  - **Compactness Constraint:** Changelog entries must be extremely compact, concise, and direct (one high-level bullet point per feature or fix). Do not include sub-bullets or overly verbose implementation details.
    - *Verbose (Bad):*
      - **App-Specific Triggers**: Restrict word and hotkey expansions to specific applications, window classes, or window titles.
        - Added new CLI flags `--include-apps` and `--exclude-apps` (accepting comma-separated lists) to `taurine add` and `taurine script`.
        - Added support for prefix specifiers: `exe:<name>` (process name exact match), `class:<name>`, and `title:<substring>`.
    - *Compact (Good):*
      - **App-Specific Triggers**: Restrict word/hotkey expansions to specific applications, window classes, or window titles via new CLI flags.

# Test Isolation
- **Hermetic Default:** The default suite (`cargo test` / `cargo nextest run`) must never affect the host machine: no typed keys, no clipboard reads/writes, no windows, no spawned processes, no audio playback, no mic access, no network, no model downloads, no real DB or keystore writes. Temp dirs, in-memory DBs, and dummy transcribers only.
- **Live Tests:** Any test needing the host must carry BOTH `#[ignore]` AND an early return unless `host_tests_allowed()` (opt-in via `TAURINE_ALLOW_HOST_INPUT=1`). Run only via `TAURINE_ALLOW_HOST_INPUT=1 cargo test -- --ignored` on a throwaway machine.
- **Reuse Seams:** RecordingInjector / test_injector, FakeClipboard, set_mock_clip helpers, mock keystores, `TAURINE_DATA_DIR` temp dirs under TEST_LOCK. Do not invent new host-touching test paths.
- **Forbidden Ungated:** Real injector calls, inject_expansion, inject_transcript with text, fire_voice_trigger, rdev simulation, SendInput, real clipboard, native_shell_open, Command spawns, updater self-spawn, voice cues, capture start, model downloads, global-DB stats writes.
- **Review:** Reject any test addition touching the above without both gates, with a pointer to this rule.

# Git Commit Standards
We strictly follow Conventional Commits. When proposing a commit message, you must adhere to these exact rules:
1. **Header:** `type(scope): concise description`
2. **Body:** Separated from the header by a single blank line. Use concise bullet points.
3. **Format Constraint:** You are strictly forbidden from using backticks (`) anywhere within the commit message body.

### Expected Output Format:
```text
feat(engine): extract text expansion logic to module

- decouple sys namespace variables from core engine
- enforce utf-8 boundary safety in backspace undo logic
```