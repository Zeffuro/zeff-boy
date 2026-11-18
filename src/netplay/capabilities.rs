use crate::emu_backend::ActiveSystem;

pub(crate) fn rollback_session(system: ActiveSystem) -> bool {
    matches!(
        system,
        ActiveSystem::Nes
            | ActiveSystem::Md
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
        let expected = [
            (ActiveSystem::GameBoy, false, true),
            (ActiveSystem::GameBoyAdvance, false, false),
            (ActiveSystem::Nes, true, false),
            (ActiveSystem::Coleco, false, false),
            (ActiveSystem::Pce, true, false),
            (ActiveSystem::WonderSwan, true, true),
            (ActiveSystem::MasterSystem, true, false),
            (ActiveSystem::GameGear, false, false),
            (ActiveSystem::Sg1000, true, false),
            (ActiveSystem::Md, true, false),
        ];
        let specs = crate::emu_backend::system_specs();
        assert_eq!(specs.len(), expected.len());
        for (system, rollback, linked) in expected {
            assert!(
                specs.iter().any(|spec| spec.system == system),
                "{system:?} is missing from system specs"
            );
            assert_eq!(rollback_session(system), rollback, "{system:?}");
            assert_eq!(linked_devices(system), linked, "{system:?}");
        }
    }
}
