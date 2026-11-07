package io.github.officers.demo;

import io.github.officers.OfdConverter;
import io.github.officers.OfdConverterPool;

import java.nio.file.Files;
import java.nio.file.Paths;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.concurrent.CountDownLatch;

/**
 * Phase-2 acceptance harness (two-act demo, byte-identity gate): register
 * the SimSun font bytes -> 8-converter pool -> 8 concurrent converts -> all
 * outputs byte-equal to each other AND byte-identical to the reference PDF
 * produced by the rofd-convert CLI on the same machine with the same font
 * state (cross-process determinism, kernel TTC face-ranking fix).
 *
 * Touches only the public L2 surface (OfdConverter / OfdConverterPool);
 * the package-private L1 (NativeLink / NativeLoader) is off limits here.
 *
 * usage: InteropMain &lt;fixture.ofd&gt; &lt;font-file&gt; &lt;cli-reference.pdf&gt; [act-label]
 */
public final class InteropMain {
    private static final int POOL_SIZE = 8;

    public static void main(String[] args) {
        if (args.length < 3) {
            System.err.println(
                    "usage: InteropMain <fixture.ofd> <font-file> <cli-reference.pdf> [act-label]");
            System.exit(2);
        }
        String tag = args.length >= 4 ? args[3] : "ACT";
        try {
            run(args[0], args[1], args[2], tag);
        } catch (Exception e) {
            System.err.println("[" + tag + "] FAIL: unexpected exception");
            e.printStackTrace();
            System.exit(1);
        }
        System.out.println("[" + tag + "] BYTES MATCH CLI");
        System.exit(0); // explicit exit: the converter cleaner thread is a daemon anyway
    }

    private static void run(String fixturePath, String fontPath, String referencePath, String tag)
            throws Exception {
        System.out.println("[" + tag + "] rofd-convert java acceptance: " + POOL_SIZE
                + "-converter pool vs CLI reference");
        byte[] ofd = Files.readAllBytes(Paths.get(fixturePath));
        byte[] font = Files.readAllBytes(Paths.get(fontPath));
        byte[] cliPdf = Files.readAllBytes(Paths.get(referencePath));
        System.out.println("[" + tag + "] fixture " + ofd.length + " bytes, font " + font.length
                + " bytes, reference " + cliPdf.length + " bytes, kernel version "
                + OfdConverter.getVersion());

        // Same font state as the CLI's --font flag: bytes into the process
        // registry BEFORE any converter snapshots it (kernel D9).
        boolean registered = OfdConverter.registerFont(font);
        System.out.println("[" + tag + "] font registered: " + fontPath + " -> " + registered);
        require(registered, "registerFont failed for " + fontPath);

        // Same options as the CLI defaults: all content layers on, kernel budgets.
        OfdConverter.OfdConvertOptions opts = new OfdConverter.OfdConvertOptions();
        opts.watermark = Boolean.TRUE;
        opts.annotations = Boolean.TRUE;
        opts.seal = Boolean.TRUE;

        try (OfdConverterPool pool = new OfdConverterPool(POOL_SIZE)) {
            final byte[][] outputs = new byte[POOL_SIZE][];
            final CountDownLatch done = new CountDownLatch(POOL_SIZE);
            final long[] failures = new long[]{0};
            for (int t = 0; t < POOL_SIZE; t++) {
                final int idx = t;
                new Thread(() -> {
                    try {
                        OfdConverter c = pool.borrow();
                        try {
                            outputs[idx] = c.convertPdf(ofd, opts);
                            drainWarnings(tag, idx, c.takeWarnings());
                        } finally {
                            pool.returnConverter(c);
                        }
                    } catch (RuntimeException e) {
                        synchronized (failures) {
                            failures[0]++;
                        }
                        System.err.println("[" + tag + "] converter " + idx + " failed:");
                        e.printStackTrace();
                    } catch (InterruptedException e) {
                        Thread.currentThread().interrupt();
                    } finally {
                        done.countDown();
                    }
                }, "acceptance-convert-" + t).start();
            }
            done.await();
            require(failures[0] == 0,
                    failures[0] + "/" + POOL_SIZE + " pooled conversions failed");
            for (int i = 0; i < POOL_SIZE; i++) {
                require(outputs[i] != null, "converter " + i + " produced no output");
            }

            // Assertion 1: all 8 pooled outputs are byte-equal to each other.
            for (int i = 1; i < POOL_SIZE; i++) {
                require(Arrays.equals(outputs[0], outputs[i]),
                        "pooled output " + i + " differs from output 0 (" + outputs[i].length
                                + " vs " + outputs[0].length + " bytes)");
            }
            System.out.println("[" + tag + "] " + POOL_SIZE + "/" + POOL_SIZE
                    + " pooled outputs byte-equal: OK (" + outputs[0].length + " bytes)");

            // Assertion 2: byte-identical to the CLI-produced reference PDF.
            byte[] java = outputs[0];
            System.out.println("[" + tag + "] java sha256: " + sha256(java)
                    + " (" + java.length + " bytes)");
            System.out.println("[" + tag + "] cli  sha256: " + sha256(cliPdf)
                    + " (" + cliPdf.length + " bytes)");
            if (!Arrays.equals(java, cliPdf)) {
                System.err.println("[" + tag + "] FAIL: java output is not byte-identical"
                        + " to the CLI reference");
                System.err.println(diagnose(java, cliPdf));
                System.exit(1);
            }
        }
    }

    /** Kernel warnings are non-fatal: surface every line on stderr, keep converting. */
    private static void drainWarnings(String tag, int idx, String[] warnings) {
        if (warnings == null) {
            return;
        }
        for (String w : warnings) {
            System.err.println("[" + tag + "] warning (converter " + idx + "): " + w);
        }
    }

    /** First-divergence diagnostics for the failure report. */
    private static String diagnose(byte[] a, byte[] b) {
        int n = Math.min(a.length, b.length);
        int diff = -1;
        for (int i = 0; i < n; i++) {
            if (a[i] != b[i]) {
                diff = i;
                break;
            }
        }
        if (diff < 0) {
            return "common prefix identical for " + n + " bytes; lengths differ (java="
                    + a.length + ", cli=" + b.length + ")";
        }
        return "first divergence at byte " + diff + " (java=0x"
                + Integer.toHexString(a[diff] & 0xff) + ", cli=0x"
                + Integer.toHexString(b[diff] & 0xff) + "); lengths java=" + a.length
                + ", cli=" + b.length;
    }

    static void require(boolean cond, String msg) {
        if (!cond) {
            System.err.println("FAIL: " + msg);
            System.exit(1);
        }
    }

    static String sha256(byte[] b) throws Exception {
        byte[] d = MessageDigest.getInstance("SHA-256").digest(b);
        StringBuilder sb = new StringBuilder();
        for (byte x : d) {
            sb.append(String.format("%02x", x));
        }
        return sb.toString();
    }
}
