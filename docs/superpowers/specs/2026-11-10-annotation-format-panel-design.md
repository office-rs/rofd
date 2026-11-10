# 批注文字格式面板（字体 / 字号 / 颜色）设计

日期：2026-11-10
状态：已实现（内核 + gpui-view + web SDK；xilem-view 面板待补，见 §5.3；§1.6 的"富 runs 塌缩"已由 [`2026-11-10-annotation-rich-text-design.md`](2026-11-10-annotation-rich-text-design.md) 取代并实现）
关联：[2026-11-09 批注文字流式排版](2026-11-09-annotation-text-wrap-design.md)

## 0. 问题与目标

### 0.1 现状

文字批注（Typewriter，无边框 TextBox）与文本框（有边框 TextBox）已经能编辑、折行、
向下增高、保存重开。但字体族（`font`）、字号（`size`）、字色（`color`）在设计上不可改：
创建工具写死默认值，编辑态没有任何入口修改；文档里已有的批注也改不了样式。
用户不得不回 WPS 改。

### 0.2 目标

选中一个文字批注 / 文本框后，在框上方浮出一条格式栏：

```
[ 宋体      ▾ ] [ 16.0        ▾ ] [ A ▾ ]
```

- **字体名称下拉**：列出可用字体（文档声明族 ∪ 运行时注册族）。
- **字号下拉**：Word 中文字号（初号…八号）与数字磅值（8…72）。
- **颜色按钮**：点开调色板浮层（自动 / 主题色网格 / 标准色），选色即生效。
- 三个适配器各自实现这份 UI（见 §2 分层结论），宿主应用零额外代码。

### 0.3 非目标

- 不实现加粗 / 斜体 / 下划线等富文本编辑（富文本框 `runs` 仍然只读保真，首次任意
  格式修改按既有规则塌缩为平铺样式）。
- 不实现"其他颜色 / 取色器"（WPS 面板底部两项）——v1 只给静态色板。§5 记为后续。
- 不给 Note / Watermark / 图章等非文字批注显示格式栏。
- 不改正文（body）样式。
- 不做段落格式（对齐 / 行距 / 缩进）。

## 1. 交互规范

### 1.1 显示与隐藏

- 触发：`AnnotationSelection::Single(id)` 且该批注 payload 是 `TextBox`（含 Typewriter
  与有边框文本框）。不区分工具（Text / Hand / Create 均可）。
- 隐藏：选区清空 / 变成 Multi / 换成非 TextBox 批注 / 文档换页后框滚出视口。
- 面板自身**不改变选区、不移动光标、不抢焦点**：点面板上的控件不得让批注失去选中态。
- 编辑中（有文字光标）继续显示；IME 组合期间也显示（组合串预览在框内，不影响面板）。

### 1.2 锚点

- 面板左上角 x = 框的左边界（视口坐标），y = 框顶 − 面板高 − 8px。
- 框顶空间不足（`y < 4`）时翻到框下方：y = 框底 + 8。
- x 越界时整体平移，保证面板完整落在视口内（与滚动条留 4px 间距）。
- 锚点由适配器在**每次布局 / 渲染**时从组件现算——不适配器缓存，也就不会随
  滚动 / 缩放 / 翻页失位；因此不新增"视口变化"回调（三个适配器本来就看得见
  wheel / zoom / resize / 页面切换这些输入与命令）。

### 1.3 字体下拉

- 数据源：`OfdComponent::font_families()` = 文档声明族（`Resources.fonts` 的
  `family_name`）∪ 运行时注册族（`FontStore::registered_family_names`，含默认字体族）。
  归一化去重、稳定排序（文档声明族在前，保持声明序；其余按名称排序）。
- 空列表时显示文档当前族（不可选第二项），不报错。
- 显示值：当前批注 `font` 解析出的族名；解析不到时显示 `font` 原串，再不行显示"字体"。
- 选择后写入 `payload.font = FontId::new(族名)`（详见 §4.2）。

### 1.4 字号下拉

