// Modified by Longbridge for gpui-fast.
//! Browser source frames sampled in the ordered GPUI render pass.
use crate::fast::browser_surface::{BrowserSurfaceMode, Source, lookup};
use crate::fast::frame::FrameTarget;
use gpui::{DynamicSurfaceId, PaintSurface};
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use wgpu::util::DeviceExt;

#[wasm_bindgen(
    inline_js = "export function copyVideoFrame(queue, video, texture, width, height) { queue.copyExternalImageToTexture({source:video}, {texture, colorSpace:'srgb', premultipliedAlpha:false}, [width,height]); }"
)]
extern "C" {
    #[wasm_bindgen(catch, js_name = copyVideoFrame)]
    fn copy_video_frame(
        queue: &JsValue,
        video: &web_sys::HtmlVideoElement,
        texture: &JsValue,
        width: u32,
        height: u32,
    ) -> Result<(), JsValue>;
}

#[derive(Default)]
pub(crate) struct SurfaceCache(RefCell<Cache>);
#[derive(Default)]
struct Cache {
    pipelines: Option<Pipelines>,
    copies: HashMap<DynamicSurfaceId, CopyTexture>,
    watch: Option<crate::fast::device_watch::DeviceWatch>,
}
struct CopyTexture {
    texture: wgpu::Texture,
    width: u32,
    height: u32,
    serial: u64,
}
struct Pipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}
struct Pipelines {
    copy: Pipeline,
    external: Pipeline,
    sampler: wgpu::Sampler,
}

