//! Minecraft's inventory, drawn the Minecraft map's way: the real Minecraft
//! (SkyCraft's Fabric mod) owns the inventory and publishes it every frame;
//! the guest fills `frame::MinecraftUi` from it, so the HUD draws the
//! MW2-style hotbar and inventory screen, with the soldier standing in it,
//! and the hotbar selection raises the gun. While the screen is open,
//! Minecraft passes its mouse and number keys over, and the clicks the HUD
//! makes go back for Minecraft to apply with vanilla's container rules.
use std::collections::HashMap;

use bevy::prelude::*;
use frame::{McClick, McSlot, McStack, MinecraftUi};
use minecraft_terrain::pack::PackStack;

use crate::proto;

/// Seconds between looks for Minecraft's files while they aren't there.
const PACKS_RETRY_SECS: f32 = 5.0;

/// Main thread only: Minecraft's resource pack caches in a `RefCell`.
#[derive(Default)]
pub(crate) struct GuestInventory {
    icons: render_anim::ItemIconAtlas,
    packs: Option<PackStack>,
    packs_retry: f32,
    /// MW2 weapon index of each of Minecraft's gun items (`proto::WEAPONS`).
    weapons: HashMap<u32, u32>,
    /// What the guest is holding down for Minecraft's inventory screen.
    buttons: Vec<MouseButton>,
    keys: Vec<KeyCode>,
    /// The slot of the last click, which a double click gathers at.
    last_slot: Option<u32>,
    last: Option<proto::UiSnapshot>,
}

const DIGITS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

impl GuestInventory {
    /// Opens Minecraft's resource pack once its files are here.
    fn load_packs(&mut self, delta: f32) {
        if self.packs.is_none() {
            self.packs_retry -= delta;
            if self.packs_retry <= 0.0 {
                self.packs_retry = PACKS_RETRY_SECS;
                match assets::minecraft_map::root() {
                    Some(root) => match PackStack::open(vec![root.join("resourcepacks/local/minecraft-26.3")]) {
                        Ok(packs) => {
                            diag::info!(Launch, "SkyCraft: Minecraft item icons from {}", root.display());
                            self.packs = Some(packs);
                        }
                        Err(error) => diag::warn!(Launch, "SkyCraft: Minecraft's files: {error}"),
                    },
                    // The first time: fetch them from Mojang, as the Minecraft map does.
                    None => assets::minecraft_map::prepare(),
                }
            }
        }
    }

    fn press_button(&mut self, input: &mut ButtonInput<MouseButton>, button: MouseButton, down: bool) {
        if down && !self.buttons.contains(&button) {
            self.buttons.push(button);
            input.press(button);
        } else if !down && let Some(i) = self.buttons.iter().position(|b| *b == button) {
            self.buttons.swap_remove(i);
            input.release(button);
        }
    }

    fn press_key(&mut self, input: &mut ButtonInput<KeyCode>, key: KeyCode, down: bool) {
        if down && !self.keys.contains(&key) {
            self.keys.push(key);
            input.press(key);
        } else if !down && let Some(i) = self.keys.iter().position(|k| *k == key) {
            self.keys.swap_remove(i);
            input.release(key);
        }
    }

    fn release_all(&mut self, buttons: &mut ButtonInput<MouseButton>, keys: &mut ButtonInput<KeyCode>) {
        for button in self.buttons.drain(..) {
            buttons.release(button);
        }
        for key in self.keys.drain(..) {
            keys.release(key);
        }
    }

    /// The HUD's stack for one of Minecraft's: a gun is `iw4:weapon/<index>`,
    /// as on the Minecraft map, so it draws MW2's icon.
    fn stack(&self, stack: &proto::UiStack) -> McStack {
        let weapon = self.weapons.get(&stack.weapon).copied();
        McStack {
            id: weapon.map_or_else(|| stack.id.clone(), |w| format!("iw4:weapon/{w}")),
            count: stack.count.min(u32::from(u8::MAX)) as u8,
            weapon,
            durability: stack.durability,
        }
    }

