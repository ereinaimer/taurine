# Privacy Policy

**Taurine is a local-first application.** We believe your keystrokes, clipboard contents, and automations are strictly your business.

This document outlines how Taurine handles your data to guarantee your privacy.

## 1. Local Data Storage
All of your data is stored locally on your device. Taurine does not have a central server, nor does it sync your data to the cloud.
- **Automations & Scripts**: Stored in a local SQLite database (`taurine.db`).
- **Settings**: Stored locally.
- **Metrics**: Your usage statistics (how many times a snippet was expanded, time saved) are calculated and stored locally.

## 2. Keystroke Monitoring & Clipboard
As a text expander and automation tool, the Taurine background daemon requires system-level permissions to monitor your keystrokes and read your clipboard.
- **Keystrokes**: The daemon actively listens to keystrokes to detect your configured trigger sequences. However, **no keystrokes are ever logged, saved, or transmitted.** The engine only keeps a tiny, rolling, in-memory buffer of your most recent keystrokes, which is immediately discarded.
- **Clipboard**: The daemon only reads your clipboard at the exact moment a script automation needs to inject its output, or when a snippet specifically uses the `[clip]` variable. It does not monitor or store clipboard history.

## 3. Telemetry & Analytics
**Taurine collects absolutely zero telemetry.**
- No crash reports are sent anywhere.
- No usage analytics are collected by the developers.
- No "phone home" mechanisms exist in the codebase.

## 4. Internet Connectivity
Taurine functions completely offline, with two strict exceptions:

1. **Inline AI Copilot (Opt-in)**: If you explicitly configure an AI provider (like OpenAI, Gemini, or Claude) and provide your own API key, Taurine will connect to that provider's API.
   - Taurine **only** sends the specific text prompt you type between the delimiters (e.g., `` `your prompt here` ``).
   - It does not send your screen context, clipboard, or keystroke history to the AI provider.
   - Your API keys are stored securely in your operating system's native credential manager (macOS Keychain, Windows Credential Manager, or Linux Secret Service).
2. **Self-Updates (Opt-in)**: If you run `taurine update`, the CLI will ping the official GitHub Releases page to check for new versions and download the binaries.

## 5. Security & Auditing
Taurine's code is publically available. Anyone is free to audit the source code, inspect the network traffic, and verify that the application behaves exactly as described above.

If you have any security or privacy concerns, please open an issue on our [GitHub repository](https://github.com/ereinaimer/taurine).
