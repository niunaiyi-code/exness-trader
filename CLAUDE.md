# CLAUDE.md

## Model

This project uses a single model for everything: `claude-fable-5-2` via `https://api.thqllm.com`.

There are no custom subagents and no model routing. Launch with `claude` from either PowerShell or bash.

## Project facts

- Rust workspace: `crates/`
- Web frontend: `web/`
- Build/compile scripts: `scripts/`
- MQL5 sources live outside this repo, under `../mql5/`; use LF line endings there.
- `GL_LOG` / `GL_PRINT` macros are intentional commercial-build switches, not dead code.
- Supports both standard (USD) and cent (USC) accounts; notification thresholds use 1000-unit steps for both by design.
- Swap-free accounts: no overnight interest.
- News avoidance blocks first orders only, not grid additions (by design).
- Never run `git commit` or `git push` unless explicitly asked.

