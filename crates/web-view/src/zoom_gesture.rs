//! 缩放手势状态机（spec §8）：手势中不重栅（旧位图拉伸），停稳 200ms
//! 后换代 zoom_key。时间由调用方注入（Instant 单调，AGENTS §4.4）。
//!
//! 纯逻辑、不 cfg 门控——native 可测（parse_key 先例）。Instant 类型经
//! cfg 别名：wasm 用 web_time（performance.now()），native 用 std。

use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// 手势停稳阈值：最后一次 zoom 变化后静默这么久才推进缓存代际。
pub const SETTLE: Duration = Duration::from_millis(200);

/// 缩放手势状态机。`settled` 是当前缓存代际的 zoom（页位图按它栅格化）；
/// 手势期间的中间 zoom 只体现为旧位图的拉伸比，代际不动——直到停稳。
#[derive(Debug)]
pub struct ZoomGesture {
    /// 当前缓存代际的 zoom。
    settled: f64,
    /// 手势中：(目标 zoom, 停稳时刻)。
    pending: Option<(f64, Instant)>,
}

impl ZoomGesture {
    pub fn new(settled: f64) -> Self {
        Self {
            settled,
            pending: None,
        }
    }

    /// 手势中每步调用。返回拉伸比（raw / 当前代际），None = 恰好回到
    /// 已停稳档，无需拉伸。停稳时刻随每一步顺延（RESTART 语义）。
    pub fn zoom_step(&mut self, raw: f64, now: Instant) -> Option<f64> {
        if raw == self.settled {
            self.pending = None;
            return None;
        }
        self.pending = Some((raw, now + SETTLE));
        Some(raw / self.settled)
    }

    /// 每帧调用。到期则推进代际并返回新 zoom（此刻缓存 key.zoom 随
    /// `settled()` 变化，旧条目自然失效，应整体重栅）。
    pub fn maybe_settle(&mut self, now: Instant) -> Option<f64> {
        let (target, at) = self.pending?;
        if now >= at {
            self.settled = target;
            self.pending = None;
            return Some(target);
        }
        None
    }

    /// 当前缓存代际的 zoom（缓存键分量）。
    pub fn settled(&self) -> f64 {
        self.settled
    }
}

/// 一帧内一个页段的处置方式（由 [`page_blit_plan`] 裁定，
/// CpuCanvasRenderTarget::draw_layers 消费）。
#[derive(Debug, PartialEq, Eq)]
pub enum PageBlitPlan {
    /// 缓存命中：位图是 settled 代内容，按拉伸比画（手势中 > 1 的比把
    /// 旧位图顶着用；settled 态比恒为 1.0，同分支原样画）。
    StretchCached,
    /// 手势中的 miss：段是 raw 代（origin/raster_size 已按 raw zoom），
    /// 位图 raw 尺寸 1:1 画，**不入缓存**——缓存键是 settled 代，入缓存
    /// = 以 settled 键存 raw 内容（代际投毒：手势无停稳捏回原档后键
    /// 重新命中，页面长期错尺寸）。停稳后下一帧自然走 RasterAndCache。
    OneShotRaw,
    /// settled 态 miss：栅格 + 入缓存 + 原样画（键与内容同代）。
    RasterAndCache,
}

/// 纯逻辑裁定：`(cache_hit, gesturing)` → 页段处置。
/// 拉伸比只作用于**已缓存**的 blit——手势中的 miss 永不被拉伸。
pub fn page_blit_plan(cache_hit: bool, gesturing: bool) -> PageBlitPlan {
    match (cache_hit, gesturing) {
        (true, _) => PageBlitPlan::StretchCached,
        (false, true) => PageBlitPlan::OneShotRaw,
        (false, false) => PageBlitPlan::RasterAndCache,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gesture_defers_settle_then_commits() {
        let t0 = Instant::now();
        let mut g = ZoomGesture::new(1.0);
        assert_eq!(g.zoom_step(1.1, t0), Some(1.1), "gesture scales old bitmap");
        assert_eq!(g.maybe_settle(t0 + SETTLE - Duration::from_millis(1)), None);
        assert_eq!(g.maybe_settle(t0 + SETTLE), Some(1.1));
        assert_eq!(g.settled(), 1.1);
        assert_eq!(g.zoom_step(1.1, t0 + SETTLE), None, "same zoom = no scale");
    }

    #[test]
    fn multi_step_tracks_latest_target_and_ratio() {
        let t0 = Instant::now();
        let mut g = ZoomGesture::new(1.0);
        assert_eq!(g.zoom_step(1.1, t0), Some(1.1));
        // 第二步：目标与拉伸比都换代（分母仍是旧代 1.0，不累计）。
        assert_eq!(
            g.zoom_step(1.25, t0 + Duration::from_millis(50)),
            Some(1.25)
        );
        // 停稳时刻随最后一步顺延：t0+250ms 前不推进。
        assert_eq!(
            g.maybe_settle(t0 + Duration::from_millis(249)),
            None,
            "deadline follows the latest step"
        );
        assert_eq!(g.maybe_settle(t0 + Duration::from_millis(250)), Some(1.25));
        assert_eq!(g.settled(), 1.25);
    }

    #[test]
    fn return_to_settled_clears_pending() {
        let t0 = Instant::now();
        let mut g = ZoomGesture::new(1.0);
        assert_eq!(g.zoom_step(1.1, t0), Some(1.1));
        // 捏回到原代际：无需拉伸，代际也无需推进。
        assert_eq!(g.zoom_step(1.0, t0 + Duration::from_millis(30)), None);
        assert_eq!(g.maybe_settle(t0 + SETTLE + SETTLE), None);
        assert_eq!(g.settled(), 1.0);
    }

    #[test]
    fn mid_gesture_miss_never_enters_cache() {
        // 代际投毒修复的核心不变量：手势中的 miss 绝不走“栅格入缓存”
        // ——段是 raw 代（origin/raster_size），缓存键是 settled 代，
        // 入缓存 = 以 settled 键存 raw 内容；若手势无停稳捏回原档
        // （return_to_settled 路径），键重新命中被投毒条目，页面长期
        // 错尺寸。手势中 miss 只能一次性 raw 位图 1:1 画。
        assert_eq!(
            page_blit_plan(false, true),
            PageBlitPlan::OneShotRaw,
            "gesturing miss must not be cached"
        );
    }

    #[test]
    fn blit_plan_covers_all_states() {
        use PageBlitPlan::*;
        // 命中：缓存是 settled 代内容，手势中按拉伸比顶着（settled 态
        // 拉伸比为 1.0，同一分支）。
        assert_eq!(page_blit_plan(true, true), StretchCached);
        assert_eq!(page_blit_plan(true, false), StretchCached);
        // settled 态 miss：栅格 + 入缓存 + 原样画。
        assert_eq!(page_blit_plan(false, false), RasterAndCache);
        // 手势中 miss：见 mid_gesture_miss_never_enters_cache。
    }
}
