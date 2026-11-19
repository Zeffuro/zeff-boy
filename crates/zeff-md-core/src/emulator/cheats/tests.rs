use super::*;
use crate::emulator::{LoadConfig, Region, TimingModel};
use crate::hardware::bus::scheduled::ScheduledBus;
use crate::hardware::sound::schedule::SoundInterval;
use zeff_emu_common::cheats::CheatValue;
use zeff_m68k::bus::Direction;

fn fixture(program: &[u16], timing: TimingModel, region: Region) -> Emulator {
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new_with_timing(
        image,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
        timing,
    )
    .unwrap()
}

fn patch(address: u32, value: u8) -> CheatPatch {
    CheatPatch::WideRamWrite {
        address,
        value: CheatValue::Constant(value),
    }
}

fn write_host_ram(machine: &mut Emulator, patches: &[CheatPatch], debugger: bool) {
    if debugger {
        let writes: Vec<_> = patches
            .iter()
            .map(|patch| match patch {
                CheatPatch::WideRamWrite {
                    address,
                    value: CheatValue::Constant(value),
                } => (*address, *value),
                _ => panic!("unconditional byte fixture required"),
            })
            .collect();
        machine.debug_write_ram(&writes).unwrap();
    } else {
        assert_eq!(
            machine.apply_ram_cheats(patches),
            CheatApplyStats {
                applied: patches.len(),
                ..Default::default()
            }
        );
    }
}

