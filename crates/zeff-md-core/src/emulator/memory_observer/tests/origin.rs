use super::*;
use crate::cartridge::{Cartridge, registers::LockOn};
use crate::emulator::{Emulator, LoadConfig, Region};
use crate::hardware::bus::{
    Bus, BusError, Prefetch,
    scheduled::{CompactBus, ScheduledBus},
};
use crate::hardware::sound::schedule::SoundSchedule;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_m68k::timing::InstructionByteOrigin;

fn access(logical: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: logical,
        bus_address: logical & 0x00ff_fffe,
        function_code: FunctionCode::SupervisorProgram,
        kind: AccessKind::Prefetch,
        lanes,
    }
}

fn plain() -> Cartridge {
    let mut bytes = vec![0; 0x400];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x200..0x208].copy_from_slice(&[0x4e, 0x71, 0x4e, 0x71, 0x60, 0xfa, 0x4e, 0x71]);
    Cartridge::new(bytes).unwrap()
}

fn patches(cart: &mut Cartridge, address: u32, value: u8) {
    cart.rom_overlay = cart
        .prepare_rom_overlay(&[CheatPatch::WideRomWrite {
            address,
            value: CheatValue::Constant(value),
        }])
        .unwrap();
}

fn offsets(input: InstructionInput) -> [Option<u64>; 2] {
    input.origin.map(InstructionByteOrigin::storage_offset)
}

fn irq_bus(cart: Cartridge, port_history: bool) -> Bus {
    let mut bus = Bus::new(cart, Region::NtscUs);
    bus.irq_level = 4;
    if !port_history {
        let registers = *bus.vdp.registers();
        let interrupts =
            crate::hardware::vdp::VdpInterrupts::at_counters_zero(bus.now, 0, false, false);
        bus.vdp = crate::Vdp::from_known_state(interrupts, registers, bus.region).unwrap();
    }
    assert_eq!(bus.vdp.interrupts().has_port_history(), port_history);
    bus
}

fn fetch(
    route: u8,
    bus: &mut Bus,
    request: BusAccess,
    observed: bool,
) -> ObservedCycle<InstructionInput, BusError> {
    fn run<B: ClockedBus>(
        bus: &mut B,
        request: BusAccess,
        clock: CpuClocks,
        observed: bool,
    ) -> ObservedCycle<InstructionInput, B::Error> {
        if observed {
            bus.read_instruction_cycle_observed(
                request,
                clock,
                InterruptAperture::FollowingPrefetch,
            )
        } else {
            ObservedCycle::unobserved(bus.read_instruction_cycle(request, clock))
        }
    }
    let clock = bus.cpu_clock;
    let mut sound = zeff_z80::Cpu::new();
    let mut schedule = SoundSchedule::default();
    match route {
        0 => run(bus, request, clock, observed),
        1 => run(
            &mut ScheduledBus::new(bus, &mut sound, &mut schedule),
            request,
            clock,
            observed,
        ),
        2 => {
            let result = run(
                &mut CompactBus(ScheduledBus::new(bus, &mut sound, &mut schedule)),
                request,
                clock,
                observed,
            );
            ObservedCycle {
                cycle: CycleResult {
                    result: result.cycle.result.map_err(|error| *error),
                    wait_clocks: result.cycle.wait_clocks,
                },
                samples: result.samples,
            }
        }
        _ => unreachable!(),
    }
}

#[derive(Default)]
struct Capture {
    inputs: Vec<(u32, InstructionInput)>,
    words: Vec<(u32, u16)>,
    memory: Vec<MemoryAccess>,
}

