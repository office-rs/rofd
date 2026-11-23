# rofd-convert-cli

> Auto-loaded when you work in this subtree. The convert-line picture:
> crates/convert/AGENTS.md + docs/agents/commands.md.

The bin `rofd-convert`: depends only on rofd-convert (golden rule 7).

- **Zero-dependency argv parsing** (no clap).
- Exit codes: **0** success / **2** usage error / **3** conversion failure.
- `--font` / `--fonts-dir` / `--default-font` registration failures are
  **warn-and-continue**.
- `--no-watermark` / `--no-annotations` / `--no-seal` (content toggles
  default all-on, D17 — only opt-outs are offered).
- Warnings go to stderr (`ConvertWarning` Display, one per line).
