struct GlobalParams { viewport_size: vec2<f32>, premultiplied_alpha: u32, pad: u32 }
struct SurfaceParams { bounds: vec4<f32>, mask: vec4<f32>, opacity: vec4<f32> }
@group(0) @binding(0) var<uniform> globals: GlobalParams;
@group(1) @binding(0) var<uniform> surface: SurfaceParams;
@group(1) @binding(1) var video_sampler: sampler;
@group(1) @binding(2) var video_texture: VIDEO_TEXTURE;
struct VertexOut { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_surface(@builtin(vertex_index) index: u32) -> VertexOut {
    let uv = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let position = surface.bounds.xy + uv * surface.bounds.zw;
    var out: VertexOut;
    out.position = vec4<f32>(position / globals.viewport_size * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}
@fragment fn fs_surface(input: VertexOut) -> @location(0) vec4<f32> {
    if (any(input.position.xy < surface.mask.xy) || any(input.position.xy >= surface.mask.zw)) { discard; }
    let color = VIDEO_SAMPLE;
    return vec4<f32>(color.rgb * select(1.0, surface.opacity.x, globals.premultiplied_alpha != 0u), color.a * surface.opacity.x);
}
