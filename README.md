# bevy_pbr for ExtractMeMaybe

`bevy_pbr` 0.20.1 as published on crates.io (upstream bevyengine/bevy `a0577b5c`,
`.cargo_vcs_info.json`), with three changes for
[ExtractMeMaybe](https://github.com/smashedbanan/ExtractMeMaybe): an app can supply the ambient
light per fragment, the main pass draws correctly at a smaller size under
`MainPassResolutionOverride`, as an upscaler such as FSR needs, and an app can stop a point or spot
light at a plane.

Each change is marked `emm:` in the source.

## The ambient hook

- `render/ambient_hook.wesl`: `ambient_light_color(world_position, world_normal)`, which
  `pbr_ambient.wesl`'s diffuse and specular halves multiply in place of `lights.ambient_color`.
  Bevy's default
  returns `lights.ambient_color`; an app that inserts `CustomAmbientHook` before `PbrPlugin`
  finishes registers its own shader at `embedded://bevy_pbr/render/ambient_hook.wesl` instead.
- `mesh_view_bindings`: binding 39, `ambient_data: array<vec4<f32>>`, a read-only storage buffer
  from the render-world resource `AmbientDataBuffer` (one zero `vec4` until the app replaces it).

## The light clip plane

`LightClipPlane(Vec4)` on a `PointLight` or `SpotLight`: `pbr_lighting.wesl`'s `point_light`, which
`spot_light` calls, gives no light to a point `p` with `dot(plane.xyz, p) + plane.w < 0`.
`ExtractedPointLight` and `GpuClusteredLight` carry it as `clip_plane` (`ClusteredLight` in
`mesh_view_types.wesl`), `Vec4::W` (nothing clipped) without one. The light grows from 80 to 96
bytes, so `MAX_UNIFORM_BUFFER_CLUSTERABLE_OBJECTS` drops from 204 to 170 to keep WebGL 2's
uniform buffer within 16 KiB.

## The main pass at a smaller size

Under `MainPassResolutionOverride` the main pass draws in the top-left corner of window-size
textures, `view.main_pass_viewport`, while `view.viewport` stays the window's. Stock 0.20.1 maps
some screen positions through the window's size, so at a smaller main pass they land outside the
picture. The shader changes below read `main_pass_viewport` instead, or scale by a ratio of the
two sizes: the occlusion test by `main_pass_viewport.zw / viewport.zw`, the cluster lookup by its
inverse. A read that addresses a window-size texture in absolute texels, or adds `viewport.xy` and
divides by the window-size texture, stays on `viewport`, as changing it would scale twice. The two
Rust changes, in `transmission/node.rs` and `ssao/mod.rs`, set the transmissive pass's viewport
and size SSAO's textures and dispatch:

- `transmission/node.rs`: the transmissive pass's `steps > 0` branch sets
  `Viewport::from_viewport_and_override`, as its `steps == 0` branch already did, so transmissive
  meshes draw in the corner too.
- `transmission.wesl`: `specular_transmissive_light` maps its exit point by the main pass's size,
  so the copy is read in the corner. `fetch_transmissive_background_non_rough` clamps its colour
  read to the main pass's viewport, inset half a texel, as a native read clamps to its last texel:
  the copy holds no scene past the corner. `fetch_transmissive_background`, the rough blur, scales
  its tap offsets by the main pass's size over the window's, so the blur spans the same share of
  the picture, and clamps each tap's colour read the same way. Their depth tests stay unclamped,
  since past the corner the depth holds the clear, which a native read past the edge returns too.
- `render/view_transformations.wesl`: `frag_coord_to_uv` and `ndc_to_frag_coord` use the main
  pass's viewport, so `frag_coord_to_ndc` and every shader built on them follow.
- `render/clustered_forward.wesl`: `view_fragment_cluster_index` scales `cluster_factors.xy` from
  clusters per window pixel to clusters per main-pass pixel. The fix is in the shader because
  `prepare_lights` divides by the extracted view's viewport, which is the window's size whether
  or not the override is in place when it runs.
- `ssao/`: `mod.rs` sizes SSAO's textures and its dispatch at the main pass's size;
  `preprocess_depth.wesl`, `ssao.wesl` and `spatial_denoise.wesl` divide by it.
  `preprocess_depth.wesl`'s gather also reads the corner's last texel for a texel past the corner,
  as the clamp sampler does past a native texture's edge; that changes no stored texel, as it
  holds only at an odd size's last texel, whose every store lands past its mip's edge.
- `render/pbr_functions.wesl` and `ssr/raymarch.wesl`: contact shadows' march is sized to the main
  pass (`depth_size`), and `depth_texel_clamped` clamps to the main pass's last texel rather than
  the depth texture's.
- `render/mesh_preprocess.wesl`: the occlusion test scales a mesh's bounds into the main pass's
  corner of the window-size depth pyramid, in both phases. `PreviousViewData` holds no sizes, so
  both use this frame's: after a size change the early phase may cull wrongly for one frame, and
  the late phase retests every mesh it culled. `render/occlusion_culling.wesl` is left as it is,
  as its one caller passes it the scaled bounds. The pyramid itself (`bevy_core_pipeline`'s
  downsample) is built over the whole window-size depth, so a pyramid texel that straddles the
  corner's right or bottom edge holds the clear's far depth: a mesh whose bounds reach one is
  never culled. That costs draws, not correctness.

Every ratio of the two sizes selects exactly 1 on an axis the override leaves equal, as a GPU's
division may round x / x below 1.

Without an override `main_pass_viewport` equals `viewport`, and every change draws what stock
draws for a camera whose viewport covers its target. For a camera with a sub-viewport and no
override, three reads now follow the viewport where stock followed the whole target: contact
shadows' march size, their texel clamp (which screen-space reflections reach too, on wasm), and
the transmission colour reads' clamps, the rough blur's taps among them.

### Affected but not made correct under the override

These call a changed shared function without a fix of their own:

- Screen-space reflections (`ssr.wesl`): `frag_coord_to_ndc` and the cluster lookup. Their march
  is sized to the depth texture on every target (`depth_size`, from `textureDimensions`). On native
  targets it reads depth through samplers and never reaches `depth_texel_clamped`; on wasm it
  does, and so clamps to the main pass's last texel.
- Volumetric fog (`volumetric_fog/volumetric_fog.wesl`): `frag_coord_to_ndc` and the cluster
  lookup.
- Deferred lighting: `deferred/functions.wesl`'s `pbr_input_from_deferred_gbuffer` calls
  `frag_coord_to_ndc`, and `deferred/deferred_lighting.wesl` calls `apply_pbr_lighting`, with its
  cluster lookup and contact shadows.
- Meshlets: `meshlet/visibility_buffer_resolve.wesl`'s `resolve_vertex_output` calls
  `frag_coord_to_ndc`.
- Clustered decals (`decal/clustered.wesl`): the cluster lookup.

### Untouched

These call nothing the fork changed: order-independent transparency, whose draw and resolve (in
`bevy_core_pipeline`) address their window-size buffers through `viewport.z`.
