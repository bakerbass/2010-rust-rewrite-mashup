//! Byte layout of `Local\SkyCraft_MW2_v1`, mirrored from SkyCraft's
//! `protocol/skycraft_mw2.h` (the source of truth). Change both together and
//! bump `VERSION`.

pub const MAGIC: u32 = 0x4732_574D; // "MW2G"
pub const VERSION: u32 = 5;
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
/// Skyrim's frames, counted in its Present: the guest renders one per frame.
pub const HDR_HOST_FRAME: usize = 0x0C;
pub const HDR_GUEST_HEARTBEAT: usize = 0x18;
pub const HDR_VIEWPORT_W: usize = 0x20;
pub const HDR_VIEWPORT_H: usize = 0x24;

// Control (Minecraft -> guest, seqlock) at 0x100: the gun in Minecraft's hand.
pub const OFF_CONTROL: usize = 0x100;
pub const CT_SEQ: usize = 0x00;
pub const CT_WEAPON: usize = 0x04;
pub const CT_BUTTONS: usize = 0x08;
pub const BTN_ATTACK: u32 = 1 << 0;
pub const BTN_ADS: u32 = 1 << 1;
pub const BTN_RELOAD: u32 = 1 << 2;
pub const BTN_MELEE: u32 = 1 << 3;
pub const BTN_HOLD_BREATH: u32 = 1 << 7;
pub const WEAPON_NONE: u32 = 0;
/// Minecraft's gun items and the MW2 weapons they are.
pub const WEAPONS: [(u32, &str); 3] = [(1, "cheytac_mp"), (2, "spas12_mp"), (3, "ump45_mp")];

// SPSC rings: head (writer) at +0x00, tail (reader) at +0x40, entries from +0x80.
pub const RING_HEAD: usize = 0x00;
pub const RING_TAIL: usize = 0x40;
pub const RING_DATA: usize = 0x80;

// Shots (guest -> Minecraft ring) at 0x1000; hits (Minecraft -> guest ring) at 0x9000.
pub const OFF_SHOT_RING: usize = 0x1000;
pub const SHOT_ENTRIES: u64 = 256;
pub const SHOT_BYTES: usize = 0x50;
pub const OFF_HIT_RING: usize = 0x9000;
pub const HIT_ENTRIES: u64 = 256;
pub const HIT_BYTES: usize = 0x38;
pub const HIT_MISS: u32 = 0;
pub const HIT_WORLD: u32 = 1;
pub const HIT_BLOCK: u32 = 2;
pub const HIT_ACTOR: u32 = 3;
pub const HIT_KILL: u32 = 4;
pub const HIT_MOB: u32 = 5;

/// One bullet for Minecraft to trace (`Shot` in the header). Minecraft
/// coordinates and blocks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shot {
    pub id: u32,
    pub blast: u32,
    pub weapon: u32,
    pub origin: [f64; 3],
    pub direction: [f32; 3],
    pub range: f32,
    pub damage: f32,
    pub min_damage: f32,
    pub damage_range: f32,
    pub min_damage_range: f32,
    pub head_multiplier: f32,
    pub body_multiplier: f32,
}

/// What a shot hit, back from Minecraft (`Hit` in the header).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hit {
    pub shot_id: u32,
    pub kind: u32,
    pub point: [f64; 3],
    pub damage: f32,
    pub headshot: bool,
}

// The inventory (Minecraft -> guest, seqlock) at 0xD000. Stacks: 0..41 the
// vanilla inventory (0..9 hotbar, 9..36 main, 36..40 armour feet to head, 40
// offhand), 41..45 the crafting grid, 45 its result, 46 the cursor's.
pub const OFF_UI: usize = 0xD000;
pub const UI_SEQ: usize = 0x00;
pub const UI_FLAGS: usize = 0x04;
pub const UI_SELECTED: usize = 0x08;
pub const UI_GUI_UNIT: usize = 0x0C;
pub const UI_STACKS: usize = 0x20;
pub const UI_STACK_BYTES: usize = 0x40;
pub const UI_ID_BYTES: usize = 48;
pub const US_WEAPON: usize = 0x30;
pub const US_COUNT: usize = 0x34;
pub const US_DURABILITY: usize = 0x38;
pub const UI_INVENTORY_SLOTS: usize = 41;
pub const UI_CRAFTING: usize = 41;
pub const UI_RESULT: usize = 45;
pub const UI_CURSOR: usize = 46;
pub const UI_STACK_COUNT: usize = 47;
pub const UI_ACTIVE: u32 = 1 << 0;
pub const UI_INVENTORY_OPEN: u32 = 1 << 1;

