# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Mouse Hotkey Triggers**: Trigger snippet expansions and actions using mouse button combinations.
- **Dynamic System Tray Icons**: Display state-aware play and pause tray icons reflecting Taurine's active and paused states.
- **Zero-Latency Text & Undo Fast-Path**: Inject plain text expansions up to 1,000 characters and backspace undos atomically with zero clipboard touches or artificial delay loops.
- **Instant Shell Launcher**: Launch application and URL triggers natively with sub-millisecond response times, bypassing shell interpreter startup overhead.
- **Audio Themes & Volume**: Choose from 12 built-in sound themes for pause and resume cues and configure cue playback volume (0-100%, defaulting to 50%).
- **Natural Language Date Countdowns**: Support `how many <unit> until <target>` queries for holidays (christmas, new year, halloween, valentine, etc.), relative anchors (end of day/week/month/year), weekdays (next friday), and natural dates — returns numeric duration (e.g. `25 days`, `3 weeks`).
- **Natural Language Date Queries**: Support `what is the date <target>` / `what date is it <target>` queries for the same target types — returns formatted date using configured date format (e.g. `December 25, 2024`).
- **NL Question Patterns for Unit Converter**: Support three natural language question styles for unit conversions — target-first (`how many inches in a foot?`), singular articles (`a mile to feet`, `an hour to minutes`), and complex prepositional (`how many cm are in 5 inches?`). All patterns transform to canonical format and reuse existing conversion engine.
- **Offline Inline Dictionary**: Search for word definitions, synonyms, and antonyms locally using an offline database (defaulting to Lite with an option to background-download a Full dictionary, and expanded triggers supporting apostrophized/hyphenated terms and plural opposites).
- **Data-Rate Conversions**: Inline conversions between data rates with case-sensitive bit/byte distinction (e.g. `50 Mbps to MBps` → `6.25 MBps`).
- **NL Prefixes for Timezone Queries**: Share unit converter's prefix list (`convert`, `what is`, `what's`, `how much is`) with timezone parser so queries like `convert 10am pst to tokyo time` now work.
- **Conversational Current Time Queries**: Support `what time is it in <city>` and `what's the time in <city>?` for natural current time lookups.
- **Conversational Time Conversions**: Support `when it is <time> in <from_tz> what time is it in <to_tz>?` for long-form timezone conversions.

### Changed
- **Sub-Millisecond Expansion Pipeline**: Pre-compiled snippet execution plans, atomic single-line cursor repositioning, and high-precision clipboard synchronization for instant expansions.
- **Silent Dictionary Downloads**: Background offline dictionary downloads and updates now complete silently without sending desktop notifications.
- **Single-Pass Fallbacks & Atomic Undo**: Consolidated natural language fallbacks into a single zero-allocation evaluation pass, added intent pre-filters, and batched backspace undo Win32 injections.
- **Zero-Latency Hot-Path Pipeline**: Optimized keystroke processing latency across static snippet interpolation, dictionary triggers, Windows hook layout decoding, and autocomplete matching.
- **Zero-Overhead Hook & Hotkey Evaluation**: Eliminate redundant syscalls and process environment lock contention during typing.
- **Streamlined Exchange**: Export and import now focus purely on trigger data, removing support for bundling local metrics, settings, or secrets into exchange files.

### Fixed
- **Hotkey Trigger Reliability**: Fixed an issue where rapid hotkey combinations failed intermittently after typing due to modifier state synchronization race conditions.
- **Multi-Line Text Ordering**: Route multi-line and tabbed expansions through the clipboard pipeline to preserve line order and prevent premature command execution in terminals.
- **Sequential Hotkey Reliability**: Fixed an issue where rapid consecutive hotkeys and application launchers failed to execute due to overly aggressive synthetic modifier releases.
- **Hook Resilience & Self-Healing**: Prevented false hook restarts during typing pauses, deferred active-window inspection to match time, added auto-reenabling recovery on macOS, and bounded lock contention to ensure zero keyboard dropped frames.
- **Text Expansion Reliability**: Fixed a regression where word expansions, swallowing, and completions would fail to trigger or misbehave during rapid typing.
- **Modifier Tracking & Hook Resilience**: Fixed stuck modifier keys after dropped release events and eliminated keyboard unresponsiveness by pairing active hardware state checks with instant Raw Input failover.
- **Alias Completions**: Tab-completions now work for the `tau` alias in PowerShell, Bash, Zsh, Fish, and Elvish (previously only the full `taurine` command name completed). Re-run `taurine completions install` to pick up the updated scripts.

