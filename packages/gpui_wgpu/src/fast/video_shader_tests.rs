// Modified by Longbridge for gpui-fast.
#[test]
fn native_video_wgsl_validates_with_source_orientation() {
    let source = include_str!("video.wgsl")
        .replace("VIDEO_TEXTURE", "texture_2d<f32>")
        .replace(
            "VIDEO_SAMPLE",
            "textureSampleLevel(video_texture, video_sampler, uv, 0.0)",
        );
    let module = wgpu::naga::front::wgsl::parse_str(&source).expect("native video WGSL parses");
    wgpu::naga::valid::Validator::new(
        wgpu::naga::valid::ValidationFlags::all(),
        wgpu::naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("native video WGSL validates");
}