    /// One of the HUD's clicks as Minecraft's.
    fn click(&mut self, click: McClick) -> Option<proto::UiClick> {
        let index = |slot: McSlot| match slot {
            McSlot::Inventory(i) => i as u32,
            McSlot::Crafting(i) => (proto::UI_CRAFTING + i) as u32,
            McSlot::Result => proto::UI_RESULT as u32,
        };
        let right = |right: bool| if right { proto::UI_CLICK_RIGHT } else { 0 };
        Some(match click {
            McClick::Slot { slot, right: r, shift } => {
                self.last_slot = Some(index(slot));
                proto::UiClick {
                    kind: proto::UI_CLICK_SLOT,
                    slot: index(slot),
                    flags: right(r) | if shift { proto::UI_CLICK_SHIFT } else { 0 },
                    ..default()
                }
            }
            McClick::Outside { right: r } => proto::UiClick { kind: proto::UI_CLICK_OUTSIDE, flags: right(r), ..default() },
            McClick::Gather { right: r } => proto::UiClick {
                kind: proto::UI_CLICK_GATHER,
                slot: self.last_slot?,
                flags: right(r),
                ..default()
            },
            McClick::Swap { slot, hotbar } => proto::UiClick {
                kind: proto::UI_CLICK_SWAP,
                slot: index(slot),
                hotbar: hotbar as u32,
                ..default()
            },
            McClick::Spread { slots, right: r } => proto::UiClick {
                kind: proto::UI_CLICK_SPREAD,
                flags: right(r),
                spread_mask: slots
                    .iter()
                    .filter(|&&i| i < proto::UI_INVENTORY_SLOTS)
                    .fold(0u64, |mask, &i| mask | (1 << i)),
                ..default()
            },
            // Minecraft's own screen closing puts the cursor and crafting grid away.
            McClick::Close => return None,
        })
    }
}

