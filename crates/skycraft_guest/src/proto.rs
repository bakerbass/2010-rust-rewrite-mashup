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
