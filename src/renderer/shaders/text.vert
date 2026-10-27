attribute vec2 a_position;
attribute vec2 a_offset;
attribute vec4 a_origin;
attribute float a_coverage;
// xy: the run's scale about its origin; zw: that origin.
uniform vec4 u_transform;
uniform vec2 u_pixel_scale;
uniform vec4 u_tint;
uniform float u_fringe;
// Small text's stems thicken by this many physical pixels per edge (see
// renderer::text::embolden): an opaque core survives the TV's 2x scale-up.
uniform float u_embolden;
varying PAINT_PRECISION vec4 v_color;
varying PAINT_PRECISION float v_coverage;
varying PAINT_PRECISION float v_logical_y;
void main() {
    // Snap each glyph's insertion origin/baseline, not individual outline
    // vertices: curves keep their shape. The run's place and each glyph's
    // place in it snap apart, so a moving run steps whole pixels together;
    // snapped as one sum, each glyph crossed its pixel boundary on a
    // different frame and the letters shimmered against each other.
    vec2 origin = floor(a_origin.xy * u_transform.xy * u_pixel_scale + vec2(0.5)) / u_pixel_scale +
        floor(u_transform.zw * u_pixel_scale + vec2(0.5)) / u_pixel_scale;
    // With MSAA, use the original outline; inward offsets can fold thin stems.
    // Single-sample targets retain a narrower analytic boundary fringe.
    // Boundary offsets point along the outline normal (inner vertices
    // inward): moving both sides out by u_embolden thickens the stems.
    vec2 normal = a_offset * (a_coverage > 0.5 ? -2.0 : 2.0);
    vec2 position = origin + a_position * u_transform.xy +
        (a_offset * u_fringe + normal * u_embolden) / u_pixel_scale;
    gl_Position = vec4(position.x / 960.0 - 1.0, 1.0 - position.y / 540.0, 0.0, 1.0);
    v_color = u_tint;
    v_coverage = mix(1.0, a_coverage, step(0.001, u_fringe));
    v_logical_y = position.y;
}
