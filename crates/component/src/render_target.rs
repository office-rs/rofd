use rofd_render::Scene;

/// Abstract render surface. The host (native xilem canvas, wasm WebGPU) implements
/// this to blit a `imaging::record::Scene` (re-exported as [`rofd_render::Scene`])
/// to the GPU. The native xilem path consumes the scene directly via
/// `Painter::replay` and does not use this trait; the wasm WebGpuRenderTarget
/// implements it, converting the imaging scene to a `vello::Scene` via
/// `imaging_vello::VelloSceneSink` before rendering.
pub trait RenderTarget {
    /// Blit the scene to the GPU. Returns `true` when the frame reached the
    /// screen; `false` means the caller should retry on the next frame
    /// (feeds the component's `scene_needs_paint`).
    fn draw_scene(&mut self, scene: &Scene) -> bool;
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
}
