# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
