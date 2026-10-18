use imaging::kurbo::{Affine, Rect};
use imaging::record::replay;
use imaging::Painter;
use rofd_dom::PageId;
use rofd_render::{Scene, DESK_BG};

/// Body 层的一段：组件按页切好的只读向量场景（**页局部坐标**，origin 绝不
/// 烘入——页缓存按页复用、滚动零重组的前提）。
pub struct PageSegment<'a> {
    pub page_id: PageId,
    pub scene: &'a Scene,
    /// 该页在视口中的落位（已含缩放；滚动只改它——body 不重建）。
    pub origin: (f64, f64),
    /// 当前 zoom 下的页栅格尺寸（scene px；消费方按 dpr 放大分配位图）。
    pub raster_size: (u32, u32),
}

/// 组件每帧产出的分层结果：body 页（可缓存）+ 动态覆盖层（每帧新）。
/// 两层都不携带 desk 灰底——背景由消费方（默认合流或呈现层）补画。
pub struct SceneLayers<'a> {
    pub pages: Vec<PageSegment<'a>>,
    pub overlay: &'a Scene,
}

/// 页段 + overlay 的统一合流（spec 2026-10-15 §7.1）：desk 灰底最先铺满
/// `viewport`，各页段按 origin 平移追加（几何保持页局部），overlay 不平移
/// 追加。默认 [`RenderTarget::draw_layers`] 与组件 `scene_cache` 的构建共用
/// 此函数——背景色与层序只有这一个事实来源。
pub(crate) fn merge_scene_layers(
    pages: &[(&Scene, (f64, f64))],
    overlay: &Scene,
    viewport: (f64, f64),
) -> Scene {
    let mut merged = Scene::new();
    {
        let mut painter = Painter::new(&mut merged);
        painter.fill_rect(Rect::new(0.0, 0.0, viewport.0, viewport.1), DESK_BG);
    }
    for (scene, origin) in pages {
        merged.append_transformed(scene, Affine::translate(*origin));
    }
    replay(overlay, &mut merged);
    merged
}

/// Abstract render surface. The host (native xilem canvas, wasm WebGPU) implements
/// this to blit a `imaging::record::Scene` (re-exported as [`rofd_render::Scene`])
/// to the GPU. The native xilem path consumes the scene directly via
/// `Painter::replay` and does not use this trait; the wasm WebGpuRenderTarget
/// implements it, converting the imaging scene to a `vello::Scene` via
/// `imaging_vello::VelloSceneSink` before rendering.
///
/// The segmented path ([`RenderTarget::draw_layers`]) hands over per-page body
/// scenes plus an overlay instead of a single merged scene: the default
/// implementation merges them itself, so GPU targets keep today's behavior
/// unchanged, while targets that want page-level caching (web CPU fallback)
/// override it and consume the segmentation.
pub trait RenderTarget {
    /// Blit the scene to the GPU. Returns `true` when the frame reached the
    /// screen; `false` means the caller should retry on the next frame
    /// (feeds the component's `scene_needs_paint`).
    fn draw_scene(&mut self, scene: &Scene) -> bool;

    /// 分段路径（spec 2026-10-15 §7.1）。默认实现经
    /// [`merge_scene_layers`] 合流（desk 灰底最先，再各页段按 origin 平移、
    /// overlay 追加），整体经 `draw_scene` 上屏——GPU 实现不覆写即保持现状
    /// 行为；需要页级缓存的实现（web CPU 回退）才覆写。
    fn draw_layers(&mut self, layers: SceneLayers<'_>) -> bool {
        let pages: Vec<(&Scene, (f64, f64))> = layers
            .pages
            .iter()
            .map(|seg| (seg.scene, seg.origin))
            .collect();
        let merged = merge_scene_layers(&pages, layers.overlay, self.size());
        self.draw_scene(&merged)
    }

