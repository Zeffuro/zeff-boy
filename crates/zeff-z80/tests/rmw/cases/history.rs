use super::*;
use zeff_z80::{MemptrProfile, ResetState};

pub(super) fn distinguishing_histories() {
    for source in [0x07ff_u16, 0x27ff] {
        for bit in 0..8 {
            for carry in [0, 1] {
                for value in [0, 0xff, 0x28, 0x80] {
                    for mode in 0..3 {
                        let mut bus = Bus::new();
                        bus.bytes[..9].copy_from_slice(&[
                            0x3a,
                            source as u8,
                            (source >> 8) as u8,
                            0x21,
                            0,
                            0x40,
                            if carry == 1 { 0x37 } else { 0xb7 },
                            0xcb,
                            0x46 | (bit << 3),
                        ]);
                        bus.bytes[usize::from(source)] = 0;
                        bus.bytes[0x4000] = value;
                        let mut cpu = Cpu::new_with_memptr_profile(
                            ResetState::new(0, 0x8000),
                            MemptrProfile::Nmos,
                        );
                        let mut prefix = PrefixState::default();
                        let mut state = BusState::default();
                        for _ in 0..4 {
                            loop {
                                let complete = match mode {
                                    0 => {
                                        cpu.step_with_bus(&mut bus).unwrap();
                                        true
                                    }
                                    1 => cpu.step_bounded(&mut bus, &mut prefix).unwrap().complete,
                                    _ => {
                                        cpu.step_bus_segment(&mut bus, &mut state).unwrap().complete
                                    }
                                };
                                if complete {
                                    break;
                                }
                            }
                        }
                        let set = value & (1 << bit) != 0;
                        let expected = carry
                            | 0x10
                            | ((source.wrapping_add(1) >> 8) as u8 & 0x28)
                            | if bit == 7 && set { 0x80 } else { 0 }
                            | if set { 0 } else { 0x44 };
                        assert_eq!(
                            cpu.regs().f,
                            expected,
                            "source={source:04x} bit={bit} mode={mode}"
                        );
                        assert_eq!(cpu.memptr(), Some(source.wrapping_add(1)));
                        assert_eq!(cpu.trap(), None);
                        assert_eq!(bus.bytes[0x4000], value);
                        assert!(
                            !bus.trace
                                .borrow()
                                .events
                                .iter()
                                .flatten()
                                .any(|event| matches!(event, Access::Write(_, _)))
                        );
                    }
                }
            }
        }
    }
    let mut bus = Bus::new();
    bus.bytes[..2].copy_from_slice(&[0xcb, 0x7e]);
    let mut cpu = Cpu::new_with_memptr_profile(ResetState::default(), MemptrProfile::Nmos);
    let before = cpu.regs().f;
    cpu.step_with_bus(&mut bus).unwrap();
    assert_eq!(cpu.memptr(), None);
    assert_eq!(cpu.regs().f, before);
    assert_eq!(cpu.state(), CpuState::Suspended);
    assert!(cpu.trap().is_some());
}
