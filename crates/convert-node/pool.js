'use strict';
// Slot pool over worker_threads (convert spec D11/D15): each worker owns
// ONE Converter in its own JS realm — the safe pool shape regardless of
// the engine's Send-ness (which phase 1 confirmed anyway). The main
// thread round-robins conversions as Promises. A crashed worker rejects
// its pending queue and is replaced.

const { Worker } = require('worker_threads');
const path = require('path');

module.exports = (addon) => {
  const WORKER = path.join(__dirname, 'pool-worker.js');

  function createPool(size = 4) {
    if (!(size >= 1)) {
      throw new Error('pool size must be >= 1');
    }
    const workers = [];
    const queues = new Map(); // worker -> [{id, resolve, reject}]
    const nextId = { v: 1 };
    let rr = 0;
    let closed = false;

    for (let i = 0; i < size; i++) {
      spawn(i);
    }

    function spawn(slot) {
      const w = new Worker(WORKER);
      const q = [];
      workers[slot] = w;
      queues.set(w, q);
      w.on('message', ({ id, ok, bytes, warnings, error }) => {
        const pending = q.shift();
        if (!pending || pending.id !== id) {
          return;
        }
        if (ok) {
          pending.resolve({ bytes: Buffer.from(bytes), warnings });
        } else {
          pending.reject(new Error(error));
        }
      });
      w.on('error', (err) => {
        for (const p of q.splice(0)) {
          p.reject(err);
        }
        if (!closed) {
          spawn(slot); // replace a crashed worker
        }
      });
    }

    function busyCount(w) {
      return queues.get(w).length;
    }

    function convertToPdf(ofd, options) {
      if (closed) {
        return Promise.reject(new Error('pool is closed'));
      }
      // Pick the least-loaded worker (round-robin among equals).
      let best = workers[rr++ % workers.length];
      for (const w of workers) {
        if (busyCount(w) < busyCount(best)) {
          best = w;
        }
      }
      const q = queues.get(best);
      const id = nextId.v++;
      return new Promise((resolve, reject) => {
        q.push({ id, resolve, reject });
        best.postMessage({ id, ofd, options });
      });
    }

    async function close() {
      closed = true;
      await Promise.allSettled(workers.map((w) => w.terminate()));
    }

    return {
      convertToPdf,
      close,
      get size() {
        return workers.length;
      },
    };
  }

  return { createPool };
};
