// Canonical sprite batch shaders for SDL3 GPU (HLSL).
// Offline backends: SPIR-V (Vulkan), DXIL (D3D12), metallib (Metal).
// Entry points: VSMain (vertex), PSMain (fragment).
//
// Resource contract (reflection manifest must match):
//   - 1 uniform buffer  (space1, b0): frame view size
//   - 1 sampled texture (space2, t0): atlas
//   - 1 sampler         (space2, s0): atlas sampler
// Vertex TEXCOORD layout matches crates/mmd-engine instance packing (T7+).

cbuffer FrameUniforms : register(b0, space1)
{
    float2 view_size; // pixels, e.g. 1920x1080
    float2 _pad0;
};

struct VSInput
{
    float2 corner : TEXCOORD0;        // unit quad in [-0.5, 0.5]
    float2 uv : TEXCOORD1;            // unit quad UV in [0, 1]
    float2 instance_pos : TEXCOORD2;  // top-left pixel position
    float2 instance_size : TEXCOORD3; // display-quad size in pixels
    float4 uv_rect : TEXCOORD4;       // atlas rect (u0,v0,u1,v1)
    float4 tint : TEXCOORD5;          // premultiplied RGBA tint
};

struct VSOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
    float4 tint : TEXCOORD1;
};

VSOutput VSMain(VSInput input)
{
    VSOutput output;

    float2 world = input.instance_pos + (input.corner + float2(0.5, 0.5)) * input.instance_size;
    float2 ndc = (world / view_size) * 2.0 - 1.0;
    ndc.y = -ndc.y; // y-down pixel space → y-up clip space

    output.position = float4(ndc, 0.0, 1.0);
    output.uv = lerp(input.uv_rect.xy, input.uv_rect.zw, input.uv);
    output.tint = input.tint;
    return output;
}

Texture2D<float4> SpriteAtlas : register(t0, space2);
SamplerState AtlasSampler : register(s0, space2);

float4 PSMain(VSOutput input) : SV_Target0
{
    float4 texel = SpriteAtlas.Sample(AtlasSampler, input.uv);
    // Atlas + tint already premultiplied; multiply keeps blend contract.
    return texel * input.tint;
}
