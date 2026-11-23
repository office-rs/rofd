# rofd-io

> Auto-loaded when you work in this subtree. **The main rules live in
> docs/agents/io.md** (full write/write fidelity/three-layer
> passthrough/ID conventions/error layering; spec
> 2026-11-11-rofd-full-ofd-write) — read it before touching this crate.

Subtree essentials:

- `parse_ofd` delivers `LoadReport { document, package, warnings }`;
  `PackageHandle`'s `consumed` set is the package-layer passthrough filter.
- The only write entry is `write_ofd_with_text_layout(doc, pkg, hints)`;
  `write_ofd(doc)` is the pure-generation convenience face. The surgical
  save is retired (D1).
- After changing save logic, `cargo test -p rofd-io write_fidelity` must
  stay green (real samples: `xilem-app --test c2_save -- --ignored`).
- `rofd-io` + `rofd-dom` are rword's cross-repo frozen surface (git
  dependency following tags): a public API change is a cross-repo break —
  think about the rword side first (docs/agents/architecture.md
  §cross-repo frozen surface).
