import { defineConfig, type Plugin } from "vite";
import vue from "@vitejs/plugin-vue";
import rwordFonts from "@office-rs/font-runtime/vite";
import { fileURLToPath } from "node:url";

// 复用 web-app 前端：App.vue 从 ../web-app/src 导入，其内部相对 import 与
// @office-rs/rofd alias 由 vite dev 管线解析。字体 / sample.ofd 从 web-app/public
// 本地加载（publicDir 指向它），桌面端脱离 CDN、离线可用。

// SDK 源码入口：alias 到 crates/web-view/sdk/src，让 vite 直接转译 TS 及其
// `../dist/rofd_web_view.js`（import.meta.url 解析的 .wasm），与 web-app 一致。
const sdkEntry = fileURLToPath(
  new URL("../web-view/sdk/src/index.ts", import.meta.url),
);

// 共享 web-app 的静态资源目录（字体 + sample.ofd）。
const webAppPublic = fileURLToPath(new URL("../web-app/public", import.meta.url));

// Under COEP `require-corp` (Vite dev under cross-origin isolation), every
// subresource fetch needs a `Cross-Origin-Resource-Policy` header. WebView2 does
// not require cross-origin isolation for WebGPU, so we don't set COOP/COEP here;
// this stays as a same-origin CORP stamp only if isolation is later enabled.
function corpAllResponses(): Plugin {
  return {
    name: "corp-all-responses",
    configureServer(server) {
      server.middlewares.use((_req, res, next) => {
        res.setHeader("Cross-Origin-Resource-Policy", "same-origin");
        next();
      });
    },
  };
}

// Tauri 期望固定的 dev 端口且不清屏；envPrefix 放行 TAURI_ 前缀变量。
export default defineConfig({
  plugins: [
    vue(),
    corpAllResponses(),
    // Noto 切片通道：build 产出 dist/rword-fonts/，dev 经 /@rword-fonts 中间件。
    // 桌面纯系统字体通道（draft 2026-10-18 D4）：本机字体经 provider 懒加载，
    // Noto 切片不进桌面——插件仍须注册但 families 为空（共享 fonts.ts 无条件
    // import virtual 模块，空 families 产合法空清单；忘注册 = dev 启动即失败）。
    rwordFonts({ families: [] }),
  ],
  resolve: {
    alias: { "@office-rs/rofd": sdkEntry },
    // App.vue 从 ../web-app/src 导入，其裸导入（vue / ant-design-vue）按 importer
    // 目录向上找 node_modules 会落到 web-app/node_modules —— CI 只装 tauri-app 依赖
    // 时该目录不存在，Rollup 解析失败。dedupe 强制这些共享依赖解析到项目根
    // （tauri-app）的 node_modules，同时避免同一份 UI 源码出现双 vue 实例。
    dedupe: ["vue", "ant-design-vue"],
  },
  publicDir: webAppPublic,
  // Keep the SDK out of dep pre-bundle: it pulls in a .wasm via import.meta.url,
  // which must be resolved by Vite's dev pipeline, not esbuild.
  optimizeDeps: { exclude: ["@office-rs/rofd"] },
  // FontWorker 动态 import woff2 解码器会拆 worker bundle——必须 ES 格式。
  worker: { format: "es" },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022", // 切片流顶层 await 最低目标
    rollupOptions: {
      output: {
        manualChunks: {
          vue: ["vue"],
          antd: ["ant-design-vue", "@ant-design/icons-vue"],
        },
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    // Repo root covers crates/web-view/sdk/dist/*.wasm, crates/web-app/src, and
    // crates/web-app/public - without this those out-of-root fetches 403.
    // `../../../fonts` is the sibling fonts checkout (same depth as web-app):
    // the file: deps stay symlinked there, and the rword-fonts dev middleware
    // serves FontWorker scripts from it - without it slices silently fall back
    // to the main-thread decode path.
    fs: { allow: ["../..", "../../../fonts"] },
  },
});
