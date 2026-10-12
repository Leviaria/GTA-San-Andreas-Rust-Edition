struct Camera {
    view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    environment: vec4<f32>,
    light: mat4x4<f32>,
    // xyz: direction towards the sun or moon, w: daylight 0..1.
    sun: vec4<f32>,
    sun_color: vec4<f32>,
    // Multiplier for the day-baked prelit colours (night darkening).
    ambient: vec4<f32>,
    sky: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;
@group(2) @binding(0) var shadow_map: texture_depth_2d;
@group(2) @binding(1) var shadow_sampler: sampler_comparison;

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
// original prelit look), environment.z: sun shadow strength. The face normal comes from screen-space derivatives,
// so moving cars and animated peds need no extra vertex data.
fn sun_shadow(world_position: vec3<f32>) -> f32 {
    let clip = camera.light * vec4<f32>(world_position, 1.0);
    let uv = clip.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let texel = 1.0 / 2048.0;
    var lit = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(f32(x), f32(y)) * texel, clip.z);
        }
    }
    // Fade out towards the edge of the shadowed square instead of cutting off.
    let edge = max(abs(clip.x), abs(clip.y));
    let inside = 1.0 - smoothstep(0.85, 1.0, edge);
    let covered = select(0.0, inside, clip.z > 0.0 && clip.z < 1.0);
    return mix(1.0, lit / 9.0, covered);
}
fn sun_light(world_position: vec3<f32>) -> vec3<f32> {
    var normal = normalize(cross(dpdx(world_position), dpdy(world_position)));
    if (dot(normal, camera.eye.xyz - world_position) < 0.0) { normal = -normal; }
    let facing = max(dot(normal, camera.sun.xyz), 0.0);
    let shadow = sun_shadow(world_position);
    let sky = mix(vec3<f32>(0.62, 0.60, 0.58), vec3<f32>(0.72, 0.76, 0.84), normal.y * 0.5 + 0.5);
    let lit = sky + camera.sun_color.rgb * 0.45 * facing * shadow * camera.sun.w;
    // Shadows also darken the original prelit colours, which already contain
    // the sun, so they read clearly even with low sun lighting.
    let shade = mix(1.0, 0.55, (1.0 - shadow) * camera.environment.z * camera.sun.w * smoothstep(0.0, 0.2, facing));
    return mix(vec3<f32>(1.0), lit, camera.environment.y) * shade * camera.ambient.rgb;
}
fn shade(color: vec4<f32>, input: VertexOut, light: vec3<f32>) -> vec4<f32> {
    let haze = (1.0 - exp(-distance(input.world_position, camera.eye.xyz) * 0.00045)) * camera.environment.x;
    return vec4<f32>(mix(color.rgb * light, camera.sky.rgb, haze), color.a);
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
struct ShadowOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};
@vertex fn vs_shadow(input: VertexIn) -> ShadowOut {
    var out: ShadowOut;
    out.position = camera.light * vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    out.alpha = input.color.a;
    return out;
}
@fragment fn fs_shadow(input: ShadowOut) {
    if (textureSample(image, image_sampler, input.uv).a * input.alpha < 0.5) { discard; }
}
// Night glows from original 2DFX lights, drawn additively after the scene.
// Colour alpha 1 marks night-only lights, 0.5 always-lit ones.
fn glow_visibility(alpha: f32) -> f32 {
    let night = 1.0 - camera.sun.w;
    return select(night, max(night, 0.35), alpha < 0.75);
}
@vertex fn vs_corona(input: VertexIn) -> VertexOut {
    let to_eye = camera.eye.xyz - input.position;
    let distance = max(length(to_eye), 0.001);
    let forward = to_eye / distance;
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), forward) + vec3<f32>(1e-5, 0.0, 0.0));
    let up = cross(forward, right);
    // Pull the sprite towards the camera so the lamp does not hide its glow,
    // and grow it gently with distance so far lamps stay visible.
    let centre = input.position + forward * min(0.6, distance * 0.5);
    let world = centre + (right * input.uv.x + up * input.uv.y) * (1.0 + distance * 0.004);
    var out: VertexOut;
    out.position = camera.view_projection * vec4<f32>(world, 1.0);
    out.uv = sign(input.uv);
    out.color = input.color;
    out.world_position = world;
    return out;
}
@fragment fn fs_corona(input: VertexOut) -> @location(0) vec4<f32> {
    let r2 = dot(input.uv, input.uv);
    let falloff = max(exp(-r2 * 4.0) - exp(-4.0), 0.0);
    return vec4<f32>(input.color.rgb * falloff * glow_visibility(input.color.a) * 1.6, 1.0);
}
@fragment fn fs_pool(input: VertexOut) -> @location(0) vec4<f32> {
    let edge = max(1.0 - dot(input.uv, input.uv), 0.0);
    return vec4<f32>(input.color.rgb * edge * edge * glow_visibility(input.color.a) * 0.45, 1.0);
}
