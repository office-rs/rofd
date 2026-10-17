import { chromium, defineConfig } from '@playwright/test';

// e2e 专用端口：本机 5173-5177 常年被残留 dev server 占用；5190 独占 + --strictPort
// （端口被占时 vite 直接失败，绝不静默跳端口让测试打到别人的服务器上）。
const PORT = 5190;
const BASE = `http://localhost:${PORT}`;

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  // 字体加载是时序敏感断言（固定等待 + 请求计数），串行单 worker 跑，
  // 避免并行测试互相挤占带宽/主线程造成抖动。
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  use: {
    baseURL: BASE,
    // 无头默认走 chrome-headless-shell，它不带 GPU 栈（requestAdapter ->
    // NotFound，Ofd.init 抛错、字体流根本不启动）。编辑器是 WebGPU-only，
    // 故强制用完整 chromium 二进制（new headless）+ SwiftShader 软件 WebGPU
    // 回退（有真 GPU 时用真适配器）。经测（参照 rword 同型配置）：
    // 不要加 --use-angle=swiftshader——它会弄坏 Dawn 适配器枚举。
    launchOptions: {
      executablePath: chromium.executablePath(),
      args: ['--enable-unsafe-webgpu', '--enable-unsafe-swiftshader'],
    },
  },
  webServer: {
    command: `npm run dev -- --port ${PORT} --strictPort`,
    url: BASE,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
