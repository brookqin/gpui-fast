//! Bounded CPU-frame upload fallback; geometry and clipping run on the GPU.
use crate::fast::frame::FrameTarget;
use gpui::{PaintSurface, Scene};
use std::{
    cell::RefCell,
    sync::{Arc, Weak},
};
use wgpu::util::DeviceExt;
#[derive(Default)]
pub(crate) struct VideoRenderer(RefCell<Cache>);
#[derive(Default)]
struct Cache {
    pipeline: Option<Pipeline>,
    textures: Vec<(u64, u32, u32, wgpu::Texture, Weak<gpui::VideoFrame>)>,
}
struct Pipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}
impl Pipeline {
    fn new(target: &FrameTarget) -> Self {
        let device = target.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("dynamic_surface_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(160),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let source = include_str!("video.wgsl")
            .replace("VIDEO_TEXTURE", "texture_2d<f32>")
            .replace(
                "VIDEO_SAMPLE",
                "textureSampleLevel(video_texture, video_sampler, uv, 0.0)",
            );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dynamic_surface_shader"),
            source: crate::fast::rounded_clip::shader_source(&source),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dynamic_surface_pipeline_layout"),
            bind_group_layouts: &[
                Some(&target.bind_group_layouts.globals),
                Some(&layout),
                None,
                Some(&target.bind_group_layouts.fast_rounded_clip.layout),
            ],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("native_video"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_surface"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_surface"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target.format,
                    blend: Some(if target.premultiplied_alpha {
                        wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
                    } else {
                        wgpu::BlendState::ALPHA_BLENDING
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { layout, pipeline }
    }
}

impl VideoRenderer {
    pub(crate) fn begin_frame(&self, scene: &Scene) {
        if !scene.surfaces.iter().any(|s| s.fast_video.is_some()) {
            self.0.borrow_mut().textures.clear();
        }
    }
    pub(crate) fn draw(
        &self,
        target: &FrameTarget,
        pass: &mut wgpu::RenderPass<'_>,
        globals: &wgpu::BindGroup,
        surface: &PaintSurface,
    ) {
        let Some(video) = &surface.fast_video else {
            return;
        };
        let f = &video.frame;
        let mut cache = self.0.borrow_mut();
        if cache.pipeline.is_none() {
            cache.pipeline = Some(Pipeline::new(target));
        }
        let slot = if let Some(slot) = cache.textures.iter().position(|t| t.0 == f.id()) {
            slot
        } else if let Some(slot) = cache
            .textures
            .iter()
            .position(|t| t.4.strong_count() == 0 && (t.1, t.2) == (f.width(), f.height()))
        {
            cache.textures[slot].0 = 0;
            cache.textures[slot].4 = Arc::downgrade(f);
            slot
        } else {
            // Do not overwrite a texture belonging to another frame in this
            // scene: queue writes precede the render submission in wgpu.
            if cache.textures.len() == 8 {
                cache.textures.remove(0);
            }
            let texture = target.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("native_video_frame"),
                size: wgpu::Extent3d {
                    width: f.width(),
                    height: f.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            cache
                .textures
                .push((0, f.width(), f.height(), texture, Arc::downgrade(f)));
            cache.textures.len() - 1
        };
        let entry = &mut cache.textures[slot];
        if entry.0 != f.id() {
            target.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &entry.3,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                f.pixels(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(f.width() * 4),
                    rows_per_image: Some(f.height()),
                },
                wgpu::Extent3d {
                    width: f.width(),
                    height: f.height(),
                    depth_or_array_layers: 1,
                },
            );
            entry.0 = f.id();
        }
        let view = entry.3.create_view(&Default::default());
        let uniform = target
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("native_video_geometry"),
                contents: bytemuck::cast_slice(&video.uniforms(surface)),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let pipeline = cache.pipeline.as_ref().unwrap();
        let group = target.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native_video_frame"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(target.atlas_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, &group, &[]);
        pass.draw(0..4, 0..1);
    }
}
