//! emm: shadow maps an app keeps across frames, in textures allocated once per layout rather than
//! per frame, which a point or spot light samples by size class and slot beside its live map.

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_render::{
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_resource::*,
    renderer::RenderDevice,
    texture::CachedTexture,
    Render, RenderApp, RenderSystems,
};

use crate::{prepare_lights, CORE_3D_SHADOW_MAP_FORMAT};

/// emm: the kept shadow textures: four size classes, each `sizes[class]` texels square, holding a
/// cube array of `point_slots[class]` cube maps and a 2D array of `spot_slots[class]` maps. A new
/// `generation` reallocates every kept texture, its contents lost: zero, which is far, until drawn.
#[derive(Resource, Clone, Default, Debug, PartialEq, ExtractResource)]
#[extract_app(RenderApp)]
pub struct KeptShadowLayout {
    pub generation: u32,
    pub sizes: [u32; 4],
    pub point_slots: [u32; 4],
    pub spot_slots: [u32; 4],
}

impl KeptShadowLayout {
    /// emm: whether `class` and `slot` name one of this layout's point slots, or its spot slots.
    pub fn holds(&self, point: bool, class: u8, slot: u32) -> bool {
        let slots = if point { self.point_slots } else { self.spot_slots };
        slots.get(usize::from(class)).is_some_and(|&slots| slot < slots)
    }
}

/// emm: on a point or spot light: it is lit only where both its live shadow, if it has one, and
/// the kept map at `slot` of size class `class` (`KeptShadowLayout`) let it through. `normal_bias`
/// stands in for the light's own on the kept map, already scaled for the class's texel size.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct KeptShadow {
    pub class: u8,
    pub slot: u32,
    pub normal_bias: f32,
}

/// emm: on a point or spot light with `shadow_maps_enabled`: a bake light, whose shadow views draw
/// into the kept map at `slot` of size class `class` in place of Bevy's live shadow maps, at the
/// class's size, and only the faces whose bits are set in `faces` (bit `i` for Bevy's cube face
/// `i`, looking along +X, −X, +Y, −Y, −Z and +Z; a spot's one view is face 0). It takes no live
/// shadow map layer and its own light samples none, so it is kept hidden at intensity 0.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct KeptShadowBake {
    pub class: u8,
    pub slot: u32,
    pub faces: u8,
}

/// emm: `ClusteredLight.kept`'s flag for a light with a [`KeptShadow`], over its class in bits 24
/// to 30 and its slot below; `KEPT_SHADOW_BIT` in `mesh_view_types.wesl`.
pub(crate) const KEPT_SHADOW_BIT: u32 = 1 << 31;

/// emm: the render world's kept shadow textures for `layout`, per size class a cube array for
/// points and a 2D array for spots, each `default_view` the view the mesh view binds (40 to 43 and
/// 44 to 47).
#[derive(Resource)]
pub struct KeptShadowTextures {
    pub layout: KeptShadowLayout,
    pub point: [CachedTexture; 4],
    pub spot: [CachedTexture; 4],
}

/// emm: extracts [`KeptShadowLayout`] and keeps [`KeptShadowTextures`] to it.
pub struct KeptShadowPlugin;

impl Plugin for KeptShadowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeptShadowLayout>()
            .add_plugins(ExtractResourcePlugin::<KeptShadowLayout>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(
            Render,
            // Before `prepare_lights`, which points bake lights' views at them.
            prepare_kept_shadow_textures
                .in_set(RenderSystems::CreateViews)
                .before(prepare_lights),
        );
    }
}

/// Allocates the kept textures anew when the layout changes, and on the first frame. A class with
/// no slots gets a one-slot 1×1 dummy, as Bevy's own shadow textures do.
fn prepare_kept_shadow_textures(
    mut commands: Commands,
    layout: Res<KeptShadowLayout>,
    textures: Option<Res<KeptShadowTextures>>,
    render_device: Res<RenderDevice>,
) {
    if textures.is_some_and(|textures| textures.layout == *layout) {
        return;
    }
    let texture = |size: u32, slots: u32, faces: u32, dimension, label: &str| {
        let size = if slots == 0 { 1 } else { size.max(1) };
        let texture = render_device.create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: slots.max(1) * faces,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: CORE_3D_SHADOW_MAP_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let default_view = texture.create_view(&TextureViewDescriptor {
            label: Some(label),
            dimension: Some(dimension),
            aspect: TextureAspect::DepthOnly,
            ..Default::default()
        });
        CachedTexture {
            texture,
            default_view,
        }
    };
    commands.insert_resource(KeptShadowTextures {
        layout: layout.clone(),
        point: core::array::from_fn(|class| {
            texture(
                layout.sizes[class],
                layout.point_slots[class],
                6,
                TextureViewDimension::CubeArray,
                &format!("kept_point_shadows_{class}"),
            )
        }),
        spot: core::array::from_fn(|class| {
            texture(
                layout.sizes[class],
                layout.spot_slots[class],
                1,
                TextureViewDimension::D2Array,
                &format!("kept_spot_shadows_{class}"),
            )
        }),
    });
}
