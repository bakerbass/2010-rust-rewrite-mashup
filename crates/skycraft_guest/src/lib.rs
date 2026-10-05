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

mod inventory;
mod link;
mod motion;
pub mod proto;
mod shoot;

use link::Link;
use motion::McLink;

/// The overlay's texture format. Bevy's window surface on Windows is BGRA, and
/// IW4L's passes are built for the surface format, so the target matches it;
/// the bytes are swizzled to RGBA on the way out.
const TARGET_FORMAT: TextureFormat = TextureFormat::Bgra8UnormSrgb;
/// Until Skyrim has said how big its back buffer is.
const FALLBACK_SIZE: (u32, u32) = (1920, 1080);
const CONNECT_RETRY_SECS: f32 = 0.5;
/// The longest the guest waits for Skyrim's next frame.
const PACE_LIMIT: std::time::Duration = std::time::Duration::from_millis(50);

pub struct SkycraftGuestPlugin;

impl Plugin for SkycraftGuestPlugin {
    fn build(&self, app: &mut App) {
        // Minecraft's hotbar picks among the Minecraft map's guns, so the
        // soldier carries them all.
        sim::carry_block_world_guns(true);
        sim::hand_shots_to_guest(true);
        stay_unthrottled();
        app.init_resource::<Guest>()
            .add_systems(First, pace_to_skyrim)
            .insert_non_send(inventory::GuestInventory::default())
            .init_resource::<frame::MinecraftUi>()
            .init_resource::<frame::skycraft::HudSurfaceSize>()
            .add_systems(Update, (connect, retarget_lens, hud_on_lens).chain())
            .add_systems(
                Update,
                (drive_soldier, inventory::sync_inventory, shoot::forward_shots)
                    .after(connect)
                    .in_set(frame::ClientSet::Reconcile),
            );
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
    /// When the frame rate was last logged, and the frame count then.
    fps_mark: Option<(std::time::Instant, u64)>,
    /// Skyrim's frame counter as of the last guest frame.
    host_frame: u32,
    /// Minecraft's player, from SkyCraft's main mapping.
    mc: Option<McLink>,
    /// Where the soldier spawned; it stays there (see `motion`).
    anchor: Option<[f32; 3]>,
    /// MW2 buttons the guest is holding down for Minecraft.
    held: std::collections::BTreeSet<&'static str>,
    state_frames: u64,
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

/// The HUD's Bevy UI (the scope overlay, the hip crosshair) on the lens. Left
/// to itself it goes on the default UI camera, which is one of the window's
/// (the loading screen's or the console's), and the window is never shown.
/// The whole HUD lays itself out at the image's size: the hidden window
/// doesn't keep it (display scaling, Windows' own say).
fn hud_on_lens(
    mut commands: Commands,
    guest: Res<Guest>,
    lenses: Query<Entity, With<GuestLens>>,
    roots: Query<(Entity, Option<&bevy::ui::UiTargetCamera>), With<hud::HudRoot>>,
    mut surface: ResMut<frame::skycraft::HudSurfaceSize>,
) {
    let size = guest.target.as_ref().map(|(_, size)| size.as_vec2());
    if surface.0 != size {
        surface.0 = size;
    }
    let Ok(lens) = lenses.single() else {
        return;
    };
    for (root, target) in &roots {
        if target.is_none_or(|t| t.entity() != lens) {
            commands.entity(root).insert(bevy::ui::UiTargetCamera(lens));
        }
    }
}

/// One frame of the lens image back from the GPU, handed to Skyrim as it is
/// (BGRA; Skyrim's texture takes it that way).
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
    guest.frames += 1;
    let frame_id = guest.frames;
    if let Some(link) = guest.link.as_mut() {
        link.publish_overlay(size.x, size.y, stride, &frame.data, proto::OVERLAY_BGRA, frame_id);
    }
    if frame_id == 1 || frame_id % 600 == 0 {
        let now = std::time::Instant::now();
        let fps = guest
            .fps_mark
            .map(|(at, frames)| (frame_id - frames) as f64 / now.duration_since(at).as_secs_f64().max(1e-3));
        guest.fps_mark = Some((now, frame_id));
        diag::info!(
            Launch,
            "SkyCraft: overlay frame {frame_id} published ({}x{}, stride {stride}{})",
            size.x,
            size.y,
            fps.map_or(String::new(), |fps| format!(", {fps:.0} fps"))
        );
    }
}

/// One guest frame per Skyrim frame, once the soldier is up: rendering
/// faster only takes the GPU from Skyrim and Minecraft. Waits at most
/// `PACE_LIMIT`, so a stalled Skyrim never stops the guest.
fn pace_to_skyrim(mut guest: ResMut<Guest>) {
    let Some(link) = guest.link.as_ref() else {
        return;
    };
    if guest.anchor.is_none() {
        return;
    }
    let start = std::time::Instant::now();
    let last = guest.host_frame;
    let mut now = link.host_frame();
    while now == last && start.elapsed() < PACE_LIMIT {
        std::thread::sleep(std::time::Duration::from_micros(500));
        now = link.host_frame();
    }
    guest.host_frame = now;
}

/// A hidden window's process is throttled by Windows a few seconds in (slower
/// CPU, coarse timers); the guest has frames to make on time.
#[cfg(windows)]
fn stay_unthrottled() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, SetProcessInformation,
    };
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        StateMask: 0,
    };
    // SAFETY: the current process's pseudo-handle and a correctly sized struct.
    let ok = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&raw const state).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if ok == 0 {
        diag::warn!(Launch, "SkyCraft: couldn't turn off power throttling");
    }
}

