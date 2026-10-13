import { vi } from 'vitest';

type RafCallback = (time: number) => void;

/**
 * Deterministic requestAnimationFrame stub: queues callbacks; `flushOne`
 * runs the oldest still-live callback. Cancelled frames are dropped, so
 * `count` reflects exactly what the browser would have run.
 */
export class RafStub {
  private nextId = 1;
  private readonly pending = new Map<number, RafCallback>();
  /** Callbacks actually run. */
  calls = 0;

  install(): void {
    vi.stubGlobal('requestAnimationFrame', (cb: RafCallback): number => {
      const id = this.nextId++;
      this.pending.set(id, cb);
      return id;
    });
    vi.stubGlobal(
      'cancelAnimationFrame',
      (id: number): void => {
        this.pending.delete(id);
      },
    );
  }

  /** Pending (scheduled, not cancelled, not yet run) callbacks. */
  get count(): number {
    return this.pending.size;
  }

  /** Run the oldest pending callback. */
  flushOne(): void {
    const entry = [...this.pending.entries()][0];
    if (entry === undefined) {
      throw new Error('RafStub.flushOne: no pending rAF callback');
    }
    this.pending.delete(entry[0]);
    this.calls += 1;
    entry[1](0);
  }
}
