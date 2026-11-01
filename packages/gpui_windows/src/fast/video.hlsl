// Modified by Longbridge for gpui-fast.
// GlobalParams starts with gamma_ratios; the viewport is at byte offset 16.
cbuffer Globals: register(b0) { float4 gamma_ratios; float2 viewport; };
cbuffer Video: register(b2) { float4 bounds, mask, opacity, corners[2], polygon[2], fit, source[2]; };
Texture2D<float4> image: register(t0);
SamplerState sampling: register(s0);
struct Vertex { float4 position: SV_Position; float2 uv: TEXCOORD0; };
Vertex vs_video(uint index: SV_VertexID) {
    Vertex o; o.uv=float2(index&1u,index>>1u);
    o.position=float4((bounds.xy+o.uv*bounds.zw)/viewport*float2(2,-2)+float2(-1,1),0,1);return o;
}
float4 ps_video(Vertex i): SV_Target {
    if(any(i.position.xy<mask.xy)||any(i.position.xy>=mask.zw)) discard;
    float2 origin=corners[0].xy,u=corners[0].zw-origin,v=corners[1].zw-origin,p=i.uv-origin;
    float det=u.x*v.y-u.y*v.x;
    if(abs(det)<0.000001) discard;
    float2 uv=float2(p.x*v.y-p.y*v.x,u.x*p.y-u.y*p.x)/det;
    if(any(uv<0)||any(uv>1)) discard;
    float2 points[4]={polygon[0].xy,polygon[0].zw,polygon[1].xy,polygon[1].zw};
    bool pos=false,neg=false;
    [unroll] for(uint n=0;n<4;n++) {float2 e=points[(n+1)%4]-points[n],r=i.uv-points[n];float c=e.x*r.y-e.y*r.x;pos=pos||c>0.000001;neg=neg||c< -0.000001;}
    if(pos&&neg) discard;
    uv=(uv-0.5)/fit.xy+0.5;
    bool letterbox=any(uv<0)||any(uv>1);
    uv=source[0].xy*uv.x+source[0].zw*uv.y+source[1].xy;
    float4 color=letterbox?float4(0,0,0,1):image.SampleLevel(sampling,uv,0);
    return color*opacity.x;
}
