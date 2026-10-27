uniform sampler2D u_texture;
uniform float u_aa;
uniform vec4 u_edge_fade;
varying PAINT_PRECISION vec2 v_uv;
varying PAINT_PRECISION vec4 v_color;
varying PAINT_PRECISION vec4 v_shape;
varying PAINT_PRECISION vec2 v_params;
varying PAINT_PRECISION float v_logical_y;
// Liquid Glass: the pre-blurred backdrop seen through a lens. Within a band
// along the rim the glass bends light from just beyond its edge inward,
// compressing it the nearer the rim (a thick lens edge), and concentrates
// it there. Every piece of glass is this one material; its tint (v_color,
// top to bottom) carries the appearance's light.
float edge_coverage() {
    if (u_edge_fade.y <= u_edge_fade.x) return 1.0;
    return smoothstep(u_edge_fade.x, u_edge_fade.y, v_logical_y) *
        (1.0 - smoothstep(u_edge_fade.z, u_edge_fade.w, v_logical_y));
}
void main() {
    float soft = max(-v_params.y, 0.0);
    vec2 half_size = v_shape.zw - vec2(soft);
    float radius = min(v_params.x, min(half_size.x, half_size.y));
    vec2 q = abs(v_shape.xy) - half_size + vec2(radius);
    // The band is as deep as the shape is round: a capsule bends over less
    // than the dock's broad corners.
    float band = clamp(radius * 0.36, 6.0, 26.0);
    // Inside the band's inner edge the backdrop shows straight through:
    // most of every pane skips the lens maths.
    if (max(q.x, q.y) < radius - band - u_aa) {
        gl_FragColor = texture2D(u_texture, v_uv) * v_color * vec4(1.0, 1.0, 1.0, edge_coverage());
        return;
    }
    vec2 outside = max(q, vec2(0.0));
    float d = length(outside) + min(max(q.x, q.y), 0.0) - radius;
    // The outward normal: radial on a corner, straight out along a side.
    vec2 normal = max(outside.x, outside.y) > 0.0
        ? normalize(outside)
        : (q.x > q.y ? vec2(1.0, 0.0) : vec2(0.0, 1.0));
    normal *= sign(v_shape.xy);
    float bend = 1.0 - clamp(-d / band, 0.0, 1.0);
    bend *= bend;
    // Half the band's depth at the rim, nothing at its inner edge: monotonic,
    // so the picture compresses toward the rim without folding over.
    vec2 uv = v_uv + normal * (0.5 * band * bend) * vec2(1.0 / 1920.0, 1.0 / 1080.0);
    float alpha = soft > 0.0
        ? 1.0 - smoothstep(-soft, soft, d)
        : 1.0 - smoothstep(-u_aa, 0.0, d);
    vec3 light = v_color.rgb * (1.0 + 0.16 * bend);
    gl_FragColor = texture2D(u_texture, uv) * vec4(light, v_color.a * alpha * edge_coverage());
}
