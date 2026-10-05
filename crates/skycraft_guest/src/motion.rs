//! The soldier follows Minecraft's player. Minecraft owns where the player is,
//! so the soldier is pinned (`sim::PinnedMotion`): MW2 still runs its move for
//! weapon handling, sway, bob and sprint, then stays at a fixed spot of the
//! stand-in map with Minecraft's velocity and ground contact. That spot is
//! where it spawned, so the gun keeps that spot's lighting.
//!
//! Minecraft's camera comes from `McState` in SkyCraft's main mapping
//! (`protocol/skycraft_protocol.h`), read only.
use std::sync::atomic::{AtomicU32, Ordering, fence};

/// Map units per Minecraft block, as the Minecraft map uses (`sim::voxel::BLOCK`):
/// the 70-unit soldier stands about two blocks tall, like Steve.
const BLOCK: f32 = 36.0;
/// Minecraft's walking speed, blocks per second: a full stick of movement.
const WALK_BLOCKS_PER_SEC: f32 = 4.317;
const ANGLE2SHORT: f32 = 65536.0 / 360.0;

const MAIN_MAPPING_NAME: &str = "Local\\SkyCraft_v1";
const MAIN_MAGIC: u32 = 0x4359_4B53; // "SKYC"
const MAIN_VERSION: u32 = 11;
/// Header and both state blocks; the rest of the main mapping is not ours.
const MAIN_VIEW_BYTES: usize = 0x300;
const OFF_MC: usize = 0x200;

// McState fields.
const MC_SEQ: usize = 0x00;
const MC_FLAGS: usize = 0x04;
const MC_YAW: usize = 0x20;
const MC_PITCH: usize = 0x24;
const MC_PREV: usize = 0x70;
const MC_CUR: usize = 0x88;
const MC_TICK_MS: usize = 0xB8;

const MC_IN_WORLD: u32 = 1 << 0;
const MC_ON_GROUND: u32 = 1 << 2;
const MC_SPRINTING: u32 = 1 << 4;

/// Minecraft's player for one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct McPose {
    pub flags: u32,
    pub yaw: f32,
    pub pitch: f32,
    /// Blocks per second, Minecraft axes.
    pub velocity: [f32; 3],
}

impl McPose {
    pub fn in_world(&self) -> bool {
        self.flags & MC_IN_WORLD != 0
    }

    pub fn on_ground(&self) -> bool {
        self.flags & MC_ON_GROUND != 0
    }

    pub fn sprinting(&self) -> bool {
        self.flags & MC_SPRINTING != 0
    }

    /// MW2 view angles (pitch, yaw, roll in degrees). Minecraft's yaw 0 faces
    /// south (+Z, map -Y) and turns clockwise; MW2's 0 faces east and turns
    /// counter-clockwise. Both pitch down positive.
    pub fn iw4_angles(&self) -> [f32; 3] {
        [self.pitch, -90.0 - self.yaw, 0.0]
    }

    /// Velocity in map units per second (map X east, Y north, Z up).
    pub fn iw4_velocity(&self) -> [f32; 3] {
        let [x, y, z] = self.velocity;
        [x * BLOCK, -z * BLOCK, y * BLOCK]
    }

    /// Forward and right stick (-1..1) for the move Minecraft's player is making,
    /// so MW2's walk, sprint and bob match it.
    pub fn stick(&self) -> [f32; 2] {
        let yaw = self.yaw.to_radians();
        let (vx, vz) = (self.velocity[0], self.velocity[2]);
        let forward = -yaw.sin() * vx + yaw.cos() * vz;
        let right = -yaw.cos() * vx - yaw.sin() * vz;
        [
            (forward / WALK_BLOCKS_PER_SEC).clamp(-1.0, 1.0),
            (right / WALK_BLOCKS_PER_SEC).clamp(-1.0, 1.0),
        ]
    }
}

/// Converts MW2 view angles in degrees to the usercmd's short angles.
pub fn angles_to_short(angles: [f32; 3]) -> [i32; 3] {
    angles.map(|a| (a * ANGLE2SHORT) as i32)
}

