//! CPU 软渲染呈现目标（spec 2026-10-18-no-gpu-support §8）：vello_cpu
//! 栅格 + 页位图缓存（[`PageRasterCache`]）+ canvas2d 上屏。WebGPU 不可用
//! 时的自动回退路径，整体 `#[cfg(target_arch = "wasm32")]`。
//!
//! wasm 高水位纪律：heap 只留当前栅格工作缓冲（vello_cpu 的 RgbaImage），
//! 成品像素立即离开 heap 进 OffscreenCanvas（浏览器管理、可回收）。
//!
//! 单位纪律（与 WebGpuRenderTarget 同约定，2026-10-18 统一）：SDK 是唯一
//! 施加 dpr 的层——视口/指针坐标/缩放锚点全以**设备 px**（css × dpr）喂入，
//! 组件场景即设备 px 空间。本目标 1:1 栅格、1:1 落位，内部不出现 dpr
//! （再乘即二次放大）；工厂期的初值只是首帧前的占位，SDK 的首个 resize
//! 即以设备 px 校正。

use std::collections::HashSet;

use imaging::kurbo::Affine;
use imaging::record::replay_transformed;
use imaging_vello_cpu::VelloCpuRenderer;
use rofd_component::render_target::{PageSegment, RenderTarget, SceneLayers};
use rofd_dom::PageId;
use rofd_render::{PageCacheKey, PageRasterCache, Scene, DESK_BG};
use wasm_bindgen::JsCast;
use web_time::Instant;

use crate::raster_cap::capped_raster_size;
use crate::zoom_gesture::{page_blit_plan, PageBlitPlan, ZoomGesture};

/// CPU 呈现目标的调优参数（Task 8 从宿主配置注入）。
#[derive(Clone, Copy, Debug)]
pub struct CpuTuning {
    /// 页位图 LRU 容量（可视 ± 邻页窗口）。
    pub max_cached_pages: usize,
    /// 单页物理像素上限（防高缩放大页爆内存）。超限页降采样渲染而非跳过
    /// （[`capped_raster_size`]）。
    pub max_page_pixels: u64,
}

impl Default for CpuTuning {
    fn default() -> Self {
        Self {
            max_cached_pages: 5,
            max_page_pixels: 8_000_000,
        }
    }
}

/// 一页的成品位图：栅格一次 put 进 OffscreenCanvas，之后每帧 drawImage。
struct PageBitmap {
    canvas: web_sys::OffscreenCanvas,
}

/// overlay 的常驻栅格面（每帧整幅重写、drawImage 合成）。复用同一块
/// OffscreenCanvas，避免每帧新建触发 GC 压力——成品像素只在浏览器侧。
struct OverlaySurface {
    canvas: web_sys::OffscreenCanvas,
    ctx: web_sys::OffscreenCanvasRenderingContext2d,
    w: u16,
    h: u16,
}

impl OverlaySurface {
    fn new(w: u16, h: u16) -> Option<Self> {
        let canvas = web_sys::OffscreenCanvas::new(u32::from(w), u32::from(h)).ok()?;
        let ctx = canvas
            .get_context("2d")
            .ok()??
            .dyn_into::<web_sys::OffscreenCanvasRenderingContext2d>()
            .ok()?;
        Some(Self { canvas, ctx, w, h })
    }
}

