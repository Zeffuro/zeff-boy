use super::{cases::*, *};

fn audio_machine(region: Region, timing: TimingModel) -> Emulator {
    let template = super::super::super::cpu_continuation_tests::machine(region, false);
    let mut rom = template.rom_bytes().to_vec();
    drop(
        rom.splice(
            0x200..0x200,
            [0x33fc_u16, 0x8000, 0x00c0, 4, 0x33fc, 0x8144, 0x00c0, 4]
                .into_iter()
                .flat_map(u16::to_be_bytes),
        ),
    );
    bootstrap_sound_rom(&mut rom, 0x220);
    Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
        timing,
    )
    .unwrap()
}

pub(super) fn bootstrap_sound_rom(rom: &mut Vec<u8>, upload: usize) {
    let release = upload + 10 + 42 * 8;
    assert_eq!(
        &rom[upload..upload + 8],
        &[0x13, 0xfc, 0, 0x21, 0, 0xa0, 0, 0]
    );
    drop(
        rom.splice(
            upload..upload,
            [0x0839_u16, 0, 0x00a1, 0x1100, 0x66f6]
                .into_iter()
                .flat_map(u16::to_be_bytes),
        ),
    );
    assert_eq!(
        &rom[release..release + 8],
        &[0x13, 0xfc, 0, 0, 0, 0xa1, 0x11, 0]
    );
    drop(
        rom.splice(
            release..release,
            [
                0x13fc_u16, 0, 0x00a1, 0x1200, 0x0839, 0, 0x00a1, 0x1100, 0x67f6, 0x13fc, 1,
                0x00a1, 0x1200,
            ]
            .into_iter()
            .flat_map(u16::to_be_bytes),
        ),
    );
}

pub(super) fn assert_sound_program(emu: &Emulator, region: Region) {
    assert_eq!(
        &emu.bus.active_sound_memory().ram[..42],
        &[
            0x21, 0, 7, 0x3e, 0xb6, 0x32, 2, 0x40, 0x3e, 0x80, 0x32, 3, 0x40, 0x3e, 0x2a, 0x32, 0,
            0x40, 0x3e, 0xff, 0x32, 1, 0x40, 0x3e, 0x2b, 0x32, 0, 0x40, 0x3e, 0x80, 0x32, 1, 0x40,
            0x3a, 0, 0x40, 0x32, 0x10, 7, 0x34, 0x18, 0xf7,
        ],
        "initial Precise SOUND program {region:?}"
    );
    assert_eq!(
        (emu.sound_cpu().regs().h, emu.sound_cpu().regs().l),
        (7, 0),
        "initial Precise SOUND HL {region:?} pc={:04x} cycles={}",
        emu.sound_cpu().regs().pc,
        emu.sound_cpu().cycles()
    );
}

pub(super) fn active_sound_held() {
    for region in REGIONS {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let template = super::super::super::cpu_continuation_tests::machine(region, false);
            let mut rom = template.rom_bytes().to_vec();
            bootstrap_sound_rom(&mut rom, 0x210);
            let mut emu = Emulator::new_with_timing(
                rom,
                LoadConfig {
                    region,
                    concurrent_sound: true,
                },
                timing,
            )
            .unwrap();
            let mut reached = false;
            for _ in 0..20_000 {
                emu.step_instruction().unwrap_or_else(|fault| {
                    panic!("active SOUND setup {region:?}/{timing:?}: {fault:?}")
                });
                let cycles = emu
                    .bus
                    .owned_sound_cpu()
                    .map_or(emu.sound_cpu.cycles(), |cpu| cpu.cycles());
                if cycles >= 400
                    && (timing == TimingModel::Precise || emu.sound_schedule.pending.is_some())
                    && emu
                        .clone()
                        .drain_audio_samples()
                        .iter()
                        .any(|sample| *sample != 0.0)
                {
                    reached = true;
                    break;
                }
            }
            assert!(
                reached,
                "no active audible SOUND continuation {region:?}/{timing:?} pc={:06x} sound={:?}",
                emu.registers().pc,
                emu.sound_cpu().regs()
            );
            assert_sound_program(&emu, region);
            if timing == TimingModel::Standard {
                assert!(emu.sound_schedule.pending.as_ref().unwrap().deadline > emu.bus.now.get());
            }
            let old_pcm = emu.clone().drain_audio_samples();
            assert!(
                !old_pcm.is_empty(),
                "active SOUND PCM {region:?}/{timing:?}"
            );
            assert!(
                old_pcm.iter().any(|sample| *sample != 0.0),
                "active SOUND silence {region:?}/{timing:?}"
            );
            emu.debug_write_ports(&[
                DebugPortWrite::Byte {
                    address: 0xa11100,
                    value: 1,
                },
                DebugPortWrite::Byte {
                    address: 0xa11200,
                    value: 0,
                },
                control(0x8f02),
            ])
            .unwrap_or_else(|fault| {
                panic!("active SOUND request/reset batch {region:?}/{timing:?}: {fault:?}")
            });
            assert!(emu.clone().drain_audio_samples().starts_with(&old_pcm));
            let bytes = emu.encode_debugger_checkpoint().unwrap();
            let mut restored = emu.clone();
            restored.decode_debugger_checkpoint(&bytes).unwrap();
            for _ in 0..128 {
                assert_eq!(emu.step_instruction(), restored.step_instruction());
            }
            assert!(emu.bus.active_sound_memory().requested);
            assert!(emu.bus.active_sound_memory().reset_asserted);
            same_checkpoint(&emu, &restored);
            assert_eq!(emu.drain_audio_samples(), restored.drain_audio_samples());
        }
    }
}

