# rofd-convert（Java）

rofd 转换内核（OFD → PDF）的 JNI 绑定。兼容 Java 8（`AutoCloseable` API、零外部依赖），Maven 分发。主 jar 内嵌全部四平台 native，一条普通依赖零配置加载；各平台 classifier jar 仍作为可选项。`OfdConverter` 是包装原生引擎的长期实例：OFD 字节进，PDF 字节出。

[English](README.md)

## 安装

Maven `io.github.office-rs:rofd-convert`。一条普通依赖就是全部配置：

```xml
<dependency>
  <groupId>io.github.office-rs</groupId>
  <artifactId>rofd-convert</artifactId>
  <version>0.1.0</version>
</dependency>
```

主 jar 内嵌全部四平台 native，零配置加载：首次使用时把探测到的平台库解包到临时目录、按绝对路径加载——无需 `java.library.path`、无需 classifier、无需任何配置。

- **体积敏感场景**可改用各平台 classifier jar（`linux-x86_64` / `windows-x86_64` / `macos-x86_64` / `macos-aarch64`）。loader 查找的是同一条 `natives/<platform>/<lib>` 资源，主 jar 内嵌与 classpath 上的 classifier jar 两种来源一视同仁。
- **显式覆盖**：`-Drofd.native.path=<文件或目录>`（或环境变量 `ROFD_NATIVE_PATH`，属性优先）钉死具体库——给自编/打过补丁的构建用。**已设置但不可加载 → 立即抛 `UnsatisfiedLinkError`，不降级**——静默回退内嵌库会让"自编库没生效"的问题无人察觉。

本地开发对着自编 cdylib 时，`-Djava.library.path=target/release` 继续可用——loader 在 classpath 上找不到资源时兜底走 `System.loadLibrary`。

本仓库内的本地构建（`cd crates/convert-java`）：

```bash
mvn -DskipTests package                                              # 瘦主 jar（不含 natives）
mvn -DskipTests package -Dnative.dir=<staging> -Dskip.natives.check=true   # fat，单平台本地打包
```

- 裸 `mvn package` 产出的**总是瘦 jar**——fat 形态必须带 `-Dnative.dir`；发布出去的主 jar 是 fat，因为发布流水线会把四平台全部 staging。
- 两种 jar 形态切换构建时务必夹一次 `mvn clean`——上一次 fat 打包留在 `target/classes` 的 `natives/` 会被下一次裸打包悄悄装进"瘦" jar。

staging 目录装 `<platform>/<lib>` 目录树（如 `windows-x86_64/rofdconvert.dll`，由 `cargo build --release -p rofd-convert-java` 产出）。不加 `-Dskip.natives.check=true` 时构建强制四平台齐全——发布打包绝不允许装出缺平台的 fat jar。

## 字体

注册是进程级、append-only、相同字节幂等的——启动时做一次，**先于创建第一个 converter**（每个 converter 在创建时对进程注册表做快照，之后的注册只对新实例可见）：

```java
OfdConverter.registerFontFile("fonts/simsun.ttc");      // true = 注册成功
OfdConverter.registerFont(fontBytes);                   // boolean
OfdConverter.registerDefaultFont(fontBytes);            // 锚定进程默认字体；后锚定者生效
boolean anchored = OfdConverter.hasDefaultFont();
String[] families = OfdConverter.registeredFamilies();  // 仅注册面
```

声明了未注册字体的文档照样转换——导出层降级为 warning + 跳过字形（经 `takeWarnings()` 取走），绝不是硬失败。真实文字文档想拿全保真输出，请提前注册一个 CJK 字体（如 `simsun.ttc`）。

## 单实例（不用池）

一个工作单元一个 `OfdConverter`，经静态工厂创建。线程契约：**同一时刻单线程**——不要跨线程共享实例。`close()`（或 try-with-resources）释放 native 句柄；PhantomReference 清理线程兜底回收漏关的实例。

```java
try (OfdConverter converter = OfdConverter.create()) {
    byte[] pdf = converter.convertPdf(ofdBytes);
    for (String w : converter.takeWarnings()) {   // 取走本次转换的 warnings
        System.err.println("warning: " + w);
    }
    // 带内容开关 / 预算：
    OfdConverter.OfdConvertOptions opts = new OfdConverter.OfdConvertOptions();
    opts.watermark = false;      // null = 内核默认（开）
    opts.maxPages = 5_000L;      // null 或 <= 0 = 内核默认（10_000）
    byte[] pdf2 = converter.convertPdf(ofdBytes, opts);
}
```

失败抛 `OfdConvertException`（unchecked）：decode/layout/encode 失败、预算超限、内部错误都走这里——内核 panic 绝不跨 JNI 边界。v1 只有单一输出格式（PDF），不暴露格式参数。

## 用池

并发转换用 `OfdConverterPool`：固定槽位集，`borrow()` 阻塞直到有空槽（60 秒封顶——租约卡死属 bug），`finally` 里务必 `returnConverter()`：

```java
try (OfdConverterPool pool = new OfdConverterPool(4)) { // 4 个 native 实例
    List<Callable<byte[]>> tasks = /* 每个文档一个 callable */;
    ExecutorService exec = Executors.newFixedThreadPool(8);  // 线程数可以多于槽位数
    List<Future<byte[]>> results = exec.invokeAll(tasks);
    // 每个 callable 内部：
    //   OfdConverter c = pool.borrow();
    //   try { return c.convertPdf(ofd); } finally { pool.returnConverter(c); }
}
```

进程字体状态相同的前提下，所有实例对相同输入产出**逐字节一致的输出**——池大小与线程交错不影响字节（`crates/convert-java/demo/` 两幕验收 demo 对 CLI 钉死这一点）。

## 另见

- 内核 API：[crates/convert](../convert) · CLI [crates/convert-cli](../convert-cli)
- 设计文档：`docs/superpowers/specs/2026-11-06-rofd-convert-design.md` §6.4