fn advance_sound(machine: &mut Emulator, ticks: u64) {
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .advance_machine(ticks)
    .unwrap();
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn raw_parser_accepts_only_canonical_byte_and_word_lists() {
    for text in [
        "FF0040:5a + 00FFFFFF=FF",
        " $ff0040 : 0X5A + 0x00ffffff = $ff ",
        "+ + 0XFF0040=$5a ++ $00FFFFFF:0xff +",
        "F F 0 0 4 0 : 5 A + 0 0 F F F F F F = F F",
    ] {
        assert_eq!(
            parse_raw_cheats(text),
            Some(vec![patch(0xff0040, 0x5a), patch(0xffffff, 0xff)])
        );
    }
    assert_eq!(
        parse_raw_cheats("FF0000:00"),
        Some(vec![patch(0xff0000, 0)])
    );
    for text in [
        "",
        " + ",
        "FF0040",
        "FF0040:5",
        "FF0040:123",
        "FF0040:12345",
        "FF0040:??",
        "FF0040:5G",
        "FF0040:5A:FF",
        "FF0040=5A=FF",
        "FF0040:5A=FF",
        "F0040:5A",
        "0FF0040:5A",
        "000FF0040:5A",
        "01FF0040:5A",
        "FEFFFF:5A",
        "E00040:5A",
        "000040:5A",
        "A00040:5A",
        "C00004:5A",
        "FF0040:5A + A130F1:01",
        "FF0040:5A + nonsense",
        "FF0040:-1",
        "0x$FF0040:5A",
        "FF0040:0x$5A",
        "FF0040:5A + FF0042:12345",
        "FF0040:1234 + FF0042:5G",
        "FF0040:1234 + FFFFFF:0008",
        "FFFFFF:0000",
        "FFFFFF:ffff",
        "01FF0040:1234",
        "FEFFFF:1234",
        "FF0040:😀",
        "FF0040:éa5",
    ] {
        assert_eq!(parse_raw_cheats(text), None, "{text}");
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn raw_words_lower_to_adjacent_big_endian_bytes() {
    for (text, expected) in [
        ("FF0000:0000", vec![patch(0xff0000, 0), patch(0xff0001, 0)]),
        ("FF0001:0008", vec![patch(0xff0001, 0), patch(0xff0002, 8)]),
        (
            "00FFFFFE:ffff",
            vec![patch(0xfffffe, 0xff), patch(0xffffff, 0xff)],
        ),
        (
            " $fF0041 = 0Xa55A ",
            vec![patch(0xff0041, 0xa5), patch(0xff0042, 0x5a)],
        ),
        (
            "0x00ff0040:$00ff",
            vec![patch(0xff0040, 0), patch(0xff0041, 0xff)],
        ),
        (
            "F F 0 0 4 0 : 0 1 0 0",
            vec![patch(0xff0040, 1), patch(0xff0041, 0)],
        ),
        (
            "+ FF0040:1234 + FF0041:56 ++ $FF0043=$0008 +",
            vec![
                patch(0xff0040, 0x12),
                patch(0xff0041, 0x34),
                patch(0xff0041, 0x56),
                patch(0xff0043, 0),
                patch(0xff0044, 8),
            ],
        ),
    ] {
        assert_eq!(parse_raw_cheats(text), Some(expected), "{text}");
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn guest_word_reads_keep_cpu_prefetch_and_restore() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            for value in [0x0008_u16, 0xa55a, 0xffff] {
                let mut machine = fixture(
                    &[0x3039, 0x00ff, 0x0040, 0x4e71, 0x60f6, 0x4e71],
                    timing,
                    region,
                );
                let mut expected = machine.clone();
                expected.bus.ram[0x40..0x42].copy_from_slice(&value.to_be_bytes());
                let patches = parse_raw_cheats(&format!("FF0040:{value:04X}")).unwrap();
                assert_eq!(
                    machine.apply_ram_cheats(&patches),
                    CheatApplyStats {
                        applied: 2,
                        ..Default::default()
                    }
                );
                assert_eq!(machine, expected);
                let saved = machine.encode_frontend_state().unwrap();
                let mut restored = machine.clone();
                restored.decode_frontend_state(&saved).unwrap();
                assert_eq!(machine.step_instruction(), restored.step_instruction());
                assert_eq!(machine.cpu.registers.d[0] & 0xffff, u32::from(value));
                assert_eq!(
                    machine.encode_frontend_state().unwrap(),
                    restored.encode_frontend_state().unwrap()
                );
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn rejected_patches_and_compare_misses_preserve_machine_and_trace() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut machine = fixture(&[0x60fe, 0x4e71], timing, Region::NtscUs);
        if timing == TimingModel::Standard {
            machine.reset_and_begin_audio_trace(32).unwrap();
        }
        let mut rejected = vec![
            CheatPatch::RamWrite {
                address: 0xffff,
                value: CheatValue::Constant(1),
            },
            CheatPatch::RamWriteIfEquals {
                address: 0xffff,
                value: CheatValue::Constant(1),
                compare: CheatValue::Constant(0),
            },
            CheatPatch::RomWrite {
                address: 0x100,
                value: CheatValue::Constant(1),
            },
            CheatPatch::RomWriteIfEquals {
                address: 0x100,
                value: CheatValue::Constant(1),
                compare: CheatValue::Constant(0x60),
            },
        ];
        for address in [
            0,
            0x200001,
            0xa00000,
            0xa10000,
            0xa130f1,
            0xc00004,
            0xe00040,
            0xfeffff,
            0x1ff0040,
            u32::MAX,
        ] {
            rejected.push(patch(address, 0x55));
            rejected.push(CheatPatch::WideRamWriteIfEquals {
                address,
                value: CheatValue::Constant(1),
                compare: CheatValue::Constant(0),
            });
        }
        let before = machine.clone();
        let stats = machine.apply_ram_cheats(&rejected);
        assert_eq!(
            stats,
            CheatApplyStats {
                ignored: rejected.len(),
                ..Default::default()
            }
        );
        assert_eq!(machine, before);
        let stats = machine.apply_ram_cheats(&[CheatPatch::WideRamWriteIfEquals {
            address: 0xff0040,
            value: CheatValue::Constant(1),
            compare: CheatValue::Constant(0x99),
        }]);
        assert_eq!(
            stats,
            CheatApplyStats {
                skipped_compare: 1,
                ..Default::default()
            }
        );
        assert_eq!(machine, before);
        assert_eq!(machine.apply_ram_cheats(&[]), CheatApplyStats::default());
        assert_eq!(machine, before);
    }
    let mut machine = fixture(&[0x60fe, 0x4e71], TimingModel::Standard, Region::NtscUs);
    machine.reset_and_begin_audio_trace(32).unwrap();
    machine.bus.ram[0x40] = 0xa5;
    let stats = machine.apply_ram_cheats(&[
        CheatPatch::WideRamWrite {
            address: 0xff0040,
            value: CheatValue::PreserveWithCurrent {
                mask: 0xf0,
                base: 0x0b,
            },
        },
        CheatPatch::WideRamWriteIfEquals {
            address: 0xff0040,
            value: CheatValue::UserParameterized {
                mask: 0x0f,
                base: 0x60,
            },
            compare: CheatValue::PreserveWithCurrent {
                mask: 0x0f,
                base: 0xa0,
            },
        },
        CheatPatch::WideRamWriteIfEquals {
            address: 0xff0040,
            value: CheatValue::Constant(0),
            compare: CheatValue::Constant(0xab),
        },
        patch(0xff0000, 0x12),
        patch(0xffffff, 0x34),
        patch(0x1ffffff, 0x55),
    ]);
    assert_eq!(
        stats,
        CheatApplyStats {
            applied: 4,
            skipped_compare: 1,
            ignored: 1
        }
    );
    assert_eq!(machine.work_ram()[0x40], 0x6b);
    assert_eq!(machine.work_ram()[0], 0x12);
    assert_eq!(machine.work_ram()[0xffff], 0x34);
    assert_eq!(
        machine.finish_audio_trace().unwrap().trace.invalidated,
        Some(AudioTraceInvalidation::ExternalMutation)
    );
    machine.reset_and_begin_audio_trace(32).unwrap();
    machine.invalidate_audio_trace(AudioTraceInvalidation::Reset);
    machine.apply_ram_cheats(&[patch(0xff0040, 0)]);
    assert_eq!(
        machine.finish_audio_trace().unwrap().trace.invalidated,
        Some(AudioTraceInvalidation::Reset)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn guest_reads_and_queued_ram_opcodes_keep_hardware_prefetch_and_restore() {
    for debugger in [false, true] {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for timing in [TimingModel::Standard, TimingModel::Precise] {
                let mut machine = fixture(
                    &[0x1039, 0x00ff, 0x0040, 0x4e71, 0x60f6, 0x4e71],
                    timing,
                    region,
                );
                let before = machine.clone();
                write_host_ram(&mut machine, &[patch(0xff0040, 0x5a)], debugger);
                let mut expected = before;
                expected.bus.ram[0x40] = 0x5a;
                assert_eq!(machine, expected);
                machine.step_instruction().unwrap();
                assert_eq!(machine.cpu.registers.d[0] & 0xff, 0x5a);

                let mut machine = fixture(&[0x4ef9, 0x00ff, 0x0100, 0x4e71], timing, region);
                for (offset, byte) in [0x70, 1, 0x4e, 0xf9, 0, 0xff, 1, 0, 0x4e, 0x71]
                    .into_iter()
                    .enumerate()
                {
                    write_host_ram(
                        &mut machine,
                        &[patch(0xff0100 + offset as u32, byte)],
                        debugger,
                    );
                }
                machine.step_instruction().unwrap();
                assert_eq!(machine.queued_words(), Some([0x7001, 0x4ef9]));
                let before = machine.clone();
                write_host_ram(
                    &mut machine,
                    &parse_raw_cheats("FF0100:7002").unwrap(),
                    debugger,
                );
                let mut expected = before;
                expected.bus.ram[0x100..0x102].copy_from_slice(&[0x70, 2]);
                assert_eq!(machine, expected);
                let saved = machine.encode_frontend_state().unwrap();
                let mut restored = machine.clone();
                restored.decode_frontend_state(&saved).unwrap();
                for value in [1, 1, 2] {
                    assert_eq!(machine.step_instruction(), restored.step_instruction());
                    assert_eq!(machine.cpu.registers.d[0], value);
                    assert_eq!(
                        machine.encode_frontend_state().unwrap(),
                        restored.encode_frontend_state().unwrap()
                    );
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn pending_banked_reads_writes_and_local_epochs_survive_host_patches() {
    for debugger in [false, true] {
        for direction in [Direction::Read, Direction::Write] {
            let mut machine = fixture(&[0x4e71; 32], TimingModel::Standard, Region::NtscUs);
            machine.bus.sound.set_reset(false);
            machine.bus.sound.bank = 0x1ff;
            let program: &[u8] = if direction == Direction::Read {
                &[0x3a, 4, 0x80, 0x76]
            } else {
                &[0x3e, 0x55, 0x32, 4, 0x80, 0x76]
            };
            machine.bus.sound.ram[..program.len()].copy_from_slice(program);
            for _ in 0..32 {
                machine.step_instruction().unwrap();
                if machine
                    .sound_schedule
                    .pending
                    .as_ref()
                    .is_some_and(|pending| {
                        pending.bank_access.is_some_and(|access| {
                            access.address == 0xff8004 && access.direction == direction
                        })
                    })
                {
                    break;
                }
            }
            assert!(
                machine
                    .sound_schedule
                    .pending
                    .as_ref()
                    .is_some_and(|pending| {
                        pending.bank_access.is_some_and(|access| {
                            access.address == 0xff8004 && access.direction == direction
                        })
                    })
            );
            let deadline = machine
                .sound_deadline()
                .expect("pending banked segment")
                .get();
            assert!(machine.bus.now.get() < deadline);
            let before = machine.clone();
            write_host_ram(
                &mut machine,
                &parse_raw_cheats("FF8004:A633").unwrap(),
                debugger,
            );
            let mut expected = before;
            expected.bus.ram[0x8004..0x8006].copy_from_slice(&[0xa6, 0x33]);
            assert_eq!(machine, expected);
            let saved = machine.encode_frontend_state().unwrap();
            let mut restored = machine.clone();
            restored.decode_frontend_state(&saved).unwrap();
            let remaining = deadline - machine.bus.now.get();
            advance_sound(&mut machine, remaining - 1);
            advance_sound(&mut restored, remaining - 1);
            assert_eq!(&machine.work_ram()[0x8004..0x8006], &[0xa6, 0x33]);
            assert_eq!(machine.sound_deadline().unwrap().get(), deadline);
            advance_sound(&mut machine, 1);
            advance_sound(&mut restored, 1);
            assert_eq!(
                &machine.work_ram()[0x8004..0x8006],
                if direction == Direction::Write {
                    &[0x55, 0x33]
                } else {
                    &[0xa6, 0x33]
                }
            );
            if direction == Direction::Read {
                assert_eq!(machine.sound_cpu.regs().a, 0xff);
            }
            assert_eq!(machine.cpu, restored.cpu);
            assert_eq!(machine.sound_cpu, restored.sound_cpu);
            assert_eq!(machine.sound_schedule, restored.sound_schedule);
            assert_eq!(machine.bus, restored.bus);
        }
        for elapsed in [1, 80] {
            let mut machine = fixture(&[0x60fe, 0x4e71], TimingModel::Standard, Region::NtscUs);
            machine.bus.sound.set_reset(false);
            machine.sound_schedule.begin_epoch_field();
            let begin = machine.bus.now.get();
            machine.sound_schedule.advance(
                &mut machine.sound_cpu,
                &mut machine.bus.sound,
                &mut machine.bus.cartridge,
                &mut machine.bus.ram,
                SoundInterval::new(begin..begin + elapsed, &mut machine.bus.audio),
            );
            assert!(machine.sound_schedule.epoch.is_some());
            assert!(machine.sound_schedule.pending.is_none());
            let mut expected = machine.clone();
            write_host_ram(&mut machine, &[patch(0xff0040, 0x5a)], debugger);
            expected.bus.ram[0x40] = 0x5a;
            assert_eq!(machine, expected);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn debug_ram_batches_are_atomic_zero_time_and_invalidate_only_accepted_edits() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut machine = fixture(&[0x60fe, 0x4e71], timing, region);
            if timing == TimingModel::Standard {
                machine.reset_and_begin_audio_trace(32).unwrap();
            }
            let before = machine.clone();
            machine.debug_write_ram(&[]).unwrap();
            assert_eq!(machine, before);
            for invalid in [0, 0xa00000, 0xc00000, 0xfeffff, 0x1000000, u32::MAX] {
                assert_eq!(
                    machine.debug_write_ram(&[(0xff0041, 0xa5), (invalid, 0x12), (0xffffff, 0x34)]),
                    Err(invalid)
                );
                assert_eq!(machine, before);
            }
            let same = machine.work_ram()[0x41];
            machine.debug_write_ram(&[(0xff0041, same)]).unwrap();
            let mut expected = before;
            expected.invalidate_audio_trace(AudioTraceInvalidation::ExternalMutation);
            assert_eq!(machine, expected);
            machine
                .debug_write_ram(&[
                    (0xff0000, 0x12),
                    (0xffffff, 0x34),
                    (0xff0041, 0x55),
                    (0xff0041, 0xa5),
                ])
                .unwrap();
            expected.bus.ram[0] = 0x12;
            expected.bus.ram[0xffff] = 0x34;
            expected.bus.ram[0x41] = 0xa5;
            assert_eq!(machine, expected);
            if timing == TimingModel::Standard {
                assert_eq!(
                    machine.finish_audio_trace().unwrap().trace.invalidated,
                    Some(AudioTraceInvalidation::ExternalMutation)
                );
            }
        }
    }
}
