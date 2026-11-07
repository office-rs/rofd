package io.github.officers;

import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.net.URL;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Properties;
import java.util.function.Function;

/**
 * Tiered native-library loading for the JNI binding (convert spec §6.4:
 * thin {@code java.library.path} jar + zero-config fat jar, after rword's
 * 2026-11-05 zero-config design). Package-private on purpose — not part
 * of the public API surface; {@link NativeLink}'s static block is the
 * only caller.
 *
 * <p>Priority chain — a tier is only reached when the previous tier found
 * <em>nothing</em>; a tier that <em>found</em> something but failed to load
 * (missing file, corrupt library, wrong architecture) throws immediately,
 * never silently falling back:
 * <ol>
 *   <li>explicit override — system property {@code rofd.native.path}
 *       (takes precedence), else environment variable {@code ROFD_NATIVE_PATH}.
 *       The value may be the library file itself or a directory to resolve
 *       the mapped library name in. Set-but-unloadable fails fast on purpose:
 *       silently using the embedded library would mask "my custom build did
 *       not take effect";</li>
 *   <li>classpath resource {@code natives/<platform>/<lib>} — embedded in the
 *       fat main jar and in the platform classifier jars (same path shape,
 *       one lookup covers both), extracted to a temp directory and loaded by
 *       absolute path;</li>
 *   <li>{@link System#loadLibrary} fallback — {@code java.library.path} and
 *       system library paths; the thin-jar status quo keeps working.</li>
 * </ol>
 *
 * <p>All decisions are computed purely in {@link #resolve} (injectable
 * property/environment/classpath views — unit-testable without any real
 * native library); {@link #load} is the only side-effecting entry point.
 */
final class NativeLoader {

    /** System property overriding the library location (file or directory). */
    static final String PATH_PROP = "rofd.native.path";
    /** Environment variable overriding the library location (property wins). */
    static final String PATH_ENV = "ROFD_NATIVE_PATH";

    /** Platform keys must match the convert-release build matrix verbatim. */
    private static final String SUPPORTED_PLATFORMS =
            "linux-x86_64, windows-x86_64, macos-x86_64, macos-aarch64";

    private NativeLoader() {}

    // ------------------------------------------------------------------
    // platform detection (pure)
    // ------------------------------------------------------------------

    /**
     * Detects the running platform, e.g. {@code "windows-x86_64"}.
     * Pure function of the {@code os.name}/{@code os.arch} system
     * properties; returns {@code null} for an unknown os or arch.
     */
    static String platformKey() {
        return platformKey(System.getProperties());
    }

    /** Same mapping for an injected property view (used by {@link #resolve}). */
    static String platformKey(Properties sys) {
        String os = osPrefix(sys.getProperty("os.name", ""));
        String arch = archName(sys.getProperty("os.arch", ""));
        return (os == null || arch == null) ? null : os + "-" + arch;
    }

    /** {@code windows|linux|macos} prefix or null when unrecognized. */
    private static String osPrefix(String rawName) {
        String name = rawName.toLowerCase(Locale.ROOT);
        if (name.contains("windows")) {
            return "windows";
        }
        if (name.contains("linux")) {
            return "linux";
        }
        if (name.contains("mac")) {
            return "macos";
        }
        return null;
    }

    /** Normalized arch: {@code x86_64} or {@code aarch64}, else null. */
    private static String archName(String rawArch) {
        String arch = rawArch.toLowerCase(Locale.ROOT);
        if ("amd64".equals(arch) || "x86_64".equals(arch)) {
            return "x86_64";
        }
        if ("aarch64".equals(arch) || "arm64".equals(arch)) {
            return "aarch64";
        }
        return null;
    }

    // ------------------------------------------------------------------
    // library names and resource paths (pure, own table)
    // ------------------------------------------------------------------

    /**
     * Candidate library file names for a platform, in priority order
     * (macOS tries {@code .dylib} before the legacy {@code .jnilib}).
     * Matches {@link System#mapLibraryName} output on all three platforms
     * but stays testable and lets one host infer a foreign platform's name.
     */
    static List<String> libFileNames(String platform, String libName) {
        if (platform == null) {
            return Collections.emptyList();
        }
        if (platform.startsWith("windows")) {
            return Collections.singletonList(libName + ".dll");
        }
        if (platform.startsWith("linux")) {
            return Collections.singletonList("lib" + libName + ".so");
        }
        if (platform.startsWith("macos")) {
            return Arrays.asList("lib" + libName + ".dylib", "lib" + libName + ".jnilib");
        }
        return Collections.emptyList();
    }

