#include <metal_stdlib>
using namespace metal;
struct VideoParams { float4 bounds, mask, opacity; float4 corners[2], clip[2], fit, source[2]; };
struct VideoVertex { float4 position [[position]]; float2 uv; };
vertex VideoVertex video_vertex(uint index [[vertex_id]], constant VideoParams &s [[buffer(0)]], constant float2 &viewport [[buffer(1)]]) {
    float2 uv = float2(index & 1u, index >> 1u);
    return { float4((s.bounds.xy + uv * s.bounds.zw) / viewport * float2(2,-2) + float2(-1,1),0,1), uv };
}
fragment float4 video_fragment(VideoVertex in [[stage_in]], constant VideoParams &s [[buffer(0)]], texture2d<float> video [[texture(0)]]) {
    if (any(in.position.xy < s.mask.xy) || any(in.position.xy >= s.mask.zw)) discard_fragment();
    float2 origin=s.corners[0].xy, u=s.corners[0].zw-origin, v=s.corners[1].zw-origin, p=in.uv-origin;
    float det=u.x*v.y-u.y*v.x;
    if(abs(det)<0.000001) discard_fragment();
    float2 uv=float2(p.x*v.y-p.y*v.x,u.x*p.y-u.y*p.x)/det;
    if(any(uv<0.0)||any(uv>1.0)) discard_fragment();
    float2 points[4]={s.clip[0].xy,s.clip[0].zw,s.clip[1].xy,s.clip[1].zw};
    bool pos=false,neg=false;
    for(uint i=0;i<4;i++) { float2 edge=points[(i+1)%4]-points[i], rel=in.uv-points[i]; float cross=edge.x*rel.y-edge.y*rel.x; pos|=cross>0.000001; neg|=cross< -0.000001; }
    if(pos&&neg) discard_fragment();
    uv=(uv-0.5)/s.fit.xy+0.5;
    constexpr sampler sampling(coord::normalized, address::clamp_to_edge, filter::linear);
    bool letterbox=any(uv<0.0)||any(uv>1.0);
    uv=s.source[0].xy*uv.x+s.source[0].zw*uv.y+s.source[1].xy;
    float4 color=letterbox?float4(0,0,0,1):video.sample(sampling,uv);
    return color*s.opacity.x;
}
