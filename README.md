# Taurine

A fast, secure cross-platform text expander built in Rust.

<a href="https://github.com/ereinaimer/taurine/actions/workflows/ci.yml"><img src="https://shieldcn.dev/github/ci/ereinaimer/taurine.svg?label=build&mode=dark&statusDot=false&logo=github&size=xs&color=22c55e&logoColor=f8fafc&labelTextColor=f8fafc" alt="Build Status" /></a>
<a href="https://github.com/ereinaimer/taurine/releases"><img src="https://shieldcn.dev/github/ereinaimer/taurine/release.svg?label=version&mode=dark&logo=github&size=xs&color=a855f7&logoColor=f8fafc&labelTextColor=f8fafc" alt="Latest Release" /></a>
<a href="https://discord.gg/Kc9XmHJgsS"><img src="https://shieldcn.dev/discord/members/Kc9XmHJgsS.svg?mode=dark&size=xs&color=5865f2&logoColor=f8fafc&labelTextColor=f8fafc" alt="Discord" /></a>
<a href="https://github.com/ereinaimer/taurine/stargazers"><img src="https://shieldcn.dev/github/stars/ereinaimer/taurine.svg?mode=dark&size=xs&color=fcd34d" alt="Stars" /></a>
<a href="https://www.rust-lang.org/"><img src="https://shieldcn.dev/badge/built%20in%20rust.svg?mode=dark&size=xs&logo=rust&color=ff964c" alt="Built in Rust" /></a>

## Installation

Prebuilt binaries are available for Windows, macOS, and Linux. Download them from [GitHub Releases](https://github.com/ereinaimer/taurine/releases), or use one of the quick install scripts below:

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/ereinaimer/taurine/main/install.ps1 | iex
```

```bash
# macOS / Linux (Bash)
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/ereinaimer/taurine/main/install.sh | sh
```

After installation, you can use `tau` as a handy alias for `taurine`

<details>
<summary><b>Alternative installs</b></summary>

<br>

**Install via Cargo:**

```bash
cargo install --git https://github.com/ereinaimer/taurine
```

**Build from source:**

```bash
git clone https://github.com/ereinaimer/taurine.git
cd taurine
cargo build -r
./target/release/taurine --help
```

</details>

## Quick demo

Get Taurine running in under a minute:

1.  **Start the Taurine service**:
    ```bash
    taurine up
    ```

2.  **Add your first shortcut**:
    ```bash
    taurine add hello "Hello, world!"
    ```

3.  **Try it anywhere**:
    Type `hello` in any text field (browser, editor, chat) and press **Enter**. It instantly expands to: `Hello, world!`

> [!TIP]
> **Prefer a UI?** Simply run `taurine` to open the interactive Terminal UI and manage your triggers visually.

## Features

- **Text expansion**: turn short triggers into full text
- **Inline math**: calculate while typing
- **Inline dates & time**: "next friday" and "2 days ago" become real dates
- **Inline conversions**: convert in natural language, from practically any unit, currency, or color, e.g. "100 dollars to Euros" or "5 miles in kilometers"
- **Inline AI**: stream AI answers into any app while typing
- **Regex triggers**: match patterns and capture groups, not just words
- **Scripts and commands**: launch apps, open websites, run local shell scripts
- **Hotkeys**: trigger anything with global shortcuts, per-app if you like
- **Dynamic variables**: insert names, dates, and much more with optional defaults
- **Backup and share**: backup and share your triggers
- **Backspace undo**: instantly revert an accidental expansion

## Documentation

To learn more about Taurine, see the [documentation](docs/).

## Contributing

Taurine is open to contributions. Read [CONTRIBUTING.md](CONTRIBUTING.md) to get started, and see [CREDITS.md](CREDITS.md) for the open source projects it's built on.

## License

Taurine is source-available under the **Aimer Software License (ASL)**. It is free for non-commercial personal use. Commercial rights are reserved.

See the [LICENSE](https://github.com/ereinaimer/taurine/blob/main/LICENSE) file for full terms.