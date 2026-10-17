import { defineConfig } from 'vitest/config';
import { fileURLToPath } from 'node:url';

// 决策层单测（resolveFontConfig）：happy-dom 提供模块级 window/location；
// virtual:rword-fonts（font-runtime vite 插件契约 id）以桩模块替代。存在本
// 文件时 vitest 不再合并 vite.config.ts，dev 服务器的 COEP/插件配置不介入。
export default defineConfig({
  test: {
    environment: 'happy-dom',
    // e2e/ 是 Playwright 的（npm run e2e）；vitest 只跑单测。
    exclude: ['**/node_modules/**', '**/dist/**', 'e2e/**'],
  },
  resolve: {
    alias: {
      'virtual:rword-fonts': fileURLToPath(
        new URL('./src/__tests__/stubs/rword-fonts.ts', import.meta.url),
      ),
    },
  },
});
