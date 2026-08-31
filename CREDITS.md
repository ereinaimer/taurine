# Credits

Taurine is built on the shoulders of some outstanding open source projects.

### Rust Crates

| Crate | Role |
|---|---|
| [ratatui](https://ratatui.rs/) | Terminal UI framework powering the interactive TUI |
| [crossterm](https://github.com/crossterm-rs/crossterm) | Cross-platform terminal backend for ratatui |
| [tokio](https://tokio.rs/) | Async runtime for the service and gRPC server |
| [clap](https://docs.rs/clap) | CLI argument parsing and subcommand routing |
| [tonic](https://github.com/hyperium/tonic) + [prost](https://github.com/tokio-rs/prost) | gRPC transport and protobuf encoding for IPC |
| [rdev](https://github.com/Narsil/rdev) | Low-level keyboard and mouse event capture |
| [rusqlite](https://github.com/rusqlite/rusqlite) + [r2d2](https://github.com/sfackler/r2d2) | Embedded SQLite database with connection pooling |
| [serde](https://serde.rs/) | Serialization and deserialization across the codebase |
| [keyring](https://github.com/hwchen/keyring-rs) | Secure OS keychain storage for API keys |
| [genai](https://github.com/jeremychone/rust-genai) | Unified client for AI provider integrations |
| [service-manager](https://github.com/chipsenkbeil/service-manager-rs) | Cross-platform system service management |
| [arboard](https://github.com/1Password/arboard) | Clipboard access across all platforms |
| [reqwest](https://github.com/seanmonstar/reqwest) + [ureq](https://github.com/algesten/ureq) | HTTP clients for API calls and updates |
| [color-eyre](https://github.com/eyre-rs/color-eyre) | Application-level error reporting and panic handling |
| [tracing](https://github.com/tokio-rs/tracing) | Application-level logging and diagnostics |
| [windows-sys](https://github.com/microsoft/windows-rs) | Windows API bindings for system integration |
| [sha2](https://github.com/RustCrypto/hashes) + [aes-gcm](https://github.com/RustCrypto/AEADs) + [argon2](https://github.com/RustCrypto/password-hashes) | Cryptographic hashing, encryption, and key derivation |
| [thiserror](https://github.com/dtolnay/thiserror) | Error type derivation |
| [uuid](https://github.com/uuid-rs/uuid) | Unique identifier generation |
| [regex](https://github.com/rust-lang/regex) | Regular expression engine |
| [image](https://github.com/image-rs/image) | Image encoding and decoding |
| [zeroize](https://github.com/RustCrypto/utils) | Secure memory zeroing for sensitive data |
| [zstd](https://github.com/gyscos/zstd-rs) | Streaming compression for encrypted state files |
| [directories](https://github.com/soc/directories-rs) | Platform-specific config and data directory paths |
| [notify-rust](https://github.com/hoodie/notify-rust) | Desktop notifications |
| [rodio](https://github.com/RustAudio/rodio) | Audio playback for notification sounds |
| [colored](https://github.com/mackwic/colored) | Terminal color output for the CLI |
| [futures](https://github.com/rust-lang/futures-rs) | Async primitives and stream combinators |
| [scraper](https://github.com/causal-agent/scraper) | HTML parsing for web content extraction |

### Assets & Media

| Asset / Library | Role |
|---|---|
| [uisfx](https://github.com/romainsimon/uisfx) | Sound library providing curated UI audio packs for pause and resume audio cues |

### Tooling

| Tool | Role |
|---|---|
| [Next.js](https://nextjs.org/) + [React](https://react.dev/) | Documentation site framework |
| [Tailwind CSS](https://tailwindcss.com/) | Documentation site styling |
| [TypeScript](https://www.typescriptlang.org/) | Type checking for documentation code |
| [Fumadocs](https://fumadocs.vercel.app/) | MDX-based documentation toolkit |
| [Orama](https://oramasearch.com/) | Full-text search for documentation |
| [pre-commit](https://pre-commit.com/) | Managing and maintaining Git pre-commit hooks |

