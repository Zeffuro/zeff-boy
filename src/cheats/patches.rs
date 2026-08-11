use super::{CheatCode, CheatPatch};

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
