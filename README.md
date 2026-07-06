# Taurine

A fast, secure and easy to use automation software for text expansion and keyboard automation. Taurine helps you type less, automate repetitive work, and run keyboard shortcuts from anywhere on your computer.

<video src="https://github.com/user-attachments/assets/a10c487d-ec5e-46bd-8113-d297315447a0" controls muted width="100%"></video>

<p align="center">
  <a href="#features">Features</a> ·
  <a href="#installation">Install</a> ·
  <a href="#keyboard-shortcuts-and-hotkeys">Hotkeys</a> ·
  <a href="#scripts-and-commands">Scripts</a> ·
  <a href="#inline-ai">AI</a> ·
  <a href="#faq">FAQ</a>
</p>

<div align="center">

<!-- Outline -->

<a href="https://github.com/ereinaimer/taurine/actions/workflows/ci.yml"><img src="https://shieldcn.dev/github/ci/ereinaimer/taurine.svg?label=build&mode=dark&statusDot=false&logo=github&size=xs&variant=outline" alt="Build Status" /></a>
<a href="https://github.com/ereinaimer/taurine/releases"><img src="https://shieldcn.dev/github/ereinaimer/taurine/release.svg?label=version&mode=dark&logo=github&size=xs&variant=outline" alt="Latest Release" /></a>
<a href="https://discord.gg/Kc9XmHJgsS"><img src="https://shieldcn.dev/discord/members/Kc9XmHJgsS.svg?mode=dark&size=xs&variant=outline" alt="Discord" /></a>
<a href="https://github.com/ereinaimer/taurine/commits"><img src="https://shieldcn.dev/github/last-commit/ereinaimer/taurine.svg?mode=dark&size=xs&variant=outline" alt="Last Update" /></a>
<a href="https://github.com/ereinaimer/taurine/stargazers"><img src="https://shieldcn.dev/github/stars/ereinaimer/taurine.svg?mode=dark&size=xs&variant=outline" alt="Stars" /></a>
<a href="https://github.com/ereinaimer/taurine/network/members"><img src="https://shieldcn.dev/github/forks/ereinaimer/taurine.svg?mode=dark&size=xs&variant=outline" alt="Forks" /></a>
<a href="https://github.com/ereinaimer/taurine/issues"><img src="https://shieldcn.dev/github/issues/ereinaimer/taurine.svg?mode=dark&size=xs&variant=outline" alt="Issues" /></a>
<a href="https://github.com/ereinaimer/taurine/pulls"><img src="https://shieldcn.dev/github/open-prs/ereinaimer/taurine.svg?mode=dark&size=xs&variant=outline" alt="Pull Requests" /></a>
<a href="https://www.rust-lang.org/"><img src="https://shieldcn.dev/badge/built%20in-rust.svg?mode=dark&size=xs&logo=rust&variant=outline" alt="Built in Rust" /></a>
<a href="https://ratatui.rs/"><img src="https://shieldcn.dev/badge/built%20in-ratatui.svg?mode=dark&size=xs&logo=ratatui&variant=outline" alt="Built in Ratatui" /></a>

<!-- Colored Outline -->