- 列表 = 中文字号表 + 数字磅值表（取 rword `fontSize.ts` 全表）：

  | 中文字号 | 磅值 |  | 中文字号 | 磅值 |
  |---|---|---|---|---|
  | 初号 | 42 |  | 四号 | 14 |
  | 小初 | 36 |  | 小四 | 12 |
  | 一号 | 26 |  | 五号 | 10.5 |
  | 小一 | 24 |  | 小五 | 9 |
  | 二号 | 22 |  | 六号 | 7.5 |
  | 小二 | 18 |  | 小六 | 6.5 |
  | 三号 | 16 |  | 七号 | 5.5 |
  | 小三 | 15 |  | 八号 | 5 |

  数字项：8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72。

- 显示值：当前磅值能精确对上中文档时显示中文名，否则显示数值（一位小数，如 `16`）。
- 选择：把磅值换算成毫米写入（`mm = pt * 25.4 / 72`），保存按 `Size`（mm）落盘。
- 只允许从列表选择，不做自由输入（v1 取舍）。

### 1.5 颜色

- 触发按钮 `A ▾`：字母 A 下方一条当前色（WPS 样式）。
- 调色板浮层（贴按钮下方）：
  - `自动`：黑色 `Rgb(0,0,0)`（排首，带"自动"标签）。
  - `主题颜色`：10 列 × 6 行网格（WPS 主题色板的固定近似快照，不随主题计算）。
  - `标准色`：一行 10 色（rword `TEXT_COLORS`：黑、白、红、橙、黄、绿、青、蓝、
    深蓝、紫）。
- 选色即写入并关闭浮层；颜色不影响排版（不测高）。
- **色表归内核所有**：`rofd-component::TEXT_COLOR_PALETTE`（60 色，按 6 行 × 10 列
  排放）与 `TEXT_COLOR_STANDARD`（10 色），三端只渲染不各抄一份；wasm 侧经
  `textColorPalette() -> string[]` / `textColorStandard() -> string[]`
  （`#RRGGBB` 扁平数组）暴露给 SDK。60 个主题色是 WPS 风格的近似固定值
  （每列一个基色、每行一个明度档），后续可按 WPS 实测值替换，不改变接口。

### 1.6 修改的影响

- 字体 / 字号变化 → 重新排版测高，**只向下增高，绝不缩小**（与输入文字同一规则）。
- 字号变化后光标 offset 不变，重新定位到新行。
- 每次"选一项" = 一个 Transaction：一次 undo 恢复一次修改。
- 富文本框（`runs` 非空）第一次改字体/字号/颜色时在同一 Transaction 内塌缩为首个
  run 的平铺样式（`collapse_runs` 既有语义），一次 undo 还原完整富外观。
  **（2026-11-10 后续设计取代本条：见 [`2026-11-10-annotation-rich-text-design.md`](2026-11-10-annotation-rich-text-design.md)——
  框内有选中文字时只改选区，`runs` 升级为可编辑流式 `spans`，不再塌缩。）**

## 2. 分层结论

面板 UI 放进**三个 SDK 适配器**（用户视角：这是 SDK 自带能力，宿主不应重复搭建）。

```
rofd-editor    set_annotation_font_size_measured / set_annotation_font_family_measured
               set_annotation_color（已存在）
      │
rofd-render    FontStore 族名查询 / 未知 FontId 按族名解析
      │
rofd-component annotation_text_style() / annotation_format_anchor() / font_families()
               set_annotation_font_family() / set_annotation_font_size() / set_annotation_color()
      │
   ┌──┴───────────────┬──────────────────┐
xilem-view        gpui-view          web-view SDK
(OfdWidget 内建)  (OfdGpuiView 内建)  (DOM 浮层)
```

- **内核不做任何像素级的栏位绘制**：没有第二个输入焦点、没有无障碍缺口、不用复刻
  "chrome wins" 命中优先级（面板是独立控件，不会落在组件的 hit_test 里）。
- **适配器只做"把内核给的样式读出来 + 渲染 + 把选择写回去"**，业务语义（测高、
  塌缩、undo）全部留在内核。

## 3. 内核契约（新增 API）

### 3.1 `rofd-render`

1. `pub struct FamilyEntry { pub key: String, pub display: String }`——
   `key` 是 `normalize_family_key` 之后的规范键（写回/解析用），`display` 是给人看的
   名字。
