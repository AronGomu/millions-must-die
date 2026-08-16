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
    float2 view_size;  // pixels, e.g. 1920x1080
    // Depth normalisation for the isometric sort key. The two scalars occupy
    // what used to be padding, so `SpriteInstance` keeps its pinned 48 bytes:
    //   depth_scale = 1 / iso_map_height_px
    //   depth_bias  = -origin.y / iso_map_height_px
    // so `ground_y * depth_scale + depth_bias` is the agent's position down the
    // *map* diamond in [0, 1] and does not change when the camera does.
    float depth_scale;
    float depth_bias;
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

// A negative `uv_rect.x` marks a non-atlas instance. Two thresholds split
// three kinds — the writer emits exact sentinel values:
//   `render::instance::DIAGONAL_LINE_SENTINEL = -2.0`  →  line branch
//   `render::instance::RING_SENTINEL          = -1.0`  →  ring branch
//   sprite rects from `frame_uv_rect` have `u0 >= 0`   →  sprite branch
//
// Sentinel split (threshold values, not sentinel copies — do not "sync" to
// the sentinel or the comparison becomes `x < -2.0`, which is never true for
// line instances at exactly -2.0):
//   `uv_rect.x < MMD_LINE_THRESHOLD`                 → line
//   `uv_rect.x >= MMD_LINE_THRESHOLD && < MMD_RING_THRESHOLD` → ring
//   `uv_rect.x >= MMD_RING_THRESHOLD`                → sprite
#define MMD_LINE_THRESHOLD -1.5
#define MMD_RING_THRESHOLD 0.0

// Alpha below this is discarded outright rather than blended.
//
// Depth-buffered draw order needs a per-fragment opaque/transparent decision:
// a blended fragment that also wrote depth would occlude whatever it was
// supposed to show through. Pixel art is effectively 1-bit alpha — the tracked
// atlases put 46% of their texels at exactly 0 and almost all of the rest above
// 200 — so a cutout is honest here, and it is what lets all four atlas groups
// stay batched instead of being sorted back-to-front.
#define MMD_ALPHA_CUTOFF 0.5

// Smallest depth a *sprite* may be given. One quantum of the D16_UNORM depth
// attachment (1/65536), which the renderer clears to 0 and tests with GREATER.
//
// Without this floor, a sprite whose key saturates to exactly 0 fails `0 > 0`
// and is discarded outright — not drawn behind everything, drawn *nowhere*. An
// agent standing on the far corner of the map diamond normalises to 0, and a
// degenerate map (`IsoView` with a zero cell size) zeroes both scalars and
// would blank the entire frame. Flooring costs one quantum of sort precision
// out of 65536 and removes the whole class.
//
// Rings deliberately do NOT get this floor: they emit an exact 0 so that a ring
// wrongly placed on the sprite pipeline would fail GREATER everywhere and
// vanish, which is what keeps `rings_are_never_occluded` falsifiable.
#define MMD_DEPTH_EPSILON 0.0000152587890625

VSOutput VSMain(VSInput input)
{
    VSOutput output;

    float2 world = input.instance_pos + (input.corner + float2(0.5, 0.5)) * input.instance_size;
    float2 ndc = (world / view_size) * 2.0 - 1.0;
    ndc.y = -ndc.y; // y-down pixel space → y-up clip space

    // The agent's feet: the quad's *bottom* edge, which the packer anchors on
    // the projected ground point. It is a property of the instance, not of the
    // vertex — emitting the interpolated `world.y` instead would give one
    // sprite a depth gradient down its own quad and let it slice into the
    // sprites beside it.
    float ground_y = input.instance_pos.y + input.instance_size.y;
    float depth = max(saturate(ground_y * depth_scale + depth_bias), MMD_DEPTH_EPSILON);

    if (input.uv_rect.x < MMD_LINE_THRESHOLD)
    {
        // Diagonal line: transform the unit quad into a rotated rectangle
        // aligned along the segment vector stored in `instance_size`.
        // `instance_pos` is endpoint `a`; `instance_size` is `b - a`.
        // A zero-length segment collapses all four vertices to `a`, producing a
        // degenerate triangle with zero area that rasterises nothing.
        float thickness = input.uv_rect.y;
        float2 seg = input.instance_size;
        float seg_len = length(seg);
        float2 line_world;
        if (seg_len < 1e-6)
        {
            line_world = input.instance_pos;
        }
        else
        {
            float2 tangent = seg / seg_len;
            float2 normal = float2(-tangent.y, tangent.x);
            float2 center = input.instance_pos + seg * 0.5;
            line_world = center + input.corner.x * seg + input.corner.y * normal * thickness;
        }
        float2 line_ndc = (line_world / view_size) * 2.0 - 1.0;
        line_ndc.y = -line_ndc.y;
        output.position = float4(line_ndc, 0.0, 1.0);
        output.uv = input.uv;
        output.ring = float3(2.0, 0.0, 0.0);
    }
    else if (input.uv_rect.x < MMD_RING_THRESHOLD)
    {
        // Ring: raw unit-quad coords, no atlas lerp. The pixel stage measures
        // its own distance from the quad centre in these coordinates.
        //
        // Depth 0 because rings are a debug overlay drawn on the pipeline whose
        // depth test and write are both off; a ring that could be occluded
        // would stop annotating the body it exists to annotate. The key is
        // "position down the map diamond", so 0 is the far end of it — a value
        // that would fail the sprite pipeline's GREATER test everywhere, which
        // is exactly what makes "the ring pass is not depth-tested" a fact this
        // frame can be asked about rather than an assertion in a comment.
        output.position = float4(ndc, 0.0, 1.0);
        output.uv = input.uv;
        output.ring = float3(1.0, input.uv_rect.y, input.uv_rect.z);
    }
    else
    {
        output.position = float4(ndc, depth, 1.0);
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
    if (input.ring.x > 1.5)
    {
        // Diagonal line: premultiplied tint, no atlas sample.
        return input.tint;
    }

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
    // Alpha test before the blend: this fragment writes depth, so a nearly
    // transparent texel that survived would occlude every sprite behind it.
    clip(texel.a - MMD_ALPHA_CUTOFF);
    // Atlas + tint already premultiplied; multiply keeps blend contract.
    return texel * input.tint;
}