#[cfg(not(windows))]
fn stay_unthrottled() {}

/// Makes the soldier follow Minecraft's player: look, movement, sprint, and the
/// pinned spot with Minecraft's velocity (see `motion`).
fn drive_soldier(
    mut guest: ResMut<Guest>,
    local: Res<net::LocalPresentClient>,
    presented: Res<net::PresentedSnapshot>,
    authority: Option<ResMut<net::AuthorityWorld>>,
    mut look: ResMut<net::LookState>,
    mut actions: ResMut<net::ClientActionInput>,
    mut console: ResMut<console::ConsoleInputState>,
    (mut ui, surface): (ResMut<frame::MinecraftUi>, Res<frame::skycraft::HudSurfaceSize>),
    (kick, settings): (
        Option<Res<render_anim::occupancy::view_kick::SessionViewKick>>,
        Res<frame::GameSettings>,
    ),
) {
    if guest.link.is_none() {
        return;
    }
    if guest.mc.is_none() {
        guest.mc = McLink::open();
        if guest.mc.is_some() {
            diag::info!(Launch, "SkyCraft: following Minecraft's player");
        }
    }
    let pose = guest.mc.as_ref().and_then(McLink::read);
    let ps = presented.player(local.0).filter(|ps| ps.pm_type == 0);
    let ready = pose.is_some_and(|pose| pose.in_world()) && ps.is_some();
    guest.state_frames += 1;
    if let Some(link) = &guest.link {
        let ads = ps.map_or(0.0, |ps| ps.f_weapon_pos_frac);
        let state = proto::State {
            flags: if ready { proto::ST_READY } else { 0 } | if ads >= 0.99 { proto::ST_ADS } else { 0 },
            ads_fraction: ads,
            // MW2's ADS zoom, for Minecraft's camera (and so Skyrim's) to take.
            fov_scale: kick.as_ref().map_or(1.0, |kick| shoot::zoom(settings.fov, kick.horiz_fov_deg)),
            hotbar_top: hotbar_top(&ui, &surface),
            ..proto::State::default()
        };
        link.write_state(&state, guest.state_frames);
    }
    let (Some(pose), Some(ps), true) = (pose, ps, ready) else {
        return;
    };
    ui.third_person = pose.camera_mode != 0;
    let anchor = *guest.anchor.get_or_insert(ps.origin);
    if let Some(mut authority) = authority {
        authority.0.set_pinned_motion(
            local.0,
            Some(sim::PinnedMotion {
                origin: anchor,
                velocity: pose.iw4_velocity(),
                on_ground: pose.on_ground(),
            }),
        );
    }
    let [pitch, yaw, _] = pose.iw4_angles();
    look.angles = motion::angles_to_short([
        pitch - ps.delta_angles[0],
        yaw - ps.delta_angles[1],
        0.0,
    ]);
    actions.pad_move = pose.stick();
    // The gun's buttons, held in Minecraft while it holds one of MW2's guns.
    let (weapon, buttons) = guest
        .link
        .as_ref()
        .and_then(link::Link::read_control)
        .unwrap_or((proto::WEAPON_NONE, 0));
    let buttons = if weapon == proto::WEAPON_NONE { 0 } else { buttons };
    let firing = buttons & (proto::BTN_ATTACK | proto::BTN_ADS) != 0;
    hold(&mut console, &mut guest.held, "+attack", buttons & proto::BTN_ATTACK != 0);
    hold(&mut console, &mut guest.held, "+speed_throw", buttons & proto::BTN_ADS != 0);
    hold(&mut console, &mut guest.held, "+usereload", buttons & proto::BTN_RELOAD != 0);
    hold(&mut console, &mut guest.held, "+melee", buttons & proto::BTN_MELEE != 0);
    // MW2's one button for both: sprint at the hip (it doesn't fire or aim at
    // a sprint), hold the sniper's breath while aiming.
    let hold_breath = buttons & proto::BTN_ADS != 0 && buttons & proto::BTN_HOLD_BREATH != 0;
    hold(&mut console, &mut guest.held, "+breath_sprint", (pose.sprinting() && !firing) || hold_breath);
}

/// Where the hotbar starts, as a fraction of the overlay's height (0: none).
fn hotbar_top(ui: &frame::MinecraftUi, surface: &frame::skycraft::HudSurfaceSize) -> f32 {
    let height = surface.0.map_or(0.0, |size| size.y);
    match ui.hotbar_top {
        Some(top) if ui.active && height > 0.0 => (top / height).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// Presses or releases an MW2 button the way the console's `hold` does.
fn hold(
    console: &mut console::ConsoleInputState,
    held: &mut std::collections::BTreeSet<&'static str>,
    button: &'static str,
    down: bool,
) {
    if down && held.insert(button) {
        console.hold(button);
    } else if !down && held.remove(button) {
        console.release(button);
    }
}
