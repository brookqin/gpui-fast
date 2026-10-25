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
