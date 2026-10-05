//! SkyCraft guest only: rewrites the finished frame's alpha from the scene
//! depth, so the gun, hands and effects can be composited over Skyrim
//! (`skycraft_alpha.wgsl` says how). Runs last in post-processing, before the
//! HUD and Bevy's copy to the camera's target; the HUD then blends onto the
//! transparent pixels with its own alpha.
use crate::drawsurf::scene_depth::SceneDepthTexture;
use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::prelude::*;
use bevy::render::RenderStartup;
use bevy::render::render_resource::binding_types::{texture_2d, texture_depth_2d};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, FragmentState, LoadOp, Operations, PipelineCache,
    PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
    ShaderStages, StoreOp, TextureFormat, TextureSampleType, VertexState,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderSystems};

use super::postfx::PostFxSet;

const SHADER_PATH: &str = "embedded://render_gpu/drawsurf/skycraft_alpha.wgsl";

#[derive(Resource)]
struct SkycraftAlpha {
    layout: BindGroupLayoutDescriptor,
    pipeline: Option<CachedRenderPipelineId>,
    shader: Handle<Shader>,
}

pub(super) fn register(app: &mut App) {
    if !frame::skycraft::active() {
        return;
    }
    bevy::asset::embedded_asset!(app, "skycraft_alpha.wgsl");
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .add_systems(RenderStartup, init)
        .add_systems(Render, queue_pipeline.in_set(RenderSystems::Prepare))
        .add_systems(
            Core3d,
            draw.in_set(Core3dSystems::PostProcess).after(PostFxSet),
        );
}

fn init(mut commands: Commands, asset_server: Res<AssetServer>) {
    let layout = BindGroupLayoutDescriptor::new(
        "skycraft_alpha",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_depth_2d(),
            ),
        ),
    );
    commands.insert_resource(SkycraftAlpha {
        layout,
        pipeline: None,
        shader: asset_server.load(SHADER_PATH),
    });
}

fn queue_pipeline(gpu: Option<ResMut<SkycraftAlpha>>, cache: Res<PipelineCache>) {
    let Some(mut gpu) = gpu else {
        return;
    };
    if gpu.pipeline.is_some() {
        return;
    }
    let descriptor = RenderPipelineDescriptor {
        label: Some("skycraft_alpha".into()),
        layout: vec![gpu.layout.clone()],
        vertex: VertexState {
            shader: gpu.shader.clone(),
            entry_point: Some("vs_main".into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader: gpu.shader.clone(),
            entry_point: Some("fs_main".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        primitive: PrimitiveState::default(),
        ..default()
    };
    gpu.pipeline = Some(cache.queue_render_pipeline(descriptor));
}

fn draw(
    view: ViewQuery<(&ViewTarget, &ExtractedView, &SceneDepthTexture)>,
    gpu: Option<Res<SkycraftAlpha>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut context: RenderContext,
) {
    let Some(gpu) = gpu else {
        return;
    };
    let Some(pipeline) = gpu.pipeline.and_then(|id| cache.get_render_pipeline(id)) else {
        return;
    };
    let (target, extracted, depth) = view.into_inner();
    if extracted.viewport.z == 0
        || extracted.viewport.w == 0
        || target.main_texture_format() != TextureFormat::Rgba8Unorm
    {
        return;
    }
    let post = target.post_process_write();
    let bind = device.create_bind_group(
        Some("skycraft_alpha"),
        &cache.get_bind_group_layout(&gpu.layout),
        &BindGroupEntries::sequential((post.source, depth.view())),
    );
    let encoder = context.command_encoder();
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("skycraft_alpha"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post.destination,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::NONE.into()),
                store: StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind, &[]);
    pass.draw(0..3, 0..1);
}
