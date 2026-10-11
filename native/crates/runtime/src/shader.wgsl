struct Camera { view_projection: mat4x4<f32>, eye: vec4<f32>, environment: vec4<f32> };
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world_position: vec3<f32>,
};
@vertex fn vs_main(input: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = camera.view_projection * vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    out.color = input.color;
    out.world_position = input.position;
    return out;
}
// environment.x: outdoor haze, environment.y: sun lighting strength (0 keeps the
// original prelit look). The face normal comes from screen-space derivatives,
// so moving cars and animated peds need no extra vertex data.
const SUN: vec3<f32> = vec3<f32>(-0.42, 0.80, 0.43);
fn sun_light(world_position: vec3<f32>) -> vec3<f32> {
    var normal = normalize(cross(dpdx(world_position), dpdy(world_position)));
    if (dot(normal, camera.eye.xyz - world_position) < 0.0) { normal = -normal; }
    let facing = max(dot(normal, normalize(SUN)), 0.0);
    let sky = mix(vec3<f32>(0.62, 0.60, 0.58), vec3<f32>(0.72, 0.76, 0.84), normal.y * 0.5 + 0.5);
    let lit = sky + vec3<f32>(1.0, 0.95, 0.86) * 0.45 * facing;
    return mix(vec3<f32>(1.0), lit, camera.environment.y);
}
fn shade(color: vec4<f32>, input: VertexOut, light: vec3<f32>) -> vec4<f32> {
    let haze = (1.0 - exp(-distance(input.world_position, camera.eye.xyz) * 0.00045)) * camera.environment.x;
    return vec4<f32>(mix(color.rgb * light, vec3<f32>(0.48, 0.60, 0.70), haze), color.a);
}
@fragment fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let light = sun_light(input.world_position);
    let color = textureSample(image, image_sampler, input.uv) * input.color;
    if (color.a < 0.5) { discard; }
    return shade(color, input, light);
}
@fragment fn fs_blended(input: VertexOut) -> @location(0) vec4<f32> {
    let light = sun_light(input.world_position);
    let color = textureSample(image, image_sampler, input.uv) * input.color;
    if (color.a < 0.04) { discard; }
    return shade(color, input, light);
}
