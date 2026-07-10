# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Zeroize AI API Keys**: Utilized self-erasing memory (`Zeroizing`) to automatically clear AI API keys from system memory as soon as they are no longer needed, reducing potential exposure in core, daemon, and CLI processes.
- **Configurable RPC Settings and Token-Based Authentication**: Implemented customizable RPC communication settings with secure token-based authentication.
  - Added new configuration keys: `rpc_mode`, `rpc_host`, and `rpc_token`.
  - Added a gRPC interceptor to the daemon that validates requests against a secure Bearer token across all transport modes (TCP, sockets, and named pipes) for defense-in-depth security.
  - Automatically generates a secure cryptographically random UUID v4 token if the RPC token setting is empty.
  - Exposed these settings in the TUI Settings screen and CLI with platform-specific visibility rules (hiding TCP settings when UDS or Named Pipes are active).
  - Allowed overriding the connection token on clients using the `TAURINE_RPC_TOKEN` environment variable.
- **Linux and macOS Clipboard History Support**: Wired up full clipboard history tracking on non-Windows platforms. Uses AppKit's `changeCount` API on macOS for zero-overhead change detection, and polls `arboard` on Linux at an optimized 350ms interval to eliminate idle CPU battery drain.
- **Configurable Clipboard History and Exclusions**: Implemented customizable settings to control clipboard history recording and retention.
  - Added new configuration keys: `clipboard_history_enabled` and `clipboard_history_retention_secs`.
  - Added automated in-memory pruning and expiration of clipboard history items based on the retention duration.
  - Added warnings to TUI library screen and CLI automation addition command when users define snippets containing `[clip]` variables while clipboard history is disabled.
  - Integrated native macOS clipboard exclusions by setting standard transient pasteboard types (`org.nspasteboard.TransientType`, etc.) with empty data to prevent third-party clipboard managers from capturing sensitive expansions.
  - Exposed these settings in the TUI Settings screen and CLI configuration commands.
- **`tau` shell alias**: The install scripts now automatically set up a `tau` shell alias for `taurine`. The `update` command also ensures the alias is present after an update. A new `core::shell` module centralizes RC file manipulation logic, reused by the update, completions, and alias modules.

### Changed
- **Lock-Free Completion Checking & Decoupled Undo State**: Eliminated keyboard hook input lag and typing stuttering by making the inline trigger-assist completion check completely lock-free. In addition, de-coupled the undo state check and clearing logic from the central evaluator mutex, allowing the keyboard hook listener to bypass evaluator locking on 99.9% of normal keystrokes.
- **Default Unix Domain Sockets and Windows Named Pipes for IPC**: Changed the default gRPC communication channel from local loopback TCP to local owner-only socket connections (Unix Domain Sockets on Linux/macOS and Named Pipes on Windows) to prevent port scanning and enforce kernel-level owner-only access permissions.
- **Secure File and Directory Permissions on Unix**: Enforced owner-only permissions (`0700` for the app data directory and `0600` for the SQLite database file) on Linux and macOS to prevent unauthorized local users from reading sensitive macros, snippets, or credentials.
- **Database Optimizations on Hot paths**: Significantly reduced typing latency and eliminated keypress stuttering by optimizing SQLite interactions:
  - Switched to a shared, thread-safe connection pool (`r2d2`) configured with WAL mode, normal synchronicity, and a 5-second busy timeout to eliminate connection overhead and prevent database locking errors.
  - Implemented lock-free atomic settings caching for typing speed (WPM), clipboard delay, and script timeouts, completely bypassing database reads during expansions.
  - Offloaded daily metric logging writes from the hot keyboard hook thread to a non-blocking background worker thread.
  - Isolated parallel database tests using thread-local connection pools.
- **Tokio Runtime Reuse in `notify_daemon_reload`**: Optimized the gRPC reload notification logic to attempt to reuse the thread's existing Tokio runtime handle before falling back to a lightweight, single-threaded runtime. This prevents spawning a full multi-threaded runtime on every reload.