pub(super) fn frame_crossing() {
    for region in REGIONS {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut emu = audio_machine(region, timing);
            let mut control_machine = audio_machine(region, timing);
            let deadline = emu.bus.now.get() + region.frame_ticks() * 3;
            for _ in 0..200_000 {
                if emu.frame_ready() || emu.bus.now.get() >= deadline {
                    break;
                }
                paired_instruction(&mut emu, &mut control_machine);
            }
            assert!(
                emu.frame_ready(),
                "no complete real output frame within three fields"
            );
            output(&emu, &control_machine);
            let first_ready = emu.bus.now.get();
            let count = emu.frame_count();
            emu.clear_frame_ready();
            control_machine.clear_frame_ready();
            let near = first_ready + region.frame_ticks() - 1024;
            for _ in 0..200_000 {
                if emu.bus.now.get() >= near {
                    break;
                }
                paired_instruction(&mut emu, &mut control_machine);
            }
            assert!(
                emu.bus.now.get() >= near,
                "near-frame selector exceeded its instruction bound"
            );
            assert!(!emu.frame_ready());
            assert!(!control_machine.frame_ready());
            let old_pcm = pcm(&emu);
            assert!(old_pcm.iter().any(|bits| f32::from_bits(*bits) != 0.0));
            let registers = emu.registers();
            let inputs = emu.cpu.queued_instruction_inputs();
            let writes = [control(0x8f02); 64];
            let report = emu.debug_write_ports(&writes).unwrap();
            for write in writes {
                direct(&mut control_machine, write).unwrap();
            }
            assert_eq!(report.instructions, 0);
            assert!(report.elapsed.get() >= 1792);
            assert_eq!(emu.registers(), registers);
            assert_eq!(emu.cpu.queued_instruction_inputs(), inputs);
            assert!(
                emu.frame_ready(),
                "timed transaction lost its newly published frame"
            );
            assert_eq!(emu.frame_count(), count + 1);
            assert!(emu.framebuffer_available());
            assert_eq!(emu.framebuffer_dimensions(), (256, 224));
            output(&emu, &control_machine);
            let current_pcm = pcm(&emu);
            assert!(current_pcm.starts_with(&old_pcm));
            assert!(current_pcm.len() > old_pcm.len());
            assert!(
                current_pcm[old_pcm.len()..]
                    .iter()
                    .any(|bits| f32::from_bits(*bits) != 0.0)
            );
            assert_eq!(
                emu.encode_debugger_checkpoint().unwrap(),
                control_machine.encode_debugger_checkpoint().unwrap()
            );
            let bytes = emu.encode_debugger_checkpoint().unwrap();
            let mut restored = audio_machine(region, timing);
            restored.decode_debugger_checkpoint(&bytes).unwrap();
            assert_eq!(restored.encode_debugger_checkpoint().unwrap(), bytes);
            assert_eq!(pcm(&restored), current_pcm);
            // Each real step_field owns its initial readiness clear, including after restore.
            for _ in 0..2 {
                let count = emu.frame_count();
                for _ in 0..2 {
                    let before = [emu.bus.now, control_machine.bus.now, restored.bus.now];
                    emu.step_field().expect("timed subject future field failed");
                    control_machine
                        .step_field()
                        .expect("direct control future field failed");
                    assert_eq!(
                        emu.encode_debugger_checkpoint().unwrap(),
                        control_machine.encode_debugger_checkpoint().unwrap()
                    );
                    restored
                        .step_field()
                        .expect("decoded control future field failed");
                    assert!(emu.bus.now > before[0]);
                    assert!(control_machine.bus.now > before[1]);
                    assert!(restored.bus.now > before[2]);
                    output(&emu, &control_machine);
                    output(&emu, &restored);
                    assert_eq!(
                        emu.encode_debugger_checkpoint().unwrap(),
                        restored.encode_debugger_checkpoint().unwrap()
                    );
                    if emu.frame_ready() {
                        break;
                    }
                    assert_eq!(emu.frame_count(), count);
                }
                assert!(
                    emu.frame_ready(),
                    "no complete future frame within two pacing fields"
                );
                assert_eq!(emu.frame_count(), count + 1);
            }
        }
    }
}

