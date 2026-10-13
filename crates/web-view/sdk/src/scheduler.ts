/**
 * Self-suspending render scheduler (spec 2026-10-13 web-render-on-demand §7.2).
 *
 * Consumes the scheduling truth from the wasm `renderFrame()` return value:
 *   0  -> render again immediately (rAF),
 *   >0 -> arm exactly one wake timer for that many ms (blink deadline),
 *   -1 -> suspend: no rAF, no timer, until `kick()`.
 *
 * Gates: `paused` (host policy: pauseRendering), `suspendedHidden`
 * (visibilitychange full silence - Chromium still releases ~1 rAF/s for
 * hidden pages with pending frames), `disposed` (destroy, final).
 * `inLoopTick` suppresses kicks arriving from inside the tick itself - the
 * tick schedules on exit (rword pitfall 1).
 *
 * @internal
 */
export class RenderScheduler {
  private animFrameId: number | null = null;
  private wakeTimerId: number | null = null;
  private paused = false;
  private suspendedHidden = false;
  private disposed = false;
  private inLoopTick = false;
  private warnedMalformed = false;

  /** `render` runs one frame and returns the next scheduling need. */
  constructor(private readonly render: () => number) {}

  /** Ensure exactly one rAF is pending. Idempotent; no-op while gated. */
  kick(): void {
    if (this.inLoopTick) {
      return; // the tick schedules itself before exiting
    }
    this.ensureRaf();
  }

  /** Host policy: stop rendering and drop pending work (pauseRendering). */
  pause(): void {
    this.paused = true;
    this.clearAnimFrame();
    this.clearWakeTimer();
  }

  /** Host policy: resume after pause (schedules one frame). */
  resume(): void {
    if (this.disposed || !this.paused) {
      return;
    }
    this.paused = false;
    this.ensureRaf();
  }

  /** Page hidden: full silence - cancel everything and gate new work. */
  suspendForHidden(): void {
    this.suspendedHidden = true;
    this.clearAnimFrame();
    this.clearWakeTimer();
  }

  /** Page visible again: lift the hidden gate and schedule one frame. */
  resumeFromHidden(): void {
    if (this.disposed || !this.suspendedHidden) {
      return;
    }
    this.suspendedHidden = false;
    this.ensureRaf();
  }

  /** Final teardown: cancel everything; later kicks/timers are no-ops. */
  dispose(): void {
    this.disposed = true;
    this.clearAnimFrame();
    this.clearWakeTimer();
  }

  private ensureRaf(): void {
    if (this.paused || this.disposed || this.suspendedHidden) {
      return;
    }
    if (this.animFrameId !== null) {
      return;
    }
    this.animFrameId = requestAnimationFrame(this.tick);
  }

  private readonly tick = (): void => {
    this.animFrameId = null;
    if (this.paused || this.disposed || this.suspendedHidden) {
      return;
    }
    this.inLoopTick = true;
    try {
      this.scheduleNext(this.render());
    } finally {
      this.inLoopTick = false;
    }
  };

  /** Arm the single wake timer (replaces any stale one). */
  private ensureTimer(ms: number): void {
    if (this.paused || this.disposed || this.suspendedHidden) {
      return;
    }
    this.clearWakeTimer();
    this.wakeTimerId = setTimeout(() => {
      this.wakeTimerId = null;
      this.ensureRaf();
    }, ms);
  }

  private scheduleNext(next: number): void {
    if (next === 0) {
      this.ensureRaf();
      return;
    }
    if (next > 0 && Number.isFinite(next)) {
      this.ensureTimer(next);
      return;
    }
    // Suspend (-1) or malformed (NaN/Infinity): drop every timer - a stale
    // wake timer would fire later and render a wasted frame.
    this.clearWakeTimer();
    if (next !== -1 && !this.warnedMalformed) {
      this.warnedMalformed = true;
      console.warn(`[rofd] renderFrame returned malformed next-frame value ${next}; suspending`);
    }
  }

  private clearAnimFrame(): void {
    if (this.animFrameId !== null) {
      cancelAnimationFrame(this.animFrameId);
      this.animFrameId = null;
    }
  }

  private clearWakeTimer(): void {
    if (this.wakeTimerId !== null) {
      clearTimeout(this.wakeTimerId);
      this.wakeTimerId = null;
    }
  }
}
