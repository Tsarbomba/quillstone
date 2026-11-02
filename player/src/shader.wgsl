struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) blur: f32,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) blur: f32,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    out.blur = blur;
    return out;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

// A blur is a disc of 16 taps (a golden-angle spiral) spread over `blur` window pixels. Each tap reads a
// coarser mip level, as if the picture were already that much smaller, so 16 taps are enough for a smooth
// result at any radius. Colour is averaged weighted by alpha, so a sprite's transparent edge does not darken it.
const TAPS: i32 = 16;
const GOLDEN_ANGLE: f32 = 2.39996323;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    if (in.blur <= 0.0) {
        return textureSampleGrad(tex, samp, in.uv, ddx, ddy) * in.color;
    }
    // Spacing between taps is about 0.44 * radius pixels: the gradients are widened to pick that mip level.
    let widen = max(1.0, 0.44 * in.blur);
    var rgb = vec3<f32>(0.0);
    var alpha = 0.0;
    for (var i = 0; i < TAPS; i = i + 1) {
        let r = in.blur * sqrt((f32(i) + 0.5) / f32(TAPS));
        let angle = f32(i) * GOLDEN_ANGLE;
        let offset = ddx * (r * cos(angle)) + ddy * (r * sin(angle));
        let tap = textureSampleGrad(tex, samp, in.uv + offset, ddx * widen, ddy * widen);
        rgb = rgb + tap.rgb * tap.a;
        alpha = alpha + tap.a;
    }
    let colour = select(vec3<f32>(0.0), rgb / alpha, alpha > 0.0);
    return vec4<f32>(colour, alpha / f32(TAPS)) * in.color;
}