<!-- <a href="https://github.com/ereinaimer/taurine/actions/workflows/ci.yml"><img src="https://shieldcn.dev/github/ci/ereinaimer/taurine.svg?label=build&mode=dark&statusDot=false&logo=github&size=xs&variant=outline&color=22c55e&logoColor=f8fafc&labelTextColor=f8fafc" alt="Build Status" /></a>
<a href="https://github.com/ereinaimer/taurine/releases"><img src="https://shieldcn.dev/github/ereinaimer/taurine/release.svg?label=version&mode=dark&logo=github&size=xs&variant=outline&color=a855f7&logoColor=f8fafc&labelTextColor=f8fafc" alt="Latest Release" /></a>
<a href="https://discord.gg/Kc9XmHJgsS"><img src="https://shieldcn.dev/discord/members/Kc9XmHJgsS.svg?mode=dark&size=xs&variant=outline&color=5865f2&logoColor=f8fafc&labelTextColor=f8fafc" alt="Discord" /></a>
<a href="https://github.com/ereinaimer/taurine/commits"><img src="https://shieldcn.dev/github/last-commit/ereinaimer/taurine.svg?label=updated&mode=dark&size=xs&variant=outline&color=3b82f6&logoColor=f8fafc&labelTextColor=f8fafc" alt="Last Update" /></a>
<a href="https://github.com/ereinaimer/taurine/stargazers"><img src="https://shieldcn.dev/github/stars/ereinaimer/taurine.svg?mode=dark&size=xs&variant=outline&color=fcd34d&logoColor=f8fafc&labelTextColor=f8fafc" alt="Stars" /></a>
<a href="https://github.com/ereinaimer/taurine/network/members"><img src="https://shieldcn.dev/github/forks/ereinaimer/taurine.svg?mode=dark&size=xs&variant=outline&color=8b5cf6&logoColor=f8fafc&labelTextColor=f8fafc" alt="Forks" /></a>
<a href="https://github.com/ereinaimer/taurine/issues"><img src="https://shieldcn.dev/github/issues/ereinaimer/taurine.svg?mode=dark&size=xs&variant=outline&color=06b6d4&logoColor=f8fafc&labelTextColor=f8fafc" alt="Issues" /></a>
<a href="https://github.com/ereinaimer/taurine/pulls"><img src="https://shieldcn.dev/github/open-prs/ereinaimer/taurine.svg?mode=dark&size=xs&variant=outline&color=14b8a6&logoColor=f8fafc&labelTextColor=f8fafc" alt="Pull Requests" /></a>
<a href="https://www.rust-lang.org/"><img src="https://shieldcn.dev/badge/built%20in-rust.svg?mode=dark&size=xs&logo=rust&variant=outline&color=ff964c&logoColor=f8fafc&labelTextColor=f8fafc" alt="Built in Rust" /></a>
<a href="https://ratatui.rs/"><img src="https://shieldcn.dev/badge/built%20in-ratatui.svg?mode=dark&size=xs&logo=ratatui&variant=outline&color=e11d48&logoColor=f8fafc&labelTextColor=f8fafc" alt="Built in Ratatui" /></a> -->

<!-- Colored -->


<!-- <a href="https://github.com/ereinaimer/taurine/actions/workflows/ci.yml"><img src="https://shieldcn.dev/github/ci/ereinaimer/taurine.svg?label=build&mode=dark&statusDot=false&logo=github&size=xs&color=22c55e&logoColor=f8fafc&labelTextColor=f8fafc" alt="Build Status" /></a>
<a href="https://github.com/ereinaimer/taurine/releases"><img src="https://shieldcn.dev/github/ereinaimer/taurine/release.svg?label=version&mode=dark&logo=github&size=xs&color=a855f7&logoColor=f8fafc&labelTextColor=f8fafc" alt="Latest Release" /></a>
<a href="https://discord.gg/Kc9XmHJgsS"><img src="https://shieldcn.dev/discord/members/Kc9XmHJgsS.svg?mode=dark&size=xs&color=5865f2&logoColor=f8fafc&labelTextColor=f8fafc" alt="Discord" /></a>
<a href="https://github.com/ereinaimer/taurine/commits"><img src="https://shieldcn.dev/github/last-commit/ereinaimer/taurine.svg?label=updated&mode=dark&size=xs&color=3b82f6&logoColor=f8fafc&labelTextColor=f8fafc" alt="Last Update" /></a>
<a href="https://github.com/ereinaimer/taurine/stargazers"><img src="https://shieldcn.dev/github/stars/ereinaimer/taurine.svg?mode=dark&size=xs&color=fcd34d" alt="Stars" /></a>
<a href="https://github.com/ereinaimer/taurine/network/members"><img src="https://shieldcn.dev/github/forks/ereinaimer/taurine.svg?mode=dark&size=xs&color=8b5cf6&logoColor=f8fafc&labelTextColor=f8fafc" alt="Forks" /></a>
<a href="https://github.com/ereinaimer/taurine/issues"><img src="https://shieldcn.dev/github/issues/ereinaimer/taurine.svg?mode=dark&size=xs&color=06b6d4&logoColor=f8fafc&labelTextColor=f8fafc" alt="Issues" /></a>
<a href="https://github.com/ereinaimer/taurine/pulls"><img src="https://shieldcn.dev/github/open-prs/ereinaimer/taurine.svg?mode=dark&size=xs&color=14b8a6&logoColor=f8fafc&labelTextColor=f8fafc" alt="Pull Requests" /></a>
<a href="https://www.rust-lang.org/"><img src="https://shieldcn.dev/badge/built%20in%20rust.svg?mode=dark&size=xs&logo=rust&color=ff964c" alt="Built in Rust" /></a>
<a href="https://ratatui.rs/"><img src="https://shieldcn.dev/badge/built%20in-ratatui.svg?mode=dark&size=xs&logo=ratatui&color=e11d48&logoColor=f8fafc&labelTextColor=f8fafc" alt="Built in Ratatui" /></a> -->

