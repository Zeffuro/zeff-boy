use super::*;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::ClockedBus;

fn configured(words: &[u16], region: Region, concurrent: bool) -> Emulator {
    let mut rom = vec![0; 0x200];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in words.iter().enumerate() {
        rom[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound: concurrent,
        },
    )
    .unwrap();
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x155;
    machine.bus.sound.ram[..3].copy_from_slice(&[0, 0x18, 0xfd]);
    machine
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

#[test]
fn actual_byte_and_word_rom_mode_writes_match_ignored_rom_writes() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for (opcode, values) in [
                (0x1080, &[0, 2, 0x7e, 0x80, 0xfe][..]),
                (0x3080, &[0, 1, 0xff, 0x200, 0x7eff, 0xfe00, 0xfeff][..]),
            ] {
                for &value in values {
                    let mut target =
                        configured(&[opcode, 0x4e71, 0x60fe, 0x4e71], region, concurrent);
                    target.cpu.registers.d[0] = value;
                    let mut control = target.clone();
                    target.cpu.registers.a[0] = 0xa11000;
                    control.cpu.registers.a[0] = 0x180;
                    let sound = target.bus.sound.clone();
                    assert_eq!(target.step_instruction(), control.step_instruction());
                    assert_eq!(target.bus.sound, sound);
                    target.cpu.registers.a[0] = 0;
                    control.cpu.registers.a[0] = 0;
                    assert_eq!(target, control);
                    let raw = target.encode_state().unwrap();
                    assert_eq!(
                        u32::from_le_bytes(raw[8..12].try_into().unwrap()),
                        crate::save_state::STATE_VERSION
                    );
                    for _ in 0..8 {
                        assert_eq!(target.step_instruction(), control.step_instruction());
                        assert_eq!(target, control);
                    }
                }
            }
        }
    }
}

#[test]
fn actual_immediate_word_zero_selects_rom_mode() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            let mut target = configured(&[0x33fc, 0, 0x00a1, 0x1000, 0x60fe], region, concurrent);
            let mut control = configured(&[0x33fc, 0, 0, 0x0180, 0x60fe], region, concurrent);
            assert_eq!(target.step_instruction(), control.step_instruction());
            assert_eq!(target.cpu.registers, control.cpu.registers);
            assert_eq!(target.bus.now, control.bus.now);
            assert_eq!(target.bus.sound, control.bus.sound);
            assert_eq!(target.sound_cpu, control.sound_cpu);
            assert_eq!(target.sound_schedule, control.sound_schedule);
            assert_eq!(target.bus.audio, control.bus.audio);
            assert_eq!(target.bus.vdp, control.bus.vdp);
            assert_eq!(target.cpu.registers.pc, 0x108);
        }
    }
}

#[test]
fn dram_lower_read_and_neighbor_faults_freeze_before_following_ram_store() {
    let cases = [
        (0x1080, 0xa11000, 1, ByteLanes::Upper, Direction::Write),
        (0x1080, 0xa11000, 0xff, ByteLanes::Upper, Direction::Write),
        (0x3080, 0xa11000, 0x100, ByteLanes::Both, Direction::Write),
        (0x3080, 0xa11000, 0xffff, ByteLanes::Both, Direction::Write),
        (0x1080, 0xa11001, 0, ByteLanes::Lower, Direction::Write),
        (0x1080, 0xa11001, 0xff, ByteLanes::Lower, Direction::Write),
        (0x3080, 0xa11002, 0, ByteLanes::Both, Direction::Write),
        (0x3080, 0xa110fe, 0, ByteLanes::Both, Direction::Write),
        (0x3080, 0xa10f00, 0, ByteLanes::Both, Direction::Write),
        (0x3080, 0xa11300, 0, ByteLanes::Both, Direction::Write),
        (0x3010, 0xa11000, 0, ByteLanes::Both, Direction::Read),
        (0x1010, 0xa11000, 0, ByteLanes::Upper, Direction::Read),
        (0x1010, 0xa11001, 0, ByteLanes::Lower, Direction::Read),
        (0x1080, 0xa10013, 0x38, ByteLanes::Lower, Direction::Write),
    ];
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for (opcode, address, value, lanes, direction) in cases {
                let mut machine = configured(
                    &[opcode, 0x13fc, 0x5a, 0x00ff, 0, 0x60fe],
                    region,
                    concurrent,
                );
                machine.cpu.registers.a[0] = address;
                machine.cpu.registers.d[0] = value;
                assert_eq!(
                    main_fault(machine.step_instruction().unwrap_err()),
                    TimingFault::Bus {
                        access: access(address, lanes),
                        direction,
                        source: BusError::UnsupportedAccess {
                            address: address & !1,
                            direction,
                        },
                    }
                );
                assert_eq!(machine.peek(0xff0000), Some(0));
                let frozen = machine.clone();
                for _ in 0..3 {
                    assert_eq!(
                        machine.step_instruction().map_err(main_fault),
                        Err(TimingFault::Frozen)
                    );
                    assert_eq!(machine, frozen);
                }
            }
        }
    }
}

#[test]
fn rom_mode_acknowledges_one_transfer_and_invalidates_prefetch_like_rom_write() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            let mut target = configured(&[0x4e71, 0x60fe, 0x4e71], region, concurrent);
            target.bus.prefetch = Prefetch::UpperOnly(0x5a);
            let mut control = target.clone();
            let now = target.bus.now;
            let clock = target.bus.cpu_clock;
            for (machine, address) in [(&mut target, 0xa11000), (&mut control, 0x180)] {
                let access = access(address, ByteLanes::Both);
                let result = if concurrent {
                    ScheduledBus::new(
                        &mut machine.bus,
                        &mut machine.sound_cpu,
                        &mut machine.sound_schedule,
                    )
                    .write_cycle(access, 0xff, clock)
                } else {
                    machine.bus.write_cycle(access, 0xff, clock)
                };
                assert_eq!(result.result, Ok(()));
                assert_eq!(result.wait_clocks, 0);
            }
            assert_eq!(target.bus.prefetch.upper(), None);
            assert_eq!(target.bus.now.get() - now.get(), 28);
            assert_eq!(target.bus.cpu_clock.0 - clock.0, 4);
            assert_eq!(target, control);
        }
    }
}
