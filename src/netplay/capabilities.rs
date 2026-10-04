use crate::emu_backend::ActiveSystem;

pub(crate) fn shared_console(system: ActiveSystem) -> bool {
    matches!(system, ActiveSystem::Nes)
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn every_system_exposes_only_implemented_session_adapters() {
        let specs = crate::emu_backend::system_specs();
        assert_eq!(specs.len(), 9);
        for spec in specs {
            assert_eq!(
                shared_console(spec.system),
                spec.system == ActiveSystem::Nes
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
