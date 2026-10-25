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

/// Fit is resolved from the current frame dimensions at presentation time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DynamicSurfaceFit {
    /// Stretch to the requested rectangle.
    #[default]
    Fill,
    /// Preserve aspect ratio with space around the frame.
    Contain,
    /// Preserve aspect ratio and clip excess pixels to the rectangle.
    Cover,
}

impl Window {
    /// Paint a dynamic source at the current scene order and content mask.
    /// The renderer skips identities whose producer has been released.
    pub fn paint_dynamic_surface(&mut self, bounds: Bounds<Pixels>, source: DynamicSurfaceId) {
        self.paint_dynamic_surface_with_fit(bounds, source, DynamicSurfaceFit::Fill);
    }

    /// Paint a source whose decoded dimensions may become available later.
    pub fn paint_dynamic_surface_with_fit(
        &mut self,
        bounds: Bounds<Pixels>,
        source: DynamicSurfaceId,
        fit: DynamicSurfaceFit,
    ) {
        self.invalidator.debug_assert_paint();
        self.next_frame.scene.insert_primitive(PaintSurface {
            order: 0,
            bounds: self.snap_bounds(bounds),
            content_mask: self.snapped_content_mask(),
            fast_dynamic_source: source,
            fast_dynamic_opacity: self.element_opacity,
            fast_dynamic_fit: fit,
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
