# rofd-convert (Java)

JNI bindings for the rofd conversion kernel (OFD to PDF). Java 8 compatible (`AutoCloseable` API, no external dependencies), Maven-distributed. The main jar embeds all four platform natives, so a single plain dependency loads with zero configuration; the per-platform classifier jars remain available as an option. An `OfdConverter` is a long-lived instance wrapping a native engine: OFD bytes in, PDF bytes out.

[中文文档](README.zh-CN.md)

## Install

Maven `io.github.office-rs:rofd-convert`. One plain dependency is the entire setup:

```xml
<dependency>
  <groupId>io.github.office-rs</groupId>
  <artifactId>rofd-convert</artifactId>
  <version>0.1.0</version>
</dependency>
```

The main jar embeds the natives of all four platforms and loads zero-config: at first use the detected platform's library is extracted to a temp directory and loaded from there by absolute path — no `java.library.path`, no classifier, no configuration.

- **Size-sensitive setups** may use the per-platform classifier jars instead (`linux-x86_64` / `windows-x86_64` / `macos-x86_64` / `macos-aarch64`). The loader looks up the same `natives/<platform>/<lib>` resource whether it lives in the fat main jar or in a classifier jar on the classpath.
- **Explicit override**: `-Drofd.native.path=<file-or-dir>` (or the `ROFD_NATIVE_PATH` environment variable; the property wins) pins a specific library — for custom or patched builds. When set but not loadable it fails fast with `UnsatisfiedLinkError` instead of silently falling back to the embedded library, so a custom build that did not take effect cannot go unnoticed.

For dev runs against a locally built cdylib, `-Djava.library.path=target/release` keeps working — the loader falls back to `System.loadLibrary` when no classpath resource matches.

Local builds from this repo (`cd crates/convert-java`):

```bash
mvn -DskipTests package                                              # thin main jar (no natives inside)
mvn -DskipTests package -Dnative.dir=<staging> -Dskip.natives.check=true   # fat, single-platform local packaging
```

- A plain `mvn package` always produces the **thin** jar — the fat shape requires `-Dnative.dir`; the published main jar is fat because the release pipeline stages all four platforms.
- Switch jar shapes with `mvn clean` between builds — a previous fat packaging leaves `natives/` in `target/classes`, which a plain re-package would silently embed into the "thin" jar.

The staging dir holds `<platform>/<lib>` trees (e.g. `windows-x86_64/rofdconvert.dll`, built by `cargo build --release -p rofd-convert-java`). Without `-Dskip.natives.check=true` the build enforces that all four platforms are staged — release packaging must never ship a partial fat jar.

## Fonts

Registration is process-wide, append-only, and idempotent on identical bytes — do it once at startup, **before creating the first converter** (each converter snapshots the process registry at creation; later registrations only become visible to new instances):

```java
OfdConverter.registerFontFile("fonts/simsun.ttc");      // true = registered
OfdConverter.registerFont(fontBytes);                   // boolean
OfdConverter.registerDefaultFont(fontBytes);            // anchor the process default font; a later anchor wins
boolean anchored = OfdConverter.hasDefaultFont();
String[] families = OfdConverter.registeredFamilies();  // registered only
```

Documents declaring an unregistered family still convert — the export layer degrades to warnings and skipped glyphs (drained via `takeWarnings()`), never a hard failure. Register a CJK font (e.g. `simsun.ttc`) up front for full-fidelity output on real-text documents.

## Single instance (no pool)

One `OfdConverter` per unit of work, created via the static factory. Thread contract: **one thread at a time** — do not share an instance across threads. `close()` (or try-with-resources) releases the native handle; a PhantomReference cleaner reclaims leaked instances as a last resort.

```java
try (OfdConverter converter = OfdConverter.create()) {
    byte[] pdf = converter.convertPdf(ofdBytes);
    for (String w : converter.takeWarnings()) {   // drain this conversion's warnings
        System.err.println("warning: " + w);
    }
    // with content flags / budgets:
    OfdConverter.OfdConvertOptions opts = new OfdConverter.OfdConvertOptions();
    opts.watermark = false;      // null = kernel default (on)
    opts.maxPages = 5_000L;      // null or <= 0 = kernel default (10_000)
    byte[] pdf2 = converter.convertPdf(ofdBytes, opts);
}
```

Failures throw `OfdConvertException` (unchecked): decode/layout/encode failures, budget breaches, and internal errors all surface there — kernel panics never cross the JNI boundary. v1 has a single output format (PDF); no format parameter is exposed.

## With a pool

For concurrent conversion, use `OfdConverterPool`: a fixed set of slots, `borrow()` blocks until one is free (60s cap — a stuck lease is a bug), always `returnConverter()` in a `finally`:

```java
try (OfdConverterPool pool = new OfdConverterPool(4)) { // 4 native instances
    List<Callable<byte[]>> tasks = /* one callable per document */;
    ExecutorService exec = Executors.newFixedThreadPool(8);  // threads may outnumber slots
    List<Future<byte[]>> results = exec.invokeAll(tasks);
    // inside each callable:
    //   OfdConverter c = pool.borrow();
    //   try { return c.convertPdf(ofd); } finally { pool.returnConverter(c); }
}
```

With the same process font state, all instances produce **byte-identical output** for identical input — pool size and thread interleaving do not affect the bytes (the two-act acceptance demo in `crates/convert-java/demo/` pins this against the CLI).

## See also

- Kernel API: [crates/convert](../convert) · CLI [crates/convert-cli](../convert-cli)
- Spec: `docs/superpowers/specs/2026-11-06-rofd-convert-design.md` §6.4
