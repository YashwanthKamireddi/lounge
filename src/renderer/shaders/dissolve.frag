uniform sampler2D u_texture;
uniform sampler2D u_texture2;
uniform float u_mix;
uniform vec4 u_tint2;
uniform vec4 u_lift;
varying PAINT_PRECISION vec2 v_uv;
varying PAINT_PRECISION vec4 v_color;
varying PAINT_PRECISION vec4 v_shape;
varying PAINT_PRECISION vec2 v_params;
varying PAINT_PRECISION float v_logical_y;
// Two full-screen images mixed in one opaque pass: on the TV's Mali GPU two
// stacked, blended 1080p layers miss the 20 ms frame; this holds it. The
// lift (alpha 0 in the dark appearance) frosts it toward a light material.
void main() {
    vec3 a = texture2D(u_texture, v_uv).rgb * v_color.rgb;
    vec3 b = texture2D(u_texture2, v_uv).rgb * u_tint2.rgb;
    gl_FragColor = vec4(mix(mix(a, b, u_mix), u_lift.rgb, u_lift.a), 1.0);
}
