# Browser backend capabilities

`gpui_web`, `gpui_platform` and `gpui_wgpu` expose additive `webgpu` and
`webgl` features. Defaults keep both backends and the existing threading choice.
Consumers that disable defaults must select at least one browser backend.
`Auto` tries only compiled backends; an explicit unavailable backend is an error.
Native target dependencies remain independent of the browser feature selection.

Release crates are generated from this source with
`bun script/prepare-release.ts --tag v0.1.10`. The published package mapping and
wgpu API adaptation are part of that existing release procedure.

Validation: independent WASM application compiled in WebGPU-only, WebGL-only,
and dual configurations. Both individual backends painted an actual GPUI canvas
in Chromium 154 on Linux software graphics. This is correctness evidence, not a
hardware performance comparison. Native and other-browser validation are tracked
separately and are not implied by these checks.

## Image and SVG capabilities

Core defaults preserve `image-formats`, `svg-text` and `svg-raster`. Disabling
these retains PNG and path-based SVG icons, while ordinary bitmap frame buffers
are unaffected. Unsupported image formats return decoder errors. The compact SVG
profile rejects text and embedded images before parsing rather than returning a
partially rendered icon; embedded images require both SVG capabilities because
nested SVG may itself contain text. Font resolution and its original tests live
in `fast::svg_fonts`; full builds retain system and bundled font resolution.

`parse_svg` and `render_single_frame` now return `anyhow::Result` so capability
errors can be distinguished by their message from parser errors. Code explicitly
annotating their old `usvg::Error` return type must update that annotation.
`render_parsed` still returns `usvg::Error`. Default rendering behavior is kept.
