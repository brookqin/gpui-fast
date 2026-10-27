struct GlobalParams { viewport_size: vec2<f32>, premultiplied_alpha: u32, pad: u32 }
struct SurfaceParams { bounds: vec4<f32>, mask: vec4<f32>, opacity: vec4<f32>, corners: array<vec4<f32>, 2>, clip: array<vec4<f32>, 2>, fit: vec4<f32> }
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
    var uv = input.uv;
    var letterbox = false;
    if (surface.opacity.y > 0.5) {
        let origin = surface.corners[0].xy;
        let u = surface.corners[0].zw - origin;
        let v = surface.corners[1].zw - origin;
        let p = input.uv - origin;
        let det = u.x * v.y - u.y * v.x;
        if (abs(det) < 0.000001) { discard; }
        uv = vec2<f32>(p.x * v.y - p.y * v.x, u.x * p.y - u.y * p.x) / det;
        if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) { discard; }
        let points = array<vec2<f32>, 4>(surface.clip[0].xy, surface.clip[0].zw, surface.clip[1].xy, surface.clip[1].zw);
        var positive = false;
        var negative = false;
        for (var i = 0u; i < 4u; i++) {
            let edge = points[(i + 1u) % 4u] - points[i];
            let rel = input.uv - points[i];
            let cross = edge.x * rel.y - edge.y * rel.x;
            positive = positive || cross > 0.000001;
            negative = negative || cross < -0.000001;
        }
        if (positive && negative) { discard; }
        uv = (uv - vec2<f32>(0.5)) / surface.fit.xy + vec2<f32>(0.5);
        letterbox = any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0));
    }
    let sampled = VIDEO_SAMPLE;
    let color = select(sampled, vec4<f32>(0.0, 0.0, 0.0, 1.0), letterbox);
    return vec4<f32>(color.rgb * select(1.0, surface.opacity.x, globals.premultiplied_alpha != 0u), color.a * surface.opacity.x);
}
