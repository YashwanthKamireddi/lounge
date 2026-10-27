uniform sampler2D u_texture;
uniform vec4 u_tint2;
uniform vec4 u_lift;
#ifndef DISSOLVE_SINGLE
uniform sampler2D u_texture2;
uniform float u_mix;
#endif
varying PAINT_PRECISION vec2 v_uv;
varying PAINT_PRECISION vec4 v_color;
varying PAINT_PRECISION vec4 v_shape;
varying PAINT_PRECISION vec2 v_params;
varying PAINT_PRECISION float v_logical_y;
// Two full-screen images, each in its own tint, mixed in one pass: on the
// TV's Mali GPU two stacked, blended 1080p layers miss the 20 ms frame; this
// holds it. The lift (alpha 0 in the dark appearance) frosts it toward a
// light material; u_tint2's alpha is the pass's opacity.
void main() {
#ifdef DISSOLVE_SINGLE
    // At rest (every frame Home and Settings sit still) only one image
    // shows: the renderer binds it with its own tint, so this
    // full-screen pass makes one texture fetch per pixel instead of two.
    vec3 c = texture2D(u_texture, v_uv).rgb * u_tint2.rgb;
#else
    vec3 a = texture2D(u_texture, v_uv).rgb * v_color.rgb;
    vec3 b = texture2D(u_texture2, v_uv).rgb * u_tint2.rgb;
    vec3 c = mix(a, b, u_mix);
#endif
    gl_FragColor = vec4(mix(c, u_lift.rgb, u_lift.a), u_tint2.a);
}
