# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
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