impl InstructionObserver for Capture {
    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        self.inputs.push((address, input));
        self.instruction_word(address, input.value);
    }
    fn instruction_word(&mut self, address: u32, value: u16) {
        self.words.push((address, value));
    }
    fn memory_access(&mut self, event: MemoryAccess) {
        self.memory.push(event);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sampled_bytes_follow_selected_storage_overlay_and_active_save_owner() {
    let mut cart = crate::cartridge::ssf2::tests::cartridge(&[]);
    patches(&mut cart, 0x480201, 0x71);
    assert!(
        cart.ssf2
            .as_mut()
            .unwrap()
            .control(crate::cartridge::ssf2::tests::access(0xa130f3), 9)
    );
    let high = cart.read_sampled(0x80200).unwrap();
    let low = cart.read_sampled(0x80201).unwrap();
    assert_eq!((high.value, low.value), (9, 0x71));
    assert_eq!(high.origin.storage_offset(), Some(0x480200));
    assert_eq!(low.origin.storage_offset(), Some(0x480201));
    assert_eq!(&cart.bytes[0x480200..0x480202], &[9, 9]);

    let mut cart = plain();
    cart.bytes = vec![0x69; 0x210000].into();
    cart.save = Some(crate::cartridge::ram::headerless_odd8_fixture());
    cart.save.as_mut().unwrap().bytes[0] = 0x35;
    assert_eq!(cart.rom_source_offset(0x200001), Some(0x200001));
    for (address, expected) in [(0x200000, 0xff), (0x200001, 0x35)] {
        let byte = cart.read_sampled(address).unwrap();
        assert_eq!(byte.value, expected);
        assert_eq!(byte.origin, InstructionByteOrigin::UNKNOWN);
    }
    let byte = plain().read_sampled(0x1000).unwrap();
    assert_eq!(byte.value, 0xff);
    assert_eq!(byte.origin, InstructionByteOrigin::UNKNOWN);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn timed_routes_keep_alias_lanes_values_four_clocks_and_irq_aperture() {
    for route in 0..3 {
        for port_history in [false, true] {
            for observed in [false, true] {
                for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                    let mut bus = irq_bus(plain(), port_history);
                    let logical = 0x01000200 + u32::from(lanes == ByteLanes::Lower);
                    let result = fetch(route, &mut bus, access(logical, lanes), observed);
                    let input = result.cycle.result.unwrap();
                    assert_eq!(input.value, 0x4e71);
                    assert_eq!(
                        offsets(input),
                        [
                            (lanes != ByteLanes::Lower).then_some(0x200),
                            (lanes != ByteLanes::Upper).then_some(0x201),
                        ]
                    );
                    assert_eq!(result.cycle.wait_clocks, 0);
                    assert_eq!(bus.cpu_clock, CpuClocks(4));
                    assert_eq!(bus.now.get(), 28);
                    assert_eq!(
                        result.samples.map(|samples| samples.levels()),
                        (observed && !port_history && lanes == ByteLanes::Both).then_some((4, 4))
                    );
                }
            }
        }
        let mut bus = Bus::new(plain(), Region::NtscUs);
        for (index, value) in [
            (1, 0x54),
            (15, 2),
            (19, 17),
            (20, 0),
            (21, 0),
            (22, 0xc0),
            (23, 0x7f),
        ] {
            bus.advance_machine(1).unwrap();
            let write = bus.vdp.queue_register(index, value, bus.region).unwrap();
            bus.advance_machine(write.publish_at.get() - bus.now.get())
                .unwrap();
        }
        for (index, byte) in bus.ram[0x8000..0x8022].iter_mut().enumerate() {
            *byte = index as u8 ^ 0x5a;
        }
        bus.vdp.write_control(0x4400, bus.region).unwrap();
        bus.vdp.write_control(0x0080, bus.region).unwrap();
        let begin = bus.now.get();
        let result = fetch(route, &mut bus, access(0x200, ByteLanes::Both), true);
        let input = result.cycle.result.unwrap();
        assert_eq!(input.value, 0x4e71);
        assert_eq!(offsets(input), [Some(0x200), Some(0x201)]);
        assert!(result.cycle.wait_clocks > 0);
        assert_eq!(result.samples, None);
        let clocks = u64::from(result.cycle.wait_clocks) + 4;
        assert_eq!(bus.cpu_clock, CpuClocks(clocks));
        assert_eq!(bus.now.get() - begin, clocks * 7);
        assert_eq!(bus.vdp.fifo().len(), 4);
        assert_eq!(
            &bus.vdp.memory().vram()[0x400..0x41a],
            &bus.ram[0x8000..0x801a]
        );
        assert_eq!(&bus.vdp.memory().vram()[0x41a..0x422], &[0; 8]);
        assert_eq!(bus.vdp.registers().registers()[19], Some(0));
        assert_eq!(bus.vdp.registers().registers()[20], Some(0));
        for _ in 0..4096 {
            if bus.vdp.fifo().is_empty() {
                break;
            }
            bus.idle(bus.cpu_clock, 1);
        }
        assert!(bus.vdp.fifo().is_empty());
        assert_eq!(
            &bus.vdp.memory().vram()[0x400..0x422],
            &bus.ram[0x8000..0x8022]
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_owned_word_lanes_keep_only_driven_sources_and_preserve_faults() {
    for route in 0..3 {
        let mut cart = plain();
        cart.bytes = vec![0x5a; 0x80000].into();
        std::sync::Arc::make_mut(&mut cart.bytes)[0x7e000..0x7e008]
            .copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        cart.realtec = Some(crate::cartridge::realtec::Realtec::new());
        patches(&mut cart, 0x7e201, 0x34);
        for (lanes, value, source) in [
            (ByteLanes::Upper, 0x5a00, [Some(0x7e200), None]),
            (ByteLanes::Lower, 0x0034, [None, Some(0x7e201)]),
            (ByteLanes::Both, 0x5a34, [Some(0x7e200), Some(0x7e201)]),
        ] {
            let mut bus = Bus::new(cart.clone(), Region::NtscUs);
            let logical = 0x01000200 + u32::from(lanes == ByteLanes::Lower);
            let result = fetch(route, &mut bus, access(logical, lanes), true);
            let input = result.cycle.result.unwrap();
            assert_eq!(input.value, value);
            assert_eq!(offsets(input), source);
            assert_eq!(result.cycle.wait_clocks, 0);
        }
        let mut bus = Bus::new(cart, Region::NtscUs);
        let before = bus.clone();
        let result = fetch(route, &mut bus, access(0x100, ByteLanes::Both), true);
        assert!(matches!(
            result.cycle.result,
            Err(BusError::UnqualifiedCartridgeReset { address: 0x100 })
        ));
        assert_eq!(result.cycle.wait_clocks, 0);
        assert_eq!(result.samples, None);
        assert_eq!(bus, before);
    }
}

fn banked(concurrent_sound: bool) -> Emulator {
    let mut cart = crate::cartridge::ssf2::tests::cartridge(&[0x4ef9, 0x0108, 0x0200]);
    let mut page = vec![0; 0x220];
    let program: [u16; 8] = [0x13fc, 9, 0x00a1, 0x30f3, 0x4e71, 0x4e71, 0x4e71, 0x4e71];
    for (index, word) in program.iter().enumerate() {
        page[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    crate::cartridge::ssf2::tests::page_program(&mut cart, 1, &page);
    crate::cartridge::ssf2::tests::page_program(&mut cart, 9, &page);
    let mut machine = Emulator::new_with_cartridge(
        cart,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
    )
    .unwrap();
    machine.step_instruction().unwrap();
    assert_eq!(machine.cpu.registers.pc, 0x01080200);
    machine
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn real_bank_control_keeps_old_queue_and_new_refill_with_equal_opcode_bytes() {
    for concurrent in [false, true] {
        let mut actual = banked(concurrent);
        let mut reference = actual.clone();
        let old = actual.cpu.queued_instruction_inputs().unwrap();
        assert_eq!(offsets(old[0]), [Some(0x80200), Some(0x80201)]);
        let mut capture = Capture::default();
        let step = actual.step_instruction_traced(&mut capture).unwrap();
        assert_eq!(reference.step_instruction().unwrap(), step);
        assert_eq!(step.elapsed.get(), 140);
        let queue = actual.cpu.queued_instruction_inputs().unwrap();
        assert_eq!(queue.map(|word| word.value), [0x4e71; 2]);
        assert_eq!(offsets(queue[0]), [Some(0x80208), Some(0x80209)]);
        assert_eq!(offsets(queue[1]), [Some(0x48020a), Some(0x48020b)]);
        assert_eq!(actual.bus.cartridge.read(0x80208), Some(0x4e));
        for _ in 0..2 {
            assert_eq!(
                actual.step_instruction_traced(&mut capture),
                reference.step_instruction()
            );
            assert_eq!(actual, reference);
            assert_eq!(
                actual.encode_state().unwrap(),
                reference.encode_state().unwrap()
            );
        }
        assert_eq!(
            capture
                .inputs
                .iter()
                .map(|(_, word)| offsets(*word)[0].unwrap())
                .collect::<Vec<_>>(),
            [0x80200, 0x80202, 0x80204, 0x80206, 0x80208, 0x48020a]
        );
        assert_eq!(
            capture.words,
            capture
                .inputs
                .iter()
                .map(|(address, word)| (*address, word.value))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            capture.memory,
            [MemoryAccess {
                old_value: None,
                address: 0xa130f3,
                value: Some(9),
                direction: Direction::Write
            }]
        );
        assert!(
            capture
                .inputs
                .iter()
                .all(|(address, _)| *address > 0x00ff_ffff)
        );
        struct Legacy(Vec<(u32, u16)>);
        impl InstructionObserver for Legacy {
            fn instruction_word(&mut self, address: u32, value: u16) {
                self.0.push((address, value));
            }
        }
        let mut legacy = Legacy(Vec::new());
        let mut old_observer = banked(concurrent);
        for _ in 0..3 {
            old_observer.step_instruction_traced(&mut legacy).unwrap();
        }
        assert_eq!(legacy.0, capture.words);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn noncart_owners_and_open_bus_never_inherit_equal_storage_tags() {
    for route in 0..3 {
        let mut bus = Bus::new(plain(), Region::NtscUs);
        let first = fetch(route, &mut bus, access(0x200, ByteLanes::Both), false)
            .cycle
            .result
            .unwrap();
        assert_eq!(offsets(first), [Some(0x200), Some(0x201)]);
        bus.ram[0x200..0x202].copy_from_slice(&[0x4e, 0x71]);
        let ram = fetch(route, &mut bus, access(0xffff0200, ByteLanes::Both), true)
            .cycle
            .result
            .unwrap();
        assert_eq!(ram.value, first.value);
        assert_eq!(ram.origin, [InstructionByteOrigin::UNKNOWN; 2]);
        let mut boot = vec![0; 2048];
        boot[0x200..0x202].copy_from_slice(&[0x4e, 0x71]);
        bus.tmss = Some(crate::hardware::tmss::Tmss::new(boot).unwrap());
        let boot = fetch(route, &mut bus, access(0x200, ByteLanes::Both), false)
            .cycle
            .result
            .unwrap();
        assert_eq!(boot.value, first.value);
        assert_eq!(boot.origin, [InstructionByteOrigin::UNKNOWN; 2]);
        bus.tmss = None;
        bus.cartridge.save = Some(crate::cartridge::ram::headerless_odd8_fixture());
        bus.prefetch = Prefetch::Full(0x4e71);
        let mut request = access(0x400000, ByteLanes::Both);
        request.kind = AccessKind::Data;
        request.function_code = FunctionCode::SupervisorData;
        let open = fetch(route, &mut bus, request, true).cycle.result.unwrap();
        assert_eq!(open.value, first.value);
        assert_eq!(open.origin, [InstructionByteOrigin::UNKNOWN; 2]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn lock_on_and_sf001_sample_actual_rom_or_save_and_empty_cells() {
    for mapping in [LockOn::Empty, LockOn::HalfMiB, LockOn::MiB, LockOn::Patch] {
        let mut cart = plain();
        cart.bytes = vec![0x71; 0x340000].into();
        cart.lock_on = Some(mapping);
        let byte = cart.read_sampled(0x300200).unwrap();
        let expected = match mapping {
            LockOn::Empty => None,
            LockOn::Patch => Some(0x300200),
            _ => Some(0x200200),
        };
        assert_eq!(byte.origin.storage_offset(), expected);
        assert_eq!(
            byte.value,
            if mapping == LockOn::Empty { 0xff } else { 0x71 }
        );
    }
    let mut cart = Cartridge::new(crate::cartridge::sf001::tests::rom(
        crate::cartridge::sf001::Revision::HiddenSave,
    ))
    .unwrap();
    assert!(cart.sf001.as_mut().unwrap().control(0xe00, 0x80));
    assert_eq!(
        cart.read_sampled(0x200).unwrap().origin.storage_offset(),
        Some(0x380200)
    );
    let byte = cart.read_sampled(0x3c0000).unwrap();
    assert_eq!(byte.value, 0xff);
    assert_eq!(byte.origin, InstructionByteOrigin::UNKNOWN);
    assert!(cart.sf001.as_mut().unwrap().control(0xe00, 0x40));
    assert!(cart.read_sampled(0x200).is_none());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn real_raw_frontend_imports_reset_queue_origin_without_changing_v1_hardware() {
    for concurrent_sound in [false, true] {
        let mut machine = Emulator::new_with_cartridge(
            plain(),
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound,
            },
        )
        .unwrap();
        machine.step_instruction().unwrap();
        let tags = machine.cpu.queued_instruction_inputs().unwrap();
        assert!(tags.iter().all(|word| {
            word.origin
                .iter()
                .all(|origin| origin.storage_offset().is_some())
        }));
        assert_eq!(machine.clone().cpu.queued_instruction_inputs(), Some(tags));
        let raw = machine.encode_state().unwrap();
        let front = machine.encode_frontend_state().unwrap();
        for frontend in [false, true] {
            let mut restored = machine.clone();
            restored.step_instruction().unwrap();
            if frontend {
                restored.decode_frontend_state(&front).unwrap();
            } else {
                restored.decode_state(&raw).unwrap();
            }
            assert_eq!(restored.cpu, machine.cpu);
            assert_eq!(restored.encode_state().unwrap(), raw);
            if frontend {
                assert_eq!(restored.encode_frontend_state().unwrap(), front);
            }
            let imported = restored.cpu.queued_instruction_inputs().unwrap();
            assert_eq!(imported.map(|word| word.value), tags.map(|word| word.value));
            assert!(
                imported
                    .iter()
                    .all(|word| word.origin == [InstructionByteOrigin::UNKNOWN; 2])
            );
            assert_ne!(imported, tags);
        }
    }
}

mod owners;
mod parallel;
