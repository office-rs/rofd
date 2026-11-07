package io.github.officers;

import java.lang.ref.PhantomReference;
import java.lang.ref.Reference;
import java.lang.ref.ReferenceQueue;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * L2: a long-lived converter instance. Thread contract (convert spec D15,
 * 2026-11-07 revision): one thread at a time — pool it via
 * {@link OfdConverterPool} instead of sharing. Implements AutoCloseable;
 * a PhantomReference cleaner thread is the last-resort reclaim for
 * never-closed instances (Java 8 has no Cleaner API, spec §6.4 mechanical
 * choice).
 *
 * <p>All failures surface as {@link OfdConvertException} with the native
 * boundary's stable wording: a concurrent call on one instance reports
 * "converter handle in use", use after close reports "stale converter
 * handle" (see the JNI surface's throw_take).
 */
public final class OfdConverter implements AutoCloseable {
    private static final ReferenceQueue<OfdConverter> DEAD = new ReferenceQueue<OfdConverter>();
    private static final ConcurrentLinkedQueue<PhantomRef> PENDING =
            new ConcurrentLinkedQueue<PhantomRef>();
    private static final Thread CLEANER =
            new Thread(OfdConverter::drain, "rofd-converter-cleaner");

    static {
        CLEANER.setDaemon(true);
        CLEANER.start();
    }

    /** Keeps the native handle reachable until the referent is collected. */
    static final class PhantomRef extends PhantomReference<OfdConverter> {
        final long handle;

        PhantomRef(OfdConverter referent) {
            super(referent, DEAD);
            this.handle = referent.handle;
        }
    }

    private final long handle;
    private final AtomicBoolean closed = new AtomicBoolean(false);

    private OfdConverter() {
        this.handle = NativeLink.nNewConverter();
        PENDING.add(new PhantomRef(this));
    }

    /** Create a standalone instance (spec D15: one thread at a time —
     * for concurrency, prefer {@link OfdConverterPool}). */
    public static OfdConverter create() {
        return new OfdConverter();
    }

    /** Kernel package version (lockstep across all channels, spec §7). */
    public static String getVersion() {
        return NativeLink.nVersion();
    }

    /** Register font bytes into the process-level registry (append-only, process-wide). */
    public static boolean registerFont(byte[] bytes) {
        return NativeLink.nRegisterFont(bytes);
    }

    /** Register a font file into the process-level registry. */
    public static boolean registerFontFile(String path) {
        return NativeLink.nRegisterFontFile(path);
    }

    /** Anchor the process default font (kernel D19); a later anchor wins. */
    public static boolean registerDefaultFont(byte[] bytes) {
        return NativeLink.nRegisterDefaultFont(bytes);
    }

    /** Whether a default font is anchored in the process registry. */
    public static boolean hasDefaultFont() {
        return NativeLink.nHasDefaultFont();
    }

    /** Font families visible to the process registry. */
    public static String[] registeredFamilies() {
        return NativeLink.nRegisteredFamilies();
    }

    /** Convert .ofd bytes to PDF with kernel defaults. Non-fatal kernel
     * warnings: {@link #takeWarnings()}. */
    public byte[] convertPdf(byte[] ofd) {
        return convertPdf(ofd, null);
    }

    /**
     * Convert .ofd bytes to PDF; {@code null} opts means kernel defaults.
     * v1 has a single output format (pdf), so no format parameter is
     * exposed at L2. Non-fatal kernel warnings: {@link #takeWarnings()}.
     */
    public byte[] convertPdf(byte[] ofd, OfdConvertOptions opts) {
        if (closed.get()) {
            throw new OfdConvertException("converter is closed");
        }
        OfdConvertOptions o = opts == null ? OfdConvertOptions.DEFAULTS : opts;
        boolean watermark = o.watermark == null ? true : o.watermark.booleanValue();
        boolean annotations = o.annotations == null ? true : o.annotations.booleanValue();
        boolean seal = o.seal == null ? true : o.seal.booleanValue();
        long maxPages = o.maxPages == null ? 0L : o.maxPages.longValue();
        long maxMediaBytes = o.maxMediaBytes == null ? 0L : o.maxMediaBytes.longValue();
        return NativeLink.nConvert(handle, 0 /* Pdf */, ofd,
                watermark, annotations, seal, maxPages, maxMediaBytes);
    }

    /** Drain and clear the warnings of the last SUCCESSFUL conversion.
     * A failed conversion clears the slot (no stale pre-failure set),
     * and closing the instance drains it. */
    public String[] takeWarnings() {
        return NativeLink.nTakeWarnings(handle);
    }

    @Override
    public void close() {
        if (closed.compareAndSet(false, true)) {
            NativeLink.nClose(handle);
        }
    }

    private static void drain() {
        while (true) {
            try {
                Reference<?> ref = DEAD.remove();
                if (ref instanceof PhantomRef) {
                    // Never closed explicitly: last-resort native release.
                    // Racing an explicit close is harmless (stale close).
                    NativeLink.nClose(((PhantomRef) ref).handle);
                }
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                return;
            }
        }
    }

    /**
     * Content/budget overrides for {@link #convertPdf(byte[], OfdConvertOptions)}.
     * Every field may be null: content flags then fall back to the kernel
     * default of all layers on (kernel D17), budgets to the kernel limits
     * (10000 pages / 1 GiB media). Non-positive budget values mean the
     * same as null — the native contract maps {@code <= 0} to "unset".
     */
    public static final class OfdConvertOptions {
        static final OfdConvertOptions DEFAULTS = new OfdConvertOptions();

        /** Watermark layer; null = kernel default (on). */
        public Boolean watermark;
        /** Annotation layer; null = kernel default (on). */
        public Boolean annotations;
        /** Seal layer; null = kernel default (on). */
        public Boolean seal;
        /** Page budget; null or {@code <= 0} = kernel default. */
        public Long maxPages;
        /** Media byte budget; null or {@code <= 0} = kernel default. */
        public Long maxMediaBytes;
    }
}
