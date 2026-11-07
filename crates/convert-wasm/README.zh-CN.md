# rofd-convert-wasm

浏览器内 headless OFD（GB/T 33190）→ PDF 转换——wasm-bindgen `--target
web`，纯 CPU（无 WebGPU / Canvas），任意浏览器与 Web Worker 可用
（推荐在 Worker 中运行，避免阻塞主线程）。

## 构建

```bash
node crates/convert-wasm/sdk/build.mjs   # wasm-pack --target web --release -> sdk/dist/wasm
```

## 用法

```js
import init, {
  registerFont,
  registerDefaultFont,
  convertToPdf,
} from "@office-rs/rofd-convert-wasm";

await init();                    // 拉取并实例化 wasm
registerFont(simsunBytes);       // 浏览器里字体是显式的
registerDefaultFont(notoBytes);  // 可选：锚定默认字体
const { bytes, warnings } = convertToPdf(ofdBytes);
```

字体：浏览器没有系统字体发现——已注册（或文档内嵌）字节是唯二来
源；无法嵌入 PDF 的字体面其文字会被跳过并给出警告行（见
`warnings`）。

`convertToPdf` 以内核默认选项运行（内容三层全开、默认预算）。错误
抛异常；非致命警告随结果返回。

## 验收

```bash
node crates/convert-wasm/demo/run.mjs   # 主线程 + Worker == CLI 字节
```