/// CPU 软渲染呈现目标：实现 [`RenderTarget`] 的分段路径
/// （[`RenderTarget::draw_layers`]）以消费页级缓存。
pub struct CpuCanvasRenderTarget {
    canvas: web_sys::HtmlCanvasElement,
    ctx: web_sys::CanvasRenderingContext2d,
    cache: PageRasterCache<PageBitmap>,
    /// 单页物理像素上限：栅格前经 [`capped_raster_size`] 预检 + 超限降
    /// 采样，与 insert 内部判定同参镜像——降采样后的尺寸必过 insert 的
    /// cap 检查；降采样后仍不可能（零尺寸/cap=0）才整页跳过。
    max_page_pixels: u64,
    /// 已发过降采样告警的页（每页首条 [`warn_capped_once`]；只增不清，
    /// 上界 = 页数——zoom 代际推进后同页再次降采样不重复刷屏）。
    warned_capped: HashSet<PageId>,
    overlay: Option<OverlaySurface>,
    /// 视口尺寸（设备 px = canvas 物理尺寸；SDK resize 喂 css × dpr）。
    width: u32,
    height: u32,
    /// desk 灰底的 CSS 色串（与 [`DESK_BG`] 同值，见 new）。
    desk_fill: String,
    /// 最近一次喂入的 zoom（原始值，非缓存代际）。
    zoom: f64,
    /// 是否已喂过首个状态（首帧以当前 zoom 直接立为缓存代际基线，
    /// 避免用 0.0 初始值当分母）。
    fed_state: bool,
    gesture: ZoomGesture,
    /// 手势期间的旧位图拉伸比（settle 后清空）。
    pending_scale: Option<f64>,
    font_gen: u32,
    body_revision: u64,
}

impl CpuCanvasRenderTarget {
    /// `width`/`height` 为 canvas 物理像素数（与 WebGpuRenderTarget::new 同
    /// 契约：工厂期的初值只是占位，SDK 的首个 resize 以设备 px 校正）。
    pub fn new(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        tuning: CpuTuning,
    ) -> Result<Self, String> {
        canvas.set_width(width);
        canvas.set_height(height);
        let ctx = canvas
            .get_context("2d")
            .map_err(|e| format!("get 2d context: {e:?}"))?
            .and_then(|obj| obj.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
            .ok_or_else(|| "no 2d context".to_string())?;
        // desk 灰底 CSS 色串：从 rofd_render::DESK_BG 派生（单一事实来源），
        // 不内联第二份字面量。
        let [r, g, b, _a] = DESK_BG.to_rgba8().to_u8_array();
        let desk_fill = format!("#{r:02X}{g:02X}{b:02X}");
        Ok(Self {
            canvas,
            ctx,
            cache: PageRasterCache::new(tuning.max_cached_pages, tuning.max_page_pixels),
            max_page_pixels: tuning.max_page_pixels,
            warned_capped: HashSet::new(),
            overlay: None,
            width,
            height,
            desk_fill,
            zoom: 0.0,
            fed_state: false,
            gesture: ZoomGesture::new(0.0),
            pending_scale: None,
            font_gen: 0,
            body_revision: 0,
        })
    }

    /// 视口 resize（设备 px，与 WebGPU 路径同约定）。同步 canvas 物理尺寸；
    /// **不失效页位图**——页栅格尺寸只由 页尺寸×zoom 决定（zoom 已是设备
    /// px/mm），与视口无关（spec §7.3）。
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        // resize 清空 canvas 内容；overlay 面按尺寸惰性重建（见 draw_layers）。
    }

    /// 宿主（WasmOfd）在 zoom/字体/文档变化时喂进来。zoom 变化进
    /// 手势状态机（手势中记录拉伸比，draw_layers 用旧位图按比顶着）；
    /// font_gen/body_revision 变化只影响缓存键（旧条目自然 miss）。
    ///
    /// zoom ≤ 0 的喂入一概不采纳（非法值经 raw/settled 会产生 inf/0
    /// 拉伸比）：首喂时保持未初始化基线等下一帧合法值；运行中则整体
    /// 忽略该帧的 zoom 分量，手势与缓存代际原样保留。
    pub fn note_state(&mut self, zoom: f64, font_gen: u32, body_revision: u64, now: Instant) {
        if !self.fed_state || self.zoom <= 0.0 || zoom <= 0.0 {
            if zoom > 0.0 {
                // 首喂/自非法基线恢复：以当前 zoom 直接立为代际基线
                // （无手势、无拉伸）。
                self.zoom = zoom;
                self.gesture = ZoomGesture::new(zoom);
                self.pending_scale = None;
                self.fed_state = true;
            }
        } else if zoom != self.zoom {
            self.pending_scale = self.gesture.zoom_step(zoom, now);
            self.zoom = zoom;
        }
        self.font_gen = font_gen;
        self.body_revision = body_revision;
    }

