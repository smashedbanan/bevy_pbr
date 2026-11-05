import package::render::mesh_view_bindings::lights;

/// The light the ambient, diffuse and specular, multiplies at a fragment: Bevy's own ambient light.
///
/// emm: an app that inserts `CustomAmbientHook` before `PbrPlugin` finishes registers its own
/// shader at `embedded://bevy_pbr/render/ambient_hook.wesl` with this function, which may read
/// `mesh_view_bindings::ambient_data`.
fn ambient_light_color(world_position: vec3<f32>, world_normal: vec3<f32>) -> vec3<f32> {
    return lights.ambient_color.rgb;
}
