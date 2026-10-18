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
}
