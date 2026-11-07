package io.github.officers;

import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.TimeUnit;

/**
 * L2: fixed-size slot pool (convert spec D11/D15 — the kernel instance is
 * currently {@code Send}, and the pool is the safe-superset L2 shape that
 * stays correct even if a warm engine lands; each instance is still used
 * by one thread at a time). borrow() blocks until an instance is free;
 * always returnConverter() it in a finally block.
 */
public final class OfdConverterPool implements AutoCloseable {
    private final BlockingQueue<OfdConverter> free;
    private volatile boolean closed = false;

    public OfdConverterPool(int size) {
        if (size < 1) {
            throw new IllegalArgumentException("pool size >= 1");
        }
        this.free = new ArrayBlockingQueue<>(size);
        for (int i = 0; i < size; i++) {
            free.add(OfdConverter.create());
        }
    }

    /** Blocks until a converter is free (60s cap — a stuck lease is a bug). */
    public OfdConverter borrow() throws InterruptedException {
        if (closed) {
            throw new IllegalStateException("pool is closed");
        }
        OfdConverter c = free.poll(60, TimeUnit.SECONDS);
        if (c == null) {
            throw new IllegalStateException("no converter became free within 60s");
        }
        return c;
    }

    /** Return a borrowed converter. Returning into a closed pool closes it. */
    public void returnConverter(OfdConverter c) {
        if (c == null) {
            return;
        }
        if (!closed) {
            free.offer(c);
        } else {
            c.close();
        }
    }

    @Override
    public void close() {
        closed = true;
        OfdConverter c;
        while ((c = free.poll()) != null) {
            c.close();
        }
    }
}
