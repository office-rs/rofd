// Tauri 平台文件桥：用原生对话框 + 文件系统替换 web-app 默认的浏览器实现。
// 打开走 dialog.open 拿真实路径后经 fs.readFile 读字节；保存走 dialog.save
// 拿目标路径后经 fs.writeFile 写字节。取消对话框返回 null / false。
//
// 另提供系统字体双能力（draft 2026-10-18 §3.2）：listSystemFonts（Rust 壳
// 启动扫描一次的索引）+ readSystemFont（按已扫描文件名读字节，原始 IPC）。
// fonts.ts 据双能力切换桌面纯系统字体通道。

import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { readFile, writeFile } from '@tauri-apps/plugin-fs';
import type { FileHost } from '../../../web-app/src/host';

const OFD_FILTER = { name: 'OFD 文档', extensions: ['ofd'] };

let lastPath: string | undefined;

export const tauriFileHost: FileHost = {
  async open() {
    const selected = await open({
      multiple: false,
      directory: false,
      defaultPath: lastPath,
      filters: [OFD_FILTER],
    });
    if (typeof selected !== 'string') return null; // 用户取消
    lastPath = selected;
    return await readFile(selected);
  },

  async save(bytes, suggestedName) {
    const target = await save({
      defaultPath: lastPath ?? suggestedName,
      filters: [OFD_FILTER],
    });
    if (!target) return false; // 用户取消
    lastPath = target;
    await writeFile(target, bytes);
    return true;
  },

  async listSystemFonts() {
    try {
      return await invoke<{ family: string; fileName: string }[]>(
        'list_system_fonts',
      );
    } catch {
      return []; // 索引不可用 → 调用方按空清单降级
    }
  },

  async readSystemFont(fileName) {
    try {
      // IPC 类型容差：新版返回原始字节（ArrayBuffer），降级路径可能是 number[]。
      const res = await invoke<ArrayBuffer | number[]>('read_system_font', {
        name: fileName,
      });
      if (res instanceof ArrayBuffer) {
        return new Uint8Array(res);
      }
      if (Array.isArray(res)) {
        return new Uint8Array(res);
      }
      return null;
    } catch {
      // 未扫描名/读取失败：该族跳过，替代链兜底（不报错）。
      return null;
    }
  },
};
