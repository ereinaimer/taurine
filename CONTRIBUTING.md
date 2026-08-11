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
sudo apt install build-essential protobuf-compiler libdbus-1-dev pkg-config libasound2-dev libxkbcommon-dev mold -y
```
`mold` is the linker used for all Linux builds (it's the fastest linker available and dramatically cuts link times). If your distribution's GCC is older than 12.1, install `clang` as well and the build will use it as the linker driver instead.

#### Windows
If you are compiling Taurine on Windows, you must install the Protocol Buffers compiler. You can easily do this using `winget`:
```powershell
winget install protobuf
```

#### macOS
No additional system dependencies are required.

#### Linkers
The repository pre-configures the fastest available linker for each platform, so there's nothing to install for Windows (`rust-lld`, ships with the Rust toolchain) or macOS (`rust-lld`). Linux uses `mold`, which must be installed separately (see above).

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
cargo test
```

### 7. Optional Local Speedups
The linker configuration is already set up in-repo, so a fresh clone builds fast out of the box. These extras shave more time:

- **sccache** — caches compiled dependencies across branches and projects. Install it (`cargo install sccache`), then set the `RUSTC_WRAPPER` environment variable to `sccache` in your shell profile, or add `[build] rustc-wrapper = "sccache"` to your `~/.cargo/config.toml`.
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
