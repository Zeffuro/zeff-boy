use super::*;
use crate::cartridge::parallel_adapter::rom_fixture::{CASES, attach, ordinary};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ineligible_parallel_reader_fixtures_keep_rom_receipts_and_equal_ram_unknown() {
    for (model, size, base) in CASES {
        let mut cart = ordinary(model, size, &[0x4e70, 0x4e71, 0x4e71, 0x4e71]);
        patches(&mut cart, 0x201, 0x71);
        let cart = attach(cart, model);
        assert!(!cart.supports_rom_overlay());
        for route in 0..3 {
            for port_history in [false, true] {
                for observed in [false, true] {
                    let mut bus = irq_bus(cart.clone(), port_history);
                    let result = fetch(
                        route,
                        &mut bus,
                        access(0x01000200, ByteLanes::Both),
                        observed,
                    );
                    let rom = result.cycle.result.unwrap();
                    assert_eq!(rom.value, 0x4e71);
                    assert_eq!(offsets(rom), [Some(0x200), Some(0x201)]);
                    assert_eq!(cart.bytes[0x201], 0x70);
                    assert_eq!(
                        (result.cycle.wait_clocks, bus.cpu_clock, bus.now.get()),
                        (0, CpuClocks(4), 28)
                    );
                    assert_eq!(
                        result.samples.map(|samples| samples.levels()),
                        (observed && !port_history).then_some((4, 4))
                    );
                    let write = BusAccess {
                        kind: AccessKind::Data,
                        function_code: FunctionCode::SupervisorData,
                        ..access(base, ByteLanes::Both)
                    };
                    assert_eq!(
                        bus.cartridge
                            .parallel_adapter
                            .as_mut()
                            .unwrap()
                            .write(write, 0x4e71),
                        Some(true)
                    );
                    let ram = fetch(route, &mut bus, access(base, ByteLanes::Both), observed)
                        .cycle
                        .result
                        .unwrap();
                    assert_eq!(ram.value, rom.value);
                    assert_eq!(ram.origin, [InstructionByteOrigin::UNKNOWN; 2]);
                    assert_eq!(bus.cartridge.rom_source_offset(base), None);
                    let guarded = if size == 0x20000 { 0x10000 } else { 0x100000 };
                    assert!(
                        fetch(route, &mut bus, access(guarded, ByteLanes::Both), observed)
                            .cycle
                            .result
                            .is_err()
                    );
                    assert_eq!(bus.cartridge.rom_source_offset(guarded), None);
                }
            }
        }
        for concurrent_sound in [false, true] {
            let mut m = Emulator::new_with_cartridge(
                cart.clone(),
                LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound,
                },
            )
            .unwrap();
            let queued = m.cpu.queued_instruction_inputs().unwrap()[0];
            assert_eq!(queued.value, 0x4e71);
            assert_eq!(offsets(queued), [Some(0x200), Some(0x201)]);
            assert!(!m.supports_rom_cheats());
            let start = m.cpu_clock().0;
            let mut capture = Capture::default();
            let step = m.step_instruction_traced(&mut capture).unwrap();
            let TimedOutcome::Instruction(step) = step.cpu else {
                panic!("reader NOP not consumed");
            };
            assert_eq!(step.opcode, 0x4e71);
            assert_eq!(step.exception, None);
            assert_eq!(m.cpu_clock().0 - start, 4);
            assert_eq!(capture.inputs, [(0x200, queued)]);
            assert_eq!(capture.words, [(0x200, 0x4e71)]);
            assert!(capture.memory.is_empty());
            assert_eq!(m.registers().pc, 0x202);
        }
    }
}
