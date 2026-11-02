// Modified by Longbridge for gpui-fast.
//! CoreVideo -> Metal sampling without CPU extraction or independent native views.
use crate::metal_renderer::InstanceBindings;
use crate::metal_renderer::MetalRenderer;
use anyhow::{Context as _, Result};
use gpui::{DevicePixels, PaintSurface, Size, VideoFrame};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture};
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBlendFactor, MTLCommandBuffer, MTLCompileOptions, MTLDevice, MTLLibrary, MTLPixelFormat,
    MTLPrimitiveType, MTLRenderCommandEncoder, MTLRenderPipelineDescriptor, MTLRenderPipelineState,
};
use std::{cell::RefCell, ptr, sync::Arc};

#[derive(Default)]
pub(crate) struct VideoRenderer {
    pipeline: Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>>,
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
    pub(crate) fn submitted(&mut self, command: &ProtocolObject<dyn MTLCommandBuffer>) {
        if self.submitted.is_empty() {
            return;
        }
        let frames = RefCell::new(std::mem::take(&mut self.submitted));
        let block = block2::RcBlock::new(
            move |_: ptr::NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                frames.borrow_mut().clear();
            },
        );
        // SAFETY: Metal copies the completion block. The retained frame and CV texture
        // remain alive until the command completes, including player/window teardown.
        unsafe {
            command.addCompletedHandler(block2::RcBlock::as_ptr(&block));
        }
    }
}

pub(crate) fn draw(
    renderer: &mut MetalRenderer,
    surfaces: &[PaintSurface],
    first: usize,
    bindings: &InstanceBindings,
    viewport: Size<DevicePixels>,
    encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    surface_textures: &mut Vec<CFRetained<CVMetalTexture>>,
) -> Result<()> {
    for (index, surface) in surfaces.iter().enumerate() {
        let Some(video) = &surface.fast_video else {
            renderer.fast_draw_legacy_surfaces(
                std::slice::from_ref(surface),
                first + index,
                bindings,
                viewport,
                encoder,
                surface_textures,
            )?;
            continue;
        };
        let state = &mut renderer.fast_video;
        if state.pipeline.is_none() {
            let pipeline = (|| -> Result<_> {
                let library = renderer.device.newLibraryWithSource_options_error(
                    &NSString::from_str(include_str!("video.metal")),
                    Some(&MTLCompileOptions::new()),
                )?;
                let descriptor = MTLRenderPipelineDescriptor::new();
                let vertex = library
                    .newFunctionWithName(&NSString::from_str("video_vertex"))
                    .context("locating video vertex function")?;
                let fragment = library
                    .newFunctionWithName(&NSString::from_str("video_fragment"))
                    .context("locating video fragment function")?;
                descriptor.setVertexFunction(Some(&vertex));
                descriptor.setFragmentFunction(Some(&fragment));
                // SAFETY: Color attachment zero exists in every Metal render pipeline.
                let color = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
                color.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
                color.setBlendingEnabled(true);
                color.setSourceRGBBlendFactor(MTLBlendFactor::One);
                color.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
                color.setSourceAlphaBlendFactor(MTLBlendFactor::One);
                color.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
                Ok(renderer
                    .device
                    .newRenderPipelineStateWithDescriptor_error(&descriptor)?)
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
        encoder.setRenderPipelineState(state.pipeline.as_ref().unwrap());
        // SAFETY: Metal copies the complete shader uniform arrays. Four generated
        // vertices sample texture zero; its CoreVideo owner lives until completion.
        unsafe {
            encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&uniforms).cast(),
                std::mem::size_of_val(&uniforms),
                0,
            );
            encoder.setFragmentBytes_length_atIndex(
                ptr::NonNull::from(&uniforms).cast(),
                std::mem::size_of_val(&uniforms),
                0,
            );
            encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&viewport).cast(),
                std::mem::size_of_val(&viewport),
                1,
            );
            encoder.setFragmentTexture_atIndex(Some(&texture), 0);
            encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::TriangleStrip, 0, 4);
        }
        state
            .submitted
            .push((video.frame.clone(), imported.clone()));
    }
    if !surfaces.iter().any(|s| s.fast_video.is_some()) {
        renderer.fast_video.imported = None;
    }
    Ok(())
}

impl MetalRenderer {
    pub(crate) fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        first_surface: usize,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        surface_textures: &mut Vec<CFRetained<CVMetalTexture>>,
    ) -> Result<()> {
        crate::fast::video::draw(
            self,
            surfaces,
            first_surface,
            instance_bindings,
            viewport_size,
            command_encoder,
            surface_textures,
        )
    }
}
