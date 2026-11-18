use super::support::{APIS, Api, Event, PROFILES, Pair, seeded};
use zeff_z80::{CpuState, CpuTrap};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn segmented_live_operands_waits_and_suspend_do_not_duplicate_instruction_reads() {
    for profile in PROFILES {
        let mut pair = Pair::new(&[0x3e, 0x11], 0x2000, Api::Segmented, profile);
        let mut first = pair.cycle().unwrap();
        assert!(!first.complete);
        assert_eq!(first.instruction.cycles, 4);
        pair.bytes(0x2000, &[0x3e]);
        pair.buses(|bus| {
            bus.bytes[0x2000] = 0x76;
            bus.bytes[0x2001] = 0xa5;
        });
        pair.waits(&mut first, 5, true);
        let before = pair.observed.bus.clone();
        pair.suspend();
        assert!(pair.cycle().is_none());
        assert_eq!(pair.observed.bus, before);
        pair.bytes(0x2000, &[0x3e]);
        pair.resume();
        let mut last = pair.cycle().unwrap();
        assert!(last.complete);
        assert_eq!(last.instruction.cycles, 3);
        pair.waits(&mut last, 7, true);
        pair.waits(&mut last, u32::MAX, false);
        assert_eq!(pair.cpu.regs().a, 0xa5);
        assert_eq!(pair.cpu.cycles(), 19);
        assert!(!pair.cpu.is_halted());
        pair.bytes(0x2000, &[0x3e, 0xa5]);
        assert_eq!(
            *pair.observed.bus.events.borrow(),
            vec![Event::Read(0x2000, 0x3e), Event::Read(0x2001, 0xa5)]
        );
        for code in [
            vec![0x3e, 0x5a],
            vec![0xcd, 0x34, 0x12],
            vec![0xdd, 0x26, 0x5a],
            vec![0xc6, 0x5a],
            vec![0xfd, 0xcb, 0, 0x46],
        ] {
            let mut pair = seeded(Api::Segmented, profile);
            let pc = pair.install(&code);
            let start = pair.cpu.cycles();
            let mut clocks = 0;
            let mut completed = false;
            for ordinal in 0..32 {
                let mut step = pair.cycle().unwrap();
                pair.waits(&mut step, ordinal % 5 + 1, true);
                clocks += u64::from(step.instruction.cycles);
                if step.complete {
                    completed = true;
                    break;
                }
            }
            assert!(completed);
            assert_eq!(pair.cpu.cycles() - start, clocks);
            pair.bytes(pc, &code);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn halt_opcode_poll_and_actual_interrupt_services_have_distinct_byte_observations() {
    for profile in PROFILES {
        for api in APIS {
            for mode in [0x46, 0x56, 0x5e] {
                let mut pair = Pair::new(&[0xed, mode, 0xfb, 0x76], 0x2000, api, profile);
                pair.finish();
                pair.bytes(0x2000, &[0xed, mode]);
                pair.clear();
                pair.finish();
                pair.bytes(0x2002, &[0xfb]);
                pair.clear();
                pair.finish();
                pair.bytes(0x2003, &[0x76]);
                assert!(pair.cpu.is_halted());
                pair.clear();
                pair.finish();
                pair.bytes(0, &[]);
                assert!(!pair.cpu.last_step_was_interrupt());
                assert!(pair.cpu.instruction_bytes().is_empty());
                assert_eq!(
                    *pair.observed.bus.events.borrow(),
                    vec![Event::Read(0x2004, 0)]
                );
                pair.clear();
                pair.buses(|bus| {
                    bus.irq = true;
                    bus.bytes[0xff] = 0x34;
                    bus.bytes[0x100] = 0x12;
                });
                pair.finish();
                pair.bytes(0, &[]);
                assert!(pair.cpu.last_step_was_interrupt());
                assert!(pair.cpu.instruction_bytes().is_empty());
                assert!(!pair.cpu.is_halted());
                assert!(
                    pair.observed
                        .bus
                        .events
                        .borrow()
                        .contains(&Event::Irq(0xff))
                );
                if mode == 0x5e {
                    assert_eq!(pair.cpu.regs().pc, 0x1234);
                    for event in [Event::Read(0xff, 0x34), Event::Read(0x100, 0x12)] {
                        assert!(pair.observed.bus.events.borrow().contains(&event));
                    }
                }
            }
            let mut pair = Pair::new(&[0x76], 0x2000, api, profile);
            pair.finish();
            pair.clear();
            pair.buses(|bus| bus.nmi = true);
            pair.finish();
            pair.bytes(0, &[]);
            assert!(pair.cpu.last_step_was_interrupt());
            assert_eq!(pair.cpu.regs().pc, 0x66);
            assert!(pair.observed.bus.events.borrow().contains(&Event::Nmi));
            assert!(
                pair.observed
                    .bus
                    .events
                    .borrow()
                    .contains(&Event::Read(0x2001, 0))
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn im0_instruction_source_and_real_trap_never_become_memory_instruction_reads() {
    for profile in PROFILES {
        for api in APIS {
            for opcode in [0, 0x3e, 0x21, 0xc3, 0xcd, 0xdd] {
                let mut pair = Pair::new(&[0xfb, 0], 0x3000, api, profile);
                pair.finish();
                pair.finish();
                assert!(pair.cpu.interrupts_enabled());
                pair.clear();
                pair.buses(|bus| {
                    bus.irq = true;
                    bus.ack_opcode = opcode;
                });
                pair.finish();
                pair.bytes(0, &[]);
                assert!(pair.cpu.last_step_was_interrupt());
                assert!(pair.cpu.instruction_bytes().is_empty());
                let reads = pair
                    .observed
                    .bus
                    .events
                    .borrow()
                    .iter()
                    .filter(|event| matches!(event, Event::InterruptRead(..)))
                    .count();
                assert_eq!(
                    reads,
                    match opcode {
                        0x3e => 1,
                        0x21 | 0xc3 | 0xcd => 2,
                        _ => 0,
                    }
                );
                if opcode == 0xdd {
                    assert_eq!(pair.cpu.state(), CpuState::Suspended);
                    assert_eq!(
                        pair.cpu.trap(),
                        Some(CpuTrap::UnsupportedOpcode {
                            pc: 0x3002,
                            opcode: 0xdd,
                        })
                    );
                    let bus = pair.observed.bus.clone();
                    let cpu = pair.cpu.clone();
                    assert!(pair.cycle().is_none());
                    assert_eq!(pair.cpu, cpu);
                    assert_eq!(pair.observed.bus, bus);
                    pair.bytes(0, &[]);
                } else {
                    assert_eq!(pair.cpu.trap(), None);
                    if opcode == 0x3e {
                        assert_eq!(pair.cpu.regs().a, 0x34);
                    }
                    if opcode == 0x21 {
                        assert_eq!(pair.cpu.regs().hl(), 0x1234);
                    }
                    if opcode == 0xc3 || opcode == 0xcd {
                        assert_eq!(pair.cpu.regs().pc, 0x1234);
                    }
                }
            }
        }
    }
}