### Removed
- **Unused dependencies**: Cleaned up workspace dependency bloat by removing unused crates (`chrono` from the CLI, and `sha1` and `crc32fast` from the core library) to improve compile times.


### Fixed
- **Buffer expansion capacity safety**: Fixed keyboard buffer overflow issues when typing long paths or URLs. The text input ring buffer now dynamically resizes (doubles in capacity) and unrolls its contents when full rather than silently overwriting the oldest characters, preserving trigger recognition. Added a capacity warning when the buffer reaches 80% usage.
- **Install script cross-platform compatibility fixes**: Fixed both `install.sh` and `install.ps1` for cross-platform compatibility and reliability.
  - `install.sh`: Replaced `sha256sum` with a portable checksum function supporting both `sha256sum` (Linux) and `shasum -a 256` (macOS). Replaced `sort -V` (GNU extension, unavailable on macOS) with a component-by-component numeric version comparison for downgrade prevention.
  - `install.ps1`: Wrapped main logic in a `Main()` function and replaced `exit` with `return`/`throw` to prevent the PowerShell host from terminating when the script is run via `irm ... | iex`. Fixed PATH check to use case-insensitive comparison on Windows. Added `try/finally` block to ensure temp files are cleaned up on errors. Added red `Write-Host` before error throws for better user visibility.
- **Install script bug fixes**: Fixed several additional bugs in both install scripts.
  - `install.sh`: Added `|| true` to JSON parsing pipelines to prevent `set -euo pipefail` from silently killing the script before the friendly error message. Changed `grep -q` to `grep -Fq` for PATH checks to use fixed-string matching instead of regex. Added `wait $PID` exit code checks after manifest fetch and archive download so network failures produce a clear error instead of a cryptic system error.
  - `install.ps1`: Wrapped `[version]` cast in `try/catch` in the downgrade check to gracefully handle malformed version strings. Improved checksum error handling to distinguish between a job failure (tool crash) and a genuine checksum mismatch. Guarded PATH operations against a `$null` registry value to prevent null reference crashes.

## [1.0.0-alpha.12] - 2026-07-08

### Removed
- **`fake` crate from production dependencies**: Moved `fake` (test data generation) out of `[dependencies]` and replaced its usage in `mock.*` and `lorem.*` system variables with static data pools and `rand`-based random selection. The `fake` crate is no longer compiled into release builds, reducing dependency bloat.
  - Replaced `fake::faker::lorem` with a static lorem ipsum word pool in `lorem.rs`
  - Replaced `fake::faker` mock data (names, addresses, companies, jobs, credit cards, phones, emails, domains, usernames) with comprehensive static culturally-diverse data pools in `mock.rs`

### Fixed
- **Checksum verification spinner feedback**: Checksum verification step now shows spinner and green tick
- **Install script semver pre-release parsing**: Fixed `install.ps1` crash when the release version contains a semver pre-release suffix (e.g. `1.0.0-alpha.10`). PowerShell's `[version]` cast only accepts numeric components, so the pre-release tag is now stripped before numeric comparison in the downgrade guard.

## [1.0.0-alpha.11] - 2026-07-08

### Fixed
- **install.ps1 Invoke-WithRetry argument passing**: Added missing `$ArgumentList` parameter and `-ArgumentList` to `Start-Job` so job arguments are forwarded correctly instead of being silently dropped or passing `$null` to `Invoke-RestMethod`.
- **Pre-release version comparison in updater**: Fixed `is_newer_version` using string comparison (`>`) on pre-release identifiers, which caused `"alpha.10" < "alpha.9"` lexicographically. Replaced with proper field-by-field numeric comparison per semver spec.

## [1.0.0-alpha.10] - 2026-07-08

