use crate::emu_backend::ActiveSystem;

pub(crate) fn rollback_session(system: ActiveSystem) -> bool {
    matches!(
        system,
        ActiveSystem::Nes
            | ActiveSystem::MasterSystem
            | ActiveSystem::Sg1000
            | ActiveSystem::Pce
            | ActiveSystem::WonderSwan
    )
}

pub(crate) fn linked_devices(system: ActiveSystem) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::link::remote_link_system_for_active_system(system).is_some()
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

pub(crate) fn session_mode(system: ActiveSystem) -> zeff_netplay_connect::protocol::SessionMode {
    use zeff_netplay_connect::protocol::SessionMode;
    if system == ActiveSystem::WonderSwan {
        SessionMode::LinkedDevices
    } else {
        SessionMode::SharedConsole
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn every_system_exposes_only_implemented_session_adapters() {
        let specs = crate::emu_backend::system_specs();
        assert_eq!(specs.len(), 9);
        for spec in specs {
            assert_eq!(
                rollback_session(spec.system),
                matches!(
                    spec.system,
                    ActiveSystem::Nes
                        | ActiveSystem::MasterSystem
                        | ActiveSystem::Sg1000
                        | ActiveSystem::Pce
                        | ActiveSystem::WonderSwan
                )
            );
            assert_eq!(
                linked_devices(spec.system),
                matches!(
                    spec.system,
                    ActiveSystem::GameBoy | ActiveSystem::WonderSwan
                )
            );
        }
    }
}
