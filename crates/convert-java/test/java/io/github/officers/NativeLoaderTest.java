package io.github.officers;

import java.io.File;
import java.io.IOException;
import java.net.MalformedURLException;
import java.net.URL;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Properties;

/**
 * Plain-main assertion harness for {@link NativeLoader} (JUnit is not on
 * the compile path — build-java.sh builds with bare javac). Pure-logic
 * tests only: platform/arch mapping, library file-name table, resource
 * path prefix, override precedence and fail-fast semantics, tiered
 * fallback decisions, and the failure summary wording.
 *
 * <p>No real native library is ever loaded: only {@link NativeLoader}'s
 * pure, injectable methods run, and {@link NativeLink} must never become
 * initialized here (its static block would trigger a load).
 *
 * <p>Exit code 0 = all tests passed; 1 = any failure, each failure
 * printed with its cause.
 */
public final class NativeLoaderTest {

    /** The cdylib name every assertion expects (NativeLink loads "rofdconvert"). */
    private static final String LIB = "rofdconvert";

    private static int passed = 0;
    private static final List<String> FAILURES = new ArrayList<String>();

    /** Test body allowed to throw checked exceptions (recorded as failures). */
    private interface TestBody {
        void run() throws Exception;
    }

    public static void main(String[] args) {
        run("platformKey_mapsOsAndArch", NativeLoaderTest::platformKey_mapsOsAndArch);
        run("platformKey_unknownOsYieldsNull", NativeLoaderTest::platformKey_unknownOsYieldsNull);
        run("libFileNames_perPlatform", NativeLoaderTest::libFileNames_perPlatform);
        run("resourcePaths_prefixNativesDir", NativeLoaderTest::resourcePaths_prefixNativesDir);
        run("resolve_syspropOverridesEnv", NativeLoaderTest::resolve_syspropOverridesEnv);
        run("resolve_overrideMissingFailsFastEvenWhenResourceExists",
                NativeLoaderTest::resolve_overrideMissingFailsFastEvenWhenResourceExists);
        run("resolve_overrideDirResolvesLibFile", NativeLoaderTest::resolve_overrideDirResolvesLibFile);
        run("resolve_resourceFoundWhenNoOverride", NativeLoaderTest::resolve_resourceFoundWhenNoOverride);
        run("resolve_fallsBackToLibraryWhenNoResource",
                NativeLoaderTest::resolve_fallsBackToLibraryWhenNoResource);
        run("resolve_unknownPlatformSkipsResourceLevel",
                NativeLoaderTest::resolve_unknownPlatformSkipsResourceLevel);
        run("failureSummary_listsTriedPathsAndHints",
                NativeLoaderTest::failureSummary_listsTriedPathsAndHints);

        System.out.println();
        if (FAILURES.isEmpty()) {
            System.out.println("OK: " + passed + " tests passed, 0 failed");
            return;
        }
        for (String failure : FAILURES) {
            System.out.println("FAILED: " + failure);
        }
        System.out.println("FAILED: " + passed + " passed, " + FAILURES.size() + " failed");
        System.exit(1);
    }

    private static void run(String name, TestBody body) {
        try {
            body.run();
            passed++;
            System.out.println("PASS " + name);
        } catch (Throwable t) {
            FAILURES.add(name + " -> " + t);
            System.out.println("FAIL " + name + " -> " + t);
        }
    }

    // ------------------------------------------------------------------
    // platformKey
    // ------------------------------------------------------------------

    private static void platformKey_mapsOsAndArch() {
        String origName = System.getProperty("os.name");
        String origArch = System.getProperty("os.arch");
        try {
            System.setProperty("os.name", "Windows");
            System.setProperty("os.arch", "amd64");
            assertEquals("windows-x86_64", NativeLoader.platformKey());

            System.setProperty("os.name", "Linux");
            System.setProperty("os.arch", "x86_64");
            assertEquals("linux-x86_64", NativeLoader.platformKey());

            System.setProperty("os.name", "Mac OS X");
            System.setProperty("os.arch", "aarch64");
            assertEquals("macos-aarch64", NativeLoader.platformKey());

            System.setProperty("os.arch", "arm64");
            assertEquals("macos-aarch64", NativeLoader.platformKey());
        } finally {
            System.setProperty("os.name", origName);
            System.setProperty("os.arch", origArch);
        }
    }

