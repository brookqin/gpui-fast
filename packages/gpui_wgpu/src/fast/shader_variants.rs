// Modified by Longbridge for gpui-fast.
pub(crate) fn source(webgl: bool) -> &'static str {
    #[cfg(any(not(target_family = "wasm"), feature = "webgl"))]
    if webgl {
        return WEBGL_SHADERS;
    }
    #[cfg(all(target_family = "wasm", not(feature = "webgl")))]
    assert!(!webgl, "WebGL shader transport was not compiled");
    STORAGE_BUFFER_SHADERS
}

pub(crate) const STORAGE_BUFFER_SHADERS: &str = concat!(
    include_str!("../shaders.wgsl"),
    include_str!("../shaders_storage.wgsl"),
);
#[cfg(any(not(target_family = "wasm"), feature = "webgl"))]
pub(crate) const WEBGL_SHADERS: &str = concat!(
    include_str!("../shaders.wgsl"),
    include_str!("../shaders_webgl.wgsl"),
);

pub(crate) fn uses_texture_instances(backend: crate::WgpuBackend) -> bool {
    matches!(backend, crate::WgpuBackend::Gl)
        && cfg!(all(target_family = "wasm", feature = "webgl"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn affine_dynamic_surface_shaders_validate() {
        for (texture, sample, capabilities) in [
            (
                "texture_2d<f32>",
                "textureSampleLevel(video_texture, video_sampler, uv, 0.0)",
                naga::valid::Capabilities::empty(),
            ),
            (
                "texture_external",
                "textureSampleBaseClampToEdge(video_texture, video_sampler, uv)",
                naga::valid::Capabilities::TEXTURE_EXTERNAL,
            ),
        ] {
            let source = include_str!("dynamic_surface.wgsl")
                .replace("VIDEO_TEXTURE", texture)
                .replace("VIDEO_SAMPLE", sample);
            validate_wgsl(&crate::fast::rounded_clip::shader(&source), capabilities);
        }
    }

    fn validate_wgsl(source: &str, capabilities: naga::valid::Capabilities) {
        let module = naga::front::wgsl::parse_str(source).expect("shader should parse");
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities)
            .validate(&module)
            .expect("shader should validate");
    }
}