// Inventory input (Minecraft -> guest ring) at 0xE000.
pub const OFF_UI_INPUT_RING: usize = 0xE000;
pub const UI_INPUT_ENTRIES: u64 = 128;
pub const UI_INPUT_BYTES: usize = 0x10;
pub const UI_IN_MOVE: u16 = 1;
pub const UI_IN_BUTTON_DOWN: u16 = 2;
pub const UI_IN_BUTTON_UP: u16 = 3;
pub const UI_IN_KEY_DOWN: u16 = 4;
pub const UI_IN_KEY_UP: u16 = 5;
pub const UI_MOD_SHIFT: u16 = 1 << 0;
pub const UI_MOD_CTRL: u16 = 1 << 1;

// Inventory clicks (guest -> Minecraft ring) at 0xF000.
pub const OFF_UI_CLICK_RING: usize = 0xF000;
pub const UI_CLICK_ENTRIES: u64 = 64;
pub const UI_CLICK_BYTES: usize = 0x20;
pub const UI_CLICK_SLOT: u32 = 1;
pub const UI_CLICK_OUTSIDE: u32 = 2;
pub const UI_CLICK_GATHER: u32 = 3;
pub const UI_CLICK_SWAP: u32 = 4;
pub const UI_CLICK_SPREAD: u32 = 5;
pub const UI_CLICK_RIGHT: u32 = 1 << 0;
pub const UI_CLICK_SHIFT: u32 = 1 << 1;

/// One stack of Minecraft's inventory.
#[derive(Clone, Debug, PartialEq)]
pub struct UiStack {
    pub id: String,
    /// One of `WEAPONS`, for a gun item.
    pub weapon: u32,
    pub count: u32,
    /// Fraction left of a damaged item.
    pub durability: Option<f32>,
}

/// Minecraft's inventory as of its last frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiSnapshot {
    pub flags: u32,
    pub selected: usize,
    /// One of Minecraft's GUI pixels, a fraction of the screen's height.
    pub gui_unit: f32,
    pub stacks: Vec<Option<UiStack>>,
}

/// A mouse or key event of Minecraft's inventory screen; x and y are 0..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiInput {
    pub kind: u16,
    pub code: u16,
    pub mods: u16,
    pub x: f32,
    pub y: f32,
}

/// A click for Minecraft to apply to its inventory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiClick {
    pub kind: u32,
    pub slot: u32,
    pub flags: u32,
    pub hotbar: u32,
    pub spread_mask: u64,
}

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
pub const ST_HOTBAR_TOP: usize = 0x28;
pub const ST_READY: u32 = 1 << 0;
pub const ST_ADS: u32 = 1 << 3;

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
    /// Top of the guest's hotbar, a fraction of the screen's height (0: not
    /// shown), so Minecraft can stack its health and hunger on it.
    pub hotbar_top: f32,
}

// OverlayCtl fields.
pub const OC_STATE: usize = 0x0;
pub const OC_FRAMES_PUBLISHED: usize = 0x8;
pub const OVERLAY_DIRTY: u32 = 1 << 2;
/// `SH_FLAGS`: the pixels are BGRA, as the lens renders them.
pub const OVERLAY_BGRA: u32 = 1 << 0;

// OverlaySlotHdr fields; one header is 0x40 bytes.
pub const SLOT_HDR_SIZE: usize = 0x40;
pub const SH_WIDTH: usize = 0x00;
pub const SH_HEIGHT: usize = 0x04;
pub const SH_FLAGS: usize = 0x08;
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

    #[test]
    fn rings_fit_where_the_header_puts_them() {
        assert!(OFF_SHOT_RING + RING_DATA + SHOT_ENTRIES as usize * SHOT_BYTES <= OFF_HIT_RING);
        assert!(OFF_HIT_RING + RING_DATA + HIT_ENTRIES as usize * HIT_BYTES <= OFF_UI);
    }

    #[test]
    fn inventory_regions_fit_before_the_pixels() {
        assert!(OFF_UI + UI_STACKS + UI_STACK_COUNT * UI_STACK_BYTES <= OFF_UI_INPUT_RING);
        assert!(OFF_UI_INPUT_RING + RING_DATA + UI_INPUT_ENTRIES as usize * UI_INPUT_BYTES <= OFF_UI_CLICK_RING);
        assert!(OFF_UI_CLICK_RING + RING_DATA + UI_CLICK_ENTRIES as usize * UI_CLICK_BYTES <= OFF_OVERLAY_PIXELS);
        assert_eq!(US_DURABILITY + 8, UI_STACK_BYTES);
    }
}
