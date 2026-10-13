import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RenderScheduler } from '../scheduler';
import { RafStub } from './helpers';

describe('RenderScheduler', () => {
  let raf: RafStub;

  beforeEach(() => {
    vi.useFakeTimers();
    raf = new RafStub();
    raf.install();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('schedules nothing after -1 (suspend: zero rAF, zero timers)', () => {
    const render = vi.fn(() => -1);
    const s = new RenderScheduler(render);
    s.kick();
    expect(raf.count).toBe(1);
    raf.flushOne();
    expect(render).toHaveBeenCalledTimes(1);
    expect(raf.count).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('arms exactly one timer for >0; on fire renders once and follows the new value', () => {
    let n = 0;
    const render = vi.fn((): number => {
      n += 1;
      return n === 1 ? 500 : -1;
    });
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne();
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(500);
    expect(raf.count).toBe(1);
    raf.flushOne();
    expect(render).toHaveBeenCalledTimes(2);
    expect(raf.count).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('re-arms rAF immediately for 0', () => {
    const render = vi.fn(() => 0);
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne();
    expect(raf.count).toBe(1);
  });

  it('kick is idempotent while a frame is pending', () => {
    const render = vi.fn(() => -1);
    const s = new RenderScheduler(render);
    s.kick();
    s.kick();
    s.kick();
    expect(raf.count).toBe(1);
  });

  it('clears a stale wake timer when a kicked frame suspends', () => {
    let n = 0;
    const render = vi.fn((): number => {
      n += 1;
      return n === 1 ? 500 : -1;
    });
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne(); // timer(500) armed
    s.kick(); // external kick preempts the timer
    raf.flushOne(); // render -> -1 -> suspend must drop the stale timer
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(1000);
    expect(raf.count).toBe(0);
  });

  it('warns once and suspends on malformed next values', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const render = vi.fn((): number => Number.NaN);
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne();
    expect(raf.count).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
    expect(warn).toHaveBeenCalledTimes(1);
    s.kick();
    raf.flushOne();
    expect(warn).toHaveBeenCalledTimes(1); // one-shot
    warn.mockRestore();
  });

  it('stops self-scheduling when render throws; a kick restarts', () => {
    let fail = true;
    const render = vi.fn((): number => {
      if (fail) {
        throw new Error('boom');
      }
      return -1;
    });
    const s = new RenderScheduler(render);
    s.kick();
    expect(() => {
      raf.flushOne();
    }).toThrow('boom');
    expect(raf.count).toBe(0);
    fail = false;
    s.kick();
    raf.flushOne();
    expect(render).toHaveBeenCalledTimes(2);
  });

  it('ignores kicks arriving from inside the tick (inLoopTick)', () => {
    let sched: RenderScheduler | null = null;
    const render = vi.fn((): number => {
      sched?.kick(); // simulates a proxied wasm read inside the tick
      return -1;
    });
    sched = new RenderScheduler(render);
    sched.kick();
    raf.flushOne();
    expect(raf.count).toBe(0); // suspend decision stands: no 60Hz self-run
  });

  it('pause clears both handles and is idempotent; resume kicks', () => {
    let n = 0;
    const render = vi.fn((): number => {
      n += 1;
      return n === 1 ? 500 : -1;
    });
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne();
    expect(vi.getTimerCount()).toBe(1);
    s.pause();
    expect(vi.getTimerCount()).toBe(0);
    expect(raf.count).toBe(0);
    s.pause();
    s.resume();
    expect(raf.count).toBe(1);
    s.resume(); // already running: no-op
    expect(raf.count).toBe(1);
  });

  it('dispose clears both handles and is final', () => {
    const render = vi.fn(() => 0);
    const s = new RenderScheduler(render);
    s.kick();
    s.dispose();
    expect(raf.count).toBe(0);
    s.kick();
    s.resume();
    expect(raf.count).toBe(0); // cannot revive
  });

  it('hidden suspension silences everything; visible kick revives', () => {
    const render = vi.fn(() => 500);
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne();
    expect(vi.getTimerCount()).toBe(1);
    s.suspendForHidden();
    expect(vi.getTimerCount()).toBe(0);
    expect(raf.count).toBe(0);
    s.kick(); // gated while hidden
    expect(raf.count).toBe(0);
    s.resumeFromHidden();
    expect(raf.count).toBe(1);
  });
});
