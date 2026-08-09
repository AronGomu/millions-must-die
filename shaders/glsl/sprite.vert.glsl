#version 450

// GLSL mirror of `../sprite.hlsl` VSMain. Not a second source of truth: the
// HLSL is canonical and this exists only because the Linux SPIR-V is built with
// glslc. Any edit to `sprite.hlsl` must be mirrored here and the blobs rebuilt
// — see `../generated/README.md` for the exact command and how to prove the
// mirror is faithful.
//
// `clamp(x, 0, 1)` below mirrors HLSL `saturate`, and lowers to the same
// `FClamp`. Note that the *Rust* mirror (`render::instance::iso_depth`)
// deliberately uses `.max().min()` instead, which is strictly more defined:
// `FClamp`/`saturate` leave NaN operand selection open, whereas the Rust form
// pins NaN to the floor. Neither is reachable — `depth_scale`/`depth_bias` are
// finite by construction and `IsoView::new` guards the degenerate map — so the
// shader stays faithful to the canonical HLSL rather than to the Rust mirror.

layout(location = 0) in vec2 corner;
layout(location = 1) in vec2 uv;
layout(location = 2) in vec2 instance_pos;
layout(location = 3) in vec2 instance_size;
layout(location = 4) in vec4 uv_rect;
layout(location = 5) in vec4 tint;

layout(set = 1, binding = 0) uniform FrameUniforms {
    vec2 view_size;
    float depth_scale;
    float depth_bias;
} frame;

layout(location = 0) out vec2 v_uv;
layout(location = 1) out vec4 v_tint;
layout(location = 2) out vec3 v_ring;

void main()
{
    vec2 world = instance_pos + (corner + vec2(0.5, 0.5)) * instance_size;
    vec2 ndc = (world / frame.view_size) * 2.0 - 1.0;
    ndc.y = -ndc.y;

    float ground_y = instance_pos.y + instance_size.y;
    float depth = max(clamp(ground_y * frame.depth_scale + frame.depth_bias, 0.0, 1.0), 0.0000152587890625);

    if (uv_rect.x < 0.0)
    {
        gl_Position = vec4(ndc, 0.0, 1.0);
        v_uv = uv;
        v_ring = vec3(1.0, uv_rect.y, uv_rect.z);
    }
    else
    {
        gl_Position = vec4(ndc, depth, 1.0);
        v_uv = mix(uv_rect.xy, uv_rect.zw, uv);
        v_ring = vec3(0.0, 0.0, 0.0);
    }

    v_tint = tint;
}
