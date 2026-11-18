use super::*;
use crate::cartridge::parallel_adapter::{
    Model, ParallelAdapter,
    rom_fixture::{CASES, attach, ordinary},
};
use crate::cartridge::rom_overlay::RomOverlay;
use crate::emulator::InstructionObserver;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, InstructionByteOrigin, InstructionInput};

mod pending;

const DATA: u32 = 0x8000;
const CODE: u32 = 0x8100;

fn overlay(model: Model, size: usize, program: &[u16], set: &[CheatPatch]) -> RomOverlay {
    ordinary(model, size, program)
        .prepare_rom_overlay(set)
        .unwrap()
}

fn machine(
    model: Model,
    size: usize,
    program: &[u16],
    set: &[CheatPatch],
    concurrent_sound: bool,
) -> Emulator {
    let mut cart = ordinary(model, size, program);
    cart.rom_overlay = cart.prepare_rom_overlay(set).unwrap();
    let original_ssp = match model {
        Model::First => 0x004205dc,
        Model::Second => 0x006005dc,
    };
    let m = Emulator::new_with_cartridge(
        attach(cart, model),
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
    )
    .unwrap();
    assert_eq!(
        (m.bus.cartridge.initial.ssp, m.registers().ssp),
        (original_ssp, original_ssp)
    );
    assert!(!m.supports_rom_cheats());
    m
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

fn offsets(input: InstructionInput) -> [Option<u64>; 2] {
    input.origin.map(InstructionByteOrigin::storage_offset)
}

#[derive(Default)]
struct Capture(Vec<(u32, InstructionInput)>);
impl InstructionObserver for Capture {
    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        self.0.push((address, input));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel_reader_fixture_consumes_rom_and_keeps_adapter_ram_and_guards_non_rom() {
    for (model, size, base) in CASES {
        let tail = size as u32 - 2;
        let program = [
            0x707f,
            0x727f,
            0x747f,
            0x767f,
            0x787f,
            0x1039,
            0,
            0x8000,
            0x1239,
            0,
            0x8001,
            0x3439,
            0,
            0x8000,
            0x3639,
            (tail >> 16) as u16,
            tail as u16,
            0x1839,
            ((tail + 1) >> 16) as u16,
            (tail + 1) as u16,
            0x33fc,
            0xa55a,
            (base >> 16) as u16,
            0,
            0x13fc,
            0x6b,
            (base >> 16) as u16,
            0,
            0x3a39,
            (base >> 16) as u16,
            0,
            0x1c39,
            (base >> 16) as u16,
            0,
            0x60fe,
            0x4e71,
        ];
        for concurrent in [false, true] {
            let mut m = machine(model, size, &program, &[], concurrent);
            let bytes = m.bus.cartridge.bytes.clone();
            let digest = m.bus.cartridge.rom_digest();
            for (text, expected) in [
                (None, [0x12, 0x34, 0x1234, 0x9abc, 0xbc]),
                (
                    Some(format!("008000:A55A+{tail:06X}:C73D")),
                    [0xa5, 0x5a, 0xa55a, 0xc73d, 0x3d],
                ),
                (
                    Some(format!("008000:96E1+{tail:06X}:4826")),
                    [0x96, 0xe1, 0x96e1, 0x4826, 0x26],
                ),
                (None, [0x12, 0x34, 0x1234, 0x9abc, 0xbc]),
            ] {
                m.bus.cartridge.rom_overlay = overlay(
                    model,
                    size,
                    &program,
                    &text.map(|text| patches(&text)).unwrap_or_default(),
                );
                m.reset();
                assert_eq!(
                    (m.bus.cartridge.initial.ssp, m.registers().ssp),
                    (base + 0x5dc, base + 0x5dc)
                );
                assert_eq!(m.peek(base), Some(0));
                for _ in 0..5 {
                    m.step_instruction().unwrap();
                }
                assert_eq!(m.registers().d[..5], [0x7f; 5]);
                let mut capture = Capture::default();
                m.step_instruction_traced(&mut capture).unwrap();
                assert_eq!(capture.0[0].1.value, 0x1039);
                assert_eq!(offsets(capture.0[0].1), [Some(0x20a), Some(0x20b)]);
                for _ in 0..8 {
                    m.step_instruction().unwrap();
                }
                assert_eq!(m.registers().d[..5], expected);
                assert_eq!((m.registers().d[5], m.registers().d[6]), (0x6b6b, 0x6b));
                assert_eq!(m.registers().ssp, base + 0x5dc);
                assert_eq!((m.peek(base), m.peek(base + 1)), (Some(0x6b), Some(0x6b)));
                assert_eq!(m.bus.cartridge.rom_source_offset(base), None);
                assert_eq!(
                    m.bus
                        .cartridge
                        .read_sampled(base)
                        .map(|byte| byte.origin.storage_offset()),
                    None
                );
                let mut refused = m.clone();
                let before = refused.bus.clone();
                let result = refused.bus.write_cycle(
                    access(base + 1, ByteLanes::Lower),
                    0x99,
                    refused.bus.cpu_clock,
                );
                assert!(result.result.is_err());
                assert_eq!(result.wait_clocks, 0);
                assert_eq!(refused.bus, before);
                assert!(m.encode_state().is_err());
                assert!(m.encode_frontend_state().is_err());
                assert!(m.encode_debugger_checkpoint().is_err());
                assert!(!m.supports_rom_cheats());
                let before = m.clone();
                assert!(
                    m.install_rom_patches(&[patch(DATA, 0xee), patch(size as u32, 1)])
                        .is_err()
                );
                assert_eq!(m, before);
                assert!(m.check_warm_reset().is_err());
                assert!(m.reset_cpu().is_err());
                assert!(
                    crate::hardware::bus::Bus::new_owned(m.bus.cartridge.clone(), Region::NtscUs)
                        .is_err()
                );
                assert_eq!(m, before);
                assert!(Arc::ptr_eq(&bytes, &m.bus.cartridge.bytes));
                assert_eq!(m.bus.cartridge.rom_digest(), digest);
                let guarded = if model == Model::First {
                    0x10000
                } else {
                    0x100000
                };
                assert_eq!(m.peek(guarded), None);
                assert_eq!(m.bus.cartridge.rom_source_offset(guarded), None);
                if size == 0x20000 {
                    m.bus.cartridge.rom_overlay =
                        overlay(model, size, &program, &[patch(0x10000, 0xee)]);
                    assert_eq!(m.peek(0x10000), None);
                    assert!(m.bus.cartridge.read_sampled(0x10000).is_none());
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel_reader_fixture_real_movea_jsr_rts_consumes_queued_a_then_fresh_b() {
    let program = [0x2e7c, 0x00ff, 0xfffc, 0x4eb9, 0, 0x8100, 0x60fe, 0x4e71];
    for (model, size, base) in CASES {
        for concurrent in [false, true] {
            let mut m = machine(
                model,
                size,
                &program,
                &patches("008101:07+008000:A5"),
                concurrent,
            );
            assert_eq!(m.registers().ssp, base + 0x5dc);
            m.step_instruction().unwrap();
            assert_eq!(m.bus.cartridge.initial.ssp, base + 0x5dc);
            assert_eq!(m.registers().ssp, 0x00fffffc);
            m.step_instruction().unwrap();
            assert_eq!(m.registers().pc, CODE);
            let queued = m.cpu.queued_instruction_inputs().unwrap();
            assert_eq!(queued[0].value, 0x7007);
            assert_eq!(offsets(queued[0]), [Some(0x8100), Some(0x8101)]);
            m.bus.cartridge.rom_overlay =
                overlay(model, size, &program, &patches("008101:09+008000:5A"));
            assert_eq!(m.cpu.queued_instruction_inputs().unwrap(), queued);
            assert_eq!(m.peek(CODE + 1), Some(9));
            let mut capture = Capture::default();
            for _ in 0..3 {
                m.step_instruction_traced(&mut capture).unwrap();
            }
            assert_eq!(
                (m.registers().pc, m.registers().ssp, m.registers().d[0]),
                (0x20c, 0x00fffffc, 10)
            );
            assert_eq!(capture.0[0], (CODE, queued[0]));
            assert_eq!(capture.0.last().unwrap().0, CODE + 6);
            assert!(m.encode_debugger_checkpoint().is_err());
            assert!(m.check_debug_call(CODE).is_err());
            let mut control = machine(
                model,
                size,
                &program,
                &patches("008101:09+008000:5A"),
                concurrent,
            );
            for _ in 0..5 {
                control.step_instruction().unwrap();
            }
            assert_eq!(
                (
                    control.registers().pc,
                    control.registers().ssp,
                    control.registers().d[0]
                ),
                (0x20c, 0x00fffffc, 12)
            );
            assert_eq!(control.bus.cartridge.initial.ssp, base + 0x5dc);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel_cold_prefetch_geometry_rejects_guard_crossings_without_granting_admission() {
    for (model, size, _) in CASES {
        let board = ParallelAdapter::new(model);
        for pc in [0x200, 0x8100, 0xab008100, 0xfffc] {
            assert!(board.cold_prefetch_valid(pc, size));
        }
        for pc in [0x201, size as u32 - 2, size as u32, 0x100000, 0xff008101] {
            assert!(!board.cold_prefetch_valid(pc, size));
        }
        if size == 0x20000 {
            for pc in [0xfffe, 0x10000, 0x10018, 0xab00fffe, 0xab010000] {
                assert!(!board.cold_prefetch_valid(pc, size));
            }
            for pc in [0x1001a, 0x1fffc, 0xab01001a] {
                assert!(board.cold_prefetch_valid(pc, size));
            }
        }
        let mut m = machine(model, size, &[0x60fe, 0x4e71], &[], true);
        let before = m.clone();
        assert!(!m.supports_rom_cheats());
        for text in [
            "008000:A5",
            "008000:A5+000004:0001+000006:0000",
            "008000:A5+000007:01",
        ] {
            assert!(m.install_rom_patches(&patches(text)).is_err());
            assert_eq!(m, before);
        }
    }
}