</div>

## What is Taurine?

Taurine is a local-first automation tool for people who type a lot.

It works like a text expander, but it can do much much more than replace short words with long text. Taurine can also run scripts, open apps, launch websites, generate values, and trigger automations with hotkeys.

The goal is simple: **Stop repeating the same computer tasks every day.**

## Quick demo

Let's create a simple text expansion to see how Taurine works.

Open a terminal and run the following command:

1. **Start the Taurine service**:
   ```bash
   taurine up
   ```

2. **Add your first shortcut**:
   ```bash
   taurine add hello "Hello, world!"
   ```

3. **Try it anywhere**:
   Type `>hello` followed by a **space** in any text field (browser, editor, chat).
   It instantly expands to: `Hello, world!`

> [!TIP]
> **Prefer a UI?** Simply run `taurine` to open the interactive Terminal UI and manage your automations visually.

## Why Taurine?

A lot of computer work is repetitive.

- **Typing the same things**: Email addresses, signatures, and common replies.
- **Opening the same apps**: Switching between tools and websites manually.
- **Running the same commands**: Repeating terminal workflows.
- **Copy-pasting AI responses**: Switching to a browser just to get an answer.

Taurine saves time by turning short triggers into longer text, keyboard automation, script execution, and inline AI in one simple package.

## Features