pub(super) fn precise_sound_phase() {
    for region in REGIONS {
        let mut emu = audio_machine(region, TimingModel::Precise);
        let deadline = emu.bus.now.get() + region.frame_ticks();
        let mut reached = false;
        for _ in 0..200_000 {
            if emu.bus.now.get() >= deadline {
                break;
            }
            instruction(&mut emu);
            let (pins, _) = emu.bus.timed_port_sound_phase().unwrap();
            if pins.mreq
                && pins.read
                && pins.address == Some(0x4000)
                && emu.bus.owned_sound_cpu().unwrap().cycles() >= 400
                && emu.sound_instruction_pending()
            {
                reached = true;
                break;
            }
        }
        assert!(
            reached,
            "no actual pending Precise FM read phase within one field"
        );
        assert_eq!(emu.sound_deadline(), None);
        assert_sound_program(&emu, region);
        let old_pcm = pcm(&emu);
        assert!(old_pcm.iter().any(|bits| f32::from_bits(*bits) != 0.0));
        let (_, half) = emu.bus.timed_port_sound_phase().unwrap();
        let cycles = emu.bus.owned_sound_cpu().unwrap().cycles();
        emu.debug_write_ports(&[control(0x8f02); 4]).unwrap();
        assert!(emu.bus.timed_port_sound_phase().unwrap().1 >= half + 224);
        assert!(emu.bus.owned_sound_cpu().unwrap().cycles() > cycles);
        assert!(pcm(&emu).starts_with(&old_pcm));
        emu.debug_write_ports(&[sound(0xa11100, 1)]).unwrap();
        let mut granted = false;
        for _ in 0..256 {
            if emu.bus.timed_port_sound_phase().unwrap().0.bus_ack {
                granted = true;
                break;
            }
            instruction(&mut emu);
        }
        assert!(
            granted,
            "real Precise request never reached bus acknowledge"
        );
        assert!(emu.bus.active_sound_memory().requested);
        let held_cycles = emu.bus.owned_sound_cpu().unwrap().cycles();
        emu.debug_write_ports(&[control(0x8f02); 4]).unwrap();
        assert!(emu.bus.timed_port_sound_phase().unwrap().0.bus_ack);
        assert_eq!(emu.bus.owned_sound_cpu().unwrap().cycles(), held_cycles);
        emu.debug_write_ports(&[sound(0xa11200, 0)]).unwrap();
        assert!(emu.bus.active_sound_memory().reset_asserted);
        emu.debug_write_ports(&[control(0x8f02); 8]).unwrap();
        emu.debug_write_ports(&[sound(0xa11200, 1)]).unwrap();
        assert!(!emu.bus.active_sound_memory().reset_asserted);
        assert!(emu.bus.active_sound_memory().requested);
        let bytes = emu.encode_debugger_checkpoint().unwrap();
        let mut restored = audio_machine(region, TimingModel::Precise);
        restored.decode_debugger_checkpoint(&bytes).unwrap();
        assert_eq!(restored.encode_debugger_checkpoint().unwrap(), bytes);
        let counter = emu.bus.active_sound_memory().ram[0x700];
        let cycles_before = emu.sound_cpu().cycles();
        for machine in [&mut emu, &mut restored] {
            machine.debug_write_ports(&[sound(0xa11100, 0)]).unwrap();
            assert!(!machine.bus.active_sound_memory().requested);
        }
        let mut resumed = false;
        for _ in 0..20_000 {
            paired_instruction(&mut emu, &mut restored);
            if emu.bus.active_sound_memory().ram[0x700] != counter {
                resumed = true;
                break;
            }
        }
        assert!(
            resumed,
            "reset/release did not resume SOUND {region:?}: counter={counter:02x}->{:02x} cycles={cycles_before}->{} regs={:?} phase={:?} requested={} granted={} reset={} pending={} now={:?}",
            emu.bus.active_sound_memory().ram[0x700],
            emu.sound_cpu().cycles(),
            emu.sound_cpu().regs(),
            emu.bus.timed_port_sound_phase(),
            emu.sound_bus_requested(),
            emu.sound_bus_granted(),
            emu.sound_reset_asserted(),
            emu.sound_instruction_pending(),
            emu.bus.now
        );
        assert!(!emu.bus.timed_port_sound_phase().unwrap().0.bus_ack);
        assert!(emu.bus.owned_sound_cpu().unwrap().cycles() > 0);
        assert!(pcm(&emu).starts_with(&old_pcm));
        same_checkpoint(&emu, &restored);
        output(&emu, &restored);
    }
}
