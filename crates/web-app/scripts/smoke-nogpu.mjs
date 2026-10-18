// 无 GPU 冒烟（spec 2026-10-15 §10）：headless Chrome --disable-gpu 下打开
// web-app，SDK 必须自动落入 CPU 软渲染回退并完成初始化（方法源自
// 2026-10-14 spike，实验 B：--disable-gpu 下 requestAdapter 返回 null）。
//
// 通过判据：console 出现回退标记（[rofd] WebGPU 不可用，已切换 CPU
// 软渲染）且没有 `editor init failed`。退出码契约：0 = PASS，1 = FAIL
// （Chrome 缺失 / vite 起不来 / 超时等基础设施错误一律按 FAIL 处理，
// 绝不静默通过）。
//
// Chrome 路径默认取 Windows 常见安装位；其他环境用 CHROME_PATH 覆盖：
//   CHROME_PATH="/usr/bin/google-chrome" npm run smoke:nogpu
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const PORT = 5199;
const CHROME_TIMEOUT_MS = 120_000;
const CHROME =
  process.env.CHROME_PATH ??
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe';
const MARKER = '[rofd] WebGPU 不可用，已切换 CPU 软渲染（页位图缓存已启用）';
const FAIL_MARKER = 'editor init failed';

// 脚本位于 crates/web-app/scripts/ → vite 项目根 = 上一级目录。
// 不用 `new URL('..', import.meta.url).pathname`——Windows 盘符路径会带
// 前导斜杠且反斜杠/编码处理脆弱；fileURLToPath 是唯一正确姿势。
const root = fileURLToPath(new URL('..', import.meta.url));

// Chrome 的 CONSOLE 日志（--enable-logging=stderr）可能把消息按 JSON
// 转义（非 ASCII → \uXXXX）；统一反转义后再匹配，两种形态都能命中。
function unescapeLog(text) {
  return text.replace(/\\u([0-9a-fA-F]{4})/g, (_, hex) =>
    String.fromCharCode(parseInt(hex, 16)),
  );
}

async function main() {
  // strictPort：URL 是硬编码给 Chrome 的，端口被占必须当场报错，
  // 不能让 vite 悄悄挪端口（那会让 Chrome 打开一个死链接 → 假失败）。
  const server = await createServer({
    root,
    server: { port: PORT, strictPort: true },
  });
  await server.listen();

  const profile = mkdtempSync(join(tmpdir(), 'rofd-smoke-'));
  let stderr = '';
  try {
    const proc = spawn(
      CHROME,
      [
        '--headless=new',
        '--disable-gpu',
        '--enable-logging=stderr',
        '--no-first-run',
        `--user-data-dir=${profile}`,
        '--virtual-time-budget=15000',
        '--dump-dom',
        `http://localhost:${PORT}/`,
      ],
      // stdout 是 --dump-dom 的整页 DOM：没人读它会把管道写满、Chrome
      // 永远退不出来（草图版坑）；直接丢弃，判定只看 stderr。
      { stdio: ['ignore', 'ignore', 'pipe'] },
    );
    proc.stderr.setEncoding('utf8');
    proc.stderr.on('data', (d) => {
      stderr += d;
    });

    const exited = new Promise((resolve) => {
      proc.once('error', (err) => {
        resolve({ err });
      });
      proc.once('close', (code) => {
        resolve({ code });
      });
    });
    const watchdog = setTimeout(() => {
      proc.kill();
    }, CHROME_TIMEOUT_MS);
    const result = await exited;
    clearTimeout(watchdog);

    if (result.err) {
      // Chrome 不在默认路径（ENOENT 等）：明确告知修法，按 FAIL 退出。
      console.error(`无法启动 Chrome（${CHROME}）：${result.err.message}`);
      console.error('请设置 CHROME_PATH 指向本机 chrome 可执行文件后重试。');
      process.exitCode = 1;
      return;
    }
    if (result.code !== 0) {
      console.error(`Chrome 异常退出，exit=${result.code}`);
      console.error(stderr);
      process.exitCode = 1;
      return;
    }

    const log = unescapeLog(stderr);
    const fellBack = log.includes(MARKER) || log.includes('cpu-fallback');
    const failed = log.includes(FAIL_MARKER);
    const markerLine =
      log
        .split(/\r?\n/)
        .find((line) => line.includes('CPU 软渲染') || line.includes('cpu-fallback'))
        ?.trim() ?? '(未找到回退标记行)';
    console.log(`fallback=${fellBack} initFailed=${failed} exit=${result.code}`);
    console.log(`fallback-line: ${markerLine}`);

    if (!fellBack || failed) {
      console.error('smoke:nogpu FAIL：未落入 CPU 回退或初始化失败。');
      console.error(stderr);
      process.exitCode = 1;
      return;
    }
    console.log('smoke:nogpu PASS');
  } finally {
    await server.close();
    // 临时 profile 尽力清理（Chrome 进程退出后即可删）。
    try {
      rmSync(profile, { recursive: true, force: true });
    } catch {
      // 清理失败不影响判定。
    }
  }
}

try {
  await main();
  // 给 stdout 一点冲刷时间再退出（vite/脚本日志走管道时异步）。
  await new Promise((r) => setTimeout(r, 100));
  process.exit(process.exitCode ?? 0);
} catch (e) {
  console.error(`smoke:nogpu FAIL：${e instanceof Error ? e.message : String(e)}`);
  process.exit(1);
}
