import { defineConfig, type Plugin } from 'vite';
import { fileURLToPath } from 'node:url';

// Alias the SDK package name to its source so Vite serves and transforms the
// TS (and its `../dist/rofd_web_view.js` import) directly in dev, instead of
// going through the `file:` dependency. Mirrors reditor's web-view wiring.
const sdkEntry = fileURLToPath(
  new URL('../../crates/web-view/sdk/src/index.ts', import.meta.url),
);

// Under COEP `require-corp` (set below), every subresource fetch needs a
// `Cross-Origin-Resource-Policy` header to load. Vite does not add one, so we
// stamp `same-origin` on all dev responses - this lets same-origin assets (the
// .wasm, the default font) load while keeping cross-origin isolation enabled.
function corpAllResponses(): Plugin {
  return {
    name: 'corp-all-responses',
    configureServer(server) {
      server.middlewares.use((_req, res, next) => {
        res.setHeader('Cross-Origin-Resource-Policy', 'same-origin');
        next();
      });
    },
  };
}

export default defineConfig({
  plugins: [corpAllResponses()],
  resolve: {
    alias: { '@rofd/sdk': sdkEntry },
  },
  // Keep the SDK out of the dep pre-bundle: it pulls in a .wasm via
  // import.meta.url, which must be resolved by Vite's dev pipeline, not esbuild.
  optimizeDeps: { exclude: ['@rofd/sdk'] },
  build: { target: 'esnext' },
  server: {
    // Project root is examples/web-app; `../..` is the repo root, which also
    // covers crates/web-view/sdk/dist/*.wasm - without this the wasm fetch 403s.
    fs: { allow: ['../..'] },
    // Cross-origin isolation: harmless dev default for WebGPU + wasm apps, and
    // enables SharedArrayBuffer should the wasm adopt threads later.
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
  },
});
