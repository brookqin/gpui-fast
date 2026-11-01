// Modified by Longbridge for gpui-fast.
//! Immutable native video frames, composed in the same scene as the UI.
use crate::{Bounds, PaintSurface, Pixels, Window};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// A decoded frame. Arc handles share storage; each frame has an upload identity.
#[derive(Debug)]
pub struct VideoFrame {
    /// Identity used only for renderer cache invalidation.
    id: u64,
    /// Decoded pixel width.
    width: u32,
    /// Decoded pixel height.
    height: u32,
    /// Display aspect including encoded orientation and pixel aspect.
    aspect: f32,
    /// Display UV to encoded UV, column-major affine coefficients.
    source_transform: [f32; 6],
    /// Packed BGRA pixels on platforms without a native image buffer.
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    pixels: Vec<u8>,
    /// Retained native BGRA image. Neither the decoder nor renderer maps it.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    image: objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>,
}

impl VideoFrame {
    /// Process-unique immutable image identity.
    pub fn id(&self) -> u64 {
        self.id
    }
    /// Encoded width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }
    /// Encoded height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
    /// Aspect ratio after the source orientation is applied.
    pub fn aspect(&self) -> f32 {
        self.aspect
    }
    /// Validated packed BGRA storage.
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    /// Retained CoreVideo image.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn image(&self) -> &objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer> {
        &self.image
    }
    /// Set source orientation before sharing this frame with a scene.
    pub fn set_display_transform(&mut self, transform: [f32; 6], aspect: f32) -> bool {
        if !aspect.is_finite()
            || aspect <= 0.
            || transform.iter().any(|v| !v.is_finite())
            || (transform[0] * transform[3] - transform[1] * transform[2]).abs() < 1e-6
        {
            return false;
        }
        self.source_transform = transform;
        self.aspect = aspect;
        true
    }

    fn next_id() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("video identity exhausted")
    }
    /// Validate packed storage before handing it to a graphics driver.
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    pub fn bgra(width: u32, height: u32, pixels: Vec<u8>) -> Option<Arc<Self>> {
        let len = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if width == 0 || height == 0 || width > 8192 || height > 8192 || pixels.len() != len {
            return None;
        }
        Some(Arc::new(Self {
            id: Self::next_id(),
            width,
            height,
            aspect: width as f32 / height as f32,
            source_transform: [1., 0., 0., 1., 0., 0.],
            pixels,
        }))
    }
    /// Import an immutable BGRA CoreVideo image, retaining it through submission.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn pixel_buffer(
        image: objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>,
    ) -> Option<Arc<Self>> {
        use objc2_core_video::{
            CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
            kCVPixelFormatType_32BGRA,
        };
        let (width, height) = (
            CVPixelBufferGetWidth(&image) as u32,
            CVPixelBufferGetHeight(&image) as u32,
        );
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || CVPixelBufferGetPixelFormatType(&image) != kCVPixelFormatType_32BGRA
        {
            return None;
        }
        Some(Arc::new(Self {
            id: Self::next_id(),
            width,
            height,
            aspect: width as f32 / height as f32,
            source_transform: [1., 0., 0., 1., 0., 0.],
            image,
        }))
    }
}

/// Normalized OFD appearance transform and convex clipping polygon.
#[derive(Clone, Copy, Debug)]
pub struct VideoGeometry {
    /// Four transformed corners, clockwise from the top left.
    pub corners: [[f32; 2]; 4],
    /// Convex clipping polygon in the same coordinate space.
    pub polygon: [[f32; 2]; 4],
    /// Width / height before applying the appearance transform.
    pub aspect: f32,
}

/// Scene-owned video payload.
#[derive(Clone, Debug)]
pub struct VideoSurface {
    /// Immutable decoded frame.
    pub frame: Arc<VideoFrame>,
    /// Appearance geometry.
    pub geometry: VideoGeometry,
    /// Inherited element opacity.
    pub opacity: f32,
}
impl VideoSurface {
    /// Shared 160-byte shader ABI: bounds, mask, opacity, corners, polygon, fit.
    pub fn uniforms(&self, surface: &PaintSurface) -> [f32; 40] {
        let b = surface.bounds;
        let m = surface.content_mask.bounds;
        let ratio = self.frame.aspect;
        let aspect = self.geometry.aspect;
        let fit = if ratio > aspect {
            [1., aspect / ratio]
        } else {
            [ratio / aspect, 1.]
        };
        let mut u = [0.; 40];
        u[..12].copy_from_slice(&[
            b.origin.x.0,
            b.origin.y.0,
            b.size.width.0,
            b.size.height.0,
            m.origin.x.0,
            m.origin.y.0,
            m.right().0,
            m.bottom().0,
            self.opacity,
            1.,
            0.,
            0.,
        ]);
        for (i, p) in self
            .geometry
            .corners
            .iter()
            .chain(self.geometry.polygon.iter())
            .enumerate()
        {
            u[12 + i * 2..14 + i * 2].copy_from_slice(p);
        }
        u[28..30].copy_from_slice(&fit);
        u[32..38].copy_from_slice(&self.frame.source_transform);
        u
    }
}
impl Window {
    /// Compose a decoded video frame using the current scene order and mask.
    pub fn paint_video_frame(
        &mut self,
        bounds: Bounds<Pixels>,
        frame: Arc<VideoFrame>,
        geometry: VideoGeometry,
    ) {
        if !geometry.aspect.is_finite()
            || geometry.aspect <= 0.
            || geometry
                .corners
                .iter()
                .chain(&geometry.polygon)
                .flatten()
                .any(|v| !v.is_finite())
        {
            return;
        }
        self.invalidator.debug_assert_paint();
        self.next_frame.scene.insert_primitive(PaintSurface {
            order: 0,
            bounds: self.snap_bounds(bounds),
            content_mask: self.snapped_content_mask(),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            image_buffer: frame.image.clone(),
            fast_video: Some(VideoSurface {
                frame,
                geometry,
                opacity: self.element_opacity,
            }),
        });
    }
}