## [1.0.0-alpha.16] - 2026-08-13

### Added
- **Background Auto-Updates**: The daemon now silently checks for and applies updates in the background every 6 hours (configurable via `Auto Update`).
- **Update Notifications**: Added a `Notify on Update` setting to show system notifications when Taurine successfully updates in the background.
- **App Expansion Stats**: Display a Top Apps card on the TUI Home page showing per-application expansion counts, keystrokes saved, and time saved.
- **Country-Level Timezone Support**: Check current time, relative offsets, and conversions using country names and ISO codes (e.g. `time in france`, `10am pst to jp`) resolved locally via static capital-city timezone mappings.
- **Post-Expansion Case Cycling**: Cycle expanded text capitalization (UPPERCASE, lowercase, Sentence case, Title Case) using Left and Right arrow keys within the active undo window.

### Changed
- **Uninstall Check**: Check if Taurine is installed prior to running uninstall scripts and skip remaining steps with a clean status message if missing.
- **Optional Script Timeout**: Setting `script_timeout` to `0` now disables the execution timeout entirely, letting scripts run until they finish (previously this fell back to a 20-second kill).
- **Internal RPC Authentication**: Store the local RPC token in the OS Credential Manager and remove it from user settings and the TUI.
- **IP Lookups & AI Provider Hardening**: Upgrade IP lookups to HTTPS, scope custom AI endpoints, and cap parallel AI completions.
- **Export Secrets & Perms Hardening**: Enforce 8-character export passwords, 0600 file permissions, zeroized password buffers, and exclude authentication tokens from exchange files.
- **Resource Limits & Buffer Safety**: Enforce strict caps on HTTP response sizes, internal key buffers, script output drains, regex catalog entries, and dead-key loop retries.
- **AI Expansion Protection**: AI-generated text no longer executes embedded script commands.
- **Private Temp Directory Isolation**: Temporary scripts, images, and desktop integration files are now stored securely in a private user temp folder with restricted permissions.
- **RPC Auth Hardening**: Rate-limited local API with constant-time token verification and masked secrets.
- **Local Transport Security**: Windows IPC named pipes reject remote clients; Windows clipboard text reading is strictly size-bounded.
- **Backspace Undo Duration**: Increase the backspace undo window from 2.5 seconds to 5.0 seconds.
- **Default Pause Notifications**: Disable pause notifications by default (`pause_notifications_enabled` defaults to `false`).
- **Dependency-Free Linux Tray**: The Linux system tray icon no longer requires GTK or the AppIndicator libraries, and it now recovers automatically when the desktop tray service restarts or the session connection drops (previously the icon could disappear for the rest of the session).

### Fixed
- **CLI Configuration Alignment**: `taurine config set` and `taurine config reset` now support all configurable settings, and `reset --all` restores every setting to its default.
- **Library Export Protection**: Exports from the Library UI now enforce the same encryption password minimum, private file permissions, and credential zeroing as the command-line exporter.
- **Whitespace No Longer Implies Multiplication**: In the inline calculator, numbers separated only by whitespace or line breaks (e.g. `2 2`) are no longer multiplied together; such input is left as typed text. Implicit multiplication (e.g. `2(3)`, `2pi`) still works when operands are directly adjacent, and explicit multiplication (`2 * 2`) is unaffected.
- **Graceful Shutdown**: Handle OS termination signals (SIGINT/SIGTERM) in the daemon and TUI to ensure clean resource teardown and terminal restoration.
- **Documentation Site Security**: Upgrade the documentation site's build toolchain (PostCSS, Sharp, js-yaml, brace-expansion) to resolve security advisories.