- [**Text expansion**](#text-expansion) — turn short triggers into full text
- [**Keyboard shortcuts**](#keyboard-shortcuts-and-hotkeys) — run automations with hotkeys
- [**Scripts and commands**](#scripts-and-commands) — launch apps, open websites, and run local scripts
- [**Dynamic variables**](#dynamic-variables) — insert names, dates, UUIDs, mock data, and more
- [**Inline math**](#inline-math) — calculate while typing
- [**Inline AI**](#inline-ai) — use AI without switching apps
- [**CLI and TUI**](#basic-commands) — manage automations from the terminal
- [**Import and export**](#import-and-export) — backup and share your automations

## Installation

Prebuilt binaries are available for Windows, macOS, and Linux. You can download them manually from our [GitHub Releases](https://github.com/ereinaimer/taurine/releases) page, or use one of the quick install scripts below:

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/ereinaimer/taurine/main/scripts/install/install.ps1 | iex
```

### macOS / Linux (Bash)

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/ereinaimer/taurine/main/scripts/install/install.sh | sh
```

<details>
<summary><b>Install via Cargo</b></summary>

<br>

If you already have Rust installed, you can install Taurine directly from the repository using cargo:

```bash
cargo install --git https://github.com/ereinaimer/taurine
```
</details>

<details>
<summary><b>Build from source</b></summary>

<br>

Requirements:

<details>
<summary>Install Rust</summary>

You can install Rust using the official command from [rustup.rs](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

If you're on Windows, Download and run:
[rustup-init.exe](https://win.rustup.rs)
</details>

Once Rust is ready, clone and build the project:

```bash
git clone https://github.com/ereinaimer/taurine.git
cd taurine
cargo build -r
```

Run Taurine:

```bash
./target/release/taurine --help
```

</details>

## Managing the service

The Taurine daemon must be running in the background for text expansion and hotkeys to work.

*   **Start the daemon**: `taurine up`
*   **Check the status**: `taurine status`
*   **Stop the daemon**: `taurine down`
*   **Restart the daemon**: `taurine restart`

## Text expansion

Create shortcuts for text you use often.

```bash
taurine add em "jane.doe@gmail.com"
```

Then type this anywhere:

```text
>em 
```

Taurine replaces it with your full email address instantly.

<details>
<summary>Usages</summary>

You can use text expansion for:

* Email signatures
* Common replies
* Addresses
* Links
* Code snippets
* Support messages
* Templates
* Notes
* Repeated phrases
</details>

## Keyboard shortcuts and hotkeys

Taurine can also trigger automations with hotkeys.

That means you can press a keyboard shortcut to open an app, launch a website, run a command, or execute a script.

Here's how you can setup a hotkey to open a website you frequently visit through a Windows Powershell script:

```powershell
taurine add script --hotkey 'alt+y' 'Start-Process https://youtube.com' -l powershell -m silent
```

Now press **Alt + Y** and Taurine will instantly open YouTube in your default browser.

<details>
<summary>Usages</summary>

For example, you could create hotkeys for:

* Alt + W = Opening WhatsApp
* Alt + C = Opening ChatGPT
* Alt + G = Opening GitHub
* Alt + N = Opening notes app
* Alt + P = Opening project folder
* Alt + R = Running a PowerShell script
</details>

## Scripts and commands

> [!WARNING]
> Taurine can run scripts and type into other apps. This is a powerful feature, so only run automations you trust. Always read and verify any automation script before adding it to your system.

Taurine can run scripts when you trigger an automation.

This is useful when a normal text snippet is not enough.

Add an inline script:

```bash
taurine add script ip "Invoke-RestMethod https://api.ipify.org" -l powershell
```

Add a script file:

```bash
taurine add script build --file ./scripts/build.sh -l bash
```

<details>
<summary>Usages</summary>

You can create automations that:

* Open a website
* Run a shell command
* Insert command output into the current app
* Automate developer workflows
</details>

## Dynamic variables

Taurine allows you to create flexible snippets using `[variable_name]` placeholders. You can pass values to these placeholders at expansion time by appending them to the shortcut with a colon.

For example, if you add this shortcut:

```bash
taurine add hi "Hello, [name]!"
```

Typing `>hi:erein ` will expand to:

```text
Hello, erein!
```

### Default values

You can provide a fallback by using the `[variable=default]` syntax. If no value is passed, Taurine uses the default.

```bash
taurine add hi "Hello, [name=friend]!"
```

*   **With value**: `>hi:erein ` → `Hello, erein!`
*   **Without value**: `>hi ` → `Hello, friend!`

### Arguments with spaces

If your argument contains spaces, wrap it in single (`'`) or double (`"`) quotes:

*   **Double quotes**: `>hi:"John Doe" ` → `Hello, John Doe!`
*   **Single quotes**: `>hi:'Jane Doe' ` → `Hello, Jane Doe!`

<details>
<summary>Usages</summary>

Dynamic variables are perfect for:

*   **Personalized Emails**: `Hi [name], checking in on [topic=our last meeting]...`
*   **Support Work**: `Ref: Ticket #[id]. Status: [status=Resolved].`
*   **Development**: `console.log("[label=DEBUG]:", [value]);`
*   **Quick Links**: `https://github.com/search?q=[query]`
*   **Templated Notes**: `Meeting with [person] regarding [project=Taurine].`

Infinite Possibilities!
</details>

### Transformers

Transformers allow you to manipulate text casing, encoding, and more using a simple dot notation. You can chain multiple transformers together (e.g., `[variable.trim.upper]`) for complex formatting.

#### Example 1: Formatting with Defaults
Combine transformers with default values to ensure your snippets always look perfect:

```bash
taurine add hi "Hello, [name.title=friend]!"
```
*   **With value**: `>hi:erein ` → `Hello, Erein!`
*   **Without value**: `>hi ` → `Hello, Friend!`

#### Example 2: Clean Search Links
Automatically encode text for use in URLs:

```bash
taurine add search "https://google.com/search?q=[query | url.encode]"
```

For a full list of available transformers and advanced chaining, check the [Transformers Documentation](#todo:link-to-transformers-docs).

### System Variables

Taurine includes a wide range of **built-in system variables** that inject data directly from your machine or generate values on the fly. These use a `[namespace.variable]` syntax to avoid clashing with your custom variables.

Some of the most useful system variables include:

*   **Date/Time**: `[date]`, `[time]`, `[date.calc(+1d)]`
*   **Clipboard**: `[clip]` (inserts your current clipboard text)
*   **Network**: `[net.ip]`, `[net.lip]`, `[net.online]`
*   **Random**: `[random.pass]`, `[uuid.v4]`, `[lorem.sentence]`
*   **Environment**: `[env(USER)]` (access any environment variable)

For the full list of available namespaces and variables, refer to the [System Variables Documentation](#todo:link-to-system-variables-docs).

**Timestamping:**
```bash
taurine add timestamp "Created on: [date.format('MMMM D, YYYY')] at [time]"
```

**Generating test data:**
```bash
taurine add user "Name: [mock.name]\nEmail: [mock.email]\nJob: [mock.job_title]"
```

## Inline math

Taurine features a built-in mathematical engine that lets you use any text field as a quick calculator. Simply type a mathematical expression and press **Space**.

Example:
- `>5+3 ` expands to `8`
- `>(10+5)*2 ` expands to `30`
- `>2pi ` expands to `6.2832`

<details>
<summary>Supported Features</summary>

- **Operators**: `+`, `-`, `*`, `/`, `%` (modulo), `^` (power)
- **Constants**: `pi`, `e` (case-insensitive)
- **Functions**: `sqrt(x)`, `abs(x)`, `floor(x)`, `ceil(x)`, `round(x)`
- **Implicit Multiplication**: `>2(3+4)` → `14`, `>3pi` → `9.4248`
- **Scientific Notation**: `>1.5e3` → `1500`, `>1e-2` → `0.01`

Results are automatically formatted up to 4 decimal places.
</details>

## Inline AI

Taurine brings AI directly into the app you are already using. Instead of switching to a browser, you can stream AI responses directly into your active text field.

### Setup

1.  **Set your provider**:
    ```bash
    taurine config set ai_provider gemini
    ```
    (You can also configure this through the Taurine TUI).

2.  **Set your API key**:
    Input your API key when prompted. It will be stored safely on your system.

3.  **Choose your model**:
    View available models for your provider:
    ```bash
    taurine ai models --provider gemini
    ```
    Set the model you want to use:
    ```bash
    taurine config set ai_model <model-name>
    ```

### How to use

Trigger AI expansion just like any other shortcut:

1.  Type `>ai ` followed by a **Space**.
2.  A backtick (`` ` ``) appears. Type your prompt for the AI.
3.  Close the prompt with another backtick (`` ` ``).
4.  Hit **Space**.

Taurine will generate the response and insert it right where you are typing.

## Import and export

Taurine allows you to backup your automations, move them to another machine, or share them with others. Exports use the .tau file format.

### Export

You can export your automations using the CLI:

```bash
taurine export <path> <flags>
```

By default, if no path parameter is passed, it exports to the current working directory.

Options:
* `--no-encrypt`: Export without encryption (default is encrypted).
* `--with-settings`: Include your app settings in the export.
* `--with-metrics`: Include your usage stats.

### Import

To import automations:

```bash
taurine import <path> <flags>
```

Options:
* `--on-conflict <action>`: How to resolve trigger collisions (prompt, skip, overwrite).
* `--include-settings`: Overwrite local settings with imported values.
* `--include-metrics [mode]`: Include imported metrics (ignore, merge, overwrite).

You can also use the Terminal UI to manage imports and exports visually by pressing `i` or `x` in the library view.

## Basic commands

Add a text shortcut:

```bash
taurine add em "jane.doe@gmail.com"
```

List your automations (alias `ls`):

```bash
taurine list
```

Remove an automation (alias `rm`):

```bash
taurine remove sig
```

Open help:

```bash
taurine --help
```

## Documentation

Full documentation is coming soon. Use `taurine --help` for local command reference.

## FAQ

### Does Taurine work offline?
Yes, Taurine is local-first. Text expansion, scripts, and hotkeys work entirely offline. Only Inline AI requires an internet connection to reach the AI provider.

### Do I need to know programming?
No. You do not need to be a programmer to use the basic text expansion features. If you can remember a shortcut like `>sig`, you can use Taurine.

### Is Taurine only for developers?
No. While it has powerful features for developers, it is for anyone who repeats the same typing or computer actions every day, including writers, students, and support teams.

### Does Taurine support Windows, macOS, and Linux?
Yes, Taurine is designed to be cross-platform and will have prebuilt binaries for all three major operating systems.

### Can Taurine run scripts?
Yes. You can add scripts using the `taurine add script` command and trigger them with text triggers or hotkeys.

### Is inline AI required?
No, Inline AI is an optional feature. You can use Taurine solely for text expansion and hotkeys without ever configuring an AI provider.

### Where are my automations stored?
Your automations are stored locally on your machine in a secure database. Taurine is local-first and respects your privacy.

## AI Usage

AI coding assistants were utilized during the development of Taurine for faster iteration. However, to ensure quality and security, the codebase is regularly audited through a comprehensive test suite of 700+ tests and manual review.

## Credits

Taurine is built on the shoulders of some outstanding open source projects. See [CREDITS.md](CREDITS.md) for a full list of dependencies and tooling used in this project.

## License

Taurine is source-available under the **Aimer Software License (ASL)**. It is free for non-commercial personal use. Commercial rights are reserved.

See the [LICENSE](https://github.com/ereinaimer/taurine/blob/main/LICENSE) file for full terms.

## Support Taurine

If Taurine helps you, you can support the project by starring the repo, reporting bugs, sharing feedback, or by directly shaping its future through contributions. Check out [CONTRIBUTING.md](CONTRIBUTING.md) to get started!

<a href="https://x.com/intent/tweet?text=I%27m%20using%20Taurine%20to%20save%20a%20lot%20of%20time%20with%20fast%20text%20expansions%20and%20keyboard%20automations%2C%20check%20it%20out%3A%20https%3A%2F%2Fgithub.com%2Fereinaimer%2Ftaurine"><img src="https://shieldcn.dev/badge/share%20on%20x.svg?mode=dark&size=xs&logo=x&color=000000&logoColor=f8fafc&labelTextColor=f8fafc" alt="Share on X" /></a>
<a href="https://www.reddit.com/submit?title=I%27m%20using%20Taurine%20to%20save%20a%20lot%20of%20time%20with%20fast%20text%20expansions%20and%20keyboard%20automations%2C%20check%20it%20out%3A&url=https%3A%2F%2Fgithub.com%2Fereinaimer%2Ftaurine"><img src="https://shieldcn.dev/badge/share%20on%20reddit.svg?mode=dark&size=xs&logo=reddit&color=ff4500&logoColor=f8fafc&labelTextColor=f8fafc" alt="Share on Reddit" /></a>
<a href="https://www.linkedin.com/sharing/share-offsite/?url=https%3A%2F%2Fgithub.com%2Fereinaimer%2Ftaurine"><img src="https://shieldcn.dev/badge/share%20on-linkedin.svg?mode=dark&size=xs&logo=ri%3ABsLinkedin&color=0077b5&logoColor=f8fafc&labelTextColor=f8fafc" alt="Share on LinkedIn" /></a>
<a href="https://mastodon.social/share?text=I%27m%20using%20Taurine%20to%20save%20a%20lot%20of%20time%20with%20fast%20text%20expansions%20and%20keyboard%20automations%2C%20check%20it%20out%3A%20https%3A%2F%2Fgithub.com%2Fereinaimer%2Ftaurine"><img src="https://shieldcn.dev/badge/share%20on%20mastodon.svg?mode=dark&size=xs&logo=mastodon&color=6364ff&logoColor=f8fafc&labelTextColor=f8fafc" alt="Share on Mastodon" /></a>
<a href="https://api.whatsapp.com/send?text=I%27m%20using%20Taurine%20to%20save%20a%20lot%20of%20time%20with%20fast%20text%20expansions%20and%20keyboard%20automations%2C%20check%20it%20out%3A%20https%3A%2F%2Fgithub.com%2Fereinaimer%2Ftaurine"><img src="https://shieldcn.dev/badge/share%20on%20whatsapp.svg?mode=dark&size=xs&logo=whatsapp&color=25d366&logoColor=f8fafc&labelTextColor=f8fafc" alt="Share on WhatsApp" /></a>
<a href="https://t.me/share/url?url=https%3A%2F%2Fgithub.com%2Fereinaimer%2Ftaurine&text=I%27m%20using%20Taurine%20to%20save%20a%20lot%20of%20time%20with%20fast%20text%20expansions%20and%20keyboard%20automations%2C%20check%20it%20out%3A"><img src="https://shieldcn.dev/badge/share%20on%20telegram.svg?mode=dark&size=xs&logo=telegram&color=0088cc&logoColor=f8fafc&labelTextColor=f8fafc" alt="Share on Telegram" /></a>