### Changed
- **Portable checksum computation in release workflow**: Replaced `sha256sum` with a portable detection chain (`sha256sum` -> `shasum -a 256` -> `openssl dgst -sha256`) so the checksum step works on macOS runners where `sha256sum` is not available.
- **Scoped workflow permissions**: Changed top-level `permissions` from `contents: write` to `contents: read`, with `contents: write` scoped only to the `publish` job that creates the release. The `build` jobs no longer have unnecessary write access.

### Added
- **Install script resilience improvements**: Both `install.sh` and `install.ps1` now include:
  - Retry logic with exponential backoff for network operations (manifest fetch, archive download)
  - SHA-256 checksum verification when the manifest provides it, protecting against corrupted downloads
  - Graceful handling of old binaries that lack the `--version` flag (no longer fails the version check)
  - Downgrade prevention: skips installation if the local version is newer than the latest release
  - `install.sh`: cleanup trap ensures temp directory is removed on `EXIT`, `INT`, or `TERM` signals
- **Checksum support in release manifest**: The release workflow now computes `sha256sum` for each artifact and includes it in `manifest.json` under each platform entry
- **Checksum verification in internal updater**: `taurine update` now verifies the downloaded archive SHA-256 against the manifest checksum before extracting
- **`--version` flag regression test**: Added a unit test verifying the `--version` flag parses correctly and the version constant is a valid semver string

### Fixed
- **Missing `--version` flag**: Added support for `taurine --version` to output the current version (e.g. `taurine 1.0.0-alpha.9`). The install scripts rely on this to detect existing installations and skip redundant re-downloads.
- **Windows update extraction failure**: Fixed a bug where `taurine update` failed on Windows with "file is being used by another process" during zip extraction. The downloaded archive file handle was not explicitly dropped before PowerShell's `Expand-Archive` tried to read it, causing an exclusive access lock conflict. Added an explicit `drop(archive_file)` before the extraction step.

## [1.0.0-alpha.9] - 2026-07-06

### Added
- **In-Process Self-Updater**: Added a built-in auto-update system capable of silently checking for and installing new releases on Windows, macOS, and Linux without external dependencies.
- **Cross-Platform Install Scripts**: Added official `install.sh` and `install.ps1` scripts for frictionless one-line setups.
- **Auto-Update TUI Setting**: Added an `Auto Update` toggle to the TUI to allow users to opt-out of background update checks.

### Changed
- **Unified Distribution Architecture**: Standardized the binary install location across all operating systems to live inside the canonical OS user data directory (`%LOCALAPPDATA%\Taurine\bin\taurine.exe` on Windows, `~/.local/share/taurine/bin/taurine` on Linux).
- **Custom Release Pipeline**: Replaced cargo-dist workflow with a streamlined GitHub Actions matrix that creates clean release artifacts and JSON manifests natively.

### Removed
- **Axoupdater & Cargo-Dist**: Removed third-party release managers (`cargo-dist`, `axoupdater`) in favor of a leaner in-house release pipeline and self-replacement architecture.


## [1.0.0-alpha.8] - 2026-07-06

