#[path = "indexed_rmw.rs"]
mod indexed_rmw;
use super::super::prefix_tests::{Bus, restored};
use super::*;

fn read(address: u16, value: u8, interrupts: bool) -> Bus {
    Bus {
        sample: CycleSample {
            address,
            value,
            known_mask: 255,
            m1: false,
            write: false,
        },
        interrupts: CycleInterrupts {
            maskable_pending: interrupts,
            nmi_pending: interrupts,
        },
        calls: std::cell::Cell::new(0),
    }
}

pub(super) fn retirement() {
    indexed_rmw::retirement();
    for prefix in [0xdd, 0xfd] {
        for profile in [MemptrProfile::Legacy, MemptrProfile::Nmos] {
            for opcode in 0x40..=0x7f {
                for (base, displacement, value, waits) in [
                    (0, 0x80, 0, 0),
                    (0xffff, 0x7f, 0xff, 2),
                    (0x17bb, 6, 0x80, 0),
                ] {
                    case((prefix, opcode), profile, base, displacement, value, waits);
                }
            }
        }
    }
}

fn case(
    instruction: (u8, u8),
    profile: MemptrProfile,
    base: u16,
    displacement: u8,
    value: u8,
    waits: u32,
) {
    let (prefix, opcode) = instruction;
    let mut cpu = Cpu::new_with_memptr_profile(ResetState::new(0xfffe, 0x2345), profile);
    cpu.regs.ix = base;
    cpu.regs.iy = base;
    cpu.regs.r = 0xff;
    cpu.regs.i = 0xa7;
    cpu.regs.f = Z80_FLAG_CARRY;
    let mut state = BusState::default();
    for byte in [prefix, 0xcb] {
        let mut bus = read(cpu.regs.pc, byte, false);
        assert!(!cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete);
    }
    let effective = base.wrapping_add_signed(i16::from(displacement as i8));
    let before_regs = cpu.regs;
    cpu.interrupt_flip_flop_1 = true;
    cpu.interrupt_flip_flop_2 = true;
    cpu.enable_interrupts_delay = 1;
    for (phase, byte) in [displacement, opcode, value].into_iter().enumerate() {
        let request = cpu.next_indexed_cb_cycle(&state).unwrap();
        let kind = request.kind();
        assert_eq!(kind.t_states(), [3, 5, 4][phase]);
        assert_eq!(
            kind.address(),
            if phase == 2 { effective } else { phase as u16 }
        );
        assert_eq!(
            request.preceding_address(),
            if phase == 0 { 0xa780 } else { phase as u16 - 1 }
        );
        let receipt = IndexedMemoryReceipt::Read {
            address: kind.address(),
            value: byte,
            known_mask: 255,
        };
        if prefix == 0xdd && profile == MemptrProfile::Nmos && opcode == 0x40 && base == 0 {
            guards(&cpu, state, receipt, phase);
        }
        for malformed in [
            IndexedMemoryReceipt::Internal,
            IndexedMemoryReceipt::Write {
                address: kind.address(),
                value: byte,
            },
            IndexedMemoryReceipt::Read {
                address: kind.address() ^ 1,
                value: byte,
                known_mask: 255,
            },
            IndexedMemoryReceipt::Read {
                address: kind.address(),
                value: byte,
                known_mask: 254,
            },
        ] {
            assert!(
                cpu.next_indexed_cb_cycle(&state)
                    .unwrap()
                    .complete(malformed, waits)
                    .is_err()
            );
        }
        let (mut oracle, mut expected_state) = (cpu.clone(), state);
        let mut bus = read(kind.address(), byte, true);
        let mut expected = oracle
            .step_bus_segment(&mut bus, &mut expected_state)
            .unwrap();
        assert_eq!(bus.calls.get(), 1);
        assert!(oracle.extend_bus_segment(&mut expected.instruction, waits));
        let (mut clone, mut clone_state) = restored(&cpu, &state);
        let completion = request.complete(receipt, waits).unwrap();
        let mut stale = cpu.clone();
        stale.regs.f ^= 1;
        let stale_before = stale.clone();
        let mut stale_state = state;
        assert!(matches!(
            stale.retire_indexed_cb_cycle(&mut stale_state, completion),
            Err(CycleError::StaleContinuation)
        ));
        assert_eq!((stale, stale_state), (stale_before, state));
        let completion = cpu
            .next_indexed_cb_cycle(&state)
            .unwrap()
            .complete(receipt, waits)
            .unwrap();
        assert_eq!(
            cpu.retire_indexed_cb_cycle(&mut state, completion).unwrap(),
            expected
        );
        let completion = clone
            .next_indexed_cb_cycle(&clone_state)
            .unwrap()
            .complete(receipt, waits)
            .unwrap();
        assert_eq!(
            clone
                .retire_indexed_cb_cycle(&mut clone_state, completion)
                .unwrap(),
            expected
        );
        assert_eq!((&cpu, state), (&oracle, expected_state));
        assert_eq!((&clone, clone_state), (&cpu, state));
        assert_eq!(cpu.enable_interrupts_delay, u8::from(phase != 2));
        assert_eq!(
            cpu.memptr(),
            if profile == MemptrProfile::Nmos && phase != 0 {
                Some(effective)
            } else {
                None
            }
        );
    }
    assert!(!state.pending());
    assert_eq!(cpu.regs.pc, 2);
    assert_eq!(cpu.cycles(), 20 + 3 * u64::from(waits));
    assert_eq!(cpu.regs.r, before_regs.r);
    let mut registers = cpu.regs;
    registers.pc = before_regs.pc;
    registers.f = before_regs.f;
    assert_eq!(registers, before_regs);
    assert_eq!(
        cpu.regs.f,
        cb::bit_flags(
            (opcode >> 3) & 7,
            value,
            Z80_FLAG_CARRY,
            (effective >> 8) as u8
        )
    );
}

