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
- **slice.py 必须用 ≥ `50eb570` 的版本**（ASCII 并入 common-0 预载片，draft
  2026-10-17 §4.7"数字变粗"修正）：中文字族切片若不带 ASCII，混排数字会
  穿过全族切片落全局兜底，字形粗细不一致。切完可用 fonts 仓库 verify.py
  验证覆盖契约（切片并集 == cmap CJK 区 ∪ cmap ASCII）。
- 整字体 `public/fonts/*.ttf|ttc|otf` 已 `git rm --cached`（磁盘保留做
  `?legacyFonts=1` fixture）；新 clone 无此目录时 legacy 缝自动降级（console.warn）。
- tauri 发布**默认不含**商用切片（安装包分发=网络分发商用子集，需授权）；
  `fonts-sliced/` 缺席即降级，README 同样适用。
- **桌面（tauri）不依赖上述本机切片**：宿主桥提供 `listSystemFonts` +
  `readSystemFont` 双能力时（`tauri-app/src/host/tauri.ts` → `src-tauri/src/
  font_index.rs` 启动扫描索引 + 按名读取命令），`resolveFontConfig()` 切换
  **纯系统字体通道**（draft 2026-10-18）：manifest 集为空（Noto 切片不进
  桌面）、SDK 按文档声明族懒加载本机字节（零再分发、无白名单——任何已
  安装族自动可用）、默认字体宋体。浏览器路径逐字节不变（决策层单测
  `npm test`）。