/// Fills the HUD's Minecraft inventory from the real Minecraft's, passes on
/// the clicks the HUD made last frame, and replays Minecraft's mouse and
/// number keys into the inventory screen.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_inventory(
    guest: Res<crate::Guest>,
    mut inventory: NonSendMut<GuestInventory>,
    mut ui: ResMut<MinecraftUi>,
    weapons: Option<Res<assets::PreparedWeapons>>,
    (local, presented): (Res<net::LocalPresentClient>, Res<net::PresentedSnapshot>),
    (mut images, mut buttons, mut keys): (ResMut<Assets<Image>>, ResMut<ButtonInput<MouseButton>>, ResMut<ButtonInput<KeyCode>>),
    surface: Res<frame::skycraft::HudSurfaceSize>,
    time: Res<Time>,
) {
    let inventory = &mut *inventory;
    let Some(link) = guest.link.as_ref() else {
        return;
    };
    if inventory.weapons.is_empty()
        && let Some(weapons) = weapons.as_ref()
    {
        for (item, name) in proto::WEAPONS {
            if let Ok(Some(index)) = weapons.0.resolve_index(name) {
                inventory.weapons.insert(item, index);
            }
        }
    }
    for click in std::mem::take(&mut ui.clicks) {
        if let Some(click) = inventory.click(click) {
            link.push_ui_click(&click);
        }
    }
    // Minecraft picks and drops on its own hotbar.
    ui.select = None;
    ui.drop_selected = None;

    if let Some(snapshot) = link.read_ui() {
        inventory.last = Some(snapshot);
    }
    let alive = presented.player(local.0).is_some_and(|ps| ps.pm_type == 0);
    let snapshot = inventory.last.clone().unwrap_or_default();
    let active = alive && snapshot.flags & proto::UI_ACTIVE != 0 && snapshot.stacks.len() == proto::UI_STACK_COUNT;
    if !active {
        ui.active = false;
        ui.inventory_open = false;
        ui.pointer = None;
        ui.weapon_request = None;
        inventory.release_all(&mut buttons, &mut keys);
        while link.pop_ui_input().is_some() {}
        return;
    }
    ui.active = true;

    // The stacks, their icons and names.
    let stacks: Vec<Option<McStack>> = snapshot.stacks.iter().map(|s| s.as_ref().map(|s| inventory.stack(s))).collect();
    ui.slots = stacks[..proto::UI_INVENTORY_SLOTS].to_vec();
    ui.crafting = std::array::from_fn(|i| stacks[proto::UI_CRAFTING + i].clone());
    ui.result = stacks[proto::UI_RESULT].clone();
    ui.cursor = stacks[proto::UI_CURSOR].clone();
    let selected = ui.slots.get(snapshot.selected).cloned().flatten();
    inventory.load_packs(time.delta_secs());
    if let Some(packs) = inventory.packs.as_ref() {
        let ids: Vec<&str> = stacks.iter().flatten().map(|s| s.id.as_str()).collect();
        inventory.icons.publish(&mut ui, ids, packs, &mut images);
        if ui.selected != snapshot.selected {
            ui.selected_name = selected
                .as_ref()
                .filter(|s| s.weapon.is_none())
                .map(|s| (inventory.icons.item_name(packs, &s.id), 0.0));
        }
    }
    ui.selected = snapshot.selected;
    // The hotbar picks the gun; anything else shows Minecraft's own hand, and
    // an empty hand MW2's bare hands.
    ui.weapon_request = selected.as_ref().and_then(|s| s.weapon);
    ui.holding_item = ui.weapon_request.is_none();
    ui.empty_hand = selected.is_none();

    let size = surface.0.unwrap_or(Vec2::ONE);
    // The hotbar matches Minecraft's own, which its health and hunger sit on.
    ui.gui_unit = (snapshot.gui_unit > 0.0 && snapshot.gui_unit < 0.1).then(|| snapshot.gui_unit * size.y);

    // The inventory screen, and Minecraft's mouse and keys on it.
    let open = snapshot.flags & proto::UI_INVENTORY_OPEN != 0;
    if ui.inventory_open && !open {
        inventory.release_all(&mut buttons, &mut keys);
    }
    ui.inventory_open = open;
    if !open {
        ui.pointer = None;
        while link.pop_ui_input().is_some() {}
        return;
    }
    // Up to one press or release a frame, so a quick click still lands as a
    // press, then a release.
    while let Some(input) = link.pop_ui_input() {
        ui.pointer = Some(Vec2::new(input.x, input.y) * size);
        inventory.press_key(&mut keys, KeyCode::ShiftLeft, input.mods & proto::UI_MOD_SHIFT != 0);
        inventory.press_key(&mut keys, KeyCode::ControlLeft, input.mods & proto::UI_MOD_CTRL != 0);
        let button = if input.code == 1 { MouseButton::Right } else { MouseButton::Left };
        let digit = DIGITS.get(usize::from(input.code).wrapping_sub(1)).copied();
        match (input.kind, digit) {
            (proto::UI_IN_BUTTON_DOWN, _) => inventory.press_button(&mut buttons, button, true),
            (proto::UI_IN_BUTTON_UP, _) => inventory.press_button(&mut buttons, button, false),
            (proto::UI_IN_KEY_DOWN, Some(key)) => inventory.press_key(&mut keys, key, true),
            (proto::UI_IN_KEY_UP, Some(key)) => inventory.press_key(&mut keys, key, false),
            _ => continue,
        }
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_marks_inventory_slots_only() {
        let mut inventory = GuestInventory::default();
        let click = inventory
            .click(McClick::Spread { slots: vec![0, 3, 40, 41], right: true })
            .unwrap_or_default();
        assert_eq!(click.kind, proto::UI_CLICK_SPREAD);
        assert_eq!(click.spread_mask, 1 | 1 << 3 | 1 << 40);
        assert_eq!(click.flags, proto::UI_CLICK_RIGHT);
    }

    #[test]
    fn gather_lands_where_the_last_click_was() {
        let mut inventory = GuestInventory::default();
        assert!(inventory.click(McClick::Gather { right: false }).is_none());
        inventory.click(McClick::Slot { slot: McSlot::Crafting(2), right: false, shift: false });
        let gather = inventory.click(McClick::Gather { right: false }).unwrap_or_default();
        assert_eq!((gather.kind, gather.slot), (proto::UI_CLICK_GATHER, 43));
    }

    #[test]
    fn guns_draw_as_mw2_weapons() {
        let mut inventory = GuestInventory::default();
        inventory.weapons.insert(1, 57);
        let gun = inventory.stack(&proto::UiStack { id: "skycraft:mw2_intervention".into(), weapon: 1, count: 1, durability: None });
        assert_eq!((gun.id.as_str(), gun.weapon), ("iw4:weapon/57", Some(57)));
        let dirt = inventory.stack(&proto::UiStack { id: "minecraft:dirt".into(), weapon: 0, count: 300, durability: None });
        assert_eq!((dirt.id.as_str(), dirt.weapon, dirt.count), ("minecraft:dirt", None, 255));
    }
}
