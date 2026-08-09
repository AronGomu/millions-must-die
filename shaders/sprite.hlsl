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
    // (is_ring, inner, outer). Interpolant only — the vertex *input* layout is
    // unchanged, so the reflection manifest's resource contract still reads
    // 1 uniform buffer / 1 texture / 1 sampler.
    float3 ring : TEXCOORD2;
};

// A negative `uv_rect.x` marks a hitbox-ring instance instead of an atlas
// sprite. Safe because every rect `frame_uv_rect` can emit is a ratio of
// non-negative integers, so a sprite's `u0 >= 0` always
// (`crates/mmd-engine/src/render/atlas.rs`, pinned by
// `ring_instances_keep_the_pinned_layout`). Reusing the field is what keeps
// `SpriteInstance` at its locked 48 bytes.
//
// This is the *threshold*, not the value: the writer emits
// `render::instance::RING_SENTINEL = -1.0`, and any negative selects the
// branch. The invariant is `RING_SENTINEL < MMD_RING_THRESHOLD <= 0.0` — do
// not "sync" this to -1.0, which would make the test `x < -1.0`, never true,
// and silently render every ring as an atlas sprite.
#define MMD_RING_THRESHOLD 0.0

VSOutput VSMain(VSInput input)
{
    VSOutput output;

    float2 world = input.instance_pos + (input.corner + float2(0.5, 0.5)) * input.instance_size;
    float2 ndc = (world / view_size) * 2.0 - 1.0;
    ndc.y = -ndc.y; // y-down pixel space → y-up clip space

    output.position = float4(ndc, 0.0, 1.0);

    if (input.uv_rect.x < MMD_RING_THRESHOLD)
    {
        // Ring: raw unit-quad coords, no atlas lerp. The pixel stage measures
        // its own distance from the quad centre in these coordinates.
        output.uv = input.uv;
        output.ring = float3(1.0, input.uv_rect.y, input.uv_rect.z);
    }
    else
    {
        output.uv = lerp(input.uv_rect.xy, input.uv_rect.zw, input.uv);
        output.ring = float3(0.0, 0.0, 0.0);
    }

    output.tint = input.tint;
    return output;
}

Texture2D<float4> SpriteAtlas : register(t0, space2);
SamplerState AtlasSampler : register(s0, space2);

float4 PSMain(VSOutput input) : SV_Target0
{
    if (input.ring.x > 0.5)
    {
        // Annulus in unit-quad space: `outer` is the true body radius, so the
        // ring's outer edge traces the contact circle the simulation separates
        // on. Everything else in the quad is discarded, which is what makes it
        // a ring rather than a disc.
        float d = length(input.uv - 0.5);
        if (d < input.ring.y || d > input.ring.z)
        {
            discard;
        }
        // Tint is already premultiplied; no atlas is sampled on this path.
        return input.tint;
    }

    float4 texel = SpriteAtlas.Sample(AtlasSampler, input.uv);
    // Atlas + tint already premultiplied; multiply keeps blend contract.
    return texel * input.tint;
}