### Added
- **Configurable AI Hyperparameters (`ai_temperature`, `ai_max_tokens`, `ai_system_prompt`)**: Introduced new configuration settings to fully customize the behavior of the inline AI engine. Users can now tweak temperature and maximum tokens, and provide a completely custom system prompt to override default formatting rules.
- **Configurable Script Execution Timeout (`script_timeout`)**: Introduced the `script_timeout` configuration setting to control the maximum allowed execution time (in milliseconds) for script variables (defaults to `5000` ms).
- **Configurable gRPC RPC Port (`rpc_port`)**: Introduced the `rpc_port` configuration setting, allowing users to customize the port the gRPC RPC server binds to (defaults to `50051`, accepts values in the range `1024-65535`).
- **Nested Snippets (`use` variable)**: Introduced the `[use("trigger_name")]` system variable, allowing users to compose and embed other text snippets directly within their macros.
- **Safety Limits for Macros**: Added robust save-time validation limits (max 5 recursion depth, max 3 AI calls per expanded macro) to prevent infinite loops, thread locking, and runaway API quotas.
- **JSON Formatting Transformers (`json.pretty` & `json.minify`)**: Added native `json.pretty` and `json.minify` formatters to prettify or compact JSON payloads.
- **Emoji Stripper Transformer (`stripemoji`)**: Added a native `stripemoji` transformer to remove all emoji characters, Zero Width Joiners, and Variation Selectors from text, leaving regular characters, punctuation, and whitespace intact.
- **URL Cleaner Transformer (`url.clean`)**: Added a native `url.clean` transformer to instantly strip tracking and query parameters from URLs (removing everything starting from the `?`).
- **Slug Transformer (`slug`)**: Added a native `slug` transformer to easily sanitize strings into clean, filesystem-safe and URL-safe slugs (lower-casing, replacing spaces/punctuation with hyphens, and stripping emojis).
- **Text Extractor Transformers (`ext.*`)**: Introduced a comprehensive suite of 14 regex-based and syntax-aware text extractor transformers under the `ext` namespace. Each extractor scans the full input and returns all matches as a newline-separated list.
  - `ext.url` — extracts `http/https` URLs.
  - `ext.email` — extracts email addresses.
  - `ext.phone` — extracts phone numbers in a wide range of formats (international, local, parenthesised).
  - `ext.mention` — extracts `@handle` mentions.
  - `ext.hashtag` — extracts `#tag` hashtags (rejects pure-digit tags).
  - `ext.ip` — extracts IPv4 and IPv6 addresses.
  - `ext.mac` — extracts colon- or dash-separated MAC addresses.
  - `ext.path` — extracts POSIX and Windows file/directory paths.
  - `ext.path.filename` — extracts filenames with extensions, pulling them from full paths via `std::path::Path` or matching standalone filenames.
  - `ext.path.dir` — extracts parent directories from paths.
  - `ext.jwt` — extracts JSON Web Tokens.
  - `ext.semver` — extracts semantic versions (e.g. `v1.2.3`, `0.4.0-alpha.1+build`).
  - `ext.mdcode` — extracts the inner contents of triple-backtick markdown code blocks.
  - `ext.mdtable` — extracts complete markdown tables (requires a header separator row).
  - `ext.mdlist` — extracts contiguous bulleted and numbered lists.

### Changed
- **Inline AI Delimiter Overhaul**: Replaced the hardcoded inline AI triggers (`>ai:` and backticks) with a customizable symmetric and asymmetric delimiter system (`ai_delimiter_mode`, `ai_open_delimiter`, and `ai_close_delimiter`), defaulting to asymmetric mode using `>>` and `<<`. Removed the word "Thinking..." from the inline loading spinner.
- **Default Configuration Updates**: Enabled triggerless mode (`triggerless_mode`) and ignore fullscreen (`ignore_fullscreen`) by default. Changed the default action delimiter (`action_delimiter`) from Space to Enter. Lowered the default script execution timeout (`script_timeout`) from 20 seconds to 15 seconds.
- **JSON Extractor Namespace**: Renamed the standalone `json(path)` extractor to `json.get(path)`.
- **URL & Base64 Namespace Reorganization**: Reorganized the standalone `urlencode`/`urldecode` and `base64encode`/`base64decode` encoding/decoding transformers under their respective `url` and `base64` namespaces as `url.encode`/`url.decode` and `base64.encode`/`base64.decode`.
- **Unified Singular Naming Convention**: Renamed all `lorem` modifiers and line/text transformers to their singular forms to enforce syntactic consistency across all system variables, modifiers, and transformers:
  - `lorem.words` -> `lorem.word`
  - `lorem.sentences` -> `lorem.sentence`
  - `lorem.paragraphs` -> `lorem.paragraph`
  - `onlydigits` -> `onlydigit`
  - `prefixlines` -> `prefixline`
  - `suffixlines` -> `suffixline`
  - `joinlines` -> `joinline`
  - `splitlines` -> `splitline`
  - `removeemptylines` -> `removeemptyline`
  - `compactlines` -> `compactline`
  - `sortlines` -> `sortline`
  - `uniqlines` -> `uniqline`

