//! 单页像素上限的降采样裁定（spec 2026-10-15-no-gpu-support §8「拒绝栅格
//! 并降采样/分块」的降采样选项）：纯逻辑、不 cfg 门控——native 可测
//! （zoom_gesture / parse_key 先例）。
//!
//! CPU 回退目标对超限页不再整页跳过（灰洞 + 批注悬空），而是等比降采样
//! 栅格、blit 时放大回整页落位（略糊而非消失）。本模块只裁定"目标尺寸"，
//! 栅格/缩放/告警由 `cpu_canvas_render_target` 消费（wasm 侧）。

/// 裁定单页栅格尺寸：`(w, h, cap) → (w', h')`。
///
/// - 不超上限（`w*h ≤ cap`）：原样返回（精确、零改动）。
/// - 超上限：按 `s = sqrt(cap / (w*h))` 等比降采样，返回值满足
///   `w'*h' ≤ cap` 且 `w' ≥ 1、h' ≥ 1`（cap ≥ 1 时），纵横比保持。
/// - 零尺寸输入原样透传（「连栅格都不可能」的情形仍由调用方预检机器
///   兜底跳过，见 draw_layers 的 RasterAndCache 分支）。
/// - cap == 0（病态配置）：返回 `(0, 0)`——任何正尺寸页都不可能满足，
///   调用方按零尺寸路径跳过。
///
/// f64 舍入可能在整数边界两侧各抬高 1px，floor 后乘积理论上仍可能微量
/// 超限；末尾的收缩循环兜底保证 `w'*h' ≤ cap` 契约绝对成立（每步砍较大
/// 边 1px 保纵横比；cap ≥ 1 时 1×1 必满足，循环有界不触零）。
pub fn capped_raster_size(w: u32, h: u32, cap: u64) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (w, h);
    }
    if cap == 0 {
        return (0, 0);
    }
    let pixels = u64::from(w) * u64::from(h);
    if pixels <= cap {
        return (w, h);
    }
    let s = (cap as f64 / pixels as f64).sqrt();
    let mut cw = ((w as f64) * s).floor().max(1.0) as u32;
    let mut ch = ((h as f64) * s).floor().max(1.0) as u32;
    while u64::from(cw) * u64::from(ch) > cap {
        if cw >= ch {
            cw -= 1;
        } else {
            ch -= 1;
        }
    }
    (cw, ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A4 @ 300%（最大档）× dpr1 ≈ 8.02Mpx，刚过 8M 默认上限——正是本次
    /// 修复瞄准的现实尺寸：普通硬件普通缩放即可触达，绝不许静默消失。
    #[test]
    fn under_cap_returns_exact() {
        assert_eq!(capped_raster_size(1000, 2000, 8_000_000), (1000, 2000));
        assert_eq!(capped_raster_size(2000, 3999, 8_000_000), (2000, 3999));
    }

    #[test]
    fn boundary_at_cap_returns_exact() {
        // 恰好等于上限（2000×4000 = 8M）不算超限。
        assert_eq!(capped_raster_size(2000, 4000, 8_000_000), (2000, 4000));
    }

    #[test]
    fn over_cap_downscales_within_cap_preserving_aspect() {
        // 2381×3368 = 8,019,208 > 8M：A4 满档缩放的现实超限页。
        let (cw, ch) = capped_raster_size(2381, 3368, 8_000_000);
        assert!(
            u64::from(cw) * u64::from(ch) <= 8_000_000,
            "capped {cw}x{ch} must satisfy the cap"
        );
        assert!(cw >= 1 && ch >= 1, "never collapse to zero");
        let src = 2381.0 / 3368.0;
        let dst = f64::from(cw) / f64::from(ch);
        assert!(
            ((src - dst) / src).abs() < 0.01,
            "aspect preserved: {src} vs {dst}"
        );
    }

    #[test]
    fn extreme_over_cap_hard_downscale() {
        // 病态巨页（4.29Gpx）也要收进 8M——等比硬缩，不拒绝。
        let (cw, ch) = capped_raster_size(65535, 65535, 8_000_000);
        assert!(u64::from(cw) * u64::from(ch) <= 8_000_000);
        assert!(cw >= 1 && ch >= 1);
    }

    #[test]
    fn zero_dims_pass_through_for_precheck() {
        assert_eq!(capped_raster_size(0, 500, 8_000_000), (0, 500));
        assert_eq!(capped_raster_size(500, 0, 8_000_000), (500, 0));
    }

    #[test]
    fn zero_cap_is_impossible() {
        assert_eq!(capped_raster_size(100, 100, 0), (0, 0));
    }

    #[test]
    fn degenerate_aspect_satisfies_cap_via_shrink_loop() {
        // 极长条页（1000:1）+ cap=1：floor 后必命中收缩循环，1×1 收口
        //（契约 w'*h' ≤ cap 绝对成立，包括浮点边界）。
        assert_eq!(capped_raster_size(100_000, 100, 1), (1, 1));
    }
}
