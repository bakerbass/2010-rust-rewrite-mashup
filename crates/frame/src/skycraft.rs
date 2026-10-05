//! SkyCraft guest mode: IW4L runs beside Skyrim and Minecraft (SkyCraft) and
//! plays only MW2's guns. The map it loads stands in for weapon, body and HUD
//! content and is never drawn; the gun and the HUD go into shared memory for
//! Skyrim to composite, instead of a world into a window.
use std::sync::OnceLock;

/// Set to `1` by the SkyCraft plugin when it starts the guest.
pub const ENV: &str = "IW4L_SKYCRAFT";

/// Whether this process is a SkyCraft guest.
pub fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| std::env::var_os(ENV).is_some_and(|value| value == "1"))
}
