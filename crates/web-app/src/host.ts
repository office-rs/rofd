// 平台文件桥：把「打开 / 保存 OFD」从浏览器专属实现里抽出来，做成可替换的
// 注入点。web-app 用默认的浏览器实现（file input + Blob 下载）；tauri-app 复用
// 同一份 App.vue，只在挂载前调 `setFileHost` 换成原生对话框 + 文件系统实现。
//
// 约定：host 只负责「拿到字节 / 写出字节」，不碰编辑器与 UI 提示——loadOfd、
// message.success 等留在 App.vue，保证桥保持平台无关且无 antd 依赖。

/** 平台文件桥。open 取消返回 null；save 写出成功返回 true、取消返回 false。 */
export interface FileHost {
  /** 让用户选择一个 .ofd 文件，返回其字节；用户取消返回 null。 */
  open(): Promise<Uint8Array | null>;
  /** 把 OFD 字节写出（浏览器下载 / 原生保存对话框）。`suggestedName` 是
   * 默认文件名；写出成功返回 true，用户取消返回 false。 */
  save(bytes: Uint8Array, suggestedName: string): Promise<boolean>;
}

/** 默认浏览器实现：隐藏 <input type=file> 打开、Blob + <a download> 保存。 */
const browserFileHost: FileHost = {
  open() {
    return new Promise((resolve) => {
      const input = document.createElement('input');
      input.type = 'file';
      input.accept = '.ofd';
      input.style.display = 'none';
      // change 未触发就丢弃（用户取消）：无原生 cancel 事件，靠 window focus
      // 兜底——对话框关闭后窗口重新获得焦点，若此时仍无文件则视为取消。
      input.addEventListener('change', async () => {
        const file = input.files?.[0];
        input.remove();
        if (!file) {
          resolve(null);
          return;
        }
        resolve(new Uint8Array(await file.arrayBuffer()));
      });
      input.addEventListener(
        'cancel',
        () => {
          input.remove();
          resolve(null);
        },
        { once: true },
      );
      document.body.appendChild(input);
      input.click();
    });
  },

  save(bytes, suggestedName) {
    const blob = new Blob([bytes], { type: 'application/ofd' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = suggestedName;
    a.click();
    URL.revokeObjectURL(url);
    // 浏览器无法感知下载是否被用户取消，一律视为已触发保存。
    return Promise.resolve(true);
  },
};

let current: FileHost = browserFileHost;

/** 覆盖当前文件桥（tauri-app 在挂载前调用，注入原生实现）。 */
export function setFileHost(host: FileHost): void {
  current = host;
}

/** 当前生效的文件桥。App.vue 通过它执行打开 / 保存。 */
export const fileHost: FileHost = {
  open: () => current.open(),
  save: (bytes, name) => current.save(bytes, name),
};
