package io.github.officers;

/** Conversion failure at the kernel boundary (parse/assemble/encode/budget/internal). */
public class OfdConvertException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    OfdConvertException(String message) {
        super(message);
    }
}