2. `FontStore::family_entries() -> Vec<FamilyEntry>`——**列"可选"字体，不是"已注册"
   字体**（rword `get_font_families` 同义；分片懒加载下已注册族只是冰山一角）：
   - 原生：`fontique::Collection::family_names()`（系统字体库 + 已注册族，由
     fontique 的 System 后端枚举，不引入平台 crate）+ 文档声明族；过滤内部名
     `__rofs_*`。首次调用会触发系统字体扫描，结果缓存在组件里。
   - wasm：fontique 的系统后端为空，改由 SDK 把 manifest 的**可加载族**
     （规范键 + `displayName`）经 `set_available_font_families()` 推给组件，
     再并上文档声明族与已注册族（过滤 `__rofs_*`）。
   - 去重按规范键，声明族 / SDK 提供项优先保留 display。
3. `FontStore::family_display_name(&FontId) -> Option<String>`：注册族名 → 声明族名 →
   `None`。
4. `declared_families`（FontId → 声明族）改存**显示名**而不是归一化键；解析时先
   `normalize_family_key` 探切片链，命中就用切片，否则追加 `Named(显示名)` 交给
   fontique 按名匹配（原生系统字体；`宋体`/`SimSun` 都是真实族名）。这是"选族 →
   保存 → 原生重开"能真正换字体的前提；现有实现里"声明族无切片"会直接落默认族。
5. `family_list_for` 再补一段：`families` / `declared_families` 都未命中、
   `font_id.0` 非空时，按规范键探切片链，仍空则以 `FontId` 原串作具名族；
   最后才是 default / SansSerif 兜底（外来文件、字体资源缺失的路径）。
5. `pub fn annotation_viewport_rect(...)` 由 `pub(crate)` 提升为 `pub`（锚点计算复用）。
6. `pub fn format_panel_anchor(ann_rect, panel_size, viewport, margin) -> (f64, f64)`
   （见 §3.5）。

### 3.2 字体需求收集（必须改）

不新增 Font 资源意味着 `font_req::collect_family` 经 `resources.fonts` 查不到这个
FontId。改成：查不到时把 `normalize_family_key(font_id.0)` 也计入需求族列表。
否则 web 上选了一个还没加载的族，watcher 不会去补片，画面落回默认字体。

### 3.3 `rofd-editor`

`Editor::set_annotation_font_size_measured(id, size_mm, measure)` 与
`Editor::set_annotation_font_family(id, family, measure)`：

- 取 before 快照 → `collapse_runs` → 写 `size` / `font` → `measure(&after.payload)`
  返回 `Some(h)` 时同事务改 `rect.h` → `replace_txn`。
- `measure` 契约与 `insert_text_measured` 相同（只返回需要增高的目标高度）。

### 3.4 `rofd-component`

```rust
pub struct AnnotationTextStyle {
    pub family: String,   // 显示名；解析不到时为空串
    pub size_mm: f64,
    pub color: rofd_dom::Color,
    pub rich: bool,       // runs 非空（格式栏只读展示仍可用，改动会塌缩）
}

impl OfdComponent {
    pub fn annotation_text_style(&mut self, id: &AnnotationId) -> Option<AnnotationTextStyle>;
    pub fn annotation_format_anchor(&mut self, id: &AnnotationId) -> Option<rofd_dom::Rect>;
        // 视口坐标的批注矩形（annotation_viewport_rect 的薄封装）
    pub fn font_families(&mut self) -> Vec<FamilyEntry>;
    pub fn set_available_font_families(&mut self, families: Vec<FamilyEntry>);
        // wasm：SDK 把 manifest 的可加载族推进来（§3.1-2）
    pub fn set_annotation_font_family(&mut self, id: &AnnotationId, family: &str);
        // 写 payload.font = FontId(族名)（§4.2，内存态、不落盘）
    pub fn set_annotation_font_size(&mut self, id: &AnnotationId, size_mm: f64);
    pub fn set_annotation_color(&mut self, id: &AnnotationId, color: Color);
}
```

- 三个 setter 一律：`ensure_font_store` → editor 命令（带 `measure_textbox_growth`）→
  `after_annotation_change()`（清页缓存、置脏、fire `on_change`）。