### Removed
- **RPC Token Env Override**: Removed `TAURINE_RPC_TOKEN`; the local RPC token is now exclusively managed internally in the OS credential store.
- **Hash Transformer**: Removed the `md5` hash modifier (SHA-256 and SHA-512 remain).
- **Port Modifier**: Removed the `net.port(n)` template modifier (IP and lookup modifiers remain).
- **Hex Generator**: Removed the `random.hex(len)` template modifier.
- **Node ESM Language**: Removed the `node-esm` script language and `.mjs` auto-detection; use `--lang node` for Node.js scripts.
- **Node ESM Modifier**: Removed the `exec.node_esm(...)` template variable; use `exec.node(...)` for inline Node scripts.
- **Action Key Setting**: Removed the `action_key` setting (Space or Enter). **Enter** is now the only expansion key; pressing Space types a plain space character.
- **Removed:** Inline trigger history navigation (double-tap Up/Down) — arrow keys now always pass through to your application.
- **Trigger Character & Mode Settings**: Removed the `trigger_char` (default `>`) and `triggerless_mode` settings. Expansion is now natively triggerless — word triggers expand directly without a prefix character in all cases.

## [1.0.0-alpha.15] - 2026-08-02

### Added
- **AI Capture Paste Support**: Paste text directly into the inline AI capture prompt buffer, respecting the 64KB cap without triggering auto-submit.
- **Natural Language Timezone Queries**: Check current time in any city (`time in tokyo`, `tokyo time`), convert between timezones (`10am pst to ist`), or combine relative offsets with timezones (`3 hours from now in tokyo`, `berlin 30 minutes from now`) — pure local computation via bundled IANA database.
- **Electrical Unit Conversions**: Inline conversions for voltage (V/mV/kV), current (A/mA), and resistance (ohm/kohm/megohm) with natural language support (e.g. `12V=mV` → `12000mv`, `1A=mA` → `1000ma`).
- **Bit Conversions**: Inline conversions between bits, kilobits, megabits, gigabits and bytes (e.g. `100mb=mbit` → `800mbit`) with natural language support.
- **Kilopascal (kPa) Conversions**: Inline conversions between kilopascals, PSI, bar, and other pressure units (e.g. `35psi=kpa` → `241.32kpa`).
- **NL Command Prefixes**: Support command/query prefixes (`convert`, `transform`, `change`, `calculate`, `compute`, `what is`, `what's`, `how much is`) for natural language unit conversions.
- **Extended Unit Coverage**: Added nautical miles (nmi), milliseconds/µs/ns, atmospheres/torr/mmHg, megawatts (MW)/milliwatts (mW), and micrograms (µg/mcg) with case-sensitive MW/mW disambiguation.
- **Frequency Unit Conversions**: Inline conversions between hertz, kilohertz, megahertz, gigahertz, and terahertz (e.g. `1MHz=Hz` → `1000000hz`) with natural language support.
- **Force Unit Conversions**: Inline conversions between newtons, dynes, pound-force, and kilogram-force (e.g. `1N=dyn` → `100000dyn`) with natural language support.
- **Energy Unit Conversions**: Inline conversions between joules, kilojoules, calories (food), BTU, watt-hours, kilowatt-hours, and electronvolts (e.g. `1cal=J` → `4184j`) with natural language support.
- **British Spelling Support**: Accept British English unit spellings (metre, litre, centimetre, etc.) for length and volume conversions in both compact and natural language syntax.
- **Angle Unit Conversions**: Inline conversions between degrees, radians, gradians, and turns (e.g. `180deg=rad` → `3.14rad`) with natural language support (`180 degrees to rad`).
- **CSS Unit Conversions**: Inline px ↔ rem and px ↔ em conversions (e.g. `24px=rem` → `1.5rem`) at default 16px base font size, using both compact and natural language syntax.
- **Inline Color Conversions**: Convert hex, named, and CSS function colors between hex/rgb/rgba/hsl/hsla formats directly in any text field (e.g. `#3b82f6=rgb` or `red to hex`).
- **Multi-Word Triggers**: Word triggers can now contain spaces (e.g. `my email address`) in both prefix and triggerless modes. Requires Enter as the action key; incompatible with instant expand mode.
- **Natural Language Emoji**: Support natural language emoji matching with exact word aliases and triggerless expansion via `word emoji` suffix.
- **Natural Language Unit Conversion**: Support natural language expressions (e.g. 100 dollars to Euros) with casing/spacing preservation and triggerless mode compatibility.
- **Inline Currency to Words**: Natively convert typed currency amounts (e.g. $1,200, ₹10,00,000, or EUR 50.99) into their spoken English representation on Enter, supporting lakh/crore Indian formats, ~40 major ISO 4217 code prefixes, and negative values.
- **Newline System Variable**: Add `[newline]` system variable to insert line breaks safely in trigger expansions without Windows path collisions.
- **Comma Preservation**: Preserve formatting of comma-separated inputs (such as Western and Indian lakh/crore groupings) in mathematical calculations and unit conversions.
- **Inline Date & Time**: Automatically expand relative dates and times (requiring an explicit direction signal) on Enter key press, with customizable formats and dialect overrides.
- **Configurable System Tray**: Allow toggling system tray icon visibility via a new setting customizable through the CLI and TUI.
- **System Tray Controls**: System tray support was added with the quit, pause, resume options.
- **AI Remove All**: `taurine ai remove --all` removes all configured provider API keys at once. Uninstall scripts now purge keyring entries automatically.
- **Empty Output Validation**: Prevent creating triggers with empty or whitespace-only content across all creation paths (CLI, TUI, import).
- **Global `--json` Flag**: All CLI commands support a `--json` flag for machine-readable JSON output. List commands return structured data (triggers, settings, AI providers/models), action commands return status objects.
- **Mouse Support in Import/Export Modals**: Click to select fields, scroll to navigate, and double-click to execute actions in the import and export overlays.
- **OS-Native Uninstallation**: Support standard system uninstallation, registering Taurine in the Windows Installed Apps list and providing clean uninstall scripts for all platforms.
- **Triggerless Tab Completion**: Intercept the Tab key in triggerless mode to trigger completion suggestions for the typed word tail, while passing through to the OS when no matches exist.
- **Interactive Import Options**: When importing from the CLI without any optional flags, Taurine now opens an interactive overlay where you can choose whether to include settings, sensitive settings, and stats, and pick a conflict resolution strategy.
- **Input Validation Limits**: Enforce maximum lengths for trigger strings (200 chars), output content (100KB), script content (1MB), trigger names (200 chars), and descriptions (1000 chars).
- **Dead Reference Detection**: Warn when a trigger references a non-existent trigger via [use()], preventing silent runtime failures.
- **App Filter Cleaning**: Automatically trim whitespace and validate prefix specifiers (exe:, class:, title:) in app filter settings.
- **Tag Normalization**: Tags are trimmed, lowercased, and deduplicated on save. Maximum 20 tags per trigger, 50 characters per tag.
- **Duplicate Name Warning**: Warn when creating or updating a trigger whose display name is already used by another trigger.
- **Unicode Normalization**: NFC-normalize trigger strings and keyboard input for reliable matching of accented characters across different input methods.
- **Color Conversion Transformers**: Convert text and clipboard colors between Hex, RGB, and HSL formats natively.

