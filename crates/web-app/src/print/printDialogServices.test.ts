// Print dialog services tests (final-review fix round): pin the
// subscribe-before-submit merge gate. The Rust pump (tauri-app print.rs)
// emits its first snapshot right after job insertion — before the
// print_pdf invoke resolves in JS — so a fast-failing job's ONLY Failed
// event arrives between listener attach and invoke resolve.
// subscribeJobProgress must buffer pre-bind events and replay ours (in
// arrival order, foreign ids dropped) once the job id is known.

import { describe, expect, it } from 'vitest';
import { subscribeJobProgress, type PrintProgress } from './printDialogServices';

function progress(jobId: number, state: string): PrintProgress {
  return { jobId, state, pagesDone: 0, pagesTotal: 1, error: null };
}

/** Wire subscribeJobProgress over a controllable fake event source;
 * `emit` mirrors what the pump's listener callback would observe. */
function harness(onJobEvent: (p: PrintProgress) => void): {
  sub: ReturnType<typeof subscribeJobProgress>;
  emit: (p: PrintProgress) => void;
} {
  let listener: ((p: PrintProgress) => void) | null = null;
  const sub = subscribeJobProgress(
    (cb) => {
      listener = cb;
      return () => {
        listener = null;
      };
    },
    onJobEvent,
  );
  return { sub, emit: (p) => listener?.(p) };
}

describe('subscribeJobProgress (subscribe-before-submit merge gate)', () => {
  it('does not lose a Failed event that landed before the invoke resolved', () => {
    const seen: string[] = [];
    const { sub, emit } = harness((p) => seen.push(`${p.jobId}:${p.state}`));
    // The pump emits the job's first (and here only) snapshot before the
    // print_pdf invoke resolves:
    emit(progress(42, 'Failed'));
    // ...then the invoke resolves with the job id:
    sub.bind(42);
    expect(seen).toEqual(['42:Failed']);
    sub.close();
  });

  it('replays buffered events in arrival order and drops foreign job ids', () => {
    const seen: string[] = [];
    const { sub, emit } = harness((p) => seen.push(`${p.jobId}:${p.state}`));
    emit(progress(7, 'Spooling'));
    emit(progress(9, 'Spooling')); // another window's job
    emit(progress(7, 'Rendering'));
    sub.bind(7);
    expect(seen).toEqual(['7:Spooling', '7:Rendering']);
    // After bind, live events filter by id:
    emit(progress(8, 'Done'));
    emit(progress(7, 'Done'));
    expect(seen).toEqual(['7:Spooling', '7:Rendering', '7:Done']);
    sub.close();
  });

  it('close detaches: nothing further is delivered, buffered or via a late bind', () => {
    const seen: string[] = [];
    const { sub, emit } = harness((p) => seen.push(`${p.jobId}:${p.state}`));
    emit(progress(1, 'Spooling'));
    sub.close();
    sub.bind(1); // a late bind must not resurrect the closed subscription
    expect(seen).toEqual([]);
  });
});
