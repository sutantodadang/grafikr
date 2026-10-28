struct Uniforms {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4(
        p.x / uniforms.viewport.x * 2.0 - 1.0,
        1.0 - p.y / uniforms.viewport.y * 2.0,
        0.0,
        1.0,
    );
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

struct QuadIn {
    @location(0) pos: vec2<f32>,
    @location(1) center: vec2<f32>,
    @location(2) half: vec2<f32>,
    @location(3) radius: f32,
    @location(4) border: f32,
    @location(5) fill: vec4<f32>,
    @location(6) stroke: vec4<f32>,
};

struct QuadOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half: vec2<f32>,
    @location(2) radius: f32,
    @location(3) border: f32,
    @location(4) fill: vec4<f32>,
    @location(5) stroke: vec4<f32>,
};

@vertex
fn vs_quad(input: QuadIn) -> QuadOut {
    var out: QuadOut;
    out.clip = to_clip(input.pos);
    out.local = input.pos - input.center;
    out.half = input.half;
    out.radius = input.radius;
    out.border = input.border;
    out.fill = input.fill;
    out.stroke = input.stroke;
    return out;
}

fn sd_round(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half_size.x, half_size.y));
    let q = abs(p) - half_size + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_quad(input: QuadOut) -> @location(0) vec4<f32> {
    let outer = clamp(0.5 - sd_round(input.local, input.half, input.radius), 0.0, 1.0);
    var inner = outer;
    if (input.border > 0.0) {
        inner = clamp(
            0.5 - sd_round(input.local, input.half - vec2<f32>(input.border), input.radius - input.border),
            0.0,
            1.0,
        );
    }
    let t = inner / max(outer, 0.0001);
    let rgb = mix(srgb_to_linear(input.stroke.rgb), srgb_to_linear(input.fill.rgb), t);
    let alpha = outer * mix(input.stroke.a, input.fill.a, t);
    return vec4<f32>(rgb, alpha);
}

struct TextIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct TextOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_text(input: TextIn) -> TextOut {
    var out: TextOut;
    out.clip = to_clip(input.pos);
    out.uv = input.uv;
    out.color = input.color;
    return out;
}

@fragment
fn fs_text(input: TextOut) -> @location(0) vec4<f32> {
    let coverage = textureSample(atlas, atlas_sampler, input.uv).r;
    return vec4<f32>(srgb_to_linear(input.color.rgb), coverage * input.color.a);
}

@fragment
fn fs_image(input: TextOut) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    return vec4<f32>(texel.rgb * input.color.rgb, texel.a * input.color.a);
}