    fn key(&self, id: &PageId) -> PageCacheKey {
        PageCacheKey {
            page_id: id.clone(),
            zoom: self.gesture.settled(),
            // 场景已是设备 px（zoom 即设备 px/mm），物理缩放已烘进 zoom 与
            // raster_size——键的 dpr 维度对本目标恒为常量 1.0。
            dpr: 1.0,
            font_gen: self.font_gen,
            body_revision: self.body_revision,
        }
    }

    /// 视口尺寸的 u16 饱和转换（overlay 面尺寸）：病态巨视口钳到 u16 上限。
    fn phys_size_u16(&self) -> (u16, u16) {
        (
            self.width.min(u32::from(u16::MAX)) as u16,
            self.height.min(u32::from(u16::MAX)) as u16,
        )
    }

    /// 软栅格一段场景：视口 `(w, h)` 设备 px；`scale` 为场景坐标 → 视口的
    /// 等比缩放（1.0 = 1:1）。vello_cpu 按坐标原样栅格化、不自动缩放——
    /// 小视口上 1:1 replay 只会裁掉右下角，降采样必须经 `replay_transformed`
    /// 前置 `Affine::scale`；scale == 1.0 时前置恒等阵，与裸 replay 逐位
    /// 等价（乘单位阵不引入误差）。零尺寸/栅格失败 → None。
    fn raster_scaled(scene: &Scene, w: u16, h: u16, scale: f64) -> Option<imaging::RgbaImage> {
        if w == 0 || h == 0 {
            return None;
        }
        let mut renderer = VelloCpuRenderer::new(w, h);
        replay_transformed(scene, &mut renderer, Affine::scale(scale));
        renderer.finish().ok()
    }

    /// 栅格一段页场景并上传为成品位图，目标尺寸 `(w, h)`（u16 饱和）。
    /// 小于 `seg.raster_size` 时（超限降采样）按 `min(目标/源)` 等比缩放
    /// 栅格——纵横比由调用方 [`capped_raster_size`] 保证，min 取两侧较小
    /// 者防 floor ±1px 裁边。
    fn raster_page_bitmap(seg: &PageSegment<'_>, w: u32, h: u32) -> Option<PageBitmap> {
        let (sw, sh) = seg.raster_size;
        if sw == 0 || sh == 0 || w == 0 || h == 0 {
            return None; // 零尺寸：栅格不可能（预检机器兜底路径）
        }
        let vw = w.min(u32::from(u16::MAX)) as u16;
        let vh = h.min(u32::from(u16::MAX)) as u16;
        let scale = (f64::from(vw) / f64::from(sw)).min(f64::from(vh) / f64::from(sh));
        let img = Self::raster_scaled(seg.scene, vw, vh, scale)?;
        Self::upload(&img)
    }

    /// 画一个页位图：落位 origin（场景 = 设备 px）1:1；**尺寸由
    /// `seg.raster_size` 派生，不取位图自身尺寸**——手势中段即 raw 代
    /// （= 本帧应呈现的整页落位），settled 态即整页尺寸。正常（未超限）
    /// 条目位图 == raster_size，行为不变；超限降采样条目位图更小，由
    /// drawImage 在此放大回整页（略糊而非灰洞）。旧实现「位图尺寸 ×
    /// 手势拉伸比」与本式在未超限时数值恒等（raster_size ∝ zoom）。
    /// 关联函数显式收 ctx——调用方持 cache 借用时按字段拆分借用，
    /// 避免 `&self` 整体借用与 `&mut self.cache` 冲突。
    fn blit_page(
        ctx: &web_sys::CanvasRenderingContext2d,
        bitmap: &PageBitmap,
        seg: &PageSegment<'_>,
    ) {
        let dw = f64::from(seg.raster_size.0);
        let dh = f64::from(seg.raster_size.1);
        let _ = ctx.draw_image_with_offscreen_canvas_and_dw_and_dh(
            &bitmap.canvas,
            seg.origin.0,
            seg.origin.1,
            dw,
            dh,
        );
    }