    /**
     * Classpath probe paths for the platform's library:
     * {@code natives/<platform>/<file>} — the same shape inside the fat
     * main jar and the platform classifier jars.
     */
    static List<String> resourcePaths(String platform, String libName) {
        List<String> names = libFileNames(platform, libName);
        List<String> paths = new ArrayList<String>(names.size());
        for (String name : names) {
            paths.add("natives/" + platform + "/" + name);
        }
        return paths;
    }

    // ------------------------------------------------------------------
    // chain decision (pure)
    // ------------------------------------------------------------------

    /** Outcome of the loading chain decision. */
    enum Kind { LOAD_FILE, LOAD_RESOURCE, FALLBACK_LIBRARY, FAIL_OVERRIDE }

    /**
     * Immutable decision: {@code path} carries the override value for
     * {@code LOAD_FILE}/{@code FAIL_OVERRIDE} and the resource path for
     * {@code LOAD_RESOURCE}; {@code url} is set only for
     * {@code LOAD_RESOURCE}.
     */
    static final class Decision {
        final Kind kind;
        final String path;
        final URL url;

        Decision(Kind kind, String path, URL url) {
            this.kind = kind;
            this.path = path;
            this.url = url;
        }
    }

    /**
     * Pure chain decision, no side effects. {@code sys}/{@code env} are the
     * property/environment views to resolve against and the platform is
     * detected from {@code sys}; {@code finder} abstracts the classpath
     * resource lookup so tests can inject hits and misses.
     */
    static Decision resolve(String libName, Properties sys, Map<String, String> env,
            Function<String, URL> finder) {
        String override = overrideValue(sys, env);
        if (override != null) {
            return resolveOverride(override, platformKey(sys), libName);
        }
        String platform = platformKey(sys);
        if (platform == null) {
            // Unknown os/arch: no resource path can be formed — go straight
            // to the loadLibrary fallback (System.loadLibrary may still know
            // more about this host than we do).
            return new Decision(Kind.FALLBACK_LIBRARY, null, null);
        }
        for (String path : resourcePaths(platform, libName)) {
            URL url = finder.apply(path);
            if (url != null) {
                return new Decision(Kind.LOAD_RESOURCE, path, url);
            }
        }
        return new Decision(Kind.FALLBACK_LIBRARY, null, null);
    }

    /** Property first, environment second; null/blank values count as unset. */
    private static String overrideValue(Properties sys, Map<String, String> env) {
        String value = sys.getProperty(PATH_PROP);
        if (value == null || value.trim().isEmpty()) {
            value = env.get(PATH_ENV);
        }
        return (value == null || value.trim().isEmpty()) ? null : value.trim();
    }

    /**
     * Resolves the override value against the filesystem: an existing file
     * wins as-is; a directory is probed for the platform's mapped library
     * names. Anything else is a set-but-unloadable override: fail-fast.
     * Loadable paths are absolutized ({@link System#load} rejects relative
     * paths; relative overrides resolve against the process working dir).
     */
    private static Decision resolveOverride(String override, String platform, String libName) {
        File file = new File(override);
        if (file.isFile()) {
            return new Decision(Kind.LOAD_FILE, file.getAbsoluteFile().getPath(), null);
        }
        if (file.isDirectory()) {
            for (String name : libFileNames(platform, libName)) {
                File candidate = new File(file, name);
                if (candidate.isFile()) {
                    return new Decision(Kind.LOAD_FILE, candidate.getAbsoluteFile().getPath(), null);
                }
            }
        }
        return new Decision(Kind.FAIL_OVERRIDE, override, null);
    }

    // ------------------------------------------------------------------
    // failure message (pure)
    // ------------------------------------------------------------------

