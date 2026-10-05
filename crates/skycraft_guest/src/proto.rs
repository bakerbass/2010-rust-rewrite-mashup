//! Byte layout of `Local\SkyCraft_MW2_v1`, mirrored from SkyCraft's
//! `protocol/skycraft_mw2.h` (the source of truth). Change both together and
//! bump `VERSION`.

pub const MAGIC: u32 = 0x4732_574D; // "MW2G"
pub const VERSION: u32 = 1;
pub const MAPPING_NAME: &str = "Local\\SkyCraft_MW2_v1";

pub const OFF_HEADER: usize = 0x0;
pub const OFF_OVERLAY_CTL: usize = 0x300;
pub const OFF_OVERLAY_SLOT_HDR: usize = 0x340;
pub const OFF_OVERLAY_PIXELS: usize = 0x10000;
pub const MAX_OVERLAY_W: u32 = 3840;
pub const MAX_OVERLAY_H: u32 = 2160;
pub const OVERLAY_SLOT_BYTES: usize = MAX_OVERLAY_W as usize * MAX_OVERLAY_H as usize * 4;
pub const OVERLAY_SLOTS: usize = 3;
pub const MAPPING_BYTES: usize = OFF_OVERLAY_PIXELS + OVERLAY_SLOT_BYTES * OVERLAY_SLOTS;

// Header fields.
pub const HDR_MAGIC: usize = 0x00;
pub const HDR_VERSION: usize = 0x04;
pub const HDR_GUEST_PID: usize = 0x08;
pub const HDR_GUEST_HEARTBEAT: usize = 0x18;
pub const HDR_VIEWPORT_W: usize = 0x20;
pub const HDR_VIEWPORT_H: usize = 0x24;

// State (guest -> Minecraft, seqlock) at 0x200.
pub const OFF_STATE: usize = 0x200;
pub const ST_SEQ: usize = 0x00;
pub const ST_FLAGS: usize = 0x04;
pub const ST_WEAPON: usize = 0x08;
pub const ST_AMMO_CLIP: usize = 0x0C;
pub const ST_AMMO_STOCK: usize = 0x10;
pub const ST_ADS_FRACTION: usize = 0x14;
pub const ST_FOV_SCALE: usize = 0x18;
pub const ST_SPREAD: usize = 0x1C;
pub const ST_GUEST_FRAME: usize = 0x20;
pub const ST_READY: u32 = 1 << 0;

/// What the guest tells Minecraft each frame (`State` in the header).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub flags: u32,
    pub weapon: u32,
    pub ammo_clip: i32,
    pub ammo_stock: i32,
    pub ads_fraction: f32,
    pub fov_scale: f32,
    pub spread: f32,
}

// OverlayCtl fields.
pub const OC_STATE: usize = 0x0;
pub const OC_FRAMES_PUBLISHED: usize = 0x8;
pub const OVERLAY_DIRTY: u32 = 1 << 2;

// OverlaySlotHdr fields; one header is 0x40 bytes.
pub const SLOT_HDR_SIZE: usize = 0x40;
pub const SH_WIDTH: usize = 0x00;
pub const SH_HEIGHT: usize = 0x04;
pub const SH_FRAME_ID: usize = 0x10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_size_matches_header() {
        // kOffOverlayPixels + 3 slots of 3840 x 2160 RGBA.
        assert_eq!(MAPPING_BYTES, 0x10000 + 3 * 3840 * 2160 * 4);
        assert_eq!(OFF_OVERLAY_SLOT_HDR + OVERLAY_SLOTS * SLOT_HDR_SIZE, 0x400);
    }
}
