//! 页位图缓存引擎（spec 2026-10-15 §7.4）：平台无关 LRU + 像素上限。
//! 位图载体 B 参数化（web=OffscreenCanvas 句柄、纯像素宿主=RgbaImage）。
//! 失效 = key 五元组不匹配；LRU 窗口默认可视 ±2 页；单页像素上限防
//! 高缩放大页爆内存（A3@400% ≈ 113MB 的封顶，spec §附录 A.1 推算表）。

use std::collections::{HashMap, VecDeque};

use rofd_dom::PageId;

/// 缓存键五元组：任一分量不匹配即视为失效（页 / 缩放 / DPR / 字体代 / 正文修订）。
#[derive(Clone, Debug, PartialEq)]
pub struct PageCacheKey {
    pub page_id: PageId,
    pub zoom: f64,
    pub dpr: f64,
    pub font_gen: u32,
    pub body_revision: u64,
}

struct Entry<B> {
    key: PageCacheKey,
    bitmap: B,
}

pub struct PageRasterCache<B> {
    max_pages: usize,
    max_page_pixels: u64,
    entries: HashMap<PageId, Entry<B>>,
    lru: VecDeque<PageId>, // front = 最旧
}

impl<B> PageRasterCache<B> {
    pub fn new(max_pages: usize, max_page_pixels: u64) -> Self {
        Self {
            max_pages: max_pages.max(1),
            max_page_pixels,
            entries: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    fn touch(lru: &mut VecDeque<PageId>, id: &PageId) {
        lru.retain(|x| x != id);
        lru.push_back(id.clone());
    }

    pub fn get(&self, key: &PageCacheKey) -> Option<&B> {
        self.entries
            .get(&key.page_id)
            .filter(|e| &e.key == key)
            .map(|e| &e.bitmap)
    }

    /// 超像素上限的页返回 None（调用方自行降采样/分块策略，不缓存）。
    /// 注意 get() 是 &self 无法刷新 LRU——命中刷新由调用方经
    /// `touch_key` 完成（web-view 每帧对可见页调用）。
    pub fn touch_key(&mut self, key: &PageCacheKey) {
        if self.entries.contains_key(&key.page_id) {
            Self::touch(&mut self.lru, &key.page_id);
        }
    }

    pub fn insert(
        &mut self,
        key: PageCacheKey,
        w: u32,
        h: u32,
        build: impl FnOnce() -> B,
    ) -> Option<&B> {
        if u64::from(w) * u64::from(h) > self.max_page_pixels {
            return None;
        }
        if !self.entries.contains_key(&key.page_id) && self.entries.len() >= self.max_pages {
            if let Some(victim) = self.lru.pop_front() {
                self.entries.remove(&victim);
            }
        }
        let id = key.page_id.clone();
        let bitmap = build();
        self.entries.insert(id.clone(), Entry { key, bitmap });
        Self::touch(&mut self.lru, &id);
        self.entries.get(&id).map(|e| &e.bitmap)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::PageId;

    fn key(id: u32, zoom: f64, font_gen: u32) -> PageCacheKey {
        PageCacheKey {
            page_id: PageId::new(format!("P{id}")),
            zoom,
            dpr: 1.0,
            font_gen,
            body_revision: 0,
        }
    }

    #[test]
    fn hit_miss_and_key_sensitivity() {
        let mut c: PageRasterCache<u8> = PageRasterCache::new(5, 8_000_000);
        assert!(c.get(&key(1, 1.0, 0)).is_none());
        c.insert(key(1, 1.0, 0), 100, 100, || 7);
        assert_eq!(c.get(&key(1, 1.0, 0)), Some(&7));
        assert!(c.get(&key(1, 2.0, 0)).is_none(), "zoom mismatch");
        assert!(c.get(&key(1, 1.0, 1)).is_none(), "font_gen mismatch");
        assert!(c.get(&key(2, 1.0, 0)).is_none(), "page mismatch");
    }

    #[test]
    fn lru_evicts_oldest() {
        let mut c: PageRasterCache<u8> = PageRasterCache::new(2, 8_000_000);
        c.insert(key(1, 1.0, 0), 10, 10, || 1);
        c.insert(key(2, 1.0, 0), 10, 10, || 2);
        // 触碰 P1：P2 变最旧（get 是 &self 不刷新 LRU，触碰由 touch_key 承担）。
        c.touch_key(&key(1, 1.0, 0));
        let _ = c.get(&key(1, 1.0, 0));
        c.insert(key(3, 1.0, 0), 10, 10, || 3);
        assert!(c.get(&key(2, 1.0, 0)).is_none(), "LRU victim");
        assert!(c.get(&key(1, 1.0, 0)).is_some());
    }

    #[test]
    fn oversized_page_rejected() {
        let mut c: PageRasterCache<u8> = PageRasterCache::new(5, 100);
        assert!(c.insert(key(1, 1.0, 0), 20, 20, || 1).is_none()); // 400 px > 100 上限
        assert_eq!(c.len(), 0);
    }
}