    private static void platformKey_unknownOsYieldsNull() {
        String origName = System.getProperty("os.name");
        String origArch = System.getProperty("os.arch");
        try {
            System.setProperty("os.name", "SunOS");
            System.setProperty("os.arch", "x86_64");
            assertNull(NativeLoader.platformKey());
        } finally {
            System.setProperty("os.name", origName);
            System.setProperty("os.arch", origArch);
        }
    }

    // ------------------------------------------------------------------
    // libFileNames / resourcePaths
    // ------------------------------------------------------------------

    private static void libFileNames_perPlatform() {
        assertEquals(Arrays.asList("rofdconvert.dll"),
                NativeLoader.libFileNames("windows-x86_64", LIB));
        assertEquals(Arrays.asList("librofdconvert.so"),
                NativeLoader.libFileNames("linux-x86_64", LIB));
        assertEquals(Arrays.asList("librofdconvert.dylib", "librofdconvert.jnilib"),
                NativeLoader.libFileNames("macos-aarch64", LIB));
    }

    private static void resourcePaths_prefixNativesDir() {
        List<String> paths = NativeLoader.resourcePaths("windows-x86_64", LIB);
        assertEquals(Collections.singletonList("natives/windows-x86_64/rofdconvert.dll"), paths);
    }

    // ------------------------------------------------------------------
    // resolve — tier 1: explicit override
    // ------------------------------------------------------------------

    private static void resolve_syspropOverridesEnv() throws IOException {
        File fromProp = newTempFile("from-prop.dll");
        File fromEnv = newTempFile("from-env.dll");
        Properties sys = windowsSys();
        sys.setProperty(NativeLoader.PATH_PROP, fromProp.getPath());
        Map<String, String> env = new HashMap<String, String>();
        env.put(NativeLoader.PATH_ENV, fromEnv.getPath());

        NativeLoader.Decision d = NativeLoader.resolve(LIB, sys, env, path -> null);

        assertEquals(NativeLoader.Kind.LOAD_FILE, d.kind);
        assertEquals(fromProp.getPath(), d.path);
    }

    private static void resolve_overrideMissingFailsFastEvenWhenResourceExists() {
        File missing = new File(tempRoot(), "does-not-exist.dll");
        Properties sys = windowsSys();
        sys.setProperty(NativeLoader.PATH_PROP, missing.getPath());

        // Finder would "find" any resource — fail-fast must not degrade to it.
        NativeLoader.Decision d = NativeLoader.resolve(
                LIB, sys, Collections.<String, String>emptyMap(),
                path -> url("file:/fake/" + path));

        assertEquals(NativeLoader.Kind.FAIL_OVERRIDE, d.kind);
        assertEquals(missing.getPath(), d.path);
    }

    private static void resolve_overrideDirResolvesLibFile() throws IOException {
        File dir = newTempFolder();
        Properties sys = windowsSys();
        sys.setProperty(NativeLoader.PATH_PROP, dir.getPath());

        // Directory without the platform lib file: fail-fast, no fallback.
        NativeLoader.Decision empty = NativeLoader.resolve(
                LIB, sys, Collections.<String, String>emptyMap(),
                path -> url("file:/fake/" + path));
        assertEquals(NativeLoader.Kind.FAIL_OVERRIDE, empty.kind);

        // Directory containing the platform lib file: LOAD_FILE on dir/lib.
        File lib = new File(dir, "rofdconvert.dll");
        assertTrue("could not create " + lib, lib.createNewFile());
        lib.deleteOnExit();
        NativeLoader.Decision d = NativeLoader.resolve(
                LIB, sys, Collections.<String, String>emptyMap(),
                path -> url("file:/fake/" + path));
        assertEquals(NativeLoader.Kind.LOAD_FILE, d.kind);
        assertEquals(new File(dir, "rofdconvert.dll").getPath(), d.path);
    }

    // ------------------------------------------------------------------
    // resolve — tier 2/3: classpath resource, fallback
    // ------------------------------------------------------------------

