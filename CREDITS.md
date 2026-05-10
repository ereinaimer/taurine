# Credits

Taurine is built on the shoulders of some outstanding open source projects.

### Rust Crates

| Crate | Role |
|---|---|
| [ratatui](https://ratatui.rs/) | Terminal UI framework powering the interactive TUI |
| [crossterm](https://github.com/crossterm-rs/crossterm) | Cross-platform terminal backend for ratatui |
| [tokio](https://tokio.rs/) | Async runtime for the daemon and gRPC server |
| [clap](https://docs.rs/clap) | CLI argument parsing and subcommand routing |
| [tonic](https://github.com/hyperium/tonic) + [prost](https://github.com/tokio-rs/prost) | gRPC transport and protobuf encoding for IPC |
| [rdev](https://github.com/Narsil/rdev) | Low-level keyboard and mouse event capture |
| [rusqlite](https://github.com/rusqlite/rusqlite) | Embedded SQLite database for storing automations |
| [serde](https://serde.rs/) | Serialization and deserialization across the codebase |
| [keyring](https://github.com/hwchen/keyring-rs) | Secure OS keychain storage for API keys |
| [genai](https://github.com/jeremychone/rust-genai) | Unified client for AI provider integrations |
| [service-manager](https://github.com/chipsenkbeil/service-manager-rs) | Cross-platform system service management |
| [arboard](https://github.com/1Password/arboard) | Clipboard access across all platforms |
| [inquire](https://github.com/mikaelmello/inquire) | Interactive terminal prompts for the CLI |
| [axoupdater](https://github.com/axodotdev/axoupdater) | Self-updating client for cargo-dist releases |
| [color-eyre](https://github.com/eyre-rs/color-eyre) | Application-level error reporting and panic handling |
| [tracing](https://github.com/tokio-rs/tracing) | Application-level logging and diagnostics |
| [comfy-table](https://github.com/Nukesor/comfy-table) | Dynamic terminal table formatting for the CLI |

### Tooling

| Tool | Role |
|---|---|
| [Fumadocs](https://fumadocs.vercel.app/) | Documentation site framework |
| [OpenScreen](https://github.com/siddharthvaddem/openscreen) | Screen capture tool used for the demo video |
| [cargo-dist](https://github.com/axodotdev/cargo-dist) | Packaging, distributions, and CI workflow generation |
| [pre-commit](https://pre-commit.com/) | Managing and maintaining Git pre-commit hooks |
