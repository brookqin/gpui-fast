use crate::WebBackendPreference;

#[cfg(not(any(feature = "webgpu", feature = "webgl")))]
compile_error!("The browser renderer requires the webgpu or webgl feature");

pub(crate) fn select(preference: WebBackendPreference) -> anyhow::Result<wgpu::Backends> {
    let mut available = wgpu::Backends::empty();
    if cfg!(feature = "webgpu") {
        available |= wgpu::Backends::BROWSER_WEBGPU;
    }
    if cfg!(feature = "webgl") {
        available |= wgpu::Backends::GL;
    }
    let selected = match preference {
        WebBackendPreference::Auto => available,
        WebBackendPreference::WebGpu => wgpu::Backends::BROWSER_WEBGPU,
        WebBackendPreference::WebGl => wgpu::Backends::GL,
    };
    anyhow::ensure!(
        available.contains(selected),
        "Requested {preference:?} backend was not compiled; enable its Cargo feature"
    );
    Ok(selected)
}
