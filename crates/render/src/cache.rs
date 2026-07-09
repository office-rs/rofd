//! Per-page scene cache.
//!
//! The body scene for a page is **stable** (the page's body objects do not
//! change at runtime), so it is built once on first access and reused. The
//! annotation scene is **rebuilt when dirty**: a caller mutates annotations and
//! calls [`PageSceneCache::invalidate`] for the affected page, and the next
//! [`PageSceneCache::annotation`] call rebuilds it.

use std::collections::HashMap;

use rofd_dom::{Annotation, Page, PageId, Resources};
use vello::Scene;

use crate::text::FontStore;

/// Per-page scene cache. `body` is stable (built once); `annotation` is rebuilt
/// when dirty (call [`Self::invalidate`] when annotations change).
#[derive(Default)]
pub struct PageSceneCache {
    body: HashMap<PageId, Scene>,
    annotation: HashMap<PageId, Scene>,
    annotation_dirty: HashMap<PageId, bool>,
}

impl PageSceneCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or build the body scene (stable - cached after first build).
    ///
    /// The body scene is page-local (no page-origin translation or zoom); the
    /// caller composites it via [`vello::Scene::append`] with the page
    /// transform.
    pub fn body(&mut self, page: &Page, res: &Resources, fonts: &FontStore) -> &Scene {
        self.body
            .entry(page.id.clone())
            .or_insert_with(|| crate::build_body_scene(page, res, fonts))
    }

    /// Get or rebuild the annotation scene. Call [`Self::invalidate`] when the
    /// page's annotations change; the next call rebuilds.
    ///
    /// The annotation scene is page-local (no page-origin translation or zoom);
    /// the caller composites it via [`vello::Scene::append`] with the page
    /// transform.
    pub fn annotation(
        &mut self,
        page: &Page,
        anns: &[Annotation],
        res: &Resources,
        fonts: &FontStore,
    ) -> &Scene {
        let dirty = self
            .annotation_dirty
            .get(&page.id)
            .copied()
            .unwrap_or(true);
        if dirty {
            self.annotation
                .insert(page.id.clone(), crate::build_annotation_scene(anns, res, fonts));
            self.annotation_dirty.insert(page.id.clone(), false);
        }
        self.annotation.get(&page.id).expect("annotation just inserted")
    }

    /// Mark a page's annotation scene dirty so it rebuilds on the next
    /// [`Self::annotation`] call. Call this whenever the page's annotations
    /// are added, removed, or modified.
    pub fn invalidate(&mut self, page: &PageId) {
        self.annotation_dirty.insert(page.clone(), true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cache_is_empty() {
        let cache = PageSceneCache::new();
        assert!(cache.body.is_empty());
        assert!(cache.annotation.is_empty());
        assert!(cache.annotation_dirty.is_empty());
    }

    #[test]
    fn invalidate_marks_page_dirty() {
        let mut cache = PageSceneCache::new();
        let pid = PageId::new("P0");
        assert!(
            !cache.annotation_dirty.contains_key(&pid),
            "fresh cache has no dirty flag"
        );
        cache.invalidate(&pid);
        assert_eq!(
            cache.annotation_dirty.get(&pid),
            Some(&true),
            "invalidate sets the dirty flag"
        );
    }
}
