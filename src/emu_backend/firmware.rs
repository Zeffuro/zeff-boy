use super::ActiveSystem;

pub(crate) fn firmware_plan_for_active_system(
    system: ActiveSystem,
) -> Vec<zeff_firmware::FirmwareRequest> {
    use zeff_firmware::ExistingCoreSystem;

    zeff_firmware::firmware_plan_for_existing_core(match system {
        ActiveSystem::GameBoy => ExistingCoreSystem::GameBoy,
        ActiveSystem::GameBoyAdvance => ExistingCoreSystem::GameBoyAdvance,
        ActiveSystem::Nes => ExistingCoreSystem::Nes,
        ActiveSystem::WonderSwan => ExistingCoreSystem::WonderSwan,
        ActiveSystem::MasterSystem | ActiveSystem::Sg1000 => ExistingCoreSystem::MasterSystem,
        ActiveSystem::GameGear => ExistingCoreSystem::GameGear,
    })
}