impl Pipeline {
    fn new(target: &FrameTarget, external: bool) -> Self {
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
                        min_binding_size: wgpu::BufferSize::new(128),
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
                    ty: if external {
                        wgpu::BindingType::ExternalTexture
                    } else {
                        wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        }
                    },
                    count: None,
                },
            ],
        });
        let source = include_str!("dynamic_surface.wgsl")
            .replace(
                "VIDEO_TEXTURE",
                if external {
                    "texture_external"
                } else {
                    "texture_2d<f32>"
                },
            )
            .replace(
                "VIDEO_SAMPLE",
                if external {
                    "textureSampleBaseClampToEdge(video_texture, video_sampler, uv)"
                } else {
                    "textureSampleLevel(video_texture, video_sampler, uv, 0.0)"
                },
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
            label: Some(if external {
                "dynamic_surface_external"
            } else {
                "dynamic_surface_copy"
            }),
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

impl SurfaceCache {
    /// Cache resources belong to this renderer's device. No external texture or
    /// its bind group is retained between submissions.
    pub(crate) fn begin_frame(&self) {
        self.0
            .borrow_mut()
            .copies
            .retain(|id, _| lookup(*id).is_some());
    }

    pub(crate) fn draw(
        &self,
        target: &FrameTarget,
        pass: &mut wgpu::RenderPass<'_>,
        globals: &wgpu::BindGroup,
        surface: &PaintSurface,
    ) {
        let Some(source) = lookup(surface.fast_dynamic_source) else {
            return;
        };
        if source.stats.borrow().device_lost {
            return;
        }
        if source.video.ready_state() < 2 {
            return;
        }
        source.stats.borrow_mut().unavailable = true;
        if target.device.as_webgpu().is_none() {
            source
                .failed("Dynamic video surfaces require WebGPU; select the host fallback on WebGL");
            return;
        }
        let mut cache = self.0.borrow_mut();
        cache
            .watch
            .get_or_insert_with(|| crate::fast::device_watch::DeviceWatch::new(target.device))
            .track(source.id);
        if cache.pipelines.is_none() {
            cache.pipelines = Some(Pipelines {
                copy: Pipeline::new(target, false),
                external: Pipeline::new(target, true),
                sampler: target.device.create_sampler(&wgpu::SamplerDescriptor {
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    ..Default::default()
                }),
            });
        }
        let external = if source.mode != BrowserSurfaceMode::Copy {
            match target.device.try_import_external_texture(
                &wgpu::webgpu::ExternalTextureSource::HtmlVideoElement(source.video.clone()),
            ) {
                Ok(texture) => {
                    source.stats.borrow_mut().imports += 1;
                    Some(texture)
                }
                Err(error) => {
                    source.failed(&format!("External frame import failed: {error:?}"));
                    if source.mode == BrowserSurfaceMode::External {
                        return;
                    }
                    None
                }
            }
        } else {
            None
        };
        let view = if external.is_none() {
            match cache.copy(target, &source) {
                Ok(view) => Some(view),
                Err(error) => {
                    source.failed(&format!("Video frame copy failed: {error:?}"));
                    return;
                }
            }
        } else {
            None
        };
        let pipelines = cache.pipelines.as_ref().unwrap();
        let pipeline = if external.is_some() {
            &pipelines.external
        } else {
            &pipelines.copy
        };
        let fit = match surface.fast_dynamic_fit {
            gpui::DynamicSurfaceFit::Fill => gpui::ObjectFit::Fill,
            gpui::DynamicSurfaceFit::Contain => gpui::ObjectFit::Contain,
            gpui::DynamicSurfaceFit::Cover => gpui::ObjectFit::Cover,
        };
        let geometry = source.geometry.get();
        let b = fit
            .get_bounds(
                surface.bounds.map(|p| gpui::px(p.0)),
                gpui::size(
                    source.video.video_width().into(),
                    source.video.video_height().into(),
                ),
            )
            .map(|p| gpui::ScaledPixels(f32::from(p)));
        let b = if geometry.is_some() {
            surface.bounds
        } else {
            b
        };
        let m = surface.content_mask.bounds.intersect(&surface.bounds);
        let mut data = vec![
            b.origin.x.0,
            b.origin.y.0,
            b.size.width.0,
            b.size.height.0,
            m.origin.x.0,
            m.origin.y.0,
            m.right().0,
            m.bottom().0,
            surface.fast_dynamic_opacity,
            0.,
            0.,
            0.,
        ];
        let unit = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
        let corners = geometry.map_or(unit, |g| g.corners);
        let clip = geometry.map_or(unit, |g| g.clip);
        data[9] = if geometry.is_some() { 1. } else { 0. };
        for point in corners.into_iter().chain(clip) {
            data.extend(point);
        }
        let frame_aspect =
            source.video.video_width() as f32 / source.video.video_height().max(1) as f32;
        let ratio = geometry.map_or(1., |g| g.aspect_ratio) / frame_aspect;
        let fit = match surface.fast_dynamic_fit {
            gpui::DynamicSurfaceFit::Fill => [1., 1.],
            gpui::DynamicSurfaceFit::Contain => [(1. / ratio).min(1.), ratio.min(1.)],
            gpui::DynamicSurfaceFit::Cover => [(1. / ratio).max(1.), ratio.max(1.)],
        };
        data.extend([fit[0], fit[1], 0., 0.]);
        let uniform = target
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("dynamic_surface_geometry"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let resource = match (&external, &view) {
            (Some(texture), _) => wgpu::BindingResource::ExternalTexture(texture),
            (_, Some(view)) => wgpu::BindingResource::TextureView(view),
            _ => return,
        };
        let group = target.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dynamic_surface_frame"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&pipelines.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource,
                },
            ],
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, &group, &[]);
        pass.draw(0..4, 0..1);
        let mut stats = source.stats.borrow_mut();
        stats.draws += 1;
        stats.unavailable = false;
        stats.actual_mode = Some(if external.is_some() {
            BrowserSurfaceMode::External
        } else {
            BrowserSurfaceMode::Copy
        });
    }
}

impl Cache {
    fn copy(
        &mut self,
        target: &FrameTarget,
        source: &Source,
    ) -> Result<wgpu::TextureView, JsValue> {
        let width = source.video.video_width();
        let height = source.video.video_height();
        if width == 0 || height == 0 {
            return Err(JsValue::from_str("Video has no decoded dimensions"));
        }
        let entry = self
            .copies
            .entry(source.id)
            .or_insert_with(|| copy_texture(target.device, width, height));
        if entry.width != width || entry.height != height {
            *entry = copy_texture(target.device, width, height);
        }
        if entry.serial != source.serial.get() {
            copy_video_frame(
                target.queue.as_webgpu().unwrap().as_ref(),
                &source.video,
                entry.texture.as_webgpu().unwrap().as_ref(),
                width,
                height,
            )?;
            entry.serial = source.serial.get();
            source.stats.borrow_mut().copies += 1;
        }
        Ok(entry.texture.create_view(&Default::default()))
    }
}

fn copy_texture(device: &wgpu::Device, width: u32, height: u32) -> CopyTexture {
    CopyTexture {
        width,
        height,
        serial: 0,
        texture: device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dynamic_surface_copy_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        }),
    }
}
