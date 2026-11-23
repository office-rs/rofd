# rofd-convert-py

> Auto-loaded when you work in this subtree. The convert-line picture:
> crates/convert/AGENTS.md; build/demo: docs/agents/commands.md §four
> bindings (maturin must be pointed at the 3.14 interpreter explicitly).

The PyO3 binding (phase 3): the extension
`rofd_convert.rofd_convert_native`, the PyPI package `rofd-convert`
(abi3-py39 wheel). Depends only on rofd-convert + pyo3 0.30.

- `#[pyclass] Converter` holds a `Mutex<Option<…>>` **lease** (the same
  mechanics as the JNI handle table) — a second concurrent call on the
  same instance during the GIL-released window (`py.detach`) raises
  `ConvertError("converter in use")`.
- `convert(ofd, *, watermark=…, max_pages=…)` kwargs; throughput uses
  `threading.local`, one instance per thread.
- The build needs maturin ≥1.16 plus an explicit `-i` pointing at the
  Python 3.14 interpreter (the bare PATH python is 3.7 and cannot read
  3.14 metadata).
- Acceptance: `py -3.1 demo/run.py` — the ThreadPoolExecutor pool == CLI
  bytes.