- setter 不得改变 `AnnotationSelection` 与 `TextCursor`。
- `font_families` 需要 `&mut self`（惰性建 FontStore）。三条 `font_*` 方法都经
  `rofd-component` re-export（`pub use rofd_render::{FamilyEntry, format_panel_anchor}`），
  适配器只依赖 component，不新增 `rofd-render` 依赖边（AGENTS §3）。

### 3.5 面板几何（纯函数，供三端复用）

`rofd-render::format_panel_anchor(ann_rect, panel_size, viewport, margin) -> (f64, f64)`：

- 默认放上方；放不下翻下方；x 平移 clamp 进视口。
- 纯函数、无状态，单测覆盖四个角落与翻转。
- 放在 `rofd-render`（与 `annotation_viewport_rect` 同处），由 `rofd-component` re-export；
  适配器不直接依赖 render（见 §3.4 末条）。

## 4. 数据与持久化

### 4.1 字号单位

模型 `size` 是毫米（OFD `Size` 属性）；面板按磅（pt）展示与选择。
换算：`mm = pt * 25.4 / 72`，读取时反向。显示保留必要精度（整数磅显示整数，
10.5 显示 10.5）。

### 4.2 字体族：v1 不落盘（本会话生效）

**先回答"选了宋体，要把宋体裁剪塞进 res 吗"：不需要。** OFD 的 Font 资源允许只写
`FontName`、不写 `FontFile`——解析侧已有先例（`parse_font_res`：无 FontFile 即
"system font"），阅读器用本机同名族渲染。只有要"换台没有该字体的机器也长得一样"
时才需要嵌子集；v1 不做。

v1 的落盘取舍：**字体族不落盘**，字号 / 颜色照常持久化（都在既有 payload 字段里）。

- 选族时写 `payload.font = FontId::new(显示名)`（内存态）。渲染与测高沿用
  `family_list_for`，走 §3.1-5 的"未知 FontId → 规范键 → 切片链 → 具名族"路径：
  原生命中系统字体，wasm 命中已注册切片。
- 因为不新增 Font 资源，web 的字体 watcher 拿不到这个族名——需要 §3.2 的
  `font_req` 改动（未知 FontId 也把规范键计入需求族），否则下拉里选了未加载的族
  会落回默认字体。
- 保存时 TextObject 的 `Font="宋体"` 会随批注 XML 写出去，但包内没有对应 Font 资源：
  其他阅读器按"找不到资源"回退默认字体；rofd 自己重开时经 §3.1-5 按名字解析，
  **大概率还能恢复族名**——这是顺带的好处，不作为承诺。
- 后续做正式落盘时，改成"写 Font 资源（仅 FontName、无 FontFile）+ 手术刀增补
  `Res/Font.xml`"，接口不变；届时旧文件里的 `Font="宋体"` 仍由同一条回退路径解析。

### 4.3 颜色

写入 `payload.color`，现有 `<FillColor>` 序列化不变，重开无损。

### 4.4 颜色

写入 `payload.color`，现有 `<FillColor>` 序列化不变，重开无损。

## 5. 适配器实现要点

三端共用内核 API 与同一份视觉规格（行高 28、下拉宽 120 / 72、色块 16×16、圆角 4、
白底 1px 边框、阴影可省），但各自用本平台控件。

### 5.1 `rofd-gpui-view`

- `OfdGpuiView::render` 在画布 div 内加一个绝对定位子 div（`absolute().left(px(x)).top(px(y))`），
  坐标直接用视口坐标（元素自身的局部坐标）。
- 下拉用 gpui 的普通 div 列表（点击外部关闭：`on_mouse_down_out`）。
- 面板显示状态（哪个下拉打开）作为 `OfdGpuiView` 字段，`cx.notify()` 驱动重绘。
- 面板上的 pointer 事件由 gpui 正常派发，不会进入组件的 canvas 事件监听（父 div 的
  `on_mouse_down` 会先收到——需在面板子树上调用 `cx.stop_propagation()`）。
- 颜色按钮的色板同样绝对定位在按钮下方。

### 5.2 `rofd-web-view`（TS SDK）

