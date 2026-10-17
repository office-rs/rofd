# rofd web app

## 字体资产（切片通道）

- Noto 两族经 `@office-rs/font-runtime/vite` 从 npm 包（`file:` 过渡依赖）取切片，无需手工步骤。`file:` 过渡依赖需要 sibling 检出 `D:/code/fonts`（spec 2026-10-17 §7：npm 包上线后切 `^1.0.0`，届时移除本注记）。
- 商用六族走**本地切片**：`public/fonts-sliced/`（gitignored，不入库——商用字体
  子集化分发需单独授权）。从内网拷回该目录，或用 fonts 仓库工具自切：

  ```bash
  cd D:/code/fonts
  python tools/slice.py --input <simsun.ttc 路径> --key simsun --display-name 宋体 \
    --freq tools/freq/junda.txt --out D:/code/rofd/crates/web-app/public/fonts-sliced/simsun
  # 同姿势：simhei / msyh(--display-name 微软雅黑) / arial / times / calibri
  # 最后合并各族目录为单个 manifest.json（结构见 fonts 仓库 docs），或分别
  # 产出后在 fonts.ts 的 manifests 数组追加 URL。
  ```

  `--key` 必须用规范键（simsun/simhei/msyh/arial/times/calibri）——SDK 靠内核
  归一表（"宋体"→simsun 等）匹配声明名。
- 整字体 `public/fonts/*.ttf|ttc|otf` 已 `git rm --cached`（磁盘保留做
  `?legacyFonts=1` fixture）；新 clone 无此目录时 legacy 缝自动降级（console.warn）。
- tauri 发布**默认不含**商用切片（安装包分发=网络分发商用子集，需授权）；
  `fonts-sliced/` 缺席即降级，README 同样适用。
- **桌面（tauri）不依赖上述本机切片**：宿主桥提供 `readSystemFont` 能力时
  （`tauri-app/src/host/tauri.ts` → `src-tauri/src/fonts.rs` 白名单 IPC），
  `resolveFontConfig()` 自动改读 `%WINDIR%\Fonts` 的已授权商用字体（六文件白名
  单，零再分发）走整字体通道，并把商用切片 manifest 从配置中排除（一族一通
  道）；Noto 仍走切片。浏览器路径逐字节不变（决策层单测 `npm test`）。
