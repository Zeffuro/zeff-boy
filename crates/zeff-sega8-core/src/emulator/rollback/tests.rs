use super::*;
use crate::emulator::Sega8LoadConfig;
use crate::hardware::{
    cartridge::{Sega8MapperKind, SystemHint},
    region::Sega8Region,
    timing::Sega8VideoStandard,
};

fn fixture(
    hint: SystemHint,
    mapper: Sega8MapperKind,
    timing: Sega8VideoStandard,
    region: Sega8Region,
) -> Emulator {
    let mut rom = vec![0; 32768];
    let program = [
        0x3e, 0x84, 0xd3, 0x7f, 0x3e, 0x12, 0xd3, 0x7f, 0x3e, 0x90, 0xd3, 0x7f, 0x3e, 0x40, 0xd3,
        0xbf, 0x3e, 0x81, 0xd3, 0xbf, 0xdb, 0xdc, 0x32, 0x00, 0xc0, 0xdb, 0xdd, 0x32, 0x01, 0xc0,
        0x3a, 0x02, 0xc0, 0x3c, 0x32, 0x02, 0xc0, 0xd3, 0xbe, 0xc3, 0x14, 0x00,
    ];
    for page in rom.chunks_mut(8192) {
        page[..program.len()].copy_from_slice(&program);
        page[0x66..0x70]
            .copy_from_slice(&[0x3a, 0x03, 0xc0, 0x3c, 0x32, 0x03, 0xc0, 0xed, 0x45, 0x00]);
    }
    rom[0x66..0x70].copy_from_slice(&[0x3a, 0x03, 0xc0, 0x3c, 0x32, 0x03, 0xc0, 0xed, 0x45, 0x00]);
    let mut core = Emulator::new_with_config(
        &rom,
        Sega8LoadConfig::new(48_000)
            .with_system_hint(hint)
            .with_mapper_kind(Some(mapper))
            .with_video_standard(timing)
            .with_console_region(Some(region)),
    )
    .unwrap();
    let bus = core.bus_mut();
    bus.io_write(
        0xbf,
        if hint == SystemHint::MasterSystem {
            4
        } else {
            0
        },
    );
    bus.io_write(0xbf, 0x80);
    bus.io_write(0xbf, 0xf4);
    bus.io_write(0xbf, 0x87);
    if hint == SystemHint::MasterSystem {
        bus.io_write(0xbf, 0);
        bus.io_write(0xbf, 0xc0);
        for _ in 0..32 {
            bus.io_write(0xbe, 0x3f);
        }
        bus.io_write(0xbf, 0);
        bus.io_write(0xbf, 0x40);
    }
    core
}

fn witness(core: &Emulator) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    (
        core.encode_state().unwrap(),
        core.encode_rollback_runtime_state(),
        core.framebuffer().to_vec(),
        core.bus().cartridge_ram().to_vec(),
    )
}

#[test]
fn every_mapper_and_console_replays_exact_runtime_video_pcm_ports_and_ram() {
    for hint in [SystemHint::MasterSystem, SystemHint::Sg1000] {
        for timing in [Sega8VideoStandard::Ntsc, Sega8VideoStandard::Pal] {
            for region in [
                Sega8Region::Export,
                Sega8Region::Japanese,
                Sega8Region::JapanesePowerBaseConverter,
            ] {
                for mapper in [
                    Sega8MapperKind::Sega,
                    Sega8MapperKind::Codemasters,
                    Sega8MapperKind::Korean,
                    Sega8MapperKind::Msx,
                    Sega8MapperKind::Nemesis,
                    Sega8MapperKind::Janggun,
                ] {
                    let mut core = fixture(hint, mapper, timing, region);
                    if !core.bus().cartridge_ram_visible().is_empty() {
                        let ram = vec![0x5a; core.bus().cartridge_ram_visible().len()];
                        core.bus_mut().load_cartridge_ram(&ram).unwrap();
                    }
                    let lease = core.begin_rollback_session().unwrap();
                    lease.advance_frame(&mut core, [0x11, 0xa2]).unwrap();
                    let snapshot = lease.capture(&core).unwrap();
                    let before = witness(&core);
                    let mut expected = Vec::new();
                    for ports in [[8, 0], [8, 8], [0, 8], [0, 0], [0, 8], [0x41, 0x82]] {
                        let pcm = lease.advance_frame(&mut core, ports).unwrap();
                        assert!(!pcm.is_empty());
                        assert!(
                            pcm.iter().any(|sample| *sample != 0.0),
                            "{hint:?}/{mapper:?}/{timing:?}/{region:?}"
                        );
                        expected.push((
                            witness(&core),
                            pcm.iter()
                                .map(|sample| sample.to_bits())
                                .collect::<Vec<_>>(),
                        ));
                    }
                    lease.restore(&mut core, &snapshot).unwrap();
                    assert_eq!(witness(&core), before);
                    for (ports, (state, audio)) in
                        [[8, 0], [8, 8], [0, 8], [0, 0], [0, 8], [0x41, 0x82]]
                            .into_iter()
                            .zip(expected)
                    {
                        let pcm = lease.advance_frame(&mut core, ports).unwrap();
                        assert_eq!(
                            pcm.iter()
                                .map(|sample| sample.to_bits())
                                .collect::<Vec<_>>(),
                            audio
                        );
                        assert_eq!(witness(&core), state);
                    }
                    assert_eq!(
                        core.bus().input().read_controller(ControllerPort::One),
                        0xeb
                    );
                    assert_eq!(
                        core.bus().input().read_controller(ControllerPort::Two),
                        0xd7
                    );
                    assert_eq!(
                        core.system_ram()[3],
                        if hint == SystemHint::MasterSystem {
                            2
                        } else {
                            0
                        }
                    );
                    assert!(
                        core.framebuffer()
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .any(|pixel| pixel[..3] != [0, 0, 0]),
                        "{hint:?}/{mapper:?}/{timing:?}/{region:?}"
                    );
                    assert!(snapshot.retained_bytes() >= snapshot.state.len());
                }
            }
        }
    }
}

