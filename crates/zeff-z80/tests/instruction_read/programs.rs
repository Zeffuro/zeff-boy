use super::support::{APIS, Api, Event, PROFILES, Pair, seeded};
use zeff_z80::MemptrProfile;

fn programs(value: u8) -> Vec<Vec<u8>> {
    vec![
        vec![0x3e, value],
        vec![0x21, value, 0x12],
        vec![0x31, value, 0x12],
        vec![0x36, value],
        vec![0x32, 0, 0x40],
        vec![0x3a, 0, 0x40],
        vec![0x2a, 0, 0x40],
        vec![0x22, 0, 0x40],
        vec![0xc6, value],
        vec![0xce, value],
        vec![0xc3, value, 0x12],
        vec![0xca, value, 0x12],
        vec![0xcd, value, 0x12],
        vec![0xcc, value, 0x12],
        vec![0xc4, value, 0x12],
        vec![0xdd, 0x26, value],
        vec![0xfd, 0x2e, value],
        vec![0xdd, 0xc6, value],
        vec![0xfd, 0xce, value],
        vec![0xdd, 0x21, value, 0x12],
        vec![0xfd, 0x22, 0, 0x40],
        vec![0xed, 0x43, 0, 0x40],
        vec![0xed, 0x4b, 0, 0x40],
        vec![0xdd, 0x36, 0, value],
        vec![0xfd, 0x36, 0xff, value],
        vec![0x18, value],
        vec![0x10, value],
        vec![0xdd, 0xfd, 0xdd, 0xfd, 0xcb, 0, 0x46],
    ]
}

fn check(program: &[u8], api: Api, profile: MemptrProfile) -> Pair {
    let mut pair = seeded(api, profile);
    let pc = pair.install(program);
    pair.finish();
    pair.bytes(pc, program);
    pair.same();
    pair
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn first_real_load_records_opcode_and_operand_with_exact_default_execution() {
    for profile in PROFILES {
        for api in APIS {
            let mut pair = Pair::new(&[0x3e, 0x5a], 0x2000, api, profile);
            pair.finish();
            pair.bytes(0x2000, &[0x3e, 0x5a]);
            assert_eq!(pair.cpu.regs().a, 0x5a);
            assert_eq!(pair.cpu.regs().pc, 0x2002);
            assert_eq!(pair.cpu.cycles(), 7);
            assert_eq!(
                *pair.observed.bus.events.borrow(),
                vec![Event::Read(0x2000, 0x3e), Event::Read(0x2001, 0x5a)]
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn classified_fetch_families_record_real_operands_without_changing_execution() {
    let mut cells = 0;
    for profile in PROFILES {
        for value in [0, 0x80, 0xff] {
            let codes = programs(value);
            assert_eq!(codes.len(), 28);
            for code in codes {
                let mut whole: Option<Pair> = None;
                for api in APIS {
                    let pair = check(&code, api, profile);
                    if let Some(whole) = &whole {
                        assert_eq!(pair.cpu, whole.cpu);
                        assert_eq!(pair.observed.bus, whole.observed.bus);
                        assert_eq!(pair.observed.instruction, whole.observed.instruction);
                    } else {
                        whole = Some(pair);
                    }
                    cells += 1;
                }
            }
        }
    }
    assert_eq!(cells, 504);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn data_stack_and_io_reads_are_excluded_while_real_bus_effects_remain_exact() {
    let cases = [
        (vec![0x0a], vec![Event::Read(0x4000, 0x5a)]),
        (vec![0x1a], vec![Event::Read(0x4100, 0x5a)]),
        (vec![0xdd, 0x0a], vec![Event::Read(0x4000, 0x5a)]),
        (vec![0xfd, 0x1a], vec![Event::Read(0x4100, 0x5a)]),
        (vec![0x7e], vec![Event::Read(0x4200, 0x5a)]),
        (vec![0x86], vec![Event::Read(0x4200, 0x5a)]),
        (vec![0x3a, 0, 0x40], vec![Event::Read(0x4000, 0x5a)]),
        (
            vec![0x2a, 0, 0x40],
            vec![Event::Read(0x4000, 0x5a), Event::Read(0x4001, 0xa5)],
        ),
        (
            vec![0xed, 0x4b, 0, 0x40],
            vec![Event::Read(0x4000, 0x5a), Event::Read(0x4001, 0xa5)],
        ),
        (vec![0xdd, 0x7e, 0], vec![Event::Read(0x4300, 0x5a)]),
        (vec![0xfd, 0x86, 0], vec![Event::Read(0x4400, 0x5a)]),
        (vec![0xcb, 0x46], vec![Event::Read(0x4200, 0x5a)]),
        (vec![0xdd, 0xcb, 0, 0x46], vec![Event::Read(0x4300, 0x5a)]),
        (vec![0xfd, 0xcb, 0, 0x46], vec![Event::Read(0x4400, 0x5a)]),
        (
            vec![0xc1],
            vec![Event::Read(0x9000, 0x34), Event::Read(0x9001, 0x12)],
        ),
        (
            vec![0xc9],
            vec![Event::Read(0x9000, 0x34), Event::Read(0x9001, 0x12)],
        ),
        (
            vec![0xed, 0xa0],
            vec![Event::Read(0x4200, 0x5a), Event::Write(0x4100, 0x5a)],
        ),
        (vec![0xdb, 0x80], vec![Event::IoRead(0x80, 0x25)]),
        (vec![0xd3, 0x80], vec![Event::IoWrite(0x80, 0x5a)]),
        (vec![0xed, 0x40], vec![Event::IoRead(0, 0xa5)]),
        (vec![0x32, 0, 0x40], vec![Event::Write(0x4000, 0x5a)]),
    ];
    for profile in PROFILES {
        for api in APIS {
            for (code, required) in &cases {
                let pair = check(code, api, profile);
                let events = pair.observed.bus.events.borrow();
                for event in required {
                    assert!(events.contains(event), "{code:02x?}: {event:?}");
                }
            }
            let pair = check(&[0xcd, 0x34, 0x12], api, profile);
            let return_pc = 0x1019_u16;
            let [lo, hi] = return_pc.to_le_bytes();
            assert_eq!(pair.cpu.regs().pc, 0x1234);
            assert_eq!(pair.cpu.regs().sp, 0x8ffe);
            assert!(
                pair.observed
                    .bus
                    .events
                    .borrow()
                    .ends_with(&[Event::Write(0x8fff, hi), Event::Write(0x8ffe, lo),])
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn long_prefix_and_wrapping_fetches_keep_all_actual_bytes_beyond_cpu_window() {
    let program = [0xdd, 0xfd, 0xdd, 0xfd, 0xdd, 0xfd, 0x3e, 0x5a];
    for profile in PROFILES {
        for api in APIS {
            for pc in [0x1000, 0xfffc] {
                let mut pair = Pair::new(&program, pc, api, profile);
                pair.finish();
                pair.bytes(pc, &program);
                assert_eq!(pair.cpu.regs().a, 0x5a);
                assert_eq!(pair.cpu.regs().pc, pc.wrapping_add(8));
                assert_eq!(pair.cpu.cycles(), 31);
                assert_eq!(pair.cpu.last_m1_fetch_count(), 7);
                assert_eq!(pair.cpu.instruction_bytes(), &program[..4]);
                assert_eq!(pair.observed.instruction.borrow().len(), 8);
            }
        }
    }
}
