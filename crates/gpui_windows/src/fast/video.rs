//! Native video composed with document primitives; no per-pixel CPU transform.
use anyhow::{Context, Result};
use gpui::{PaintSurface, Scene};
use windows::{
    Win32::Graphics::{
        Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP, Fxc::D3DCompile, ID3DBlob},
        Direct3D11::{
            D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BLEND_DESC,
            D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BUFFER_DESC,
            D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_RENDER_TARGET_BLEND_DESC, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_DEFAULT, ID3D11BlendState, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext,
            ID3D11PixelShader, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
        },
        Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
    },
    core::PCSTR,
};
#[derive(Default)]
pub(crate) struct VideoRenderer {
    pipeline: Option<Pipeline>,
    frame: Option<(u64, u32, u32, ID3D11Texture2D, ID3D11ShaderResourceView)>,
}
struct Pipeline {
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    uniform: ID3D11Buffer,
    blend: ID3D11BlendState,
}
fn compile(entry: &std::ffi::CStr, target: &std::ffi::CStr) -> Result<ID3DBlob> {
    let source = include_str!("video.hlsl");
    let mut blob = None;
    let mut errors = None;
    // SAFETY: all source/entry bytes and output slots live for the synchronous compile.
    unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr().cast()),
            PCSTR(target.as_ptr().cast()),
            0,
            0,
            &mut blob,
            Some(&mut errors),
        )?;
    }
    blob.context("empty video shader")
}
impl Pipeline {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let v = compile(c"vs_video", c"vs_5_0")?;
        let p = compile(c"ps_video", c"ps_5_0")?;
        let (mut vertex, mut pixel, mut uniform, mut blend) = (None, None, None, None);
        unsafe {
            device.CreateVertexShader(
                std::slice::from_raw_parts(v.GetBufferPointer().cast(), v.GetBufferSize()),
                None,
                Some(&mut vertex),
            )?;
            device.CreatePixelShader(
                std::slice::from_raw_parts(p.GetBufferPointer().cast(), p.GetBufferSize()),
                None,
                Some(&mut pixel),
            )?;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: 160,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut uniform),
            )?;
            let mut desc = D3D11_BLEND_DESC::default();
            desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_ONE,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            device.CreateBlendState(&desc, Some(&mut blend))?;
        }
        Ok(Self {
            vertex: vertex.context("video vertex")?,
            pixel: pixel.context("video pixel")?,
            uniform: uniform.context("video uniform")?,
            blend: blend.context("video blend")?,
        })
    }
}
impl VideoRenderer {
    pub(crate) fn begin_frame(&mut self, scene: &Scene) {
        if !scene.surfaces.iter().any(|s| s.fast_video.is_some()) {
            self.frame = None;
        }
    }
    pub(crate) fn draw(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        surface: &PaintSurface,
    ) -> Result<()> {
        let Some(video) = &surface.fast_video else {
            return Ok(());
        };
        let frame = &video.frame;
        if self.pipeline.is_none() {
            self.pipeline = Some(Pipeline::new(device)?);
        }
        if self.frame.as_ref().map(|f| (f.1, f.2)) != Some((frame.width(), frame.height())) {
            let (mut texture, mut view) = (None, None);
            unsafe {
                device.CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: frame.width(),
                        Height: frame.height(),
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                        ..Default::default()
                    },
                    None,
                    Some(&mut texture),
                )?;
                device.CreateShaderResourceView(
                    texture.as_ref().context("video texture")?,
                    None,
                    Some(&mut view),
                )?;
            }
            self.frame = Some((
                0,
                frame.width(),
                frame.height(),
                texture.context("video texture")?,
                view.context("video view")?,
            ));
        }
        let cached = self.frame.as_mut().unwrap();
        let p = self.pipeline.as_ref().unwrap();
        // D3D11 orders updates/draws on this context, including draws from an earlier frame.
        unsafe {
            if cached.0 != frame.id() {
                context.UpdateSubresource(
                    &cached.3,
                    0,
                    None,
                    frame.pixels().as_ptr().cast(),
                    frame.width() * 4,
                    0,
                );
                cached.0 = frame.id();
            }
            context.UpdateSubresource(
                &p.uniform,
                0,
                None,
                video.uniforms(surface).as_ptr().cast(),
                0,
                0,
            );
            context.VSSetConstantBuffers(2, Some(&[Some(p.uniform.clone())]));
            context.PSSetConstantBuffers(2, Some(&[Some(p.uniform.clone())]));
            context.VSSetShader(&p.vertex, None);
            context.PSSetShader(&p.pixel, None);
            context.PSSetShaderResources(0, Some(&[Some(cached.4.clone())]));
            context.OMSetBlendState(&p.blend, None, u32::MAX);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            context.Draw(4, 0);
        }
        Ok(())
    }
}

/// Drop all objects belonging to the removed D3D device before recovery.
pub(crate) fn release(renderer: &mut crate::directx_renderer::DirectXRenderer) {
    renderer.fast_frame.video = VideoRenderer::default();
}