    fn size(&self) -> (f64, f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRenderTarget {
        drawn: usize,
        w: f64,
        h: f64,
    }
    impl RenderTarget for MockRenderTarget {
        fn draw_scene(&mut self, _scene: &Scene) -> bool {
            self.drawn += 1;
            true
        }
        fn size(&self) -> (f64, f64) {
            (self.w, self.h)
        }
    }

    #[test]
    fn mock_render_target_records_draws() {
        let mut rt = MockRenderTarget {
            drawn: 0,
            w: 800.0,
            h: 600.0,
        };
        let scene = Scene::new();
        rt.draw_scene(&scene);
        assert_eq!(rt.drawn, 1);
        assert_eq!(rt.size(), (800.0, 600.0));
    }

    /// 默认 draw_layers 合流语义：desk 灰底最先（铺满 target 视口）→ 各页段
    /// 按 origin 平移（几何保持页局部）→ overlay 不平移追加，最后整体走
    /// draw_scene。计数走 imaging::record 结构化判别（AGENTS §6 数对象）；
    /// 不用 Debug 字符串匹配——`fill_rule: Fill(NonZero)` 之类字段名会误数。
    #[test]
    fn default_draw_layers_merges_into_draw_scene() {
        use imaging::kurbo::{Affine, Rect};
        use imaging::peniko::{Brush, Color};
        use imaging::record::{Command, Draw, Geometry};
        use imaging::Painter;

        // 页局部场景：一个黑 fill（页底），origin 不烘入。
        let page_scene = {
            let mut s = Scene::new();
            let mut p = Painter::new(&mut s);
            p.fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::BLACK);
            s
        };
        // overlay：一个白 fill（视口原点起 5×5）。
        let overlay = {
            let mut s = Scene::new();
            let mut p = Painter::new(&mut s);
            p.fill_rect(Rect::new(0.0, 0.0, 5.0, 5.0), Color::WHITE);
            s
        };

        // 记录型 target：结构化记下 draw_scene 收到的每个 Fill
        // （几何、平移、颜色三元组）。
        struct RecordingTarget {
            w: f64,
            h: f64,
            fills: Vec<(Rect, Affine, [u8; 4])>,
        }
        impl RenderTarget for RecordingTarget {
            fn draw_scene(&mut self, scene: &Scene) -> bool {
                for cmd in scene.commands() {
                    if let Command::Draw(id) = cmd {
                        if let Draw::Fill {
                            transform,
                            brush,
                            shape,
                            ..
                        } = scene.draw_op(*id)
                        {
                            if let (Geometry::Rect(r), Brush::Solid(c)) = (shape, brush) {
                                self.fills
                                    .push((*r, *transform, c.to_rgba8().to_u8_array()));
                            }
                        }
                    }
                }
                true
            }
            fn size(&self) -> (f64, f64) {
                (self.w, self.h)
            }
        }

        let mut rt = RecordingTarget {
            w: 100.0,
            h: 100.0,
            fills: vec![],
        };
        let layers = SceneLayers {
            pages: vec![PageSegment {
                page_id: PageId::new("P1"),
                scene: &page_scene,
                origin: (20.0, 30.0),
                raster_size: (10, 10),
            }],
            overlay: &overlay,
        };
        assert!(rt.draw_layers(layers));

        // 三个 fill 全部落进合并场景（bg + 页 + overlay 缺一即漏层），
        // 顺序即 painter's-algorithm 层序。
        assert_eq!(
            rt.fills.len(),
            3,
            "desk bg + page fill + overlay fill must all land in the merged scene"
        );
        // 1) desk 灰底最先：铺满 target 视口、无平移、颜色对 composite 首操作。
        assert_eq!(rt.fills[0].0, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(rt.fills[0].1, Affine::IDENTITY);
        assert_eq!(rt.fills[0].2, [0xE0, 0xE0, 0xE0, 0xFF]);
        // 2) 页段：几何保持页局部（origin 绝不烘入），平移 = origin。
        assert_eq!(rt.fills[1].0, Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(rt.fills[1].1, Affine::translate((20.0, 30.0)));
        assert_eq!(rt.fills[1].2, [0x00, 0x00, 0x00, 0xFF]);
        // 3) overlay 最后追加、不平移。
        assert_eq!(rt.fills[2].0, Rect::new(0.0, 0.0, 5.0, 5.0));
        assert_eq!(rt.fills[2].1, Affine::IDENTITY);
        assert_eq!(rt.fills[2].2, [0xFF, 0xFF, 0xFF, 0xFF]);
    }
}
