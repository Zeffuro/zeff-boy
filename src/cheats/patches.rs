use super::{CheatCode, CheatPatch, CheatValue};

pub(crate) fn collect_enabled_patches(
    user: &[CheatCode],
    libretro: &[CheatCode],
) -> Vec<CheatPatch> {
    user.iter()
        .chain(libretro.iter())
        .filter(|cheat| cheat.enabled)
        .flat_map(|cheat| {
            cheat.patches.iter().copied().map(|patch| {
                if patch.has_user_parameter() {
                    let value = cheat
                        .parameter_value
                        .or_else(|| patch.default_user_value())
                        .unwrap_or(0);
                    patch.resolve_user_parameter(value)
                } else {
                    patch
                }
            })
        })
        .collect()
}

pub(crate) fn enabled_patch_hash(user: &[CheatCode], libretro: &[CheatCode]) -> Option<[u8; 32]> {
    let patches = collect_enabled_patches(user, libretro);
    if patches.is_empty() {
        return None;
    }
    let mut bytes = Vec::with_capacity(patches.len() * 16);
    for patch in patches {
        encode_patch(&mut bytes, patch);
    }
    Some(zeff_firmware::sha256_bytes(&bytes))
}

fn encode_patch(out: &mut Vec<u8>, patch: CheatPatch) {
    match patch {
        CheatPatch::RamWrite { address, value } => {
            out.push(0);
            out.extend_from_slice(&address.to_le_bytes());
            encode_value(out, value);
        }
        CheatPatch::WideRamWrite { address, value } => {
            out.push(1);
            out.extend_from_slice(&address.to_le_bytes());
            encode_value(out, value);
        }
        CheatPatch::RomWrite { address, value } => {
            out.push(2);
            out.extend_from_slice(&address.to_le_bytes());
            encode_value(out, value);
        }
        CheatPatch::RomWriteIfEquals {
            address,
            value,
            compare,
        } => {
            out.push(3);
            out.extend_from_slice(&address.to_le_bytes());
            encode_value(out, value);
            encode_value(out, compare);
        }
        CheatPatch::RamWriteIfEquals {
            address,
            value,
            compare,
        } => {
            out.push(4);
            out.extend_from_slice(&address.to_le_bytes());
            encode_value(out, value);
            encode_value(out, compare);
        }
        CheatPatch::WideRamWriteIfEquals {
            address,
            value,
            compare,
        } => {
            out.push(5);
            out.extend_from_slice(&address.to_le_bytes());
            encode_value(out, value);
            encode_value(out, compare);
        }
    }
}

fn encode_value(out: &mut Vec<u8>, value: CheatValue) {
    match value {
        CheatValue::Constant(value) => {
            out.push(0);
            out.push(value);
        }
        CheatValue::PreserveWithCurrent { mask, base } => {
            out.push(1);
            out.push(mask);
            out.push(base);
        }
        CheatValue::UserParameterized { mask, base } => {
            out.push(2);
            out.push(mask);
            out.push(base);
        }
    }
}
