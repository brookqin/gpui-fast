//! CoreVideo -> Metal sampling without CPU extraction or independent native views.
use crate::metal_renderer::InstanceBindings;
use crate::metal_renderer::MetalRenderer;
use foreign_types::ForeignTypeRef;
use gpui::{DevicePixels, PaintSurface, Size, VideoFrame};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture};
use std::{cell::RefCell, ptr, sync::Arc};

#[derive(Default)]
pub(crate) struct VideoRenderer {
    pipeline: Option<metal::RenderPipelineState>,
    // The last frame is reused when only scrolling/geometry changed.
    imported: Option<(u64, CFRetained<CVMetalTexture>)>,
    // Released by completion handler, never by an arbitrary frame-count delay.
    submitted: Vec<(Arc<VideoFrame>, CFRetained<CVMetalTexture>)>,
}
impl VideoRenderer {
    pub(crate) fn begin_frame(&mut self, scene: &gpui::Scene) {
        self.submitted.clear();
        if !scene.surfaces.iter().any(|s| s.fast_video.is_some()) {
            self.imported = None;
        }
    }
    pub(crate) fn submitted(&mut self, command: &metal::CommandBufferRef) {
        if self.submitted.is_empty() {
            return;
        }
        let frames = RefCell::new(std::mem::take(&mut self.submitted));
        let block = block2::RcBlock::new(move |_: ptr::NonNull<objc2::runtime::AnyObject>| {
            frames.borrow_mut().clear();
        });
        // SAFETY: Metal copies the completion block. The retained frame and CV texture
        // remain alive until the command completes, including player/window teardown.
        unsafe {
            command.add_completed_handler(&*block2::RcBlock::as_ptr(&block).cast());
        }
    }
}

pub(crate) fn draw(
    renderer: &mut MetalRenderer,
    surfaces: &[PaintSurface],
    first: usize,
    bindings: &InstanceBindings,
    viewport: Size<DevicePixels>,
    encoder: &metal::RenderCommandEncoderRef,
) {
    for (index, surface) in surfaces.iter().enumerate() {
        let Some(video) = &surface.fast_video else {
            renderer.fast_draw_legacy_surfaces(
                std::slice::from_ref(surface),
                first + index,
                bindings,
                viewport,
                encoder,
            );
            continue;
        };
        let state = &mut renderer.fast_video;
        if state.pipeline.is_none() {
            let pipeline = (|| {
                let library = renderer.device.new_library_with_source(
                    include_str!("video.metal"),
                    &metal::CompileOptions::new(),
                )?;
                let descriptor = metal::RenderPipelineDescriptor::new();
                let vertex = library.get_function("video_vertex", None)?;
                let fragment = library.get_function("video_fragment", None)?;
                descriptor.set_vertex_function(Some(&vertex));
                descriptor.set_fragment_function(Some(&fragment));
                let color = descriptor.color_attachments().object_at(0).unwrap();
                color.set_pixel_format(metal::MTLPixelFormat::BGRA8Unorm);
                color.set_blending_enabled(true);
                color.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
                color.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
                color.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
                color
                    .set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
                renderer.device.new_render_pipeline_state(&descriptor)
            })();
            match pipeline {
                Ok(p) => state.pipeline = Some(p),
                Err(e) => {
                    log::error!("video pipeline: {e}");
                    continue;
                }
            }
        }
        if state.imported.as_ref().map(|x| x.0) != Some(video.frame.id()) {
            let mut raw = ptr::null_mut();
            // SAFETY: frame is retained by the scene; cache is tied to this renderer's device.
            let status = unsafe {
                CVMetalTextureCache::create_texture_from_image(
                    None,
                    &renderer.core_video_texture_cache,
                    video.frame.image(),
                    None,
                    objc2_metal::MTLPixelFormat::BGRA8Unorm,
                    video.frame.width() as usize,
                    video.frame.height() as usize,
                    0,
                    ptr::NonNull::from(&mut raw),
                )
            };
            let Some(raw) = ptr::NonNull::new(raw) else {
                log::error!("video texture import failed: {status}");
                continue;
            };
            state.imported = Some((video.frame.id(), unsafe { CFRetained::from_raw(raw) }));
        }
        let imported = &state.imported.as_ref().unwrap().1;
        let Some(texture) = CVMetalTextureGetTexture(imported) else {
            continue;
        };
        let uniforms = video.uniforms(surface);
        let viewport = [
            i32::from(viewport.width) as f32,
            i32::from(viewport.height) as f32,
        ];
        encoder.set_render_pipeline_state(state.pipeline.as_ref().unwrap());
        encoder.set_vertex_bytes(0, 160, uniforms.as_ptr().cast());
        encoder.set_fragment_bytes(0, 160, uniforms.as_ptr().cast());
        encoder.set_vertex_bytes(1, 8, viewport.as_ptr().cast());
        encoder.set_fragment_texture(
            0,
            Some(unsafe {
                metal::TextureRef::from_ptr(objc2::rc::Retained::as_ptr(&texture) as *mut _)
            }),
        );
        encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, 0, 4);
        state
            .submitted
            .push((video.frame.clone(), imported.clone()));
    }
    if !surfaces.iter().any(|s| s.fast_video.is_some()) {
        renderer.fast_video.imported = None;
    }
}

impl MetalRenderer {
    pub(crate) fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        first_surface: usize,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        crate::fast::video::draw(
            self,
            surfaces,
            first_surface,
            instance_bindings,
            viewport_size,
            command_encoder,
        );
    }
}
