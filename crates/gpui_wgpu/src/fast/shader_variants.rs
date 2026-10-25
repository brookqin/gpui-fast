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
