//! MW2's guns firing into SkyCraft's world. The soldier's bullets are not
//! traced against the stand-in map (`sim::hand_shots_to_guest`); each one
//! goes to Minecraft, which traces it from its player's eye through its
//! blocks, Skyrim's walls and the stand-ins of Skyrim's NPCs, deals MW2's
//! damage and says what it hit, for MW2's hitmarker.
use bevy::prelude::*;
use frame::MinecraftUi;

use crate::proto;

/// Map units per Minecraft block (`motion::BLOCK`).
const BLOCK: f32 = 36.0;
/// How long MW2's hitmarker takes to fade (`_damagefeedback.gsc`: fadeOverTime(1)).
const HIT_MARKER_SECS: f32 = 1.0;
/// MW2's hit alert.
const HIT_ALERT: &str = gamemode_iw4::HIT_ALERT_ALIAS;

/// One of MW2's bullets as Minecraft's: map axes (x east, y north, z up) to
/// Minecraft's (x east, y up, z south), map units to blocks.
pub(crate) fn to_minecraft(shot: &sim::GuestShot) -> proto::Shot {
    let [x, y, z] = shot.direction;
    let length = (x * x + y * y + z * z).sqrt().max(1e-6);
    proto::Shot {
        // Pellets of one blast share its id's upper bits.
        id: (shot.shot_id << 5) | u32::from(shot.pellet & 0x1F),
        blast: shot.shot_id,
        weapon: shot.weapon,
        origin: [0.0; 3],
        direction: [x / length, z / length, -y / length],
        range: shot.range / BLOCK,
        damage: shot.damage,
        min_damage: shot.min_damage,
        damage_range: shot.max_damage_range / BLOCK,
        min_damage_range: shot.min_damage_range / BLOCK,
        head_multiplier: shot.head_scale,
        body_multiplier: shot.body_scale,
    }
}

/// Hands the soldier's bullets to Minecraft, and shows MW2's hitmarker for
/// what they hit there.
pub(crate) fn forward_shots(
    guest: Res<crate::Guest>,
    mut ui: ResMut<MinecraftUi>,
    mut sounds: MessageWriter<audio::AliasCommand>,
    time: Res<Time>,
) {
    let shots = sim::take_guest_shots();
    let Some(link) = guest.link.as_ref() else {
        return;
    };
    for shot in &shots {
        if !link.push_shot(&to_minecraft(shot)) {
            diag::warn!(Launch, "SkyCraft: Minecraft's shot ring is full; a bullet was dropped");
            break;
        }
    }
    let mut hit = false;
    while let Some(h) = link.pop_hit() {
        hit |= matches!(h.kind, proto::HIT_ACTOR | proto::HIT_KILL | proto::HIT_MOB);
    }
    if hit {
        ui.hit_marker = Some(0.0);
        sounds.write(audio::AliasCommand::Play(audio::PlayAlias {
            namespace: asset_core::AssetNamespace::Iw4,
            alias: HIT_ALERT.to_owned(),
            fallback: None,
            origin_inches: None,
            snd_ent: Some(audio::SND_ENT_LOCAL),
        }));
    } else if let Some(age) = ui.hit_marker.as_mut() {
        *age += time.delta_secs();
        if *age >= HIT_MARKER_SECS {
            ui.hit_marker = None;
        }
    }
}

/// How much MW2 has zoomed the view: the tangent of its half field of view
/// now over the tangent unzoomed. 1 at the hip.
pub(crate) fn zoom(base_fov_deg: f32, fov_deg: f32) -> f32 {
    if base_fov_deg <= 0.0 || fov_deg <= 0.0 {
        return 1.0;
    }
    let base = (base_fov_deg.to_radians() * 0.5).tan();
    let now = (fov_deg.to_radians() * 0.5).tan();
    (now / base).clamp(0.05, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(direction: [f32; 3]) -> sim::GuestShot {
        sim::GuestShot {
            shot_id: 3,
            pellet: 2,
            weapon: 9,
            origin: [0.0; 3],
            direction,
            range: 8192.0,
            damage: 98.0,
            min_damage: 80.0,
            max_damage_range: 1000.0,
            min_damage_range: 2000.0,
            head_scale: 1.5,
            body_scale: 1.0,
        }
    }

    #[test]
    fn north_is_minecraft_minus_z() {
        let mc = to_minecraft(&shot([0.0, 1.0, 0.0]));
        assert_eq!(mc.direction, [0.0, 0.0, -1.0]);
        let up = to_minecraft(&shot([0.0, 0.0, 2.0]));
        assert_eq!(up.direction, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn ranges_are_blocks_and_pellets_share_a_blast() {
        let mc = to_minecraft(&shot([1.0, 0.0, 0.0]));
        assert!((mc.range - 8192.0 / 36.0).abs() < 1e-3);
        assert!((mc.damage_range - 1000.0 / 36.0).abs() < 1e-3);
        assert_eq!((mc.blast, mc.id >> 5, mc.id & 0x1F), (3, 3, 2));
    }

    #[test]
    fn zoom_is_one_at_the_hip() {
        assert_eq!(zoom(65.0, 65.0), 1.0);
        let scoped = zoom(65.0, 20.0);
        assert!(scoped > 0.2 && scoped < 0.3, "{scoped}");
    }
}
