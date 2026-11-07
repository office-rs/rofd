package io.github.officers;

/** L1: raw JNI surface. Application code should use {@link OfdConverter}. */
final class NativeLink {
    static {
        // Tiered load: rofd.native.path override (fail-fast) -> classpath
        // natives/<platform>/ extraction -> java.library.path fallback.
        NativeLoader.load("rofdconvert");
    }

    private NativeLink() {}

    static native long nNewConverter();
    static native boolean nClose(long id);
    static native boolean nRegisterFont(byte[] bytes);
    static native boolean nRegisterFontFile(String path);
    static native boolean nRegisterDefaultFont(byte[] bytes);
    static native boolean nHasDefaultFont();
    static native String[] nRegisteredFamilies();
    static native String nVersion();
    /** @return output bytes; throws OfdConvertException on failure. */
    static native byte[] nConvert(long id, int formatOrdinal, byte[] ofd,
            boolean watermark, boolean annotations, boolean seal,
            long maxPages, long maxMediaBytes);
    static native String[] nTakeWarnings(long id);
}
