# Contributing to Taurine

First off, thank you for considering contributing to Taurine! We appreciate your time and effort. 

Whether you're helping us fix bugs, build new features, or improve our documentation, we'd love to have you on board.

And if you like the project, but just don't have time to contribute, that's fine. There are other easy ways to support the project and show your appreciation, which we would also be very happy about:

- Star the project
- Mention the project on social media
- Refer this project in your project's readme
- Mention the project at local meetups and tell your friends/colleagues


> Please join our [Discord Server](https://discord.gg/Kc9XmHJgsS) and consult with the maintainers before trying to develop any new features yourself. This ensures your efforts are aligned with the project roadmap!

## How to Contribute

### 1. Find an Issue
You can start by looking through our open issues. If you want to work on something specific that isn't listed, please [create a new issue](https://github.com/ereinaimer/taurine/issues/new/choose) to discuss it before you begin writing code.

### 2. Fork and Branch
- Fork the repository and clone it locally.
    ```bash
    git clone https://github.com/ereinaimer/taurine.git
    cd taurine
    ```
- Create a new branch for your feature or bugfix: `feature/` or `fix/`
    ```bash
    git checkout -b feature/your-feature-name
    ```

### 3. Install System Dependencies

#### Linux
If you are compiling Taurine on a Linux system, you must install the following system dependencies:
```bash
sudo apt update
sudo apt install build-essential protobuf-compiler libdbus-1-dev pkg-config libssl-dev libasound2-dev libxkbcommon-dev mold sccache -y
```
`libssl-dev` lets dev builds link the system OpenSSL instead of compiling a private copy from source (see the fast loop below). `mold` is the linker used for all Linux builds (it's the fastest linker available and dramatically cuts link times). If your distribution's GCC is older than 12.1, install `clang` as well and the build will use it as the linker driver instead.

#### Windows
If you are compiling Taurine on Windows, you must install Protocol Buffers, `sccache`, and Strawberry Perl. You can easily do this using `winget`:
```powershell
winget install protobuf
winget install Mozilla.sccache
winget install StrawberryPerl.StrawberryPerl
winget install ShiningLight.OpenSSL.Full
```
Strawberry Perl is required because the default (release/CI) build compiles OpenSSL from source, and OpenSSL's build script needs a native Windows Perl (the Perl bundled with Git for Windows does not work). After installing, open a fresh terminal so `perl` is on your `PATH` before running `cargo` commands.

The Full (not Light) OpenSSL package ships the MSVC import libraries the dev loop needs. Then point the build at it once per machine:
```powershell
setx OPENSSL_DIR "C:\Program Files\OpenSSL-Win64"
```
(The Light package has no import libraries, so dev builds cannot link against it. Strawberry Perl's copy also works but needs a hand-built import library — prefer Full.)

#### macOS
Install `sccache` via Homebrew:
```bash
brew install sccache
```
macOS dev builds usually link the system SecurityFramework with no extra setup. If you installed Homebrew OpenSSL and the build cannot find it, point the build at it with `OPENSSL_DIR` (e.g. the Homebrew `opt/openssl@3` prefix).

#### Fast dev loop (skip the vendored OpenSSL)
The default build compiles a private copy of OpenSSL from source (several minutes, needs Perl). Dev builds can link your system copy instead — same encrypted database, fraction of the time:
```bash
cargo check -p taurine_core --no-default-features
cargo check -p taurine_daemon --no-default-features
```
`--no-default-features` is what selects system crypto (plus skips voice ML on the daemon); without it you get the slow vendored build by design. Full `cargo check --workspace` / `cargo nextest run` (defaults) are for pre-push only.

Optional, voice builds only: skip the ~120MB sherpa download by fetching the prebuilt lib archive matching the `sherpa-onnx-sys` version in `Cargo.lock` from the sherpa-onnx releases page, extracting it once, and setting `SHERPA_ONNX_LIB_DIR` to its `lib` folder.

#### Compiler Wrapper & Linkers
`sccache` is the recommended local compiler cache (install only, wired per-machine so CI stays untouched): set the rustc wrapper and C-compiler launcher env vars in your shell or user cargo config to reuse compiled dependencies across builds. Fastest available linker per platform: Windows (`rust-lld`, ships with the Rust toolchain), macOS (`rust-lld`), and Linux (`mold`).

### 4. Setup Pre-commit (Recommended)
We use `pre-commit` to automatically run code formatters and linters (`cargo fmt` and `cargo clippy`) before every commit. This ensures clean code and prevents CI from failing over simple styling issues.

To set it up, install `pre-commit` via Python's package manager, then install the hooks for this repo:
```bash
pip install pre-commit
pre-commit install -c scripts/pre-commit.yaml
```

### 5. Make Your Changes
- Write clear, concise code and include comments where necessary.
- Ensure your changes follow the existing coding style of the project.
- If you're adding a new feature, consider adding tests for it.

### 6. Test Your Code
Before submitting your changes, please make sure everything builds correctly and that all tests pass:

```bash
cargo check
cargo nextest run --workspace --all-features
```

The default suite is hermetic: it never types keys, touches the clipboard, opens windows, spawns processes, plays audio, opens the mic, or writes outside temp dirs, so it is safe to run while gaming. Host-affecting tests (real `SendInput`, process spawns) are `#[ignore]`-gated and need an explicit opt-in on a throwaway machine with nothing focused:

```bash
TAURINE_ALLOW_HOST_INPUT=1 cargo test -- --ignored
```

See `AGENTS.md` (Test Isolation rule) before adding any test that touches the host.

### 7. Optional Local Speedups
- **Windows: Dev Drive** — if you have Windows 11 22H2+, move the project and your Cargo registry (`~/.cargo`) to a Dev Drive. It bypasses Defender's real-time scan of the thousands of tiny files involved in a Rust build, which is often the single biggest local speedup on Windows.

### 8. Submit a Pull Request
- Create a Pull Request (PR) against our `main` branch.
- Use the provided PR template to describe your changes and link any relevant issues.
- Once submitted, we will review your PR and provide feedback! 

## License

Please note that Taurine uses a custom Source-Available license (see [`LICENSE`](./LICENSE)). By contributing to the project, you agree to license your contributions under its terms. We've included special provisions so you can freely showcase your contributions in your portfolios and CVs!

## Community & Conduct

To ensure a welcoming environment for everyone, we ask that all contributors review and follow our [Code of Conduct](./CODE_OF_CONDUCT.md).

Thank you for helping make Taurine better!
