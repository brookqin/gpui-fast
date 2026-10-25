//! Renderer-owned dynamic surfaces. Browser objects never enter the scene.
use crate::{Bounds, PaintSurface, Pixels, Window};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A process-unique source generation, invalid once its producer is released.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DynamicSurfaceId(u64);

impl Default for DynamicSurfaceId {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicSurfaceId {
    /// Allocate a source identity. Replacing a source requires a new identity.
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("Dynamic surface identities exhausted"),
        )
    }
}

impl Window {
    /// Paint a dynamic source at the current scene order and content mask.
    /// The renderer skips identities whose producer has been released.
    pub fn paint_dynamic_surface(&mut self, bounds: Bounds<Pixels>, source: DynamicSurfaceId) {
        self.invalidator.debug_assert_paint();
        self.next_frame.scene.insert_primitive(PaintSurface {
            order: 0,
            bounds: self.snap_bounds(bounds),
            content_mask: self.snapped_content_mask(),
            fast_dynamic_source: source,
            fast_dynamic_opacity: self.element_opacity,
        });
    }

    /// Request presentation of the existing scene without invalidating layout.
    /// Call on the window's UI thread; dropping the window makes its waker inert.
    pub fn dynamic_surface_frame_requester(&self) -> Rc<dyn Fn()> {
        let present = self.needs_present.clone();
        let wake = self.platform_window.frame_waker();
        Rc::new(move || {
            present.set(true);
            if let Some(wake) = &wake {
                wake();
            }
        })
    }
}