### Removed
- **Legacy AI Presets CLI**: Removed the legacy `ai preset` command from the CLI (`taurine ai preset list/add/rm`) and its associated engine logic, as the feature was wholly replaced by the customizable inline delimiter parsing and system prompts.
- **Redundant Transformers and Aliases**:
  - `remove` transformer (in favor of `replace(target, "")`).
  - `hexencode` and `hexdecode` encoding/decoding transformers.
  - `removeemptyline` line transformer alias (standardized on `compactline`).
- **Deprecated `extracturls` and `extractemails` transformers**: Removed in favour of `ext.url` and `ext.email` respectively.

### Fixed
- **Windows Keyboard Hook Resilience**: Fixed a bug where power resume events (WM_POWERBROADCAST) were not received because the power/session monitor window was created as a message-only window (HWND_MESSAGE). Changing it to a top-level invisible window allows the supervisor to successfully capture sleep/resume states and rehook the keyboard listener.
- **Windows Sleep/Resume Double-Processing**: Fixed a bug where typing a trigger after system resume (sleep) produced duplicate characters in the buffer, preventing trigger expansion. The root cause was `rdev::grab` installing a second `WH_KEYBOARD_LL` hook without unloading the old one, causing events to be processed twice. Now the old listener is signaled to exit via `WM_QUIT` before reinstalling, ensuring a single clean hook.

## [1.0.0-alpha.7] - 2026-07-02

### Added
- **Text & Line Utilities (`wordcount`, `linecount`, `sortlines`, `uniqlines`)**: Added native text and line transformers. Includes word counting, line counting, global order-preserving deduplication, and advanced line sorting supporting alphabetical, numerical, case-insensitive, and reverse sorting.
- **Mouse Automation (`mouse` namespace)**: Added native mouse macros (`[mouse.click]`, `[mouse.rclick]`, `[mouse.mclick]`, `[mouse.move(x, y)]`, `[mouse.scroll(delta)]`, `[mouse.hold]`, `[mouse.release]`) and coordinate lookup variable `[mouse.pos]` for UI-driven automation.
- **Data Extractor Transformers (`json`, `html`, `xml`, `yaml`, `toml`, `regexmatch`)**: Introduced 6 new robust text transformers designed for extracting specific data points from structured APIs, local configuration files, or raw webpages. These pair perfectly with `http.get` and `file.read` (e.g., `[http.get(...) | json(bpi.USD.rate)]`).
- **HTTP Variables (`[http.get(...)]` & `[http.status(...)]`)**: Introduced the `http` namespace for making native, synchronous HTTP GET requests. Perfect for pulling down raw data from public APIs or performing quick URL health checks.
- **Calculation Transformer (`| calc(...)`)**: Introduced inline arithmetic and numerical calculations directly within templates (e.g. `[amount=100 | calc("* 1.15")]` -> `115`, `[count=5 | calc("+ 1")]` -> `6`, and `[val=10 | calc("x * 2 + 5")]` -> `25`).
- **Formatting Transformers (`backtick` & `squote`)**: Introduced `backtick` transformer for wrapping text in backticks for inline code snippets, and standardized single quoting strictly on `squote`.

