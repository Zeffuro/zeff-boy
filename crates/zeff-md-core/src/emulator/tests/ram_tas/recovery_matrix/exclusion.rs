use super::*;
use common::*;
use zeff_m68k::bus::Direction;

fn board_rom(profile: usize) -> Vec<u8> {
    let mut image = crate::cartridge::registers::tests::rom(profile);
    image[0x200..0x208].copy_from_slice(&[0x4a, 0xd0, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71]);
    image[0x1ffffe..].fill(0);
    let sum = image[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, p| {
            sum.wrapping_add(u16::from_be_bytes([p[0], p[1]]))
        });
    let expected = [0x1c2a_u16, 0x5d98, 0x0211][profile];
    image[0x1ffffe..].copy_from_slice(&expected.wrapping_sub(sum).to_be_bytes());
    image
}

fn target_machine(class: usize, concurrent: bool) -> Emulator {
    let mut image = if class == 3 {
        board_rom(0)
    } else {
        let mut image = vec![0; 0x400];
        image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        image[0x200..0x208].copy_from_slice(&[0x4a, 0xd0, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71]);
        image
    };
    if class == 1 {
        image[0x1b0..0x1bc].copy_from_slice(b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x3f\xff");
    }
    if class == 2 {
        image[0x180..0x18e].copy_from_slice(b"GM T-12046 -00");
        image[0x1b0..0x1bc].copy_from_slice(b"RA\xe8\x40\x00\x20\x00\x01\x00\x20\x00\x01");
    }
    let mut machine = Emulator::new(
        image,
        LoadConfig {
            concurrent_sound: concurrent,
            ..Default::default()
        },
    )
    .unwrap();
    if matches!(class, 1 | 2) {
        assert!(machine.bus.cartridge.save.is_some());
    }
    if class == 3 {
        assert!(machine.bus.cartridge.registers.is_some());
    }
    machine.cpu.registers.a[0] = [
        0x100, 0x200001, 0x200001, 0x400000, 0xa00000, 0xc00000, 0xc0000e,
    ][class];
    machine
}

struct Reject<'a> {
    inner: Adapter<'a>,
    begin: usize,
    operands: usize,
    end: usize,
}
impl ClockedBus for Reject<'_> {
    type Error = BusError;
    fn begin_rmw(&mut self, a: BusAccess, t: CpuClocks) -> Result<BusResponse<()>, BusError> {
        self.begin += 1;
        let before = self.inner.inner.bus.clone();
        let r = self.inner.begin_rmw(a, t);
        assert_eq!(r, Ok(BusResponse::Unsupported));
        assert_eq!(*self.inner.inner.bus, before);
        r
    }
    fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, BusError> {
        self.operands += 1;
        panic!("unsupported operand");
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), BusError> {
        self.operands += 1;
        panic!("unsupported write");
    }
    fn idle(&mut self, _: CpuClocks, _: u32) {
        self.operands += 1;
        panic!("unsupported idle");
    }
    fn end_rmw(&mut self, _: CpuClocks) {
        self.end += 1;
        panic!("unsupported end");
    }
}

fn targets() {
    for class in 0..7 {
        for concurrent in [false, true] {
            let mut machine = target_machine(class, concurrent);
            let before = machine.clone();
            let mut cpu = machine.cpu.clone();
            let mut bus = Reject {
                inner: adapter(&mut machine),
                begin: 0,
                operands: 0,
                end: 0,
            };
            assert!(matches!(
                cpu.step_instruction(&mut bus),
                Err(TimingFault::RmwOwnershipUnsupported { .. })
            ));
            assert_eq!((bus.begin, bus.operands, bus.end), (1, 0, 0));
            assert_eq!(machine, before);
            assert_eq!(cpu.registers, before.cpu.registers);
            assert_eq!(cpu.clock(), before.cpu.clock());
            assert_eq!(cpu.queued_words(), before.cpu.queued_words());
            machine.cpu = cpu;
            let mut calls = 0;
            frozen(&mut machine, &mut calls);
        }
    }
}