- `Ofd.init` 在 container 内插入 `<div class="rofd-format-panel">`（`position:absolute`，
  container 需要 `position:relative`，SDK 负责补上）。
- 按钮 / 下拉均为 DOM；`pointerdown` 上 `preventDefault()`，**保住隐藏 IME input 的焦点**
  （否则一点面板光标就没了）。
- 样式走内联 style，不依赖宿主 CSS。
- 面板事件不冒泡到 canvas 的监听器（`stopPropagation`）。
- 新增 SDK 配置项 `formatPanel?: boolean`（默认 true）。

### 5.3 `rofd-xilem-view`

- 形态：`ofd()` 返回的视图内部变成
  `zstack((ofd_widget, panel_view)).alignment(UnitPoint::TOP_LEFT)`（rword 的
  `popover_overlay` 同款）；面板视图用 `transformed(...).translate(x, y)` 定位
  （`transform` 走 widget options，paint 与命中测试都由 masonry 的 pass 处理）。
  面板隐藏时是零尺寸空视图。
- **位置刷新**：滚动 / 缩放 / resize 不产生 action，纯视图的 translate 会失位。
  处理：`OfdWidget` 在这些 touch 末尾（以及面板可见期间的锚点变化时）提交
  `FormatPanelChanged` action，`OfdView::message` 把它当 handled 返回
  `Action(())`，app_logic 重跑 → 重算 translate。代价是"面板可见时"滚轮每格多一次
  宿主重跑（面板不可见时零成本）；xilem-app 的树是 diff 更新，可接受。
  spec 早先设想的"自定义 anchor wrapper widget 在 layout 里读共享锚点"是更省的
  方案，但它要自己写一个带 child pod 的 masonry widget；v1 先用 action 方案，
  卡顿再换。
- 面板状态（展开哪个下拉、当前样式快照）放在 `Arc<Mutex<FormatPanelState>>`：
  - widget 在每次 touch 末尾（`after_touch`）刷新快照；
  - 面板按钮点击经 `OfdCommandQueue` 推命令（与宿主工具栏同一通道）；
  - 命令排空后触发一次 `HostCommandWake` 重跑 app_logic，面板读到新快照。
- **风险点**：masonry `zstack` 的图层顺序与命中测试（面板外区域的点击能否穿透到
  OfdWidget、面板是否画在最上层、`request_layout` 是否会让 zstack 重新布局姊妹节点）
  需要用一次性 spike 验证。spike 不通过时的退路：把面板做成 `OfdWidget` 的 child
  `WidgetPod`，由 widget 自己 `layout` / 转发事件（工作量更大，但语义确定）。
- 面板禁用开关：`ofd(queue).format_panel(false)`。
- **实现状态（2026-11-10）**：本节的 zstack 组合会把 `OfdView::Element` 从
  `Pod<OfdWidget>` 改成 `Pod<ZStack>`，需要在 `ofd_view.rs` 里补一层组合
  View 的 build/rebuild/message/state 管道；本轮先交付内核 + gpui-view +
  web SDK，xilem 面板留作后续（适配器已具备全部内核接口）。

### 5.4 三端共享的常量与展示名

- 字号表、色板、规范键↔展示名映射都来自内核（component），三端不各自抄表：
  - `rofd-component::TEXT_SIZES_PT`（含中文名）、`TEXT_COLOR_STANDARD`、
    `TEXT_COLOR_PALETTE`；
  - wasm 侧补 `textSizes()` / `textColorPalette()` / `textColorStandard()` 三个只读导出。
- web SDK 在 `fontFlow.ready()` 之后把 manifest 的 `{key, displayName}` 经
  `setAvailableFontFamilies()` 推给组件（tauri 桌面通道同型：系统字体索引的
  `listSystemFonts` 结果也走这一口），这样下拉里列的是**能加载的**全部族，
  而不是当前已注册的切片。

## 6. 测试计划（TDD，先红后绿）

### rofd-render

- `family_entries` 合并、去重（声明族 / SDK 提供项优先 display），过滤 `__rofs_*`。
- 声明族无切片时按**具名族**解析（`FontRef{family_name:"宋体"}` → 字形来自系统宋体，
  不是默认字体）。
