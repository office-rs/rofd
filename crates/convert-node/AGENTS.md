# rofd-convert-node

> Auto-loaded when you work in this subtree. The convert-line picture:
> crates/convert/AGENTS.md; build/demo: docs/agents/commands.md §four
> bindings.

The napi binding (phase 3): the cdylib `rofd_convert_node` (napi 2), the
npm package `@office-rs/rofd-convert` skeleton (platform packages ride
optionalDependencies slots). Depends only on rofd-convert + napi 2.

- `convertToPdf` (the only kernel call that can panic) goes through
  `catch_unwind` — a panic at the napi boundary would **abort Node**; the
  registration functions are pure bookkeeping and cannot panic.
- `Converter.convertToPdf(ofd, options?)` + the five registration helpers.
- The JS L2 `index.js` / `pool.js`: one instance per worker_threads
  worker, crash → reject + respawn; `cli.js` is the npm bin
  `rofd-convert`.
- Acceptance: `node demo/run.js` — the worker_threads pool == CLI bytes.
