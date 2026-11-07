struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) blur: f32,
    // The stage's colour filter: one row per output channel, [r, g, b, offset].
    @location(3) filter_r: vec4<f32>,
    @location(4) filter_g: vec4<f32>,
    @location(5) filter_b: vec4<f32>,
    // Mask transition: where this pixel is on the mask picture, and the threshold (2.0: no mask, everything shows).
    @location(6) mask_uv: vec2<f32>,
    @location(7) reveal: f32,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) blur: f32,
    @location(4) filter_r: vec4<f32>,
    @location(5) filter_g: vec4<f32>,
    @location(6) filter_b: vec4<f32>,
    @location(7) mask_uv: vec2<f32>,
    @location(8) reveal: f32,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    out.blur = blur;
    out.filter_r = filter_r;
    out.filter_g = filter_g;
    out.filter_b = filter_b;
    out.mask_uv = mask_uv;
    out.reveal = reveal;
    return out;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
// The mask picture of a mask transition (a 1x1 white one for every other quad).
@group(1) @binding(0) var mask_tex: texture_2d<f32>;
@group(1) @binding(1) var mask_samp: sampler;

// How soft the edge of a mask transition is (`pixels::MASK_EDGE`).
const MASK_EDGE: f32 = 0.1;

// A blur is a disc of 16 taps (a golden-angle spiral) spread over `blur` window pixels. Each tap reads a
// coarser mip level, as if the picture were already that much smaller, so 16 taps are enough for a smooth
// result at any radius. Colour is averaged weighted by alpha, so a sprite's transparent edge does not darken it.
const TAPS: i32 = 16;
const GOLDEN_ANGLE: f32 = 2.39996323;

// The colour filter comes last (the mask's coverage scales the alpha), over the picture times its colour (so a solid `::bg` and a shaded sprite are
// both filtered); straight alpha means the colour is not premultiplied, and alpha is left alone.
fn filtered(in: VertexOutput, c: vec4<f32>) -> vec4<f32> {
    let rgb = vec4<f32>(c.rgb, 1.0);
    let out = vec3<f32>(dot(in.filter_r, rgb), dot(in.filter_g, rgb), dot(in.filter_b, rgb));
    let dark = textureSampleLevel(mask_tex, mask_samp, in.mask_uv, 0.0).r;
    let coverage = smoothstep(0.0, MASK_EDGE, in.reveal - dark);
    return vec4<f32>(clamp(out, vec3<f32>(0.0), vec3<f32>(1.0)), c.a * coverage);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    if (in.blur <= 0.0) {
        return filtered(in, textureSampleGrad(tex, samp, in.uv, ddx, ddy) * in.color);
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
    return filtered(in, vec4<f32>(colour, alpha / f32(TAPS)) * in.color);
}