### Changed
- **Plain Output as Default**: Removed table borders from `list` and `config list` commands. Output is now plain text by default (`--plain` flag removed).
- **Rename Automations to Triggers**: Standardize terminology across CLI commands, TUI, and data exchange format by renaming automations to triggers.
- **Hot Path CPU & RAM Optimizations**: Reduce CPU overhead on the input processing hot path by deferring regex buffer allocation, eliminating O(n²) string cloning in suffix matching, and replacing per-expansion thread spawning with a fixed 2-thread pool. Cap tokio async runtime to 2 worker threads and 4 blocking threads to cut idle memory reservation.
- **Rename Conflict Resolution Flag**: Rename the database collision resolution CLI option in the import command from `--on-conflict` to `--conflict` (`-c`) for better consistency.
- **Trigger Input Validation**: Disallow newlines in word triggers; spaces are now allowed for multi-word triggers.
- **Telemetry Renamed to Stats**: Rename the "Metrics" system to "Stats" throughout the application (database tables, TUI, CLI options, and documentation) to provide more user-friendly terminology. Changed the CLI data exchange option from `--metrics` (`-m`) to `--stats` (`-t`).
- **Stats Time-Saved Cap**: Cap calculated time saved at a maximum of 5 minutes per expansion to prevent unrealistic productivity stats for large templates.
- **Stats Typing Speed Bounds**: Limit typing speed configuration (WPM) to a maximum of 150 WPM to prevent invalid or zeroed stat calculations.

