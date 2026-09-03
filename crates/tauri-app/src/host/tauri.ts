// Tauri 平台文件桥：用原生对话框 + 文件系统替换 web-app 默认的浏览器实现。
// 打开走 dialog.open 拿真实路径后经 fs.readFile 读字节；保存走 dialog.save
// 拿目标路径后经 fs.writeFile 写字节。取消对话框返回 null / false。
//
// 记住最近一次打开/保存的路径作为下次对话框的默认目录，贴近桌面应用习惯。

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
};
