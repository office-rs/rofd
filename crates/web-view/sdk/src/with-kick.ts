/**
 * Proxy wrapper that re-arms the render loop after any wasm method call
 * (spec 2026-10-13 §7.3). The wasm surface is wrapped once, right after
 * `create_wasm_ofd`; every internal holder keeps the proxy, so new wasm
 * entry points are kicked without auditing call sites.
 *
 * `renderFrame` is exempt: the loop's own call must not kick (it would
 * self-continue at 60Hz). Non-function properties pass through untouched.
 * Wrapper functions are memoized per property (`proxy.foo === proxy.foo`);
 * wasm-bindgen methods are stable, so caching the first lookup is safe.
 *
 * @internal
 */
export function withKick<T extends object>(target: T, kick: () => void): T {
  const wrappers = new Map<string | symbol, (...args: unknown[]) => unknown>();
  return new Proxy(target, {
    get(t, prop, receiver) {
      const value = Reflect.get(t, prop, receiver);
      if (typeof value !== 'function' || prop === 'renderFrame') {
        return value;
      }
      let wrapper = wrappers.get(prop);
      if (wrapper === undefined) {
        const fn = value as (...args: unknown[]) => unknown;
        wrapper = function (this: unknown, ...args: unknown[]): unknown {
          const result = fn.apply(this === receiver ? t : this, args);
          kick();
          return result;
        };
        wrappers.set(prop, wrapper);
      }
      return wrapper;
    },
  });
}
