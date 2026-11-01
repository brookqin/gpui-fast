// Modified by Longbridge for gpui-fast.
//! Pixel checks exercise the actual Metal shader, not a second shader implementation.
use crate::metal_renderer::MetalRenderer;
use gpui::{
    Bounds, ContentMask, DevicePixels, PaintSurface, ScaledPixels, Scene, VideoFrame,
    VideoGeometry, VideoSurface, point, size,
};
use objc2_core_foundation::{CFDictionary, CFRetained, CFType};
use objc2_core_video::{
    CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelFormatType_32BGRA,
};
use parking_lot::Mutex;
use std::{ptr, sync::Arc};

#[test]
fn native_video_metal_rotation_clip_scroll_and_release() {
    let attrs = CFDictionary::from_slices(
        &[unsafe { kCVPixelBufferIOSurfacePropertiesKey }],
        &[&*CFDictionary::<CFType, CFType>::empty()],
    );
    let mut raw = ptr::null_mut();
    assert_eq!(
        unsafe {
            CVPixelBufferCreate(
                None,
                16,
                8,
                kCVPixelFormatType_32BGRA,
                Some(attrs.as_opaque()),
                ptr::NonNull::from(&mut raw),
            )
        },
        0
    );
    let buffer = unsafe { CFRetained::from_raw(ptr::NonNull::new(raw).unwrap()) };
    unsafe {
        assert_eq!(
            CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()),
            0
        );
        let stride = CVPixelBufferGetBytesPerRow(&buffer);
        let base = CVPixelBufferGetBaseAddress(&buffer).cast::<u8>();
        let colors = [
            [[0, 0, 255, 255], [0, 255, 0, 255]],
            [[255, 0, 0, 255], [0, 255, 255, 255]],
        ];
        for y in 0..8 {
            for x in 0..16 {
                ptr::copy_nonoverlapping(
                    colors[y / 4][x / 8].as_ptr(),
                    base.add(y * stride + x * 4),
                    4,
                );
            }
        }
        assert_eq!(
            CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()),
            0
        );
    }
    let frame = VideoFrame::pixel_buffer(buffer).unwrap();
    let weak = Arc::downgrade(&frame);
    let unit = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    let mut surface = PaintSurface {
        order: 0,
        bounds: Bounds::new(
            point(ScaledPixels(16.), ScaledPixels(16.)),
            size(ScaledPixels(64.), ScaledPixels(128.)),
        ),
        content_mask: ContentMask {
            bounds: Bounds::new(
                point(ScaledPixels(0.), ScaledPixels(48.)),
                size(ScaledPixels(128.), ScaledPixels(112.)),
            ),
        },
        image_buffer: frame.image().clone(),
        fast_video: Some(VideoSurface {
            frame: frame.clone(),
            geometry: VideoGeometry {
                corners: [[1., 0.], [1., 1.], [0., 1.], [0., 0.]],
                polygon: unit,
                aspect: 2.,
            },
            opacity: 1.,
        }),
    };
    let mut renderer = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
    let mut scene = Scene::default();
    scene.insert_primitive(surface.clone());
    scene.finish();
    let image = renderer
        .render_scene_to_image(&scene, size(DevicePixels(128), DevicePixels(160)))
        .unwrap();
    assert_eq!(image.get_pixel(32, 64).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(64, 64).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(32, 112).0, [255, 255, 0, 255]);
    assert_eq!(image.get_pixel(64, 112).0, [0, 255, 0, 255]);
    assert_eq!(image.get_pixel(32, 32), image.get_pixel(0, 0));
    // A file's orientation metadata must produce the same pixels as an OFD
    // appearance rotation, including its display aspect and viewport clip.
    let mut oriented_frame = VideoFrame::pixel_buffer(frame.image().clone()).unwrap();
    assert!(
        Arc::get_mut(&mut oriented_frame)
            .unwrap()
            .set_display_transform([0., -1., 1., 0., 0., 1.], 0.5)
    );
    let mut oriented_surface = surface.clone();
    let oriented_video = oriented_surface.fast_video.as_mut().unwrap();
    oriented_video.frame = oriented_frame;
    oriented_video.geometry.corners = unit;
    oriented_video.geometry.aspect = 0.5;
    let mut oriented_scene = Scene::default();
    oriented_scene.insert_primitive(oriented_surface);
    oriented_scene.finish();
    let oriented = renderer
        .render_scene_to_image(&oriented_scene, size(DevicePixels(128), DevicePixels(160)))
        .unwrap();
    assert_eq!(image, oriented);
    drop(oriented_scene);
    surface.fast_video.as_mut().unwrap().geometry.polygon =
        [[0., 0.], [1., 0.], [0.5, 1.], [0., 1.]];
    surface.bounds.origin.y = ScaledPixels(-16.);
    let mut moved = Scene::default();
    moved.insert_primitive(surface.clone());
    moved.finish();
    let image = renderer
        .render_scene_to_image(&moved, size(DevicePixels(128), DevicePixels(160)))
        .unwrap();
    assert_eq!(image.get_pixel(64, 96), image.get_pixel(0, 0));
    assert_eq!(image.get_pixel(32, 96).0, [255, 255, 0, 255]);
    drop((frame, surface, scene, moved));
    assert_eq!(
        weak.strong_count(),
        0,
        "completed GPU submissions must release frames"
    );
}
