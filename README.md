# bevy_pbr for ExtractMeMaybe

`bevy_pbr` 0.20.1 as published on crates.io (upstream bevyengine/bevy `a0577b5c`,
`.cargo_vcs_info.json`), with five changes for
[ExtractMeMaybe](https://github.com/smashedbanan/ExtractMeMaybe): an app can supply the ambient
light per fragment, the main pass draws correctly at a smaller size under
`MainPassResolutionOverride`, as an upscaler such as FSR needs, an app can stop a point or spot
light at a plane, a point or spot light can cast from other layers than it lights, and a point or
spot light can sample a shadow map kept across frames beside its live one, which a hidden bake
light draws through Bevy's own shadow pass.

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

## The shadow caster layers

`ShadowCasterLayers(RenderLayers)` on a `PointLight` or `SpotLight`: its shadow views queue only
the meshes on these layers, in place of the light's own `RenderLayers`, which still pick the
cameras it lights and the meshes its views cull from (a caster must meet both). `extract_lights`
copies it to the light's render entity, removing it there when the light has none, and
`queue_shadows` reads it there in place of the shadow view's layers. `queue_shadows` tests a mesh
only when it queues it anew (newly visible to the view, or respecialized), so a light should get
its caster layers at spawn: a later change reaches a mesh only when that mesh is next queued.

## The kept shadows

`render/kept_shadows.rs`: `KeptShadowLayout`, a main-world resource, gives four size classes,
each `sizes[class]` texels square with `point_slots[class]` cube maps and `spot_slots[class]` 2D
maps. `KeptShadowPlugin` (added by `PbrPlugin`) extracts it, and the render-world resource
`KeptShadowTextures` holds per class a cube array and a 2D array (`CORE_3D_SHADOW_MAP_FORMAT`,
`RENDER_ATTACHMENT | TEXTURE_BINDING`), allocated outside `TextureCache` and anew only when the
layout's `generation` changes, so their contents last across frames. A new generation's textures
start zero, which is far: unshadowed. A class with no slots gets a one-slot dummy (six layers for a
cube array), as Bevy's own shadow textures do.

- `KeptShadow { class, slot, normal_bias }` on a point or spot light: `extract_lights` copies it
  to `ExtractedPointLight::kept_shadow` (`Changed<KeptShadow>` re-extracts the light), and
  `prepare_lights` writes `GpuClusteredLight::kept`, `slot | class << 24 | KEPT_SHADOW_BIT`
  (`1 << 31`), 0 without one, and `kept_normal_bias`, which stands in for the light's own normal
  bias on the kept map. `live_faces` is written 0 and read by no shader yet. The light grows from
  96 to 112 bytes, so `MAX_UNIFORM_BUFFER_CLUSTERABLE_OBJECTS` drops from 170 to 146.
- `mesh_view_bindings`: bindings 40 to 43, `kept_point_shadows_0` to `_3`
  (`texture_depth_cube_array`), and 44 to 47, `kept_spot_shadows_0` to `_3`
  (`texture_depth_2d_array`), by class, sampled with the comparison samplers Bevy's own shadow
  maps use (binding 3 for points, 6 for spots; the two are made alike).
- `render/shadow_sampling.wesl`: every sampling function takes the texture it samples, so the
  spot filters' texel size comes from the texture sampled, not from the sun's
  (`directional_shadow_textures`); a 128² kept spot filters over its own texels. The functions
  take array textures only, dropping the `NO_ARRAY_TEXTURES_SUPPORT` and
  `NO_CUBE_ARRAY_TEXTURES_SUPPORT` paths (WebGL 2, the iOS simulator), which the ambient hook's
  storage buffer already rules out for WebGL 2. `sample_shadow_map_hardware`, `sample_shadow_map`
  and `sample_shadow_cubemap` keep their signatures, as wrappers over the `_in` variants, for
  volumetric fog.
- `render/shadows.wesl`: `fetch_point_shadow` and `fetch_spot_shadow` are wrappers over `_in`
  variants that take the texture, layer and normal bias; `fetch_kept_point_shadow` and
  `fetch_kept_spot_shadow` call them on the light's class's kept texture at its slot with its
  kept normal bias.
- `render/pbr_functions.wesl`: a point or spot light's shadow is multiplied by its kept shadow
  when the receiver takes shadows and the light's `kept & KEPT_SHADOW_BIT` is set, whether or not
  the light has a live shadow (`SHADOWS_ENABLED`), at the transmitted light's shadow gates as at
  the direct ones. Every light field the kept fetch reads (position, depth bias, near plane, the
  cube projection terms or spot direction and tangent) is filled whether or not the light's
  shadow maps are enabled. Volumetric fog does not sample kept shadows.
- `KeptShadowBake { class, slot, faces }` on a point or spot light with `shadow_maps_enabled` makes
  it a bake light: `extract_lights` copies it to `ExtractedPointLight::kept_bake`
  (`Changed<KeptShadowBake>` re-extracts the light), and `prepare_lights` sorts bake lights right
  after the live shadowed lights of their kind and before the unshadowed ones, leaving them out of
  `point_light_shadow_maps_count` and `spot_light_shadow_maps_count`. So they take no layer in the
  live arrays and no `SHADOW_MAPS_ENABLED`, and the live lights keep their layers, while the point
  and spot view loops still reach them; `max_texture_cubes` does not limit them. The spot layer
  offset keeps every point light, bake lights included, as the shader indexes the whole sorted
  list. A bake light's views keep Bevy's visible entities and phases, but each takes its
  `ShadowView` attachment from its class's kept texture at layer `slot * 6 + face` (a spot's:
  `slot`), cleared on first use, from attachment maps of their own by class and layer, and its
  viewport at the class's size; a face whose bit in `faces` is clear has no `RootNonCameraView`, so
  Bevy draws it not at all. The views are rebuilt only when the light changes, as Bevy's are.
  `KeptShadowTextures` is prepared in `CreateViews` before `prepare_lights`, so a bake light that
  arrives with a new layout draws into the new textures; one alive across a new generation keeps
  drawing into the old ones until it changes.
- For tests: `GpuClusteredLights::values` and `GpuClusteredLight::shadow_maps_enabled` read the
  light buffer `prepare_lights` last wrote.

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