- 未知 `FontId("SimSun")` 走归一化族解析；`宋体` 与 `simsun` 解析到同一族。
- 切片族：`register_font_slice` 后 `FontId("宋体")` 命中切片链（wasm 重开路径）。
- `format_panel_anchor`：上方 / 下方翻转、左右 clamp、视口过小。
- `annotation_viewport_rect` 公开后的 roll/zoom 正确性（复用现有用例）。

### rofd-editor

- 改字号：一个 Transaction（undo 一次恢复字号与高度）。
- 改字号只增高不缩小（内容多、字号变大 → 高度增加；字号变小 → 高度不变）。
- 改字体族：写 `font = FontId(族名)`、触发 measure、单 Transaction；undo 后回滚。
- 富文本框：改字号 → runs 清空、平铺字号取首 run、一次 undo 全还原。

### rofd-component（字体需求）

- `FontId("宋体")`（资源里没有该 FontId）的批注计入需求族 `simsun`。

### rofd-component

- `annotation_text_style` 对 Typewriter / TextBox / 非文字批注 / 不存在 id 的返回。
- 三个 setter 后选区与光标不变；`on_change` 触发一次。
- 字号 setter 写 mm（pt→mm 换算）。
- `font_families` 空注册表不 panic；`set_available_font_families` 的内容并入列表。

### rofd-gpui-view / rofd-web-view / rofd-xilem-view

- gpui：选中文本框 → 面板出现；点字号项 → 文档字号改变且面板仍显示；取消选中 → 消失。
- xilem：headless harness —— 选中 → 面板视图存在；点击项 → 命令到达组件。
- web：SDK 单测 —— 面板 DOM 生成、锚点计算、pointerdown 不夺焦。
- 三端共同：面板点击不改变选区。

### 集成（真样例，`#[ignore]`）

- 打开真实 OFD → 选中批注 → 改字号 / 颜色 → 保存 → 重开，样式保留。

## 7. 风险

1. **xilem 浮层可行性**（§5.3）：先 spike（图层顺序 / 点击穿透 / 姊妹节点重布局），
   失败走 child WidgetPod 退路。
2. **字体族不落盘**（§4.2）：保存后 TextObject 的 `Font` 指向包内不存在的资源，
   其他阅读器回退默认字体；rofd 自身靠名字解析恢复。正式落盘（Font 资源 +
   `Res/Font.xml` 增补）留到后续，接口不变。
3. **跨平台族名**：规范键靠 `normalize_family_key` 别名表对齐（宋体/SimSun→simsun）；
   别名表之外的族（如不在表里的商用字体中文名）在另一端可能解析不到，降级为默认族。
   别名表是既有资产，本特性不扩表。
4. **wasm 族列表展示名**：切片族只能显示规范键；SDK 用 manifest 的 displayName
   富化展示（§5.4），内核不感知。
5. **面板遮挡选框手柄**：锚点默认在上方，框很靠上时翻到下方；宽度小于视口时 clamp。

## 8. 已定取舍

| 取舍 | 选择 | 理由 |
|---|---|---|
| 面板归属 | 三个 SDK 适配器 | 用户视角是 SDK 能力，宿主不重复搭建 |
| 字号输入 | 仅下拉 | v1 从简，WPS/Word 主路径也是下拉 |
| 颜色 | 静态色板，"其他颜色"后续 | 三端原生取色器成本高，收益低 |
| 字体持久化 | v1 不落盘（字体族仅本会话）；字号/颜色照常 | 不做 Font.xml / 全量写入；本机字体够用 |
| 字体下拉数据源 | 可选字体（系统 ∪ 声明 ∪ SDK 清单），不是已注册族 | 分片懒加载，注册族只是子集 |
| 主题色 | 近似 WPS 的固定 60 色 | 拿不到精确值，表可整体替换 |
| 高度 | 只增不减 | 与输入文字规则一致，编辑不抖动 |
| 改样式粒度 | 一次选择一个 Transaction | undo 语义直观 |
| 字号表 / 色板 | 常量放内核，三端只渲染 | 避免三端抄表漂移 |
| xilem 面板定位 | zstack + transformed + 视口变化 action 重算 | 复用 rword 已验证的 popover 形态；代价只在面板可见时 |
