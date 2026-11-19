use super::*;
use crate::emulator::{Emulator, LoadConfig, MachineFault, Region, TimingModel};

#[derive(Default)]
struct Capture {
    words: Vec<(u32, u16)>,
    writes: Vec<(BusAccess, u16)>,
    memory: Vec<MemoryAccess>,
    boundaries: Vec<TimedOutcome>,
}

impl InstructionObserver for Capture {
    fn instruction_word(&mut self, address: u32, value: u16) {
        self.words.push((address, value));
    }
    fn write(&mut self, access: BusAccess, value: u16) {
        self.writes.push((access, value));
    }
    fn memory_access(&mut self, access: MemoryAccess) {
        self.memory.push(access);
    }
    fn cpu_boundary(&mut self, outcome: TimedOutcome) {
        self.boundaries.push(outcome);
    }
}

fn fixture(program: &[u16], region: Region, concurrent: bool, timing: TimingModel) -> Emulator {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
            concurrent_sound: concurrent,
        },
        timing,
    )
    .unwrap()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn instruction_adapter_keeps_original_words_filtered_writes_lanes_and_samples_once() {
    for fail in [false, true] {
        let mut spy = Spy {
            fail,
            ..Spy::default()
        };
        let mut capture = Capture::default();
        {
            let mut bus = ObservedBus {
                inner: &mut spy,
                observer: &mut capture,
            };
            bus.instruction_word(0x1234_5678, 0xa55a);
            for kind in [
                AccessKind::Opcode,
                AccessKind::Extension,
                AccessKind::ResetVector,
                AccessKind::Vector,
                AccessKind::Stack,
                AccessKind::Data,
                AccessKind::Discard,
                AccessKind::Prefetch,
                AccessKind::InterruptAcknowledge,
            ] {
                for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                    let access = access(kind, lanes);
                    let cycle = bus.write_cycle(access, 0x1234, CpuClocks(37));
                    assert_eq!(cycle.result.is_err(), fail);
                    assert_eq!(cycle.wait_clocks, 7);
                    let observed = bus.write_cycle_observed(access, 0x5678, CpuClocks(37));
                    assert_eq!(observed.cycle.result.is_err(), fail);
                    assert_eq!(observed.samples.unwrap().levels(), (3, 5));
                }
            }
        }
        assert_eq!(spy.calls.len(), 54);
        assert_eq!(spy.words, [(0x1234_5678, 0xa55a)]);
        assert_eq!(capture.words, spy.words);
        assert!(capture.boundaries.is_empty());
        assert_eq!(capture.writes.len(), if fail { 0 } else { 12 });
        assert_eq!(capture.memory.len(), if fail { 0 } else { 16 });
        if !fail {
            for chunk in capture.writes.as_chunks::<6>().0 {
                assert_eq!(
                    chunk
                        .iter()
                        .map(|(access, value)| (access.lanes, *value))
                        .collect::<Vec<_>>(),
                    [
                        (ByteLanes::Upper, 0x1234),
                        (ByteLanes::Upper, 0x5678),
                        (ByteLanes::Lower, 0x1234),
                        (ByteLanes::Lower, 0x5678),
                        (ByteLanes::Both, 0x1234),
                        (ByteLanes::Both, 0x5678)
                    ]
                );
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn instruction_observer_reports_unchanged_ignored_and_rmw_driven_writes_without_storage_claims() {
    for concurrent in [false, true] {
        let program = [
            0x33fc, 0, 0xff, 0x40, 0x13fc, 0x5a, 0xff, 0x41, 0x23fc, 0x1234, 0x5678, 0, 0x380,
            0x4ad0, 0x4e71,
        ];
        let mut actual = fixture(&program, Region::NtscUs, concurrent, TimingModel::Standard);
        actual.cpu.registers.a[0] = 0xff0041;
        let mut expected = actual.clone();
        let mut capture = Capture::default();
        for _ in 0..4 {
            assert_eq!(
                actual.step_instruction_traced(&mut capture),
                expected.step_instruction()
            );
            assert_eq!(actual, expected);
        }
        assert_eq!(capture.writes.len(), 5);
        assert_eq!(
            capture
                .writes
                .iter()
                .map(|(access, value)| (access.bus_address, access.lanes, *value))
                .collect::<Vec<_>>(),
            [
                (0xff0040, ByteLanes::Both, 0),
                (0xff0040, ByteLanes::Lower, 0x5a),
                (0x380, ByteLanes::Both, 0x1234),
                (0x382, ByteLanes::Both, 0x5678),
                (0xff0040, ByteLanes::Lower, 0xda)
            ]
        );
        assert_eq!(actual.peek(0xff0041), Some(0x5a));
        assert_eq!(actual.peek(0x380), Some(0));
        assert!(actual.bus.rmw.is_none());
        assert_eq!(capture.boundaries.len(), 4);
        assert_eq!(
            actual.encode_state().unwrap(),
            expected.encode_state().unwrap()
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn instruction_boundary_success_precedes_late_sound_fault_without_fabricating_cpu_failure() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut actual = fixture(&[0x4e71; 32], region, true, TimingModel::Standard);
        actual.bus.sound.ram[..3].copy_from_slice(&[0x3a, 0, 0x7f]);
        actual.bus.sound.set_reset(false);
        let mut expected = actual.clone();
        let mut failed = false;
        for _ in 0..16 {
            let mut capture = Capture::default();
            let result = actual.step_instruction_traced(&mut capture);
            assert_eq!(result, expected.step_instruction());
            assert_eq!(actual, expected);
            assert_eq!(capture.words.len(), 1);
            assert_eq!(capture.words[0].1, 0x4e71);
            assert!(capture.writes.is_empty());
            assert!(matches!(
                capture.boundaries.as_slice(),
                [TimedOutcome::Instruction(_)]
            ));
            if let Err(fault) = result {
                assert!(matches!(fault, MachineFault::Sound(_)));
                failed = true;
                break;
            }
        }
        assert!(
            failed,
            "sound operand must fail after a successful MAIN boundary"
        );
    }
}
