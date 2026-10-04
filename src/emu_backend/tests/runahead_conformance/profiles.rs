use super::*;

pub(super) fn core_cases() -> &'static [CoreCase] {
    const PASS: &[FailureClass] = &[];
    const AUDIO_CADENCE: &[FailureClass] = &[FailureClass::AudioCadence];
    const GB: &[IneligibilityReason] = &[
        IneligibilityReason::NoSpeculativeWorker,
        IneligibilityReason::SramDiskWriteIsolationUnproven,
        IneligibilityReason::RecoveryGenerationIsolationUnproven,
        IneligibilityReason::UiPersistenceIsolationUnproven,
        IneligibilityReason::ReplayIsolationUnproven,
        IneligibilityReason::RemoteIsolationUnproven,
        IneligibilityReason::LinkDevice,
        IneligibilityReason::Rtc,
        IneligibilityReason::LiveCamera,
        IneligibilityReason::HostSensor,
        IneligibilityReason::Printer,
        IneligibilityReason::Rumble,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ];
    const GBA: &[IneligibilityReason] = &[
        IneligibilityReason::FeatureDisabled,
        IneligibilityReason::DetachedPanicHangContainmentUnavailable,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ];
    const NES: &[IneligibilityReason] = &[
        IneligibilityReason::NoSpeculativeWorker,
        IneligibilityReason::SramDiskWriteIsolationUnproven,
        IneligibilityReason::RecoveryGenerationIsolationUnproven,
        IneligibilityReason::UiPersistenceIsolationUnproven,
        IneligibilityReason::ReplayIsolationUnproven,
        IneligibilityReason::RemoteIsolationUnproven,
        IneligibilityReason::LightGun,
        IneligibilityReason::RemovableMedia,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ];
    const PCE: &[IneligibilityReason] = &[
        IneligibilityReason::NoSpeculativeWorker,
        IneligibilityReason::SramDiskWriteIsolationUnproven,
        IneligibilityReason::RecoveryGenerationIsolationUnproven,
        IneligibilityReason::UiPersistenceIsolationUnproven,
        IneligibilityReason::ReplayIsolationUnproven,
        IneligibilityReason::RemoteIsolationUnproven,
        IneligibilityReason::Mouse,
        IneligibilityReason::RemovableMedia,
        IneligibilityReason::CdMedia,
    ];
    const SEGA8: &[IneligibilityReason] = &[
        IneligibilityReason::FeatureDisabled,
        IneligibilityReason::DetachedPanicHangContainmentUnavailable,
    ];
    const COLECO: &[IneligibilityReason] = &[
        IneligibilityReason::NoSpeculativeWorker,
        IneligibilityReason::SramDiskWriteIsolationUnproven,
        IneligibilityReason::RecoveryGenerationIsolationUnproven,
        IneligibilityReason::UiPersistenceIsolationUnproven,
        IneligibilityReason::ReplayIsolationUnproven,
        IneligibilityReason::RemoteIsolationUnproven,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ];
    const WS: &[IneligibilityReason] = &[
        IneligibilityReason::NoSpeculativeWorker,
        IneligibilityReason::SramDiskWriteIsolationUnproven,
        IneligibilityReason::RecoveryGenerationIsolationUnproven,
        IneligibilityReason::UiPersistenceIsolationUnproven,
        IneligibilityReason::ReplayIsolationUnproven,
        IneligibilityReason::RemoteIsolationUnproven,
        IneligibilityReason::LinkDevice,
        IneligibilityReason::ObservedAudioCadenceMismatch,
    ];
    const CASES: &[CoreCase] = &[
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::GameBoy,
                fixture_system: ActiveSystem::GameBoy,
                support: RunAheadSupport::Unsupported,
                reasons: GB,
            },
            build: build_gb_backend,
            expected_local_failures: [AUDIO_CADENCE, PASS, AUDIO_CADENCE],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::GameBoyAdvance,
                fixture_system: ActiveSystem::GameBoyAdvance,
                support: RunAheadSupport::Unsupported,
                reasons: GBA,
            },
            build: build_gba_backend,
            expected_local_failures: [PASS, PASS, AUDIO_CADENCE],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::Nes,
                fixture_system: ActiveSystem::Nes,
                support: RunAheadSupport::Unsupported,
                reasons: NES,
            },
            build: build_nes_backend,
            expected_local_failures: [AUDIO_CADENCE, PASS, AUDIO_CADENCE],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::ColecoVision,
                fixture_system: ActiveSystem::Coleco,
                support: RunAheadSupport::Unsupported,
                reasons: COLECO,
            },
            build: build_coleco_backend,
            expected_local_failures: [AUDIO_CADENCE, PASS, AUDIO_CADENCE],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::PcEngine,
                fixture_system: ActiveSystem::Pce,
                support: RunAheadSupport::Unsupported,
                reasons: PCE,
            },
            build: build_pce_backend,
            expected_local_failures: [PASS, PASS, PASS],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::Sega8,
                fixture_system: ActiveSystem::MasterSystem,
                support: RunAheadSupport::Unsupported,
                reasons: SEGA8,
            },
            build: build_sms_backend,
            expected_local_failures: [PASS, PASS, PASS],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::WonderSwan,
                fixture_system: ActiveSystem::WonderSwan,
                support: RunAheadSupport::Unsupported,
                reasons: WS,
            },
            build: build_ws_backend,
            expected_local_failures: [AUDIO_CADENCE, PASS, AUDIO_CADENCE],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::Sega8,
                fixture_system: ActiveSystem::GameGear,
                support: RunAheadSupport::Unsupported,
                reasons: SEGA8,
            },
            build: build_game_gear_backend,
            expected_local_failures: [PASS, PASS, PASS],
        },
        CoreCase {
            eligibility: EligibilityResult {
                core_family: CoreFamily::Sega8,
                fixture_system: ActiveSystem::Sg1000,
                support: RunAheadSupport::Unsupported,
                reasons: SEGA8,
            },
            build: build_sg1000_backend,
            expected_local_failures: [PASS, PASS, PASS],
        },
    ];
    CASES
}

fn sega_backend(
    system: ActiveSystem,
    hint: zeff_sega8_core::hardware::cartridge::SystemHint,
) -> EmuBackend {
    let rom = super::super::fixtures::build_sms_test_rom();
    let emulator = zeff_sega8_core::emulator::Emulator::new_with_hint(&rom, 44_100, hint).unwrap();
    EmuBackend::from_sega8(
        emulator,
        std::path::PathBuf::from(format!("test.{}", system.code())),
    )
}

fn build_game_gear_backend() -> EmuBackend {
    sega_backend(
        ActiveSystem::GameGear,
        zeff_sega8_core::hardware::cartridge::SystemHint::GameGear,
    )
}

fn build_sg1000_backend() -> EmuBackend {
    sega_backend(
        ActiveSystem::Sg1000,
        zeff_sega8_core::hardware::cartridge::SystemHint::Sg1000,
    )
}
