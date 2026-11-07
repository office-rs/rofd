# rofd-convert (CLI)

`rofd-convert` 命令行工具：在脚本、CI 和终端里把 OFD（GB/T 33190）文档转成 PDF。一次转换一个进程——刻意不做并发载体，并行在 shell 层完成。

[English](README.md)

## 安装 / 构建

```bash
cargo build -p rofd-convert-cli --release   # -> target/release/rofd-convert(.exe)
```

## 用法

```text
Usage: rofd-convert <input.ofd> <output.pdf|--stdout> [options]

Options:
  --font <file>          Register a font file into the process registry (repeatable)
  --fonts-dir <dir>      Register every file in the directory (repeatable)
  --default-font <file>  Set the default fallback font
  --no-watermark         Exclude the watermark layer (default: all layers on)
  --no-annotations       Exclude annotation layers (default: on)
  --no-seal              Exclude the seal layer (default: on)
  --max-pages <n>        Page budget (default 10000)
  --max-media-bytes <n>  Embedded-media byte budget (default 1073741824)
  -h, --help             Print usage
  -V, --version          Print version
```

- 退出码：`0` 成功 · `2` 用法错误 · `3` 转换失败（含输入不可读 / 输出不可写）
- warning（非致命解析降级）逐行走 stderr；文档照样转出
- `--stdout` 二进制安全（不追加换行）
- 文本参数（flag、位置参数、数值）要求合法 UTF-8（否则退出 2）；路径值（`--font`、`--fonts-dir`、`--default-font`）原样传给操作系统，非 UTF-8 字体路径可用

单次转换：

```bash
rofd-convert report.ofd report.pdf --default-font fonts/simsun.ttc
rofd-convert report.ofd --stdout > report.pdf        # 二进制安全的 stdout
rofd-convert scanned.ofd clean.pdf --no-watermark --no-seal   # 净内容：正文 + 批注
```

## 字体注册

`--font` 把字体文件注册进进程注册表；`--fonts-dir` 注册目录下所有文件；`--default-font` 额外锚定兜底字体（正文声明的族没有注册字节时使用）。注册发生在转换之前（转换器启动时对注册表做快照）；读取失败与不可解析的字体字节只打 stderr warning，转换继续走兜底渲染。

对带文字的文档，请至少注册一个字体：注册表为空时文本整形会退到系统字体，而 PDF 子集化器未必能嵌入系统兜底字体——此时转换以编码错误失败（退出 3），而不是悄悄丢字形。

## 确定性

CLI 天生单发：一个进程转一个文档。同一台机器上，相同输入字节 + 相同进程字体状态，任何进程里产出都逐字节一致：

```bash
rofd-convert a.ofd a.pdf --default-font fonts/simsun.ttc
rofd-convert a.ofd b.pdf --default-font fonts/simsun.ttc
cmp a.pdf b.pdf && echo identical   # 永远一致
```

原生侧注意事项（spec D19/§8）：系统字体回退依赖本机字体环境。一个字体都没注册时，文本整形会退到机器上发现的字体，而各机器字体不同——跨机器逐字节可复现要求显式注册所有承载文字的字体（`--font` / `--fonts-dir` / `--default-font`）。

需要进程内并发（长期实例、上千文档、线程池）？用 [Rust crate](../convert)——它的 API 才是为此准备的。

## 另见

- 内核 API：[crates/convert](../convert)
- 设计文档：`docs/superpowers/specs/2026-11-06-rofd-convert-design.md` §6.3
