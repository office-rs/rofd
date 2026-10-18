//! CPU 软渲染呈现目标（spec 2026-10-18-no-gpu-support §8）：vello_cpu
//! 栅格 + 页位图缓存（[`PageRasterCache`]）+ canvas2d 上屏。WebGPU 不可用
//! 时的自动回退路径，整体 `#[cfg(target_arch = "wasm32")]`。
//!
//! wasm 高水位纪律：heap 只留当前栅格工作缓冲（vello_cpu 的 RgbaImage），
//! 成品像素立即离开 heap 进 OffscreenCanvas（浏览器管理、可回收）。
//!
//! dpr 纪律：canvas 物理尺寸 = css × dpr；页场景/overlay 是 dpr-free 的
//! scene px，栅格时经 `replay_transformed(Affine::scale(dpr))` 放大——
//! vello_cpu 按 1:1 坐标栅格化，不会自动缩放场景。

use imaging::kurbo::Affine;
use imaging::record::replay_transformed;
use imaging_vello_cpu::VelloCpuRenderer;
use rofd_component::render_target::{PageSegment, RenderTarget, SceneLayers};
use rofd_dom::PageId;
use rofd_render::{PageCacheKey, PageRasterCache, Scene, DESK_BG};
use wasm_bindgen::JsCast;
use web_time::Instant;

use crate::zoom_gesture::ZoomGesture;

/// CPU 呈现目标的调优参数（Task 8 从宿主配置注入）。
#[derive(Clone, Copy, Debug)]
pub struct CpuTuning {
    /// 页位图 LRU 容量（可视 ± 邻页窗口）。
    pub max_cached_pages: usize,
    /// 单页物理像素上限（防高缩放大页爆内存）。
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
    overlay: Option<OverlaySurface>,
    /// 创建时捕获的 devicePixelRatio（与 WebGPU 路径的固定 surface 配置
    /// 同语义：浏览器缩放变化不重配，直到目标重建）。
    dpr: f64,
    /// 视口 css 尺寸（canvas 物理尺寸 = css × dpr）。
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
    pub fn new(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        tuning: CpuTuning,
    ) -> Result<Self, String> {
        let dpr = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0);
        canvas.set_width((width as f64 * dpr) as u32);
        canvas.set_height((height as f64 * dpr) as u32);
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
            overlay: None,
            dpr,
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

    /// 视口 resize（css 尺寸）。同步 canvas 物理尺寸；**不失效页位图**——
    /// 页栅格尺寸只由 页尺寸×zoom×dpr 决定，与视口无关（spec §7.3）。
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.canvas.set_width((width as f64 * self.dpr) as u32);
        self.canvas.set_height((height as f64 * self.dpr) as u32);
        // resize 清空 canvas 内容；overlay 面按尺寸惰性重建（见 draw_layers）。
    }

    /// 宿主（WasmOfd）在 zoom/字体/文档变化时喂进来。zoom 变化进
    /// 手势状态机（手势中记录拉伸比，draw_layers 用旧位图按比顶着）；
    /// font_gen/body_revision 变化只影响缓存键（旧条目自然 miss）。
    pub fn note_state(&mut self, zoom: f64, font_gen: u32, body_revision: u64, now: Instant) {
        if !self.fed_state {
            // 首帧：以当前 zoom 立为缓存代际基线（无手势、无拉伸）。
            self.zoom = zoom;
            self.gesture = ZoomGesture::new(zoom);
            self.fed_state = true;
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
            dpr: self.dpr,
            font_gen: self.font_gen,
            body_revision: self.body_revision,
        }
    }

    /// 视口物理尺寸（css × dpr）。饱和转换：病态巨视口钳到 u16 上限。
    fn phys_size_u16(&self) -> (u16, u16) {
        (
            (self.width as f64 * self.dpr) as u16,
            (self.height as f64 * self.dpr) as u16,
        )
    }

    /// 软栅格一段场景：缓冲 = raster 尺寸 × dpr，场景经 `scale(dpr)`
    /// 重放（vello_cpu 1:1 栅格化，不自动缩放）。零尺寸/栅格失败 → None。
    fn raster_scaled(scene: &Scene, w: u16, h: u16, dpr: f64) -> Option<imaging::RgbaImage> {
        if w == 0 || h == 0 {
            return None;
        }
        let mut renderer = VelloCpuRenderer::new(w, h);
        replay_transformed(scene, &mut renderer, Affine::scale(dpr));
        renderer.finish().ok()
    }

    /// 栅格一段页场景到位图（尺寸 = raster_size × dpr）。
    fn raster_page(seg: &PageSegment<'_>, dpr: f64) -> Option<imaging::RgbaImage> {
        let w = (seg.raster_size.0 as f64 * dpr) as u16;
        let h = (seg.raster_size.1 as f64 * dpr) as u16;
        Self::raster_scaled(seg.scene, w, h, dpr)
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
        let scale = self.pending_scale.unwrap_or(1.0); // 手势中旧位图按比拉伸顶着

        // 1) desk 灰底是 CPU 目标自己的第一笔（合流方裁定：页场景与
        //    overlay 都不携带背景）——整幅物理矩形铺满。
        let (pw, ph) = (self.width as f64 * self.dpr, self.height as f64 * self.dpr);
        self.ctx.set_fill_style_str(&self.desk_fill);
        self.ctx.fill_rect(0.0, 0.0, pw, ph);

        // 2) 页位图：命中即 drawImage；miss 则栅格+上传+入缓存。
        for seg in &layers.pages {
            let key = self.key(&seg.page_id);
            self.cache.touch_key(&key); // 命中刷新 LRU（按页 id，每帧喂当前键）
            let bitmap: &PageBitmap = match self.cache.get(&key) {
                Some(b) => b,
                None => {
                    let Some(img) = Self::raster_page(seg, self.dpr) else {
                        continue; // 零尺寸页/栅格失败：本帧跳过
                    };
                    let w = (seg.raster_size.0 as f64 * self.dpr) as u32;
                    let h = (seg.raster_size.1 as f64 * self.dpr) as u32;
                    let Some(uploaded) = Self::upload(&img) else {
                        continue;
                    };
                    match self.cache.insert(key, w, h, move || uploaded) {
                        Some(b) => b,
                        // 超单页像素上限：不缓存、本帧跳过（防爆内存的降级）。
                        None => continue,
                    }
                }
            };
            // 落位：origin（scene px）× dpr → 物理 px；尺寸 = 位图原样
            // （或手势中按 scale 拉伸）。
            let dw = bitmap.canvas.width() as f64 * scale;
            let dh = bitmap.canvas.height() as f64 * scale;
            let _ = self.ctx.draw_image_with_offscreen_canvas_and_dw_and_dh(
                &bitmap.canvas,
                seg.origin.0 * self.dpr,
                seg.origin.1 * self.dpr,
                dw,
                dh,
            );
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
                if let Some(img) = Self::raster_scaled(layers.overlay, ow, oh, self.dpr) {
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
        if let Some(img) = Self::raster_scaled(scene, w, h, self.dpr) {
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