fn malformed() {
    for kind in 0..7 {
        let case = Case {
            kind: Kind::Immediate,
            residue: 0,
            edge: 0,
            lane: 0,
        };
        let (mut machine, _) = seed(false, &case);
        let clock = machine.bus.cpu_clock;
        let mut bad = access(0);
        match kind {
            0 => bad.lanes = ByteLanes::Both,
            1 => bad.kind = AccessKind::Prefetch,
            2 => bad.kind = AccessKind::Stack,
            3 => bad.function_code = FunctionCode::SupervisorProgram,
            4 => bad.bus_address |= 1,
            5 => bad.logical_address = 0x000e,
            _ => assert_eq!(
                adapter(&mut machine).begin_rmw(access(0), clock),
                Ok(BusResponse::Complete(()))
            ),
        }
        let before = machine.clone();
        assert_eq!(
            adapter(&mut machine).begin_rmw(bad, clock),
            Ok(BusResponse::Unsupported)
        );
        assert_eq!(machine, before);
        assert_eq!(machine.bus.rmw.is_some(), kind == 6);
        if kind == 6 {
            adapter(&mut machine).end_rmw(clock);
        }
        assert!(machine.bus.rmw.is_none());
    }
}

fn callback_guards() {
    for write in [false, true] {
        for mismatch in 0..3 {
            for concurrent in [false, true] {
                let case = Case {
                    kind: Kind::Immediate,
                    residue: 0,
                    edge: 0,
                    lane: 0,
                };
                let (mut machine, mut expected) = seed(concurrent, &case);
                let clock = machine.bus.cpu_clock;
                adapter(&mut machine).begin_rmw(access(0), clock).unwrap();
                if write {
                    if let Some(expected) = &mut expected {
                        expected.read();
                    }
                    adapter(&mut machine)
                        .read_cycle(access(0), clock)
                        .result
                        .unwrap();
                    let clock = machine.bus.cpu_clock;
                    if let Some(expected) = &mut expected {
                        expected.held(14);
                    }
                    adapter(&mut machine).idle(clock, 2);
                }
                let mut bad = access(0);
                match mismatch {
                    0 => {
                        bad.logical_address += 2;
                        bad.bus_address += 2;
                    }
                    1 => bad.lanes = ByteLanes::Lower,
                    _ => bad.function_code = FunctionCode::UserData,
                }
                let clock = machine.bus.cpu_clock;
                let before = machine.clone();
                let error = BusError::UnsupportedAccess {
                    address: bad.bus_address,
                    direction: if write {
                        Direction::Write
                    } else {
                        Direction::Read
                    },
                };
                if write {
                    assert_eq!(
                        adapter(&mut machine).write_cycle(bad, 0x8000, clock),
                        CycleResult {
                            result: Err(error),
                            wait_clocks: 0
                        }
                    );
                } else {
                    assert_eq!(
                        adapter(&mut machine).read_cycle(bad, clock),
                        CycleResult {
                            result: Err(error),
                            wait_clocks: 0
                        }
                    );
                }
                assert_eq!(machine, before);
                adapter(&mut machine).end_rmw(clock);
                if let Some(expected) = &mut expected {
                    if expected.lock.is_some() {
                        expected.end();
                    }
                    expected.check(&adapter(&mut machine).inner);
                }
                assert!(machine.bus.rmw.is_none());
                assert!(machine.sound_schedule.rmw_checkpoint.is_none());
                if let Some(expected) = expected {
                    future(&mut machine, expected);
                }
            }
        }
    }
    for concurrent in [false, true] {
        let case = Case {
            kind: Kind::Immediate,
            residue: 0,
            edge: 0,
            lane: 0,
        };
        let (mut machine, mut model) = seed(concurrent, &case);
        let clock = machine.bus.cpu_clock;
        adapter(&mut machine).begin_rmw(access(0), clock).unwrap();
        adapter(&mut machine)
            .read_cycle(access(0), clock)
            .result
            .unwrap();
        if let Some(model) = &mut model {
            model.read();
        }
        let clock = machine.bus.cpu_clock;
        let mut expected = machine.clone();
        expected.bus.clock_fault = true;
        adapter(&mut machine).idle(clock, 3);
        assert_eq!(machine, expected);
        assert_eq!(
            adapter(&mut machine).write_cycle(access(0), 0x8000, clock),
            CycleResult {
                result: Err(BusError::ClockOverflow),
                wait_clocks: 0
            },
        );
        assert_eq!(machine, expected);
        adapter(&mut machine).end_rmw(clock);
        if let Some(model) = &mut model {
            model.end();
            model.check(&adapter(&mut machine).inner);
        }
        assert!(machine.bus.rmw.is_none());
        assert!(machine.bus.clock_fault);
        assert!(machine.sound_schedule.rmw_checkpoint.is_none());
        if let Some(model) = model {
            future(&mut machine, model);
        }
    }
}

