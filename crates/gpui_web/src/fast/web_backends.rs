use crate::window::WebWindow;
use gpui_wgpu::{PreparedWebGraphics, WebBackendPreference, WgpuContext, wgpu};

pub(crate) async fn initialize(
    window: &web_sys::Window,
    preference: WebBackendPreference,
) -> anyhow::Result<(
    web_sys::HtmlCanvasElement,
    WgpuContext,
    wgpu::Surface<'static>,
)> {
    let candidates: &[WebBackendPreference] = match preference {
        WebBackendPreference::Auto => &[
            #[cfg(feature = "webgpu")]
            WebBackendPreference::WebGpu,
            #[cfg(feature = "webgl")]
            WebBackendPreference::WebGl,
        ],
        WebBackendPreference::WebGpu => &[WebBackendPreference::WebGpu],
        WebBackendPreference::WebGl => &[WebBackendPreference::WebGl],
    };
    let mut failures = Vec::new();
    for &backend in candidates {
        let canvas = WebWindow::prepare_canvas(window)?;
        match WgpuContext::new_web(&canvas, backend).await {
            Ok(PreparedWebGraphics { context, surface }) => return Ok((canvas, context, surface)),
            Err(error) => {
                let element: &web_sys::Element = canvas.as_ref();
                element.remove();
                log::warn!("Browser {backend:?} initialization failed: {error:#}");
                failures.push(format!("{backend:?}: {error:#}"));
            }
        }
    }
    anyhow::bail!(
        "No requested browser graphics backend could be initialized: {}",
        failures.join("; ")
    )
}
