# bevy_pbr for ExtractMeMaybe

`bevy_pbr` 0.20.1 as published on crates.io (upstream bevyengine/bevy `a0577b5c`,
`.cargo_vcs_info.json`), with one change for [ExtractMeMaybe](https://github.com/smashedbanan/ExtractMeMaybe):
an app can supply the ambient light per fragment.

- `render/ambient_hook.wesl`: `ambient_light_color(world_position, world_normal)`, which
  `pbr_ambient.wesl`'s diffuse and specular halves multiply in place of `lights.ambient_color`.
  Bevy's default
  returns `lights.ambient_color`; an app that inserts `CustomAmbientHook` before `PbrPlugin`
  finishes registers its own shader at `embedded://bevy_pbr/render/ambient_hook.wesl` instead.
- `mesh_view_bindings`: binding 39, `ambient_data: array<vec4<f32>>`, a read-only storage buffer
  from the render-world resource `AmbientDataBuffer` (one zero `vec4` until the app replaces it).

Each change is marked `emm:` in the source.