### Changed
- **Zero-Property Date and Time API**: Refactored `[date]` and `[time]` system variables to a highly extensible, method-based API (`.utc`, `.calc(...)`, `.format(...)`). You can now add and subtract durations (e.g., `[date.calc(+1d)]`) and use LDML tokens for fully custom formatting (e.g., `[time.format('Time:' hh:mm A)]`). All static properties (like `.iso`, `.long`, `.month`) have been removed.
- **Pruned Mock Data Generators (`mock.*`)**: Streamlined the `mock` namespace by removing hyper-niche and redundant variants (`bs`, `catch_phrase`, `status_code`, `method`, `user_agent`, `latitude`, `longitude`, `currency_name`, `currency_code`, `title`, `suffix`, and `password`). Standardized on core identity, geographic, web, and financial form-filling properties.
- **Removed `sys` Namespace & Overhauled `net` API**: Completely removed the redundant `sys.*` system variable namespace and dropped the `os_info` and `gethostname` dependencies. Overhauled the `net` namespace to support ultra-concise, essential network modifiers (`net.ip`, `net.lip`, `net.online`, `net.port(n)`).
- **Renamed `clipboard` and `execute` Namespaces**: Streamlined the `clipboard` and `execute` system variables to the conciser `clip` and `exec` namespaces.
- **Strict `uuid`, `lorem`, and `random` Variants**: Standardized `uuid` and `lorem` system variables to strictly require modifiers. Bare `[uuid]` and `[lorem]` tags are no longer supported. Replaced singular `lorem` modifiers with plural equivalents (`words`, `sentences`, `paragraphs`). Pruned all niche `random` generators in favor of simpler standard alternatives, limiting random strings and passwords to exclusively `str` and `pass`.
- **Removed `file.random_line`**: Dropped the `random_line` modifier from the `file` system variable namespace since the `read_line` modifier is sufficient for most file extraction uses.
- **Strict Casing & Formatting Transformer Syntax**: Standardized casing and formatting transformers strictly on concise identifiers (`upper`, `lower`, `snake`, `kebab`, `pascal`, `camel`, `title`, `sentence`, `quote`, `squote`, `backtick`, `unquote`) and completely pruned all redundant aliases (`doublequote`, `singlequote`, `*case` variants) for syntax unification.
- **Pruned Niche & Meme Transformers**: Completely removed impractical, edge-case, and meme-oriented transformers (`mocking`, `leet`, `train`, `shoutysnake`, `shoutykebab`, `reverse`, `crc32`, `sha1`, `rot13`, `reverselines`, `shufflelines`) from the registry to reduce feature bloat.
### Fixed
- **Pipeline Space Preservation**: Fixed aggressive space trimming bugs in global pipelines and hybrid arguments. Trailing spaces inside quotes are now correctly preserved when stripping quotes and executing case transformers.
- **Character Escaping Flow**: Fixed a bug where escaped directives like `\[key(enter)\]` would still execute due to eager unescaping during interpolation. Character unescaping is now deferred strictly to the final expansion parser. Added support for properly unescaping single (`\'`) and double (`\"`) quotes in normal text output.

## [1.0.0-alpha.6] - 2026-06-29

### Added
- **AI Pipe Transformer (`| ai(...)`)**: Introduced support for resolving dynamic generative AI prompts and text transformations directly within templates via Unix pipe syntax (e.g. `[clipboard | ai(summarize this in 3 bullets)]`).

### Changed
- **Standardized Format Transformers to Unix Pipe Syntax (`|`)**: Replaced dot notation for format modifiers (`.upper`, `.truncate(...)`) with Unix pipe syntax (`| upper`, `| truncate(...)`, e.g. `[name=john | title]`). This completely resolves default value period ambiguity and enables clean, readable transformer chaining.
- **Renamed `run` System Variable Namespace to `execute`**: Updated inline shell execution variables from `[run.*]` to `[execute.*]`.
- **Standardized Directives to Function Syntax (`key` & `delay`)**: Standardized simulated key presses and pause delays to explicit function syntax (`[key(tab)]`, `[key(ctrl+a)]`, `[delay(200ms)]`) and removed legacy dot notation (`[delay.200ms]`, `[key.enter]`).
- **Standardized Environment Variable Syntax (`env`)**: Converted environment variable resolution from dot notation (`[env.VAR]`) to function-call syntax (`[env(VAR)]` and `[env("VAR")]`).

