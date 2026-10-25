pub(crate) mod web_backends;

#[cfg(feature = "webgpu")]
pub(crate) mod browser_surface {
    pub use gpui_wgpu::{BrowserSurface, BrowserSurfaceMode, BrowserSurfaceStats};
}
