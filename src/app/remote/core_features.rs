use serde_json::{Value, json};
use zeff_emu_common::system::CoreFamily;

use super::{memory_region_json, save_ram_kind_json};
use crate::emu_backend::CoreCapabilities;

pub(super) fn core_features_json(features: &CoreCapabilities) -> Value {
    json!({
        "core_family": core_family_label(features.core_family),
        "save_ram": save_ram_kind_json(features.save_ram_kind),
        "has_battery": features.has_battery,
        "system_ram_len": features.system_ram_len,
        "video_ram_len": features.video_ram_len,
        "memory_regions": features.memory_regions.iter().map(memory_region_json).collect::<Vec<_>>(),
        "input": input_features_json(&features.input_features),
        "cheats": cheat_features_json(&features.cheat_features),
        "supports_save_states": features.supports_save_states,
        "supports_state_capture": features.supports_state_capture,
        "supports_rewind": features.supports_rewind,
        "supports_replay": features.supports_replay,
        "supports_audio": features.supports_audio,
        "supports_cheats": features.supports_cheats,
        "supports_guest_calls": features.supports_guest_calls,
        "supports_debugger": features.supports_debugger,
        "supports_execution_controls": features.supports_execution_controls,
        "supports_opcode_history": features.supports_opcode_history,
        "tas_execution_primitives": tas_execution_primitives_json(&features.tas_execution_primitives),
    })
}

fn tas_execution_primitives_json(
    primitives: &crate::emu_backend::capabilities::TasCoreCapabilityProbe,
) -> Value {
    json!({
        "system_identity_observed": primitives.system_identity_observed,
        "source_media_identity": primitives.source_media_identity.map(|identity| json!({
            "sha256": crate::tas_project::TasDigest(identity.sha256).to_hex(),
            "byte_len": identity.byte_len,
        })),
        "source_media_identity_observed": primitives.source_media_identity_observed,
        "effective_media_identity_observed": primitives.effective_media_identity_observed,
        "firmware_identity_observed": primitives.firmware_identity_observed,
        "direct_runtime_profile_requirements_match": primitives.direct_runtime_profile_requirements_match,
        "supports_state_restore": primitives.supports_state_restore,
        "persistent_state": persistent_state_json(primitives.persistent_state),
        "input_model": match primitives.input_model {
            crate::emu_backend::capabilities::TasInputModel::StandardDigitalPads { max_players } => {
                json!({ "kind": "standard_digital_pads", "max_players": max_players })
            }
            crate::emu_backend::capabilities::TasInputModel::GameBoyJoypad => {
                json!({ "kind": "game_boy_joypad" })
            }
            crate::emu_backend::capabilities::TasInputModel::ColecoStandardController { max_players } => {
                json!({ "kind": "coleco_standard_controller", "max_players": max_players })
            }
            crate::emu_backend::capabilities::TasInputModel::WonderSwanDirectButtons => {
                json!({ "kind": "wonderswan_direct_buttons" })
            }
        },
    })
}

fn persistent_state_json(
    persistent_state: crate::emu_backend::capabilities::TasPersistentStateIdentity,
) -> Value {
    match persistent_state {
        crate::emu_backend::capabilities::TasPersistentStateIdentity::Absent => {
            json!({ "kind": "absent" })
        }
        crate::emu_backend::capabilities::TasPersistentStateIdentity::VolatileOnly { size } => {
            json!({ "kind": "volatile_only", "size": size })
        }
        crate::emu_backend::capabilities::TasPersistentStateIdentity::ExternalPersistent {
            size,
        } => {
            json!({ "kind": "external_persistent", "size": size })
        }
        crate::emu_backend::capabilities::TasPersistentStateIdentity::Unknown { size } => {
            json!({ "kind": "unknown", "size": size })
        }
    }
}

fn input_features_json(features: &crate::emu_backend::InputCapabilities) -> Value {
    json!({
        "buttons": features.buttons.iter().map(|button| button.label()).collect::<Vec<_>>(),
        "supports_player_two": features.max_players >= 2,
        "max_players": features.max_players,
        "supports_lightgun": features.supports_lightgun,
        "supports_wonderswan_direct_buttons": features.supports_wonderswan_direct_buttons,
    })
}

fn cheat_features_json(features: &crate::emu_backend::CheatCapabilities) -> Value {
    json!({
        "supports_user_cheats": features.supports_user_cheats,
        "supports_libretro_database": features.supports_libretro_database,
        "supports_ram_writes": features.supports_ram_writes,
        "supports_rom_patches": features.supports_rom_patches,
        "formats": features.formats,
    })
}

fn core_family_label(family: CoreFamily) -> &'static str {
    match family {
        CoreFamily::GameBoy => "game_boy",
        CoreFamily::GameBoyAdvance => "game_boy_advance",
        CoreFamily::Nes => "nes",
        CoreFamily::ColecoVision => "coleco_vision",
        CoreFamily::PcEngine => "pc_engine",
        CoreFamily::WonderSwan => "wonder_swan",
        CoreFamily::Sega8 => "sega8",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{persistent_state_json, tas_execution_primitives_json};
    use crate::emu_backend::capabilities::{
        TasCoreCapabilityProbe, TasInputModel, TasPersistentStateIdentity, TasSourceMediaIdentity,
    };

    #[test]
    fn tas_persistent_state_json_reports_each_identity_class() {
        assert_eq!(
            persistent_state_json(TasPersistentStateIdentity::Absent),
            json!({ "kind": "absent" })
        );
        assert_eq!(
            persistent_state_json(TasPersistentStateIdentity::VolatileOnly { size: 0x2000 }),
            json!({ "kind": "volatile_only", "size": 0x2000 })
        );
        assert_eq!(
            persistent_state_json(TasPersistentStateIdentity::ExternalPersistent { size: 0x8000 }),
            json!({ "kind": "external_persistent", "size": 0x8000 })
        );
        assert_eq!(
            persistent_state_json(TasPersistentStateIdentity::Unknown { size: 0x8000 }),
            json!({ "kind": "unknown", "size": 0x8000 })
        );
    }

    #[test]
    fn tas_core_feature_status_serializes_loader_source_identity() {
        let json = tas_execution_primitives_json(&TasCoreCapabilityProbe {
            system_identity_observed: true,
            source_media_identity: Some(TasSourceMediaIdentity::new([0x5A; 32], 0x2000)),
            source_media_identity_observed: true,
            effective_media_identity_observed: true,
            firmware_identity_observed: true,
            direct_runtime_profile_requirements_match: true,
            supports_state_restore: true,
            persistent_state: TasPersistentStateIdentity::Absent,
            input_model: TasInputModel::ColecoStandardController { max_players: 2 },
        });

        assert_eq!(
            json["source_media_identity"],
            json!({ "sha256": "5a".repeat(32), "byte_len": 0x2000 })
        );
        assert_eq!(json["source_media_identity_observed"], true);
        assert_eq!(json["direct_runtime_profile_requirements_match"], true);
    }
}
