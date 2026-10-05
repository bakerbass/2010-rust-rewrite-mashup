//! The shared mapping Skyrim creates for the guest. Opened, never created: if
//! Skyrim (or `tools/fake_mw2_host.py`) isn't running, there is nothing to draw
//! for.
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::proto;

pub struct Link {
    base: *mut u8,
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    /// The writer's private overlay slot: Skyrim's front starts at 2, middle at 0.
    back: usize,
}

// The mapping is process-shared memory; every access goes through atomics or
// whole-slot copies the triple buffer hands over.
unsafe impl Send for Link {}
unsafe impl Sync for Link {}

impl Link {
    #[cfg(windows)]
    pub fn open() -> Option<Link> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{
            FILE_MAP_ALL_ACCESS, MapViewOfFile, OpenFileMappingW,
        };
        let name: Vec<u16> = proto::MAPPING_NAME.encode_utf16().chain(Some(0)).collect();
        // SAFETY: a NUL-terminated name; the handle and view are checked before use.
        unsafe {
            let handle = OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, name.as_ptr());
            if handle.is_null() {
                return None;
            }
            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, proto::MAPPING_BYTES);
            if view.Value.is_null() {
                CloseHandle(handle);
                return None;
            }
            let link = Link {
                base: view.Value.cast(),
                handle,
                back: 1,
            };
            if link.u32_at(proto::HDR_MAGIC).load(Ordering::Acquire) != proto::MAGIC
                || link.u32_at(proto::HDR_VERSION).load(Ordering::Acquire) != proto::VERSION
            {
                diag::warn!(
                    Launch,
                    "SkyCraft: {} has magic {:#x} version {}, expected {:#x} version {}",
                    proto::MAPPING_NAME,
                    link.u32_at(proto::HDR_MAGIC).load(Ordering::Acquire),
                    link.u32_at(proto::HDR_VERSION).load(Ordering::Acquire),
                    proto::MAGIC,
                    proto::VERSION
                );
                return None;
            }
            link.u32_at(proto::HDR_GUEST_PID)
                .store(std::process::id(), Ordering::Release);
            Some(link)
        }
    }

    #[cfg(not(windows))]
    pub fn open() -> Option<Link> {
        None
    }

    fn u32_at(&self, offset: usize) -> &AtomicU32 {
        // SAFETY: offsets come from `proto` and lie inside the mapped view, 4-aligned.
        unsafe { &*self.base.add(offset).cast::<AtomicU32>() }
    }

    fn u64_at(&self, offset: usize) -> &AtomicU64 {
        // SAFETY: as `u32_at`, 8-aligned.
        unsafe { &*self.base.add(offset).cast::<AtomicU64>() }
    }

    /// The size Skyrim wants the overlay at (its back buffer), clamped to the
    /// slot size. `None` until the host has written one.
    pub fn viewport(&self) -> Option<(u32, u32)> {
        let w = self.u32_at(proto::HDR_VIEWPORT_W).load(Ordering::Acquire);
        let h = self.u32_at(proto::HDR_VIEWPORT_H).load(Ordering::Acquire);
        (w > 0 && h > 0).then(|| (w.min(proto::MAX_OVERLAY_W), h.min(proto::MAX_OVERLAY_H)))
    }

    /// Writes `State` under its seqlock (odd while writing).
    pub fn write_state(&self, state: &proto::State, guest_frame: u64) {
        let at = |field: usize| proto::OFF_STATE + field;
        let seq = self.u32_at(at(proto::ST_SEQ));
        let start = seq.load(Ordering::Relaxed).wrapping_add(1) | 1;
        seq.store(start, Ordering::Relaxed);
        std::sync::atomic::fence(Ordering::Release);
        self.u32_at(at(proto::ST_FLAGS)).store(state.flags, Ordering::Relaxed);
        self.u32_at(at(proto::ST_WEAPON)).store(state.weapon, Ordering::Relaxed);
        self.u32_at(at(proto::ST_AMMO_CLIP)).store(state.ammo_clip as u32, Ordering::Relaxed);
        self.u32_at(at(proto::ST_AMMO_STOCK)).store(state.ammo_stock as u32, Ordering::Relaxed);
        for (field, value) in [
            (proto::ST_ADS_FRACTION, state.ads_fraction),
            (proto::ST_FOV_SCALE, state.fov_scale),
            (proto::ST_SPREAD, state.spread),
            (proto::ST_HOTBAR_TOP, state.hotbar_top),
        ] {
            self.u32_at(at(field)).store(value.to_bits(), Ordering::Relaxed);
        }
        self.u64_at(at(proto::ST_GUEST_FRAME)).store(guest_frame, Ordering::Relaxed);
        seq.store(start.wrapping_add(1), Ordering::Release);
    }

    /// Minecraft's inventory, or `None` while Minecraft is mid-write.
    pub fn read_ui(&self) -> Option<proto::UiSnapshot> {
        const BYTES: usize = proto::UI_STACKS + proto::UI_STACK_COUNT * proto::UI_STACK_BYTES;
        let seq = self.u32_at(proto::OFF_UI + proto::UI_SEQ);
        let mut raw = [0u8; BYTES];
        for _ in 0..4 {
            let before = seq.load(Ordering::Acquire);
            if before & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            // SAFETY: inside the mapped view; a torn copy is caught by the seqlock below.
            unsafe {
                std::ptr::copy_nonoverlapping(self.base.add(proto::OFF_UI), raw.as_mut_ptr(), BYTES);
            }
            std::sync::atomic::fence(Ordering::Acquire);
            if seq.load(Ordering::Relaxed) != before {
                continue;
            }
            let u32_le = |at: usize| u32::from_le_bytes(raw[at..at + 4].try_into().unwrap_or_default());
            let stacks = (0..proto::UI_STACK_COUNT)
                .map(|i| {
                    let at = proto::UI_STACKS + i * proto::UI_STACK_BYTES;
                    let id = &raw[at..at + proto::UI_ID_BYTES];
                    let len = id.iter().position(|&b| b == 0).unwrap_or(id.len());
                    if len == 0 {
                        return None;
                    }
                    let durability = f32::from_bits(u32_le(at + proto::US_DURABILITY));
                    Some(proto::UiStack {
                        id: String::from_utf8_lossy(&id[..len]).into_owned(),
                        weapon: u32_le(at + proto::US_WEAPON),
                        count: u32_le(at + proto::US_COUNT),
                        durability: (durability >= 0.0).then_some(durability),
                    })
                })
                .collect();
            return Some(proto::UiSnapshot {
                flags: u32_le(proto::UI_FLAGS),
                selected: (u32_le(proto::UI_SELECTED) as usize).min(8),
                gui_unit: f32::from_bits(u32_le(proto::UI_GUI_UNIT)),
                stacks,
            });
        }
        None
    }

    /// The next mouse or key event of Minecraft's inventory screen.
    pub fn pop_ui_input(&self) -> Option<proto::UiInput> {
        let ring = proto::OFF_UI_INPUT_RING;
        let head = self.u64_at(ring + proto::RING_HEAD).load(Ordering::Acquire);
        let mut tail = self.u64_at(ring + proto::RING_TAIL).load(Ordering::Relaxed);
        if tail >= head {
            return None;
        }
        if head - tail > proto::UI_INPUT_ENTRIES {
            tail = head - proto::UI_INPUT_ENTRIES;
        }
        let at = ring + proto::RING_DATA + (tail % proto::UI_INPUT_ENTRIES) as usize * proto::UI_INPUT_BYTES;
        // SAFETY: an entry inside the ring, published before `head` (acquired above).
        let input = unsafe {
            let u16_at = |o: usize| std::ptr::read_volatile(self.base.add(at + o).cast::<u16>());
            proto::UiInput {
                kind: u16_at(0),
                code: u16_at(2),
                mods: u16_at(4),
                x: f32::from_bits(self.u32_at(at + 8).load(Ordering::Relaxed)),
                y: f32::from_bits(self.u32_at(at + 12).load(Ordering::Relaxed)),
            }
        };
        self.u64_at(ring + proto::RING_TAIL).store(tail + 1, Ordering::Release);
        Some(input)
    }

    /// Hands Minecraft a click for its inventory. False if its ring is full.
    pub fn push_ui_click(&self, click: &proto::UiClick) -> bool {
        let ring = proto::OFF_UI_CLICK_RING;
        let head = self.u64_at(ring + proto::RING_HEAD).load(Ordering::Relaxed);
        let tail = self.u64_at(ring + proto::RING_TAIL).load(Ordering::Acquire);
        if head - tail >= proto::UI_CLICK_ENTRIES {
            return false;
        }
        let at = ring + proto::RING_DATA + (head % proto::UI_CLICK_ENTRIES) as usize * proto::UI_CLICK_BYTES;
        self.u32_at(at).store(click.kind, Ordering::Relaxed);
        self.u32_at(at + 4).store(click.slot, Ordering::Relaxed);
        self.u32_at(at + 8).store(click.flags, Ordering::Relaxed);
        self.u32_at(at + 12).store(click.hotbar, Ordering::Relaxed);
        self.u64_at(at + 16).store(click.spread_mask, Ordering::Relaxed);
        self.u64_at(ring + proto::RING_HEAD).store(head + 1, Ordering::Release);
        true
    }

    pub fn heartbeat(&self) {
        self.u64_at(proto::HDR_GUEST_HEARTBEAT)
            .store(tick_ms(), Ordering::Release);
    }

    /// Skyrim's frame counter.
    pub fn host_frame(&self) -> u32 {
        self.u32_at(proto::HDR_HOST_FRAME).load(Ordering::Acquire)
    }

    /// Copies one frame (RGBA8, or BGRA8 with `proto::OVERLAY_BGRA` in
    /// `flags`; rows `stride` bytes apart, top-down) into the private back
    /// slot and swaps it into the middle.
    pub fn publish_overlay(
        &mut self,
        width: u32,
        height: u32,
        stride: usize,
        rgba: &[u8],
        flags: u32,
        frame_id: u64,
    ) {
        let width = width.min(proto::MAX_OVERLAY_W);
        let height = height.min(proto::MAX_OVERLAY_H);
        let row = width as usize * 4;
        if stride < row || rgba.len() < stride * (height as usize - 1) + row {
            return;
        }
        let slot = proto::OFF_OVERLAY_PIXELS + self.back * proto::OVERLAY_SLOT_BYTES;
        // SAFETY: the back slot is ours alone until the swap below, and the copy
        // stays inside it (width and height are clamped to the slot size).
        unsafe {
            let dst = self.base.add(slot);
            if stride == row {
                std::ptr::copy_nonoverlapping(rgba.as_ptr(), dst, row * height as usize);
            } else {
                for y in 0..height as usize {
                    std::ptr::copy_nonoverlapping(
                        rgba.as_ptr().add(y * stride),
                        dst.add(y * row),
                        row,
                    );
                }
            }
        }
        let hdr = proto::OFF_OVERLAY_SLOT_HDR + self.back * proto::SLOT_HDR_SIZE;
        self.u32_at(hdr + proto::SH_FLAGS).store(flags, Ordering::Relaxed);
        self.u32_at(hdr + proto::SH_WIDTH)
            .store(width, Ordering::Relaxed);
        self.u32_at(hdr + proto::SH_HEIGHT)
            .store(height, Ordering::Relaxed);
        self.u64_at(hdr + proto::SH_FRAME_ID)
            .store(frame_id, Ordering::Relaxed);
        let old = self
            .u32_at(proto::OFF_OVERLAY_CTL + proto::OC_STATE)
            .swap(self.back as u32 | proto::OVERLAY_DIRTY, Ordering::AcqRel);
        self.back = (old & 3) as usize;
        self.u64_at(proto::OFF_OVERLAY_CTL + proto::OC_FRAMES_PUBLISHED)
            .fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(windows)]
impl Drop for Link {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{MEMORY_MAPPED_VIEW_ADDRESS, UnmapViewOfFile};
        // SAFETY: the view and handle were opened by `open` and are released once.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.base.cast(),
            });
            CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
fn tick_ms() -> u64 {
    // SAFETY: no arguments, no failure mode.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

#[cfg(not(windows))]
fn tick_ms() -> u64 {
    0
}
