# Privacy Policy

**Taurine is built on a local-first, privacy-by-design architecture.** We believe your keystrokes, clipboard contents, snippet triggers, and voice dictations are strictly your business.

This document outlines how Taurine handles your data across local execution, end-to-end encrypted synchronization, and account lifecycle management to guarantee your privacy.

## 1. Local Data Storage & Encryption at Rest
Taurine stores all user data locally on your device within an encrypted SQLite database (`taurine.db`).
- **SQLCipher 256-bit AES Encryption**: All snippet bodies, trigger definitions, script contents, activity history, and settings are encrypted at rest using SQLCipher.
- **Hardware Keystore Integration**: The local database encryption key is a cryptographically strong random secret generated on your device and stored securely in your operating system's native credential store (macOS Keychain, Windows Credential Manager, or Linux Secret Service via FreeDesktop Secret Service API). Copies of the database file cannot be read without access to the host keystore.

## 2. Zero-Knowledge End-to-End Encrypted (E2EE) Cloud Sync
Taurine provides multi-device cloud synchronization powered by a Zero-Knowledge End-to-End Encryption architecture:
- **Client-Side Encryption**: All sensitive user data—including snippet templates, script source code, and voice dictations—is encrypted on your local device before transmission to the cloud using AES-256-GCM Authenticated Encryption with Associated Data (AEAD).
- **Argon2id Key Derivation**: Encryption keys are derived client-side from your user credentials using Argon2id with cryptographic salt.
- **Zero-Knowledge Guarantee**: The cloud backend, database servers, and Taurine developers never have access to your encryption keys or plaintext data. Cloud servers store only encrypted ciphertexts and metadata required for conflict resolution. If our cloud infrastructure were compromised, your snippets, scripts, and personal data would remain cryptographically unreadable.

## 3. Keystroke Monitoring & Clipboard Access
To deliver low-latency text expansion, the Taurine background service requires operating system accessibility permissions:
- **Transient In-Memory Buffer**: Keystroke monitoring operates strictly in volatile memory using a small, rolling buffer to match trigger prefixes. Keystrokes are never logged to disk, never stored persistently, and never transmitted over any network interface.
- **Clipboard Isolation**: The clipboard is accessed only on-demand when an expansion explicitly contains the `[clipboard]` dynamic variable or when inserting script execution output. Taurine never monitors or stores clipboard history.

## 4. Activity History & Voice Dictations
Taurine offers optional activity logging to help you review recent expansions and dictate text:
- **Local Speech Recognition**: Voice dictation runs entirely on-device using local machine learning inference models (Sherpa-ONNX / Whisper). Raw microphone audio is processed in memory and never leaves your computer; audio recordings are never saved to disk or transmitted to the cloud.
- **Encrypted Activity History**: Voice transcripts and snippet expansion events are retained in your local database with a 30-day retention window. When cloud sync is active, transcripts are synchronized solely as end-to-end encrypted ciphertexts.

## 5. Usage & Quota Telemetry
Taurine calculates aggregated usage metrics to support weekly quota tracking and personal productivity statistics:
- **Aggregated Metrics**: Telemetry is strictly limited to aggregated metrics: words expanded, words spoken, time saved, and weekly quota balance countdowns.
- **Device-Isolated Counters**: Aggregated counters are tracked per device to ensure accurate quota tracking without tracking specific typing habits or snippet contents.
- **No Keystroke Telemetry**: Taurine does not collect or transmit keystroke counts, typed content, application telemetry, or crash telemetry.

## 6. Privacy & Data Purge on Logout
Signing out of Taurine securely and irreversibly clears sensitive state from the current device:
- **Complete Local Purge**: Running `taurine logout` shuts down background services, deletes the local SQLCipher database file, and wipes cached session state.
- **Keystore Key Wiping**: Data Encryption Keys (DEK), vault credentials, and authentication session tokens are permanently removed from the operating system credential manager.
- **Unsynced Mutation Safeguard**: To prevent accidental data loss, `taurine logout` verifies that all pending changes in the synchronization outbox are flushed to the cloud. If uncommitted mutations remain, logout safely aborts unless explicitly overridden with `--force`.

## 7. Third-Party AI Integrations (Opt-in)
Taurine includes an optional inline AI Copilot:
- **Direct Provider Connection**: If you configure an AI provider (e.g., OpenAI, Anthropic Claude, Google Gemini, Groq, or local endpoints), Taurine connects directly to that provider's API.
- **Minimal Context**: Only the specific prompt delimited by your expansion trigger is transmitted. Screen contents, window titles, clipboard contents, and broader typing history are never included.
- **Keystore Stored API Keys**: Provider API keys are stored in your operating system's native credential store and are never sent to Taurine cloud servers.

## 8. Security & Open Source Auditing
Taurine's client code is publicly available. Anyone is welcome to inspect the source code, verify network traffic boundaries, and audit cryptographic implementations.

If you discover any security or privacy vulnerabilities, please open an issue or reach out through our [GitHub repository](https://github.com/ereinaimer/taurine).
