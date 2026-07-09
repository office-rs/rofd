use vello::Scene;

/// Abstract render surface. The host (Phase 4b) implements this to blit a
/// `vello::Scene` to the GPU (native: wgpu surface; wasm: WebGPU canvas).
pub trait RenderTarget {
    fn draw_scene(&mut self, scene: &Scene);
    fn size(&self) -> (f64, f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRenderTarget { drawn: usize, w: f64, h: f64 }
    impl RenderTarget for MockRenderTarget {
        fn draw_scene(&mut self, _scene: &Scene) { self.drawn += 1; }
        fn size(&self) -> (f64, f64) { (self.w, self.h) }
    }

    #[test]
    fn mock_render_target_records_draws() {
        let mut rt = MockRenderTarget { drawn: 0, w: 800.0, h: 600.0 };
        let scene = Scene::new();
        rt.draw_scene(&scene);
        assert_eq!(rt.drawn, 1);
        assert_eq!(rt.size(), (800.0, 600.0));
    }
}