    /**
     * Human-readable failure text: the detected platform (or the supported
     * list when detection failed), what was tried, and the three escape
     * hatches (override property, classifier jar, java.library.path).
     */
    static String failureSummary(String libName, String platform,
            String overrideTried, List<String> resourceTried) {
        StringBuilder sb = new StringBuilder("Unable to load native library '")
                .append(libName).append('\'');
        if (platform != null) {
            sb.append(" (detected platform ").append(platform).append(')');
        } else {
            sb.append(" (unknown platform; supported platforms: ")
                    .append(SUPPORTED_PLATFORMS).append(')');
        }
        if (overrideTried != null) {
            sb.append(". The override path '").append(overrideTried)
                    .append("' (set via ").append(PATH_PROP).append(" or ")
                    .append(PATH_ENV)
                    .append(") could not be loaded; failing fast without fallback");
        }
        if (resourceTried != null && !resourceTried.isEmpty()) {
            sb.append(". Classpath resources tried: ");
            for (int i = 0; i < resourceTried.size(); i++) {
                if (i > 0) {
                    sb.append(", ");
                }
                sb.append(resourceTried.get(i));
            }
        }
        sb.append(". Escape hatches: point -D").append(PATH_PROP)
                .append("=<library file or directory> (or env var ").append(PATH_ENV)
                .append(") at a loadable library; add the platform classifier jar")
                .append(" to the classpath; or place the library on")
                .append(" -Djava.library.path.");
        return sb.toString();
    }

    /** Appends the caught error so the original cause stays visible. */
    private static String withCause(String summary, Throwable cause) {
        return summary + "\nCaused by: " + cause;
    }

    // ------------------------------------------------------------------
    // side-effecting entry point
    // ------------------------------------------------------------------

    /**
     * Runs the chain and loads the library. Throws
     * {@link UnsatisfiedLinkError} with a {@link #failureSummary} message
     * when any reached tier fails — including fail-fast overrides — so a
     * broken override or corrupt embedded library is never masked.
     */
    static void load(String libName) {
        Decision decision = resolve(libName, System.getProperties(),
                System.getenv(), NativeLoader::findResource);
        switch (decision.kind) {
            case LOAD_FILE:
                try {
                    System.load(decision.path);
                } catch (UnsatisfiedLinkError e) {
                    throw new UnsatisfiedLinkError(withCause(failureSummary(
                            libName, platformKey(), decision.path,
                            Collections.<String>emptyList()), e));
                }
                return;
            case LOAD_RESOURCE:
                try {
                    System.load(extract(decision.url, decision.path).toAbsolutePath().toString());
                } catch (IOException | UnsatisfiedLinkError e) {
                    throw new UnsatisfiedLinkError(withCause(failureSummary(
                            libName, platformKey(), null,
                            Collections.singletonList(decision.path)), e));
                }
                return;
            case FALLBACK_LIBRARY:
                try {
                    System.loadLibrary(libName);
                } catch (UnsatisfiedLinkError e) {
                    throw new UnsatisfiedLinkError(withCause(failureSummary(
                            libName, platformKey(), null,
                            resourcePaths(platformKey(), libName)), e));
                }
                return;
            case FAIL_OVERRIDE:
                throw new UnsatisfiedLinkError(failureSummary(
                        libName, platformKey(), decision.path,
                        Collections.<String>emptyList()));
            default:
                throw new AssertionError("unhandled decision kind: " + decision.kind);
        }
    }

    /**
     * Real classpath lookup: context classloader, then this class's
     * classloader, then the system classloader (Spring Boot fat jars and
     * app servers load the binding from the context loader).
     */
    private static URL findResource(String path) {
        ClassLoader context = Thread.currentThread().getContextClassLoader();
        if (context != null) {
            URL url = context.getResource(path);
            if (url != null) {
                return url;
            }
        }
        ClassLoader own = NativeLoader.class.getClassLoader();
        if (own != null && own != context) {
            URL url = own.getResource(path);
            if (url != null) {
                return url;
            }
        }
        return ClassLoader.getSystemResource(path);
    }

    /**
     * Copies the resource into a fresh temp directory and returns the
     * library file path. deleteOnExit is best-effort: a Windows JVM keeps
     * loaded DLLs locked, so leftover temp directories are accepted (same
     * trade-off as sqlite-jdbc/Netty).
     */
    private static Path extract(URL url, String resourcePath) throws IOException {
        Path dir = Files.createTempDirectory("rofd-native-");
        String fileName = resourcePath.substring(resourcePath.lastIndexOf('/') + 1);
        Path target = dir.resolve(fileName);
        try (InputStream in = url.openStream()) {
            Files.copy(in, target, StandardCopyOption.REPLACE_EXISTING);
        }
        // deleteOnExit deletes in REVERSE registration order, so the dir is
        // registered FIRST: on POSIX the file then unlinks before the dir
        // removal attempt instead of leaving an empty rofd-native-* dir.
        dir.toFile().deleteOnExit();
        target.toFile().deleteOnExit();
        return target;
    }
}
