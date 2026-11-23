# docs/agents — agent reference (cross-cutting topics)

Agent-facing reference for topics that span multiple crates and therefore
have no single subtree for a nested `AGENTS.md`. Read on demand, per the
routing table in the root `AGENTS.md`; subtree-specific rules live in nested
`crates/*/AGENTS.md` (auto-loaded when you work there).

| File | When to read |
|---|---|
| [commands.md](commands.md) | Before running anything beyond the core commands; full command reference + dev-machine pitfalls |
| [workflow.md](workflow.md) | Git / commits / release lockstep / pinned versions / test conventions |
| [architecture.md](architecture.md) | Changing crate boundaries, public APIs, or crossing layers; full crate map + print architecture |
| [io.md](io.md) | io full write / write fidelity / three-layer passthrough / ID conventions / error layering |
| [editing.md](editing.md) | Annotation editing, flowed text layout, rich text, format panel, IME |
| [fonts.md](fonts.md) | Anything touching fonts (slice chains, system channels, process registry, fallback chain) |
| [rendering.md](rendering.md) | Render pipeline, Scene IR, shaping determinism, CPU fallback |
| [conventions.md](conventions.md) | Rulings, doc-audit duty, Do/Don't, known risks |

Positioning: `docs/superpowers/specs|plans|manual/` is the append-only
process archive (the newest spec is the source of truth); this directory is
the agent layer — link to the authoritative spec instead of restating it;
when a spec moves, update the link.
