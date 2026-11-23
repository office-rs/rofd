# rofd-convert-java

> Auto-loaded when you work in this subtree. The convert-line picture:
> crates/convert/AGENTS.md; build/demo: docs/agents/commands.md §four
> bindings.

The JNI binding (phase 2): the JNI cdylib `rofdconvert` + the Java L1/L2
(package `io.github.officers`), Maven `io.github.office-rs:rofd-convert`
(thin/fat jars; fat requires all four platforms' natives). Depends only on
rofd-convert + jni 0.21.

- All 10 JNI entry points (NativeLink's `nNewConverter`…`nTakeWarnings`)
  go through `catch_panics` — a Rust panic never crosses FFI; it becomes
  `io.github.officers.OfdConvertException`.
- **Lease handle table**: every JNI call `take`s a Converter and `put`s it
  back at the end; `InUse` (concurrent occupancy) / `Stale` (closed/unknown
  id) are stable exception prefixes; ids increase monotonically with no
  ABA.
- L2: `OfdConverter`(+Options) is AutoCloseable; the threading contract is
  **one thread at a time** (D15 — use the pool to share); a
  PhantomReference cleaner thread reclaims handles from missed closes
  (Java 8 has no Cleaner API); `OfdConverterPool` is a fixed-slot pool
  (`borrow()` blocks until free).
- NativeLoader's three-tier zero-config loading: `rofd.native.path` /
  `ROFD_NATIVE_PATH` (explicit override) → classpath
  `natives/<platform>/<libfile>` (embedded in the fat jar) →
  `System.loadLibrary` fallback; Java 8 bytecode (major 52).
- Acceptance: `bash demo/run.sh`, the two-act demo — the pooled JVM
  conversion is **byte-identical** with the CLI.