fn guards(cpu: &Cpu, state: BusState, receipt: IndexedMemoryReceipt, phase: usize) {
    assert!(
        cpu.next_indexed_cb_cycle(&state)
            .unwrap()
            .complete(receipt, u32::MAX)
            .is_err()
    );
    let mut overflow = cpu.clone();
    overflow.cycles = u64::MAX;
    assert!(
        overflow
            .next_indexed_cb_cycle(&state)
            .unwrap()
            .complete(receipt, 0)
            .is_err()
    );
    if phase == 2 {
        let IndexedMemoryReceipt::Read { address, .. } = receipt else {
            unreachable!()
        };
        let wrong = rmw::Rmw::indexed_cb(address, 0);
        let mut scratch = cpu.clone();
        let (write, _) = wrong
            .complete_cycle(&mut scratch, rmw::Sample::Read(0x81))
            .unwrap();
        for operation in [wrong, write.unwrap(), rmw::Rmw::cb(cpu, 0x40)] {
            let mut writer = crate::state::Writer::new();
            writer.write_u8(2);
            operation.write_continuation(&mut writer);
            let bytes = writer.into_bytes();
            let indexed =
                indexed_cb::IndexedCb::read_continuation(&mut crate::state::Reader::new(&bytes))
                    .unwrap();
            let mut forged = state;
            forged.transfer.as_mut().unwrap().operation = Operation::IndexedCb(indexed);
            assert!(cpu.indexed_cb_cycle_pending(&forged));
            assert!(cpu.next_indexed_cb_cycle(&forged).is_err());
        }
    }
    for field in 0..5 {
        let mut forged = cpu.clone();
        match field {
            0 => forged.instruction_byte_count = 5,
            1 => forged.last_m1_fetch_count = 3,
            2 => forged.instruction_bytes[1] = 0xed,
            3 => forged.regs.pc ^= 1,
            _ => {
                if phase == 0 {
                    forged.regs.ix ^= 1;
                } else {
                    forged.instruction_bytes[2] ^= 1;
                }
            }
        }
        assert!(forged.next_indexed_cb_cycle(&state).is_err());
    }
    for profile in [false, true] {
        let completion = cpu
            .next_indexed_cb_cycle(&state)
            .unwrap()
            .complete(receipt, 0)
            .unwrap();
        let mut stale = cpu.clone();
        if profile {
            stale.memptr_profile = MemptrProfile::Legacy;
            stale.memptr = None;
        } else {
            stale.memptr = Some(stale.memptr.unwrap_or(0) ^ 1);
        }
        let before = stale.clone();
        let mut after = state;
        assert!(matches!(
            stale.retire_indexed_cb_cycle(&mut after, completion),
            Err(CycleError::StaleContinuation)
        ));
        assert_eq!((stale, after), (before, state));
    }
}