    /// 超限降采样的一次性告知（每页首条）：点名页与上限——「静默把页面
    /// 变灰洞」正是要修的缺陷，降采样渲染绝不能再静默。
    fn warn_capped_once(&mut self, page: &PageId) {
        if self.warned_capped.insert(page.clone()) {
            web_sys::console::warn_1(
                &format!(
                    "[rofd] 页面 {} 超出像素上限 {}，已降采样渲染",
                    page.0, self.max_page_pixels
                )
                .into(),
            );
        }
    }

    /// 把软栅格结果整幅写进一个 OffscreenCanvas 2d context（0,0 起）。
    fn upload(img: &imaging::RgbaImage) -> Option<PageBitmap> {
        let canvas = web_sys::OffscreenCanvas::new(img.width, img.height).ok()?;
        let ctx: web_sys::OffscreenCanvasRenderingContext2d =
            canvas.get_context("2d").ok()??.dyn_into().ok()?;
        // SAFETY: view 只在本语句与 put_image_data 之间存活，绝不跨调用
        // 持有；期间 `img` 经 &RgbaImage 不可变借用（Vec 缓冲不迁移、
        // 不扩容）。put 同步拷走像素后 view 即刻离场。
        let clamped = unsafe { js_sys::Uint8ClampedArray::view(&img.data) };
        let data = web_sys::ImageData::new_with_js_u8_clamped_array_and_sh(
            &clamped, img.width, img.height,
        )
        .ok()?;
        ctx.put_image_data(&data, 0.0, 0.0).ok()?;
        Some(PageBitmap { canvas })
    }
}

