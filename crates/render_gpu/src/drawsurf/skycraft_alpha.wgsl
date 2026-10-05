// SkyCraft guest: give the finished frame an alpha Skyrim can composite with.
// The scene's passes leave alpha at 1 everywhere, so coverage comes from the
// scene depth instead. Reverse Z, with IW4's two depth bands:
//   0                     nothing drawn (the clear)
//   (0, 1 - band]         the scene: the stand-in map's static models, which
//                         stay in the pass but are not ours to show
//   (1 - band, 1]         the viewmodel band: gun, hands, heartbeat screen
// What drew nothing but still added light over the empty clear (muzzle flash,
// tracers, sparks) is kept as straight alpha by its brightness, so it adds onto
// Skyrim the way it added onto black.

// render_backend::DEPTH_RANGE_BAND
const VIEWMODEL_DEPTH: f32 = 1.0 - 0.015625;

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var depth: texture_depth_2d;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let colour = textureLoad(scene, pixel, 0);
    let z = textureLoad(depth, pixel, 0);
    if z > VIEWMODEL_DEPTH {
        return vec4<f32>(colour.rgb, 1.0);
    }
    if z > 0.0 {
        return vec4<f32>(0.0);
    }
    let glow = max(colour.r, max(colour.g, colour.b));
    if glow <= 1.0 / 255.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(colour.rgb / glow, glow);
}
