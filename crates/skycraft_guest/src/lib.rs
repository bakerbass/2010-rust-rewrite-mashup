//! SkyCraft guest: MW2's gun and HUD, rendered offscreen at Skyrim's size and
//! handed to Skyrim through `Local\SkyCraft_MW2_v1`.
//!
//! The map is loaded as usual (it supplies weapons, the soldier and the HUD)
//! but `render_gpu` draws none of its world while `frame::skycraft::active()`.
//! The world camera renders into an image with a transparent clear instead of
//! the window, and that image is read back every frame into the overlay
//! triple buffer.
use bevy::asset::RenderAssetUsages;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::window::PrimaryWindow;

mod link;
pub mod proto;

use link::Link;

/// The overlay's texture format. Bevy's window surface on Windows is BGRA, and
/// IW4L's passes are built for the surface format, so the target matches it;
/// the bytes are swizzled to RGBA on the way out.
const TARGET_FORMAT: TextureFormat = TextureFormat::Bgra8UnormSrgb;
/// Until Skyrim has said how big its back buffer is.
const FALLBACK_SIZE: (u32, u32) = (1920, 1080);
const CONNECT_RETRY_SECS: f32 = 0.5;

pub struct SkycraftGuestPlugin;

impl Plugin for SkycraftGuestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Guest>()
            .add_systems(Update, (connect, retarget_lens).chain());
    }
}

#[derive(Resource, Default)]
struct Guest {
    link: Option<Link>,
    retry: f32,
    /// The image the lens renders into, and its size.
    target: Option<(Handle<Image>, UVec2)>,
    readback: Option<Entity>,
    frames: u64,
    rgba: Vec<u8>,
}

/// The world camera, once it renders into the overlay image.
#[derive(Component)]
struct GuestLens;

fn connect(time: Res<Time>, mut guest: ResMut<Guest>) {
    if let Some(link) = &guest.link {
        link.heartbeat();
        return;
    }
    guest.retry -= time.delta_secs();
    if guest.retry > 0.0 {
        return;
    }
    guest.retry = CONNECT_RETRY_SECS;
    if let Some(link) = Link::open() {
        diag::info!(
            Launch,
            "SkyCraft: connected to {} (viewport {:?})",
            proto::MAPPING_NAME,
            link.viewport()
        );
        guest.link = Some(link);
    }
}

/// Points the world camera at the overlay image (re-made when Skyrim's size
/// changes) and keeps a readback on it.
#[allow(clippy::type_complexity)]
fn retarget_lens(
    mut commands: Commands,
    mut guest: ResMut<Guest>,
    mut images: ResMut<Assets<Image>>,
    mut lenses: Query<(Entity, &mut Camera, Has<GuestLens>), With<render_scene::FpvLens>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Some(link) = &guest.link else {
        return;
    };
    let size = UVec2::from(link.viewport().unwrap_or(FALLBACK_SIZE));
    let resized = guest.target.as_ref().is_some_and(|(_, held)| *held != size);
    if resized && let Some(readback) = guest.readback.take() {
        commands.entity(readback).despawn();
    }
    if resized || guest.target.is_none() {
        let mut image = Image::new_target_texture(size.x, size.y, TARGET_FORMAT, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        let handle = images.add(image);
        guest.target = Some((handle, size));
        // HUD layout follows the primary window's size; keep it the overlay's.
        for mut window in &mut windows {
            window.resolution.set_physical_resolution(size.x, size.y);
        }
        diag::info!(Launch, "SkyCraft: overlay target {}x{}", size.x, size.y);
    }
    let Some((handle, _)) = guest.target.clone() else {
        return;
    };
    for (entity, mut camera, retargeted) in &mut lenses {
        if retargeted && !resized {
            continue;
        }
        camera.clear_color = ClearColorConfig::Custom(Color::NONE);
        commands.entity(entity).insert((
            RenderTarget::from(handle.clone()),
            frame::UiCamera,
            GuestLens,
        ));
    }
    if guest.readback.is_none() {
        let readback = commands
            .spawn(Readback::texture(handle))
            .observe(publish_frame)
            .id();
        guest.readback = Some(readback);
    }
}

/// One frame of the lens image back from the GPU: swizzle BGRA to RGBA and
/// hand it to Skyrim.
fn publish_frame(frame: On<ReadbackComplete>, mut guest: ResMut<Guest>) {
    let Some((_, size)) = guest
        .target
        .as_ref()
        .map(|(handle, size)| (handle.id(), *size))
    else {
        return;
    };
    let row = size.x as usize * 4;
    let rows = size.y as usize;
    if rows == 0 || frame.data.len() < row * rows {
        return;
    }
    // Rows come back padded to the copy alignment unless already a multiple of it.
    let stride = frame.data.len() / rows;
    let guest = &mut *guest;
    guest.rgba.resize(stride * rows, 0);
    for (out, bgra) in guest
        .rgba
        .chunks_exact_mut(4)
        .zip(frame.data.chunks_exact(4))
    {
        out[0] = bgra[2];
        out[1] = bgra[1];
        out[2] = bgra[0];
        out[3] = bgra[3];
    }
    guest.frames += 1;
    let frame_id = guest.frames;
    if let Some(link) = guest.link.as_mut() {
        link.publish_overlay(size.x, size.y, stride, &guest.rgba, frame_id);
    }
    if frame_id == 1 || frame_id % 600 == 0 {
        diag::info!(
            Launch,
            "SkyCraft: overlay frame {frame_id} published ({}x{}, stride {stride})",
            size.x,
            size.y
        );
    }
}