### Fixed
- **Hotkey Delete Order Sensitivity**: Fix `tau rm` not matching hotkeys when modifier key order differs from how they were added (e.g. `alt+shift+2` vs `shift+alt+2`).
- **AI Executions Double-Counting**: Fix an issue where AI-assisted expansions counted twice in the dashboard stats.
- **Hotkey & Script Keystrokes Counting**: Prevent hotkey triggers and shell scripts from falsely inflating keystrokes and time saved statistics.
- **Clipboard Paste Race Condition**: Increase clipboard write verification polling from 15 to 20 attempts for more reliable paste under system load. Raise the post-paste clipboard restore delay across all platforms to reduce the chance of restoring the clipboard before the target application finishes reading the expansion payload.
- **Simulated Event Filtering Reliability**: Increase the self-filtering time-to-live for simulated key events from 100ms to 250ms, preventing Taurine's own injected keystrokes from being processed as user input under heavy load.

## [1.0.0-alpha.14] - 2026-07-18

### Added
- **Deep Idle When Paused**: Release active background resources like fullscreen polling and clipboard updates when paused to achieve near-zero CPU/battery consumption.
- **Script Security Switch**: Add a global `scripts_enabled` configuration setting to allow IT administrators to safely block all shell script executions.
- **Triggerless Backspace Undo**: Support Backspace Undo for expansions in triggerless mode to easily revert accidental matches.
- **Rich Text Support**: Support HTML-based styled text pasting with automatic plain text fallbacks.
- **Wayland Compatibility**: Support active window detection and fullscreen detection natively across Sway, Hyprland, KDE Plasma, and GNOME on Linux via a unified tracking backend.
- **Delay Time Units**: Support seconds (`s`) and decimal/fractional seconds (e.g. `1.5s`, `0.5s`) in the `delay` system variable to make pauses in keyboard macro expansions more human-readable.
- **Inline Unit Converter**: Support inline unit, temperature, and currency conversions directly in any text field via action keys, and disable all inline features (including math, emojis, completions, and history) when instant expand mode is enabled.
- **Inline Emoji Picker**: Support quick-expanding Unicode emojis by typing a configurable trigger character (e.g. `:rocket` becomes `🚀`). Supports cycling shortcodes with Tab completion (supporting both hyphens and underscores while typing, standardizing completion suggestions to hyphens) and snippet precedence.
- **Auto-Case Snippets**: Add `--auto-case` option to match snippets case-insensitively and mirror the typed trigger's casing.
- **Image & Script Assets**: Support cross-platform image expansion and compiled, portable script file assets packed into the database.
- **Regex Triggers**: Support pattern-based triggers with positional capture group interpolation.
- **Tagging Support**: Add CLI options to categorize snippets and filter/delete automations by tag.
- **Unquoted Spaces in Arguments**: Support spaces in dynamic variable arguments without quoting when using the `Enter` action key.

### Removed
- **[mock.*] Fake Data Generator**: Removed the entire `[mock.*]` system variable namespace (~1,961 lines), including static identity pools for names, addresses, credit cards, and phone numbers. This feature was out of scope for a text expander; placeholder text needs are already covered by `[lorem.*]`.
- **Template Escape Sequences**: Remove support for legacy unescaped control characters (\n, \t, \r) in favor of HTML layout tags.

### Changed
- **Triggerless Suffix Matching**: Bypass word boundary restrictions when `Enter` is the action key, allowing suffix expansions (e.g. `notegm` expanding `gm`) while keeping the boundary guard active for `Space`.
- **Inline AI Trigger Config**: Rename the AI delimiter settings (`ai_delimiter_mode` etc) to inline AI trigger settings (`inline_ai_trigger_mode` etc) to standardize with other inline features and reduce parser jargon.
- **Action Key Config**: Rename the `action_delimiter` configuration setting to `action_key` (representing the key, Space or Enter, that triggers automation expansions).
- **Upgrade Word Boundary Matching**: Support word expansions after punctuation symbols (e.g. `.`, `,`, `(`, etc.) rather than only whitespace characters.
- **Concise CLI Help Messages**: Simplify help descriptions for the app-filtering and operating system options to reduce clutter.
- **Frictionless Install Scripts**: Move install scripts to the repository root, rewrite for POSIX sh compliance, and optimize check order for instant local-first version checks.
- **Linux Permission Model**: Run the daemon securely without `CAP_DAC_OVERRIDE` capabilities by utilizing group permissions and udev rules.
- **Linux Permissions Setup GUI**: Show a graphical password prompt via Polkit when configuring system permissions on desktops.

