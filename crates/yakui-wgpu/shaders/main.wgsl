struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) texcoord: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) texcoord: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@group(0) @binding(0) var color_texture: texture_2d<f32>;
@group(0) @binding(1) var color_sampler: sampler;

@vertex
fn vs_main(
    in: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;

    // Transform from yakui coordinates to WebGPU:
    // yakui uses (0, 0) in the top left and (1, 1) in the bottom right
    // WebGPU uses (-1, 1) in the top left and (1, -1) in the bottom right
    var adjusted: vec2<f32> = in.position;
    adjusted *= vec2(2.0, -2.0);
    adjusted += vec2(-1.0, 1.0);

    out.position = vec4<f32>(adjusted, 0.0, 1.0);
    out.texcoord = in.texcoord;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(color_texture, color_sampler, in.texcoord);
    color = vec4(color.rgb * in.color.a, color.a);

    return in.color * color;
}