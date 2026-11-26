// Kawase 4-tap blur. Run twice (offsets 1.5 then 3.5 for a moderately soft
// look) to produce the backdrop the UI shader samples. The vertex shader is
// the canonical fullscreen-triangle trick — no vertex buffer needed; the
// caller issues `draw(3, 1, 0, 0)`.

struct PushConsts {
    inv_resolution: vec2<f32>,
    offset: f32,
    _pad: f32,
};
var<push_constant> pc: PushConsts;

@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var s_src: sampler;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    // Triangle covering the full clip-space rect [-1, 1] with UVs in [0, 1].
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(positions[idx], 0.0, 1.0);
    out.uv = uvs[idx];
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let o = pc.offset * pc.inv_resolution;
    let c = textureSample(t_src, s_src, input.uv + vec2<f32>(-o.x, -o.y))
          + textureSample(t_src, s_src, input.uv + vec2<f32>( o.x, -o.y))
          + textureSample(t_src, s_src, input.uv + vec2<f32>(-o.x,  o.y))
          + textureSample(t_src, s_src, input.uv + vec2<f32>( o.x,  o.y));
    return c * 0.25;
}