### Fixed
- **Clipboard History Injection**: Fix an issue where the temporary expansion payload would end up in the clipboard history instead of the restored original text due to timing conflicts between the injector and history listener.
- **Action Key Expansion Reliability**: Fix an issue where pressing the action key (Enter) would sporadically fail to expand triggers after typing sentences with apostrophes.
- **Inline Math Delimiters**: Prevent the inline math engine from swallowing delimiters when typing plain numbers or constants without operations.
- **Linux Keyboard Layout Fallback**: Prevent daemon crashes on startup in headless environments or CI by falling back to a mock US layout when system XKB files are missing.
- **Linux Virtual Device Race**: Prevent events from being dropped during startup by waiting for the uinput virtual keyboard to finish initializing.
- **Linux Fullscreen Listener Leak**: Fix a bug where the fullscreen listener thread was leaked on shutdown, preventing the daemon from exiting cleanly.
- **Linux Clipboard Init**: Prevent daemon crashes on startup in headless environments or when no X11/Wayland display server is running.
- **Linux Clipboard Connection Conflicts**: Resolve clipboard unresponsiveness and connection conflicts under X11 by sharing the global clipboard connection.
- **Windows Resume Hook Resilience**: Fix hook unresponsiveness and daemon crashes after sleep/resume by replacing the rdev keyboard hook with a custom thread-local Win32 low-level keyboard hook, coalescing rapid wakeup events to prevent spawn-and-destroy loops, and synchronizing modifier key states with the foreground window.
- **Linux Service Startup**: Fix an issue where Taurine failed to start on Linux (`Unit not found`) due to a mismatch between the installed service file name and the expected label name.
- **Auto-Update Reliability**: Fix auto-update checks failing to retry on network issues, log check errors to the daemon, and prevent daemon panics if the cache folder cannot be created.
- **Scripts Enabled Wireup**: Fix an issue where the `scripts_enabled` configuration setting failed to load from the database or apply when updated via the CLI or TUI.

## [1.0.0-alpha.13] - 2026-07-12

### Added
- **App-Specific Triggers**: Restrict word/hotkey expansions to specific applications, window classes, or window titles.
- **Zeroize AI API Keys**: Clear AI API keys from system memory automatically after use.
- **Configurable RPC & Authentication**: Add secure Bearer token validation and configurable RPC settings.
- **Unix Clipboard History**: Enable clipboard history support on Linux and macOS with optimized resource utilization.
- **Configurable Clipboard Exclusions**: Provide options to configure history retention, exclusions, and transient pasteboard safety.
- **`tau` Shell Alias**: Configure PATH and `tau` alias across POSIX shells, Fish, Csh/Tcsh, and PowerShell with automatic duplicate cleanup.

### Changed
- **Lock-Free Input Evaluation**: Decouple undo state logic and make trigger completion check lock-free to remove typing lag.
- **Local IPC Defaults**: Default gRPC Loopback to owner-only sockets (Unix Domain Sockets or Windows Named Pipes).
- **Secure File Permissions**: Enforce owner-only permissions (0600/0700) on database and application directories.
- **Database Performance**: Use `r2d2` connection pool, WAL mode, atomic caching, and background logging.
- **Tokio Runtime Reuse**: Attempt to reuse active runtime handle before allocating a new thread-local runtime.

### Removed
- **Unused Dependencies**: Clean up workspace dependency bloat (`chrono`, `sha1`, `crc32fast`).

### Fixed
- **Linux Clipboard Listener Resilience**: Implement self-healing and startup retries for the clipboard history listener on non-Windows platforms.
- **Live Clipboard Fallback**: Resolve the [clip] variable from the live clipboard when history is empty or disabled.
- **Slash Normalization**: Normalize mixed forward and backward slashes in application filter paths.
- **Single-Instance Enforcement**: Fix transport hijacking and socket handle collisions for duplicate daemon processes.
- **Keyboard Layout Supervisor**: Gracefully compile default US keymap fallback and propagate listener errors.
- **Windows Hook Watchdog**: Avoid sleep/wake freezes by reducing retry timeouts and adding a hook watchdog timer.
- **Graceful Shutdown**: Coordinate background threads (hook, clipboard, monitors) to join and drop resources on shutdown.
- **Buffer Capacity Safety**: Ring buffer dynamically resizes and alerts users at 80% usage to prevent trigger loss.
- **Install Script Reliability**: Streamline version updates and configuration checks in `install.sh` and `install.ps1`. Prevent trim method errors and encoding crashes on Windows PowerShell 5.1, restrict profile setup logs to fresh installs, and scope green color styling to success checkmarks.

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