fn ordinary() {
    for kind in [Kind::Ldi, Kind::BankM1, Kind::BankPsg] {
        let case = Case {
            kind,
            residue: 0,
            edge: 0,
            lane: 0,
        };
        let (mut machine, expected) = seed(true, &case);
        let mut expected = expected.unwrap();
        let mut registers = machine.cpu.registers;
        let clock = machine.cpu.clock().0;
        machine.cpu = complete_cpu(registers, [0x1010, 0x4e71], clock);
        let waits = u64::from(expected.ordinary()) + u64::from(expected.ordinary());
        let mut cpu = machine.cpu.clone();
        assert_eq!(
            cpu.step_instruction(&mut adapter(&mut machine))
                .unwrap()
                .clocks,
            8 + waits
        );
        registers.pc += 2;
        registers.set_status_register(0x2714);
        assert_eq!(
            cpu,
            complete_cpu(registers, [0x4e71, 0x4e71], clock + 8 + waits)
        );
        machine.cpu = cpu;
        expected.check(&adapter(&mut machine).inner);
        let start = case.s() + if kind == Kind::BankM1 { 0 } else { 30 };
        assert_eq!(
            expected.target,
            Some((start, start + if kind == Kind::BankM1 { 60 } else { 45 }))
        );
        assert_eq!(expected.deferrals, 0);
        future(&mut machine, expected);
    }
}

fn z80_guard() {
    for profile in 0..3 {
        for opcode in [0x34, 0x35] {
            let mut machine = Emulator::new(
                board_rom(profile),
                LoadConfig {
                    concurrent_sound: true,
                    ..Default::default()
                },
            )
            .unwrap();
            machine.cpu.registers.a[0] = 0xffff000e;
            machine.bus.ram[0x0e] = 0;
            machine.step_instruction().unwrap();
            assert_eq!(machine.bus.ram[0x0e], 0);
            let board = machine.bus.cartridge.clone();
            let ram = machine.bus.ram.clone();
            machine.bus.sound.set_reset(false);
            machine.bus.sound.bank = 0x80;
            machine.bus.sound.ram[..5].copy_from_slice(&[0x21, 0, 0x80, opcode, 0x76]);
            advance(&mut machine, 400);
            assert_eq!(
                machine.sound_fault(),
                Some(crate::SoundFault::UnsupportedAccess {
                    address: 0x8000,
                    direction: Direction::Read
                })
            );
            assert_eq!(machine.sound_cpu.cycles(), 14);
            assert!(machine.sound_schedule.pending.is_none());
            assert!(machine.sound_schedule.rmw_checkpoint.is_none());
            assert_eq!(machine.bus.cartridge, board);
            assert_eq!(machine.bus.ram, ram);
            let before = machine.clone();
            assert!(machine.step_instruction().is_err());
            assert_eq!(machine, before);
        }
    }
}

pub(super) fn rows() {
    targets();
    malformed();
    callback_guards();
    ordinary();
    z80_guard();
}