### Fixed
- **System Power Resilience**: Fixed an issue where Taurine stopped working entirely after Windows woke from sleep or session unlock due to OS low-level keyboard hooks (`WH_KEYBOARD_LL`) being silently invalidated. Taurine now properly force-rehooks and introduces a 1000ms delay to allow USB keyboards to re-enumerate smoothly.
- **Windows Startup Metadata**: Fixed an issue where Windows Task Manager's Startup tab displayed `taurine-startup.exe` instead of the application name by removing startup arguments from the registry key and reading them from an adjacent `.path` file instead.
- **Clipboard History**: Windows clipboard history is now entirely event-driven using `WM_CLIPBOARDUPDATE` messages rather than polling `OpenClipboard` every 150ms. This uses 0% CPU on idle, guarantees capture of all rapid clipboard updates, and completely eliminates race conditions and lock contention.

## [1.0.0-alpha.5] - 2026-06-27

### Added
- **Ignore Fullscreen Applications**: Added new background listener mechanism to detect when a fullscreen application (like a game) is in focus, automatically pausing macro evaluation to prevent accidental text injection.

### Fixed
- **Audio Feedback**: Fixed an issue where the pause and resume sounds were not playing due to a decoding incompatibility with the upgraded `rodio` audio library.

## [1.0.0-alpha.4] - 2026-06-26

### Added
- **Triggerless Mode**: Added a new configuration to expand trigger words automatically when typing without requiring the trigger character prefix (default: disabled).
- **Configurable Action Delimiter**: Added support for configuring the text expansion trigger to use either `Space` or `Enter` as the action delimiter. This can be customized via the TUI or CLI.
- **Clipboard Restore Delay Configuration**: Introduced a new `clipboard_restore_delay_ms` setting to configure the delay between pasting and restoring the clipboard. This can be customized via the TUI or CLI to mitigate clipboard race conditions on slower systems.

### Fixed
- **Windows 10 Clipboard Race**: Fixed an issue where rapid text expansions on Windows 10 sometimes pasted the previously copied clipboard content instead of the trigger payload due to Clipboard User Service (`cbdhsvc`) locking contention.

## [1.0.0-alpha.3] - 2026-06-24

### Fixed
- **Linux Keyboard Hotplug**: Restored function after disconnecting and reconnecting keyboards on linux by supervising `/dev/input` devices and restarting evdev listeners automatically.

## [1.0.0-alpha.2] - 2026-05-21

### Added
- **Audio Feedback**: Asynchronous audio feedback when toggling pause and resume states
- **Audio Configuration**: Support for configuring pause/resume audio feedback in CLI and TUI
- **Windows Metadata**: Embedded native Windows file version metadata into the CLI binary

### Fixed
- **Inline AI Lifecycle**: Resolved injection gating issues where text expansion/taurine stopped working after using inline AI
- **Audio Output Switching**: Resolved audio playback not switching to speakers after headphones are unplugged
- **CI/CD Linux Support**: Added `libasound2-dev` and other build dependencies to CI and Linux distribution packages to support audio output

## [1.0.0-alpha.1] - 2026-05-10

### Added
- **Text Expansion**: Turn short triggers into full text instantly in any application
- **Keyboard Shortcuts & Hotkeys**: Run text expansions and scripts with global hotkeys
- **Scripts**: Support for running scripts (PowerShell, Bash, JavaScript, Python)
- **Dynamic Variables**: Support for placeholders with arguments, defaults, and transformers (e.g., URL encode, title case)
- **System Variables**: Inject system data like date, time, clipboard content, and random data (UUIDs, mock data)
- **Inline Math**: Calculate mathematical expressions directly in any text field (`>5+3`)
- **Inline AI**: Stream AI responses directly into the active text field using backticks
- **CLI & TUI**: Manage automations visually via a Terminal UI or directly from the command line
- **Import & Export**: Backup and share automations using the `.tau` file format
