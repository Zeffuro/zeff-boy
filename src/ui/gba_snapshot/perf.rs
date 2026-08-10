pub(super) fn gba_perf_info(emu: &zeff_gba_core::emulator::Emulator) -> crate::debug::PerfInfo {
    crate::debug::PerfInfo {
        fps: 0.0,
        target_fps: zeff_emu_common::system::System::GameBoyAdvance.target_fps(),
        speed_mode_label: super::super::normal_speed_mode_label(),
        frames_in_flight: 0,
        cycles: emu.cpu_cycles(),
        platform_name: "GBA",
        hardware_label: "Game Boy Advance".into(),
        hardware_pref_label: "Auto".into(),
    }
}