    private static void resolve_resourceFoundWhenNoOverride() {
        String expected = "natives/windows-x86_64/rofdconvert.dll";
        NativeLoader.Decision d = NativeLoader.resolve(
                LIB, windowsSys(),
                Collections.<String, String>emptyMap(),
                path -> expected.equals(path) ? url("file:/fake/" + path) : null);

        assertEquals(NativeLoader.Kind.LOAD_RESOURCE, d.kind);
        assertEquals(expected, d.path);
        assertEquals("file:/fake/" + expected, d.url.toExternalForm());
    }

    private static void resolve_fallsBackToLibraryWhenNoResource() {
        NativeLoader.Decision d = NativeLoader.resolve(
                LIB, windowsSys(),
                Collections.<String, String>emptyMap(),
                path -> null);

        assertEquals(NativeLoader.Kind.FALLBACK_LIBRARY, d.kind);
        assertNull(d.path);
        assertNull(d.url);
    }

    private static void resolve_unknownPlatformSkipsResourceLevel() {
        Properties sys = new Properties();
        sys.setProperty("os.name", "SunOS");
        sys.setProperty("os.arch", "x86_64");

        // The finder must not even be consulted when the platform is unknown.
        NativeLoader.Decision d = NativeLoader.resolve(
                LIB, sys, Collections.<String, String>emptyMap(),
                path -> {
                    fail("finder must not be consulted on an unknown platform");
                    return null;
                });

        assertEquals(NativeLoader.Kind.FALLBACK_LIBRARY, d.kind);
    }

    // ------------------------------------------------------------------
    // failureSummary
    // ------------------------------------------------------------------

    private static void failureSummary_listsTriedPathsAndHints() {
        String s = NativeLoader.failureSummary(LIB, "windows-x86_64", null,
                Arrays.asList("natives/windows-x86_64/rofdconvert.dll"));
        assertTrue(s, s.contains("windows-x86_64"));
        assertTrue(s, s.contains("natives/windows-x86_64/rofdconvert.dll"));
        assertTrue(s, s.contains(NativeLoader.PATH_PROP));
        assertTrue(s, s.contains("java.library.path"));
    }

    // ------------------------------------------------------------------
    // assertion helpers (JUnit-free)
    // ------------------------------------------------------------------

    private static void assertEquals(Object expected, Object actual) {
        if (expected == null ? actual != null : !expected.equals(actual)) {
            throw new AssertionError("expected <" + expected + "> but was <" + actual + ">");
        }
    }

    private static void assertNull(Object value) {
        if (value != null) {
            throw new AssertionError("expected null but was <" + value + ">");
        }
    }

    private static void assertTrue(boolean condition) {
        assertTrue("expected true", condition);
    }

    private static void assertTrue(String message, boolean condition) {
        if (!condition) {
            throw new AssertionError(message);
        }
    }

    private static void fail(String message) {
        throw new AssertionError(message);
    }

    // ------------------------------------------------------------------
    // temp files/folders (TemporaryFolder replacement)
    // ------------------------------------------------------------------

    private static File root;

    private static synchronized File tempRoot() {
        if (root == null) {
            try {
                root = Files.createTempDirectory("native-loader-test-").toFile();
            } catch (IOException e) {
                throw new IllegalStateException("cannot create temp root", e);
            }
            root.deleteOnExit();
        }
        return root;
    }

    private static File newTempFile(String name) throws IOException {
        File f = new File(tempRoot(), name);
        if (!f.createNewFile() && !f.isFile()) {
            throw new IOException("cannot create " + f);
        }
        f.deleteOnExit();
        return f;
    }

    private static File newTempFolder() throws IOException {
        File dir = Files.createTempDirectory(tempRoot().toPath(), "dir-").toFile();
        dir.deleteOnExit();
        return dir;
    }

    // ------------------------------------------------------------------
    // misc helpers
    // ------------------------------------------------------------------

    /** A Properties snapshot describing windows-x86_64 (deterministic on any host). */
    private static Properties windowsSys() {
        Properties sys = new Properties();
        sys.setProperty("os.name", "Windows");
        sys.setProperty("os.arch", "amd64");
        return sys;
    }

    private static URL url(String spec) {
        try {
            return new URL(spec);
        } catch (MalformedURLException e) {
            throw new IllegalStateException(e);
        }
    }
}