pub struct McLink {
    base: *const u8,
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// Read-only view of process-shared memory, read under McState's seqlock.
unsafe impl Send for McLink {}
unsafe impl Sync for McLink {}

impl McLink {
    #[cfg(windows)]
    pub fn open() -> Option<McLink> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{FILE_MAP_READ, MapViewOfFile, OpenFileMappingW};
        let name: Vec<u16> = MAIN_MAPPING_NAME.encode_utf16().chain(Some(0)).collect();
        // SAFETY: a NUL-terminated name; the handle and view are checked before use.
        unsafe {
            let handle = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
            if handle.is_null() {
                return None;
            }
            let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, MAIN_VIEW_BYTES);
            if view.Value.is_null() {
                CloseHandle(handle);
                return None;
            }
            let link = McLink {
                base: view.Value.cast_const().cast(),
                handle,
            };
            let magic = link.u32_at(0x0).load(Ordering::Acquire);
            let version = link.u32_at(0x4).load(Ordering::Acquire);
            if magic != MAIN_MAGIC || version != MAIN_VERSION {
                diag::warn!(
                    Launch,
                    "SkyCraft: {MAIN_MAPPING_NAME} has magic {magic:#x} version {version}, expected {MAIN_MAGIC:#x} version {MAIN_VERSION}"
                );
                return None;
            }
            Some(link)
        }
    }

    #[cfg(not(windows))]
    pub fn open() -> Option<McLink> {
        None
    }

    fn u32_at(&self, offset: usize) -> &AtomicU32 {
        // SAFETY: offsets lie inside the mapped view and are 4-aligned.
        unsafe { &*self.base.add(offset).cast::<AtomicU32>() }
    }

    fn f32_at(&self, offset: usize) -> f32 {
        f32::from_bits(self.u32_at(offset).load(Ordering::Relaxed))
    }

    fn f64_at(&self, offset: usize) -> f64 {
        // SAFETY: 8-aligned inside the view; torn reads are caught by the seqlock.
        unsafe { std::ptr::read_volatile(self.base.add(offset).cast::<f64>()) }
    }

    /// Minecraft's latest pose, or `None` while Minecraft is mid-write.
    pub fn read(&self) -> Option<McPose> {
        let seq = self.u32_at(OFF_MC + MC_SEQ);
        for _ in 0..4 {
            let before = seq.load(Ordering::Acquire);
            if before & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let at = |field: usize| OFF_MC + field;
            let flags = self.u32_at(at(MC_FLAGS)).load(Ordering::Relaxed);
            let yaw = self.f32_at(at(MC_YAW));
            let pitch = self.f32_at(at(MC_PITCH));
            let prev = [0, 8, 16].map(|o| self.f64_at(at(MC_PREV + o)));
            let cur = [0, 8, 16].map(|o| self.f64_at(at(MC_CUR + o)));
            let tick_ms = self.f32_at(at(MC_TICK_MS));
            fence(Ordering::Acquire);
            if seq.load(Ordering::Relaxed) != before {
                continue;
            }
            let per_sec = if tick_ms > 0.0 { 1000.0 / tick_ms } else { 20.0 };
            let velocity = [0, 1, 2].map(|i| ((cur[i] - prev[i]) as f32) * per_sec);
            return Some(McPose {
                flags,
                yaw,
                pitch,
                velocity,
            });
        }
        None
    }
}

#[cfg(windows)]
impl Drop for McLink {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{MEMORY_MAPPED_VIEW_ADDRESS, UnmapViewOfFile};
        // SAFETY: the view and handle were opened by `open` and are released once.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.base.cast_mut().cast(),
            });
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(yaw: f32, velocity: [f32; 3]) -> McPose {
        McPose {
            yaw,
            velocity,
            ..McPose::default()
        }
    }

    #[test]
    fn facing_south_is_map_south() {
        // Minecraft yaw 0 faces +Z, which is map -Y: MW2 yaw -90.
        assert_eq!(pose(0.0, [0.0; 3]).iw4_angles()[1], -90.0);
        // Facing west (-X) is MW2 yaw 180 (or -180).
        assert_eq!(pose(90.0, [0.0; 3]).iw4_angles()[1].rem_euclid(360.0), 180.0);
    }

    #[test]
    fn walking_where_you_look_is_full_forward() {
        let south = pose(0.0, [0.0, 0.0, WALK_BLOCKS_PER_SEC]).stick();
        assert!((south[0] - 1.0).abs() < 1e-5 && south[1].abs() < 1e-5);
        // Facing south, -X (west) is to the right.
        let strafe = pose(0.0, [-WALK_BLOCKS_PER_SEC, 0.0, 0.0]).stick();
        assert!(strafe[0].abs() < 1e-5 && (strafe[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn velocity_maps_like_the_voxel_world() {
        // MC +Z (south) is map -Y; MC +Y is map +Z.
        assert_eq!(pose(0.0, [1.0, 2.0, 3.0]).iw4_velocity(), [36.0, -108.0, 72.0]);
    }
}
