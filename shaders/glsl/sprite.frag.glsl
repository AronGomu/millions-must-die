#version 450

// GLSL mirror of `../sprite.hlsl` PSMain. See `sprite.vert.glsl` for why this
// file exists and what obligation it carries.

layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_tint;
layout(location = 2) in vec3 v_ring;

layout(set = 2, binding = 0) uniform sampler2D SpriteAtlas;

layout(location = 0) out vec4 out_color;

void main()
{
    if (v_ring.x > 0.5)
    {
        float d = length(v_uv - 0.5);
        if (d < v_ring.y || d > v_ring.z)
        {
            discard;
        }
        out_color = v_tint;
        return;
    }

    vec4 texel = texture(SpriteAtlas, v_uv);
    if (texel.a - 0.5 < 0.0)
    {
        discard;
    }
    out_color = texel * v_tint;
}