impl RenderTarget for CpuCanvasRenderTarget {
    fn draw_layers(&mut self, layers: SceneLayers<'_>) -> bool {
        // 停稳检查先行：代际推进后 key.zoom 随 settled() 变化，本帧
        // 起旧条目自然 miss、按新代重栅；拉伸比同时退场。
        if self.gesture.maybe_settle(Instant::now()).is_some() {
            self.pending_scale = None;
        }

        // 1) desk 灰底是 CPU 目标自己的第一笔（合流方裁定：页场景与
        //    overlay 都不携带背景）——整幅物理矩形铺满。
        let (pw, ph) = (self.width as f64, self.height as f64);
        self.ctx.set_fill_style_str(&self.desk_fill);
        self.ctx.fill_rect(0.0, 0.0, pw, ph);

        // 2) 页位图：处置由 page_blit_plan 裁定——手势中的旧位图经
        //    blit_page 的 raster_size 落位隐式拉伸顶着；手势中的 miss 走
        //    一次性 raw 位图 1:1（段即 raw 代），绝不入缓存（防代际投毒）。
        let gesturing = self.pending_scale.is_some();
        for seg in &layers.pages {
            let key = self.key(&seg.page_id);
            self.cache.touch_key(&key); // 命中刷新 LRU（按页 id，每帧喂当前键）
            match page_blit_plan(self.cache.get(&key).is_some(), gesturing) {
                PageBlitPlan::StretchCached => {
                    let Some(bitmap) = self.cache.get(&key) else {
                        continue; // 与 plan 判定间无窗口，纯防御
                    };
                    Self::blit_page(&self.ctx, bitmap, seg);
                }
                PageBlitPlan::OneShotRaw => {
                    let (w, h) = seg.raster_size;
                    let Some(bitmap) = Self::raster_page_bitmap(seg, w, h) else {
                        continue; // 零尺寸页/栅格失败：本帧跳过
                    };
                    Self::blit_page(&self.ctx, &bitmap, seg);
                    // bitmap 就地离场（GC 回收），不入缓存。
                }
                PageBlitPlan::RasterAndCache => {
                    // 像素上限预检 + 超限降采样（spec 2026-10-15 §8 降采样
                    // 选项）：超限页不再整页跳过（灰洞 + 批注悬空——A4 满
                    // 档缩放、dpr2 中档缩放即触达），而是按 capped_raster_size
                    // 等比降采样栅格、以降采样尺寸入缓存（同参必过 insert
                    // 的 cap 检查）、blit 放大回整页落位，并每页一次性告警。
                    // 降采样后仍超限（cap=0 病态）或零尺寸 → 预检机器兜底，
                    // 本帧跳过。
                    let (w, h) = seg.raster_size;
                    let (cw, ch) = capped_raster_size(w, h, self.max_page_pixels);
                    if cw == 0 || ch == 0 || u64::from(cw) * u64::from(ch) > self.max_page_pixels {
                        continue;
                    }
                    if (cw, ch) != (w, h) {
                        self.warn_capped_once(&seg.page_id);
                    }
                    let Some(bitmap) = Self::raster_page_bitmap(seg, cw, ch) else {
                        continue;
                    };
                    if let Some(cached) = self.cache.insert(key, cw, ch, move || bitmap) {
                        Self::blit_page(&self.ctx, cached, seg);
                    }
                }
            }
        }

        // 3) overlay：全视口稀疏栅格（批注/caret/滚动条），drawImage
        //    **合成**到主 ctx——putImageData 是整幅替换语义，会把稀疏
        //    overlay 的透明像素盖掉已画的页位图，故必须走 drawImage。
        let (ow, oh) = self.phys_size_u16();
        if ow > 0 && oh > 0 {
            if self.overlay.as_ref().is_none_or(|o| o.w != ow || o.h != oh) {
                self.overlay = OverlaySurface::new(ow, oh);
            }
            if let Some(ov) = &self.overlay {
                if let Some(img) = Self::raster_scaled(layers.overlay, ow, oh, 1.0) {
                    // SAFETY: 同 upload——view 不跨调用持有，img 不可变借用。
                    let clamped = unsafe { js_sys::Uint8ClampedArray::view(&img.data) };
                    if let Ok(data) = web_sys::ImageData::new_with_js_u8_clamped_array_and_sh(
                        &clamped,
                        u32::from(ow),
                        u32::from(oh),
                    ) {
                        let _ = ov.ctx.put_image_data(&data, 0.0, 0.0);
                        let _ = self
                            .ctx
                            .draw_image_with_offscreen_canvas(&ov.canvas, 0.0, 0.0);
                    }
                }
            }
        }
        true
    }

    fn draw_scene(&mut self, scene: &Scene) -> bool {
        // 兜底整体路径（不分段时的语义完整性）：全视口软栅格后整幅
        // put。不额外铺 desk 灰底——默认 draw_layers 合流进来的 scene
        // 已含灰底；裸调用者拿到的是场景自身的内容，按场景原样呈现。
        let (w, h) = self.phys_size_u16();
        if w == 0 || h == 0 {
            return true;
        }
        if let Some(img) = Self::raster_scaled(scene, w, h, 1.0) {
            // SAFETY: 同 upload——view 不跨调用持有，img 不可变借用。
            let clamped = unsafe { js_sys::Uint8ClampedArray::view(&img.data) };
            if let Ok(data) = web_sys::ImageData::new_with_js_u8_clamped_array_and_sh(
                &clamped,
                u32::from(w),
                u32::from(h),
            ) {
                let _ = self.ctx.put_image_data(&data, 0.0, 0.0);
            }
        }
        true
    }

    fn size(&self) -> (f64, f64) {
        (self.width as f64, self.height as f64)
    }
}
