// crusty-gui UI shader.
//
// One pipeline draws every UI primitive. Each vertex carries the SDF
// parameters of the rectangle it belongs to, plus a `mode` flag selecting
// which interpretation the fragment shader uses:
//   0 — solid color (no SDF; useful for the background gradient or
//       interpolated meshes).
//   1 — rounded rect fill (mixes the blurred backdrop with `color.rgb`
//       using `color.a` as the glass tint strength) + optional stroke.
//   2 — drop shadow (gaussian-ish falloff outside the rect).
//   3 — outer glow (sharper falloff than shadow).
//   4 — glyph: samples the R8 glyph atlas at `uv`, output `color.rgb`
//       with alpha = `color.a * atlas.r`.
//   5 — image: samples the bound user texture at `uv`, multiplied by
//       `color` as a tint (white = untinted).
//   6 — color glyph: samples the RGBA color-glyph atlas at `uv`, RGB
//       untinted, alpha scaled by `color.a` (color emoji).
//
// Coordinates are in physical pixels; the vertex shader maps to clip space
// using the `screen_size` push constant.

struct PushConsts {
    screen_size: vec2<f32>,
    inv_screen_size: vec2<f32>,
};
var<push_constant> pc: PushConsts;

@group(0) @binding(0) var t_backdrop: texture_2d<f32>;
@group(0) @binding(1) var s_backdrop: sampler;
@group(0) @binding(2) var t_atlas: texture_2d<f32>;
@group(0) @binding(3) var s_atlas: sampler;
@group(0) @binding(4) var t_image: texture_2d<f32>;
@group(0) @binding(5) var s_image: sampler;
@group(0) @binding(6) var t_color: texture_2d<f32>;
@group(0) @binding(7) var s_color: sampler;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) rect_center: vec2<f32>,
    @location(4) rect_half: vec2<f32>,
    @location(5) rounding: vec4<f32>,
    @location(6) params: vec4<f32>,
    @location(7) stroke_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) frag_pos: vec2<f32>,
    @location(3) rect_center: vec2<f32>,
    @location(4) rect_half: vec2<f32>,
    @location(5) rounding: vec4<f32>,
    @location(6) params: vec4<f32>,
    @location(7) stroke_color: vec4<f32>,
};

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    let ndc = vec2<f32>(
        input.pos.x * pc.inv_screen_size.x * 2.0 - 1.0,
        input.pos.y * pc.inv_screen_size.y * 2.0 - 1.0
    );
    out.clip_pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = input.uv;
    out.color = input.color;
    out.frag_pos = input.pos;
    out.rect_center = input.rect_center;
    out.rect_half = input.rect_half;
    out.rounding = input.rounding;
    out.params = input.params;
    out.stroke_color = input.stroke_color;
    return out;
}

// Signed-distance to a rounded rectangle with per-corner radii.
// `r` = (nw, ne, sw, se).
fn sdf_rrect(p: vec2<f32>, center: vec2<f32>, half_size: vec2<f32>, r: vec4<f32>) -> f32 {
    let local = p - center;
    let east = local.x > 0.0;
    let south = local.y > 0.0;
    var radius: f32;
    if (south) {
        if (east) { radius = r.w; } else { radius = r.z; }
    } else {
        if (east) { radius = r.y; } else { radius = r.x; }
    }
    let q = abs(local) - half_size + vec2<f32>(radius, radius);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let mode = input.params.w;
    let stroke_w = input.params.x;
    let shadow = max(input.params.y, 0.0001);
    let glow = max(input.params.z, 0.0001);

    if (mode < 0.5) {
        return input.color;
    }

    let d = sdf_rrect(input.frag_pos, input.rect_center, input.rect_half, input.rounding);

    if (mode < 1.5) {
        // Filled rrect: composite blurred backdrop with the glass tint.
        let coverage = clamp(0.5 - d, 0.0, 1.0);
        let backdrop_uv = input.frag_pos * pc.inv_screen_size;
        let backdrop = textureSample(t_backdrop, s_backdrop, backdrop_uv).rgb;
        let glass_rgb = mix(backdrop, input.color.rgb, input.color.a);

        var out_rgb = glass_rgb;
        var out_a = coverage;

        if (stroke_w > 0.0) {
            // Centered stroke band of width `stroke_w` around d = 0, drawn
            // in `stroke_color` (alpha scales the band).
            let stroke_d = abs(d);
            let band = clamp(stroke_w * 0.5 + 0.5 - stroke_d, 0.0, 1.0);
            let stroke_alpha = band * input.stroke_color.a;
            out_rgb = mix(out_rgb, input.stroke_color.rgb, stroke_alpha);
            out_a = max(out_a, stroke_alpha);
        }

        return vec4<f32>(out_rgb, out_a);
    }

    if (mode < 2.5) {
        let dn = max(d, 0.0) / shadow;
        let a = exp(-dn * dn * 2.0);
        return vec4<f32>(input.color.rgb, a * input.color.a);
    }

    if (mode < 3.5) {
        let a = 1.0 - smoothstep(0.0, glow, max(d, 0.0));
        return vec4<f32>(input.color.rgb, a * input.color.a);
    }

    if (mode < 4.5) {
        // Glyph: sample the R8 atlas as a coverage mask.
        let coverage = textureSample(t_atlas, s_atlas, input.uv).r;
        return vec4<f32>(input.color.rgb, input.color.a * coverage);
    }

    if (mode < 5.5) {
        // Image: sample the bound user texture, tinted by `color`.
        return textureSample(t_image, s_image, input.uv) * input.color;
    }

    if (mode < 6.5) {
        // Color glyph: sample the RGBA atlas, scale alpha by `color.a`.
        let texel = textureSample(t_color, s_color, input.uv);
        return vec4<f32>(texel.rgb, texel.a * input.color.a);
    }

    return input.color;
}