#[test]
fn native_load_resets_audio_phase_but_rollback_preserves_it() {
    let mut core = fixture(
        SystemHint::MasterSystem,
        Sega8MapperKind::Sega,
        Sega8VideoStandard::Ntsc,
        Sega8Region::Export,
    );
    let lease = core.begin_rollback_session().unwrap();
    for _ in 0..20 {
        lease.advance_frame(&mut core, [0; 2]).unwrap();
        if core.bus().apu().rollback_sample_phase() != 0 {
            break;
        }
    }
    let snapshot = lease.capture(&core).unwrap();
    let phase = core.bus().apu().rollback_sample_phase();
    assert_ne!(phase, 0);
    let mut native = core.clone();
    native.load_state(&snapshot.state).unwrap();
    assert_eq!(native.bus().apu().rollback_sample_phase(), 0);
    lease.advance_frame(&mut core, [0; 2]).unwrap();
    lease.restore(&mut core, &snapshot).unwrap();
    assert_eq!(core.bus().apu().rollback_sample_phase(), phase);
}

#[test]
fn mutations_foreign_snapshots_and_partial_frames_retire_execution() {
    let mut core = fixture(
        SystemHint::MasterSystem,
        Sega8MapperKind::Sega,
        Sega8VideoStandard::Ntsc,
        Sega8Region::Export,
    );
    let lease = core.begin_rollback_session().unwrap();
    let snapshot = lease.capture(&core).unwrap();
    assert!(core.begin_rollback_session().is_err());
    let mut other = core.clone();
    assert!(lease.capture(&other).is_err());
    let other_lease = other.begin_rollback_session().unwrap();
    assert!(other_lease.restore(&mut other, &snapshot).is_err());
    let other_before = witness(&other);
    assert!(
        lease
            .restore_after_session(&mut other, &snapshot, &snapshot.state)
            .is_err()
    );
    assert_eq!(witness(&other), other_before);
    assert!(other_lease.capture(&other).is_ok());
    core.step_instruction();
    assert!(lease.capture(&core).is_err());
    assert!(core.begin_rollback_session().is_err());
    lease
        .restore_after_session(&mut core, &snapshot, &snapshot.state)
        .unwrap();
    core.set_input(0, 0);
    assert!(lease.capture(&core).is_err());
    let replacement = core.begin_rollback_session().unwrap();
    let before = witness(&core);
    assert!(
        lease
            .restore_after_session(&mut core, &snapshot, &snapshot.state)
            .is_err()
    );
    assert_eq!(witness(&core), before);
    assert!(replacement.capture(&core).is_ok());
    assert!(core.load_state(&[0]).is_err());
    assert_eq!(witness(&core), before);
    assert!(replacement.capture(&core).is_ok());
    core.suspend();
    assert!(replacement.advance_frame(&mut core, [0; 2]).is_err());
    assert_eq!(core.frame_count(), 0);
}

#[test]
fn unsupported_runtime_configuration_is_refused_before_execution() {
    for change in 0..6 {
        let mut core = fixture(
            SystemHint::MasterSystem,
            Sega8MapperKind::Sega,
            Sega8VideoStandard::Ntsc,
            Sega8Region::Export,
        );
        match change {
            0 => core.set_sample_rate(44_100),
            1 => core.set_apu_channel_mutes([true, false, false, false]),
            2 => core.set_apu_sample_generation_enabled(false),
            3 => core.add_breakpoint(0),
            4 => core.set_opcode_log_enabled(true),
            _ => {
                core.step_frame();
            }
        }
        assert!(core.begin_rollback_session().is_err());
    }
    let mut core = fixture(
        SystemHint::GameGear,
        Sega8MapperKind::Sega,
        Sega8VideoStandard::Ntsc,
        Sega8Region::Export,
    );
    assert!(core.begin_rollback_session().is_err());
}
