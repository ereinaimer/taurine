# Project Overview & Tech Stack
- **Backend:** Rust.
- **Knowledge Graph:** Uses `graphify` for deterministic AST-based dependency tracking.

# Architectural Guidelines
- **Separation of Concerns:** Keep files modular. Do not create monolithic, 1,000-line behemoth files. When a file grows too large, proactively suggest extracting logic into a new, dedicated module.
- **Rust Standards:** Adhere strictly to idiomatic Rust. Run `cargo fmt` and ensure there are zero `cargo clippy` warnings before finalizing any backend code. 

# Operational Boundaries
- **NEVER** commit any files to version control yourself.
- **NEVER** push to any remote branches.
- The user will handle all git operations manually.
- **Changelog Maintenance:** Whenever a breaking change or fix is implemented, you must modify the `CHANGELOG.md` file in the project root. Add a clear, concise entry under the respective section tracking those specific changes before concluding the task.

# Git Commit Standards
We strictly follow Conventional Commits. When proposing a commit message, you must adhere to these exact rules:
1. **Header:** `type(scope): concise description`
2. **Body:** Separated from the header by a single blank line. Use concise bullet points.
3. **Format Constraint:** You are strictly forbidden from using backticks (`) anywhere within the commit message body.

### Expected Output Format:
```text
feat(engine): extract text expansion logic to module

- decouple sys namespace variables from core engine
- enforce utf-8 boundary safety in backspace undo logic
``