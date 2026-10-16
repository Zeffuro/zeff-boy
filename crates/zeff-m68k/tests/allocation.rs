use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use zeff_m68k::bus::{BusAccess, BusResponse, ByteLanes, M68kBus, RmwError};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu};
use zeff_m68k::{Cpu, StepOutcome};

#[path = "allocation/control.rs"]
mod control;

#[path = "allocation/stack_status.rs"]
mod stack_status;

#[path = "allocation/transfer.rs"]
mod transfer;

#[path = "allocation/owned_bus.rs"]
mod owned_bus;

#[path = "allocation/physical.rs"]
mod physical;

#[path = "allocation/ordinary_address_arithmetic.rs"]
mod ordinary_address_arithmetic;
#[path = "allocation/ordinary_cmpi.rs"]
mod ordinary_cmpi;
#[path = "allocation/ordinary_data_arithmetic.rs"]
mod ordinary_data_arithmetic;
#[path = "allocation/ordinary_negate.rs"]
mod ordinary_negate;
#[path = "allocation/ordinary_rmw.rs"]
mod ordinary_rmw;
#[path = "allocation/physical_address_arithmetic.rs"]
mod physical_address_arithmetic;
#[path = "allocation/physical_data_arithmetic.rs"]
mod physical_data_arithmetic;

#[path = "allocation/physical_instruction.rs"]
mod physical_instruction;

#[path = "allocation/physical_extended_memory_moves.rs"]
mod physical_extended_memory_moves;
#[path = "allocation/physical_immediate_moves.rs"]
mod physical_immediate_moves;
#[path = "allocation/physical_memory_moves.rs"]
mod physical_memory_moves;
#[path = "allocation/physical_move.rs"]
mod physical_move;
#[path = "allocation/physical_move_extensions.rs"]
mod physical_move_extensions;
#[path = "allocation/physical_move_updates.rs"]
mod physical_move_updates;
#[path = "allocation/physical_movea.rs"]
mod physical_movea;
#[path = "allocation/physical_register_moves.rs"]
mod physical_register_moves;

#[path = "allocation/physical_clear_not_remainder.rs"]
mod physical_clear_not_remainder;
#[path = "allocation/physical_cmp.rs"]
mod physical_cmp;
#[path = "allocation/physical_cmpa.rs"]
mod physical_cmpa;
#[path = "allocation/physical_cmpi.rs"]
mod physical_cmpi;
#[path = "allocation/physical_cmpi_remainder.rs"]
mod physical_cmpi_remainder;
#[path = "allocation/physical_compare_remainder.rs"]
mod physical_compare_remainder;
#[path = "allocation/physical_from_sr/mod.rs"]
mod physical_from_sr;
#[path = "allocation/physical_negate/mod.rs"]
mod physical_negate;
#[path = "allocation/physical_rmw.rs"]
mod physical_rmw;
#[path = "allocation/physical_status.rs"]
mod physical_status;
#[path = "allocation/physical_status_register/mod.rs"]
mod physical_status_register;
#[path = "allocation/physical_tst.rs"]
mod physical_tst;

#[path = "allocation/physical_tst_remainder.rs"]
mod physical_tst_remainder;

struct CountingAllocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

// The underlying allocator owns each allocation; this wrapper only counts requests.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[path = "allocation/device.rs"]
mod device;
use device::{LoopBus, MemoryBus};

#[test]
fn successful_steps_and_faults_allocate_nothing() {
    let mut cpu = Cpu::new();
    let mut bus = LoopBus;
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    for _ in 0..30_000 {
        if matches!(black_box(cpu.step(&mut bus)), StepOutcome::Executed { .. }) {
            executed += 1;
        }
    }
    cpu.registers.pc = 7;
    let fault = black_box(cpu.step(&mut bus));
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 30_000);
    assert!(matches!(fault, StepOutcome::Fault(_)));
    assert_eq!(after - before, 0);
    assert_eq!(cpu.registers.d[0], 0xffff_ff81);

    let mut bus = MemoryBus([0; 0x8000]);
    for (index, word) in [0x303c, 7, 0x5240, 0x0c40, 8, 0x57c1, 0x4e40, 0x60f0]
        .into_iter()
        .enumerate()
    {
        bus.word(0x2000 + 2 * index, word);
    }
    bus.word(128, 0);
    bus.word(130, 0x1000);
    bus.word(0x1000, 0x4e73);
    let mut cpu = Cpu::new();
    cpu.registers.pc = 0x2000;
    cpu.registers.ssp = 0x5000;
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    let mut exceptions = 0;
    for _ in 0..28_000 {
        match black_box(cpu.step(&mut bus)) {
            StepOutcome::Executed { .. } => executed += 1,
            StepOutcome::Exception { .. } => exceptions += 1,
            _ => {}
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!((executed, exceptions), (24_000, 4_000));
    assert_eq!(after - before, 0);
    assert_eq!(cpu.registers.pc, 0x2000);
    assert_eq!(cpu.registers.ssp, 0x5000);
    assert_eq!(cpu.registers.d[1], 0xff);

    let mut bus = MemoryBus([0; 0x8000]);
    let programs: [&[u16]; 15] = [
        &[0xe3a0],
        &[0x08c0, 31],
        &[0xc0fc, 0x8001],
        &[0x81fc, 1],
        &[0x80fc, 1],
        &[0x41bc, 0x7fff],
        &[0xdf8f],
        &[0x40a7],
        &[0x01c8, 1],
        &[0x48d0, 3],
        &[0x4c90, 3],
        &[0x4e77],
        &[0x4ac0],
        &[0x4ad0],
        &[0x4e70],
    ];
    for (index, words) in programs.iter().enumerate() {
        for (offset, word) in words.iter().enumerate() {
            bus.word(0x2000 + index * 8 + offset * 2, *word);
        }
    }
    let mut cpu = Cpu::new();
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    for _ in 0..2_000 {
        for index in 0..programs.len() {
            cpu.registers.pc = 0x2000 + index as u32 * 8;
            cpu.registers.a[0] = 0x4000;
            cpu.registers.ssp = 0x5000;
            cpu.registers.d[0] = 0;
            if matches!(black_box(cpu.step(&mut bus)), StepOutcome::Executed { .. }) {
                executed += 1;
            }
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 30_000);
    assert_eq!(after - before, 0);

    let mut cpu = PrefetchCpu::default();
    let mut bus = LoopBus;
    cpu.prime(&mut bus).unwrap();
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    for _ in 0..30_000 {
        if black_box(cpu.step(&mut bus)).is_ok() {
            executed += 1;
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 30_000);
    assert_eq!(cpu.clock(), CpuClocks(180_008));
    assert_eq!(cpu.registers.pc, 0);
    assert_eq!(cpu.registers.d[0], 0xffff_ff81);
    assert_eq!(after - before, 0);

    let mut bus = MemoryBus([0; 0x8000]);
    bus.word(0x2004, 0x4e71);
    let mut registers = zeff_m68k::Registers::new();
    registers.pc = 0x2000;
    registers.a[0] = 0x3008;
    registers.ssp = 0x3010;
    registers.d[0] = 0x1234_5678;
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let (mut executed, mut clocks) = (0, 0);
    for _ in 0..2_000 {
        for opcode in [
            0x3010, 0x3080, 0x2010, 0x2080, 0x2118, 0x20d8, 0x3120, 0x3058, 0x1f1f,
        ] {
            let mut cpu =
                PrefetchCpu::from_prefetched(registers, [opcode, 0x7201], CpuClocks(0)).unwrap();
            if let Ok(step) = black_box(cpu.step(&mut bus)) {
                executed += 1;
                clocks += step.clocks;
            }
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 18_000);
    assert_eq!(clocks, 228_000);
    assert_eq!(after - before, 0);

    bus.word(0x2004, 0x3000);
    registers.d[0] = 4;
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let (mut executed, mut clocks) = (0, 0);
    for _ in 0..2_000 {
        for opcode in [
            0x3028, 0x3030, 0x3038, 0x3039, 0x303a, 0x303b, 0x303c, 0x203c, 0x33d0,
        ] {
            let mut cpu =
                PrefetchCpu::from_prefetched(registers, [opcode, 0], CpuClocks(0)).unwrap();
            if let Ok(step) = black_box(cpu.step(&mut bus)) {
                executed += 1;
                clocks += step.clocks;
            }
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 18_000);
    assert_eq!(clocks, 240_000);
    assert_eq!(after - before, 0);

    bus.word(0x2004, 0x4e71);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let (mut executed, mut clocks) = (0, 0);
    for _ in 0..2_000 {
        for opcode in [
            0x4a10, 0x4a50, 0x4a90, 0x4210, 0x4250, 0x4290, 0x4410, 0x4450, 0x4490, 0x4010, 0x4050,
            0x4090, 0x4610, 0x4650, 0x4690,
        ] {
            let mut cpu =
                PrefetchCpu::from_prefetched(registers, [opcode, 0x7201], CpuClocks(0)).unwrap();
            if let Ok(step) = black_box(cpu.step(&mut bus)) {
                executed += 1;
                clocks += step.clocks;
            }
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 30_000);
    assert_eq!(clocks, 408_000);
    assert_eq!(after - before, 0);

    let programs: [&[u16]; 70] = [
        &[0xd010],
        &[0x9058],
        &[0xc0a0],
        &[0x8068, 0],
        &[0xb090],
        &[0xd0d0],
        &[0x91d0],
        &[0xb0d0],
        &[0xd110],
        &[0x9150],
        &[0xc190],
        &[0x8110],
        &[0xb150],
        &[0x5290],
        &[0x5350],
        &[0xd07c, 0x8001],
        &[0xd0bc, 0x8001, 7],
        &[0xb07a, 0x1000],
        &[0xd070, 0x6000],
        &[0x0010, 1],
        &[0x0050, 2],
        &[0x0090, 1, 2],
        &[0x0210, 1],
        &[0x0250, 2],
        &[0x0290, 1, 2],
        &[0x0410, 1],
        &[0x0450, 2],
        &[0x0490, 1, 2],
        &[0x0610, 1],
        &[0x0650, 2],
        &[0x0690, 1, 2],
        &[0x0a10, 1],
        &[0x0a50, 2],
        &[0x0a90, 1, 2],
        &[0x0c10, 1],
        &[0x0c50, 2],
        &[0x0c90, 1, 2],
        &[0x003c, 1],
        &[0x023c, 2],
        &[0x0a3c, 3],
        &[0x0670, 1, 0x6000],
        &[0x0a79, 1, 0, 0x3008],
        &[0x0cb8, 1, 2, 0x3008],
        &[0x0300],
        &[0x0340],
        &[0x0380],
        &[0x03c0],
        &[0x0800, 17],
        &[0x0840, 17],
        &[0x0880, 17],
        &[0x08c0, 17],
        &[0x0310],
        &[0x0350],
        &[0x0390],
        &[0x03d0],
        &[0x0810, 17],
        &[0x0850, 17],
        &[0x0890, 17],
        &[0x08d0, 17],
        &[0x033c, 0x1234],
        &[0x033a, 0x1000],
        &[0x083a, 17, 0x1000],
        &[0xe0d0],
        &[0xe1d0],
        &[0xe2d0],
        &[0xe3d0],
        &[0xe4d0],
        &[0xe5d0],
        &[0xe6d0],
        &[0xe7d0],
    ];
    registers.d[6] = 4;
    registers.d[1] = 17;
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    for _ in 0..2_000 {
        for words in programs {
            for (i, word) in words.iter().enumerate() {
                bus.word(0x2000 + 2 * i, *word);
            }
            bus.word(0x2000 + 2 * words.len(), 0x7201);
            bus.word(0x2002 + 2 * words.len(), 0x7402);
            let next = u16::from_be_bytes([bus.0[0x2002], bus.0[0x2003]]);
            let mut cpu =
                PrefetchCpu::from_prefetched(registers, [words[0], next], CpuClocks(0)).unwrap();
            if black_box(cpu.step_instruction(&mut bus)).is_ok() {
                executed += 1;
            }
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 140_000);
    assert_eq!(after - before, 0);

    for i in 0..8 {
        registers.set_address_register(i, 0x3008 + i as u32 * 8);
    }
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    for _ in 0..100 {
        for base in [
            0xc108, 0x8108, 0xd108, 0xd148, 0xd188, 0x9108, 0x9148, 0x9188, 0xb108, 0xb148, 0xb188,
        ] {
            for source in 0..8 {
                for destination in 0..8 {
                    let opcode = base | destination << 9 | source;
                    bus.word(0x2000, opcode);
                    bus.word(0x2002, 0x7201);
                    bus.word(0x2004, 0x7402);
                    let mut cpu =
                        PrefetchCpu::from_prefetched(registers, [opcode, 0x7201], CpuClocks(0))
                            .unwrap();
                    executed += usize::from(black_box(cpu.step_instruction(&mut bus)).is_ok());
                }
            }
        }
        for words in [
            &[0x4810][..],
            &[0x4818],
            &[0x4820],
            &[0x4828, 0x10],
            &[0x4830, 0x6000],
            &[0x4838, 0x3010],
            &[0x4839, 0, 0x3010],
            &[0x4817],
            &[0x481f],
            &[0x4827],
            &[0x482f, 0x10],
            &[0x4837, 0x6000],
        ] {
            for (i, word) in words.iter().enumerate() {
                bus.word(0x2000 + 2 * i, *word);
            }
            bus.word(0x2000 + 2 * words.len(), 0x7201);
            bus.word(0x2002 + 2 * words.len(), 0x7402);
            let next = u16::from_be_bytes([bus.0[0x2002], bus.0[0x2003]]);
            let mut cpu =
                PrefetchCpu::from_prefetched(registers, [words[0], next], CpuClocks(0)).unwrap();
            executed += usize::from(black_box(cpu.step_instruction(&mut bus)).is_ok());
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 71_600);
    assert_eq!(after - before, 0);

    for vector in [5, 6] {
        bus.word(vector * 4, 0);
        bus.word(vector * 4 + 2, 0x1000);
    }
    bus.word(0x1000, 0x4e71);
    bus.word(0x1002, 0x4e71);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    for _ in 0..200 {
        for source in [0_u16, 3] {
            registers.d = [u32::from(source); 8];
            registers.d[0] = 0x8000_0001;
            for base in [0xc0c0, 0xc1c0, 0x80c0, 0x81c0, 0x4180] {
                for (mode, register) in [
                    (0, 0),
                    (0, 7),
                    (2, 0),
                    (3, 0),
                    (4, 0),
                    (5, 0),
                    (6, 0),
                    (7, 0),
                    (7, 1),
                    (7, 2),
                    (7, 3),
                    (7, 4),
                    (2, 7),
                    (3, 7),
                    (4, 7),
                    (5, 7),
                    (6, 7),
                ] {
                    let ea = registers.address_register(register).unwrap() as usize;
                    let (extensions, address): (&[u16], usize) = match (mode, register) {
                        (0, _) => (&[], 0),
                        (2 | 3, _) => (&[], ea),
                        (4, _) => (&[], ea - 2),
                        (5, _) => (&[0x10], ea + 0x10),
                        (6, _) => (&[0xe000], ea + 0x3038),
                        (7, 0) => (&[0x3010], 0x3010),
                        (7, 1) => (&[0, 0x3010], 0x3010),
                        (7, 2) => (&[0x1000], 0x3002),
                        (7, 3) => (&[0xe000], 0x503a),
                        (7, 4) => (std::slice::from_ref(&source), 0),
                        _ => unreachable!(),
                    };
                    let opcode = base | mode << 3 | register as u16;
                    bus.word(0x2000, opcode);
                    for (i, word) in extensions.iter().enumerate() {
                        bus.word(0x2002 + i * 2, *word);
                    }
                    bus.word(0x2002 + extensions.len() * 2, 0x7201);
                    bus.word(0x2004 + extensions.len() * 2, 0x7402);
                    if address != 0 {
                        bus.word(address, source);
                    }
                    let next = u16::from_be_bytes([bus.0[0x2002], bus.0[0x2003]]);
                    let mut cpu =
                        PrefetchCpu::from_prefetched(registers, [opcode, next], CpuClocks(0))
                            .unwrap();
                    executed += usize::from(black_box(cpu.step_instruction(&mut bus)).is_ok());
                }
            }
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 34_000);
    assert_eq!(after - before, 0);

    control::control_steps(&mut bus);
    stack_status::stack_status_steps(&mut bus);
    transfer::transfer_steps(&mut bus);
    owned_bus::owned_steps(&mut bus);
    physical::physical_steps(&mut bus);
    physical_instruction::instruction_steps(&mut bus);
    physical_move::move_steps(&mut bus);
    physical_move_updates::update_steps(&mut bus);
    physical_move_extensions::extension_steps(&mut bus);
    physical_movea::movea_steps(&mut bus);
    physical_register_moves::register_steps(&mut bus);
    physical_immediate_moves::immediate_steps(&mut bus);
    physical_memory_moves::memory_steps(&mut bus);
    physical_extended_memory_moves::extended_memory_steps(&mut bus);
    physical_tst::tst_steps(&mut bus);
    physical_cmp::cmp_steps(&mut bus);
    physical_cmpa::cmpa_steps(&mut bus);
    physical_address_arithmetic::address_arithmetic_steps(&mut bus);
    ordinary_address_arithmetic::address_steps();
    ordinary_data_arithmetic::data_steps();
    physical_data_arithmetic::data_steps(&mut bus);
    physical_cmpi::compare_steps(&mut bus);
    physical_cmpi_remainder::compare_remainder_steps();
    physical_compare_remainder::compare_remainder_steps();
    physical_clear_not_remainder::run();
    physical_status::status_steps(&mut bus);
    ordinary_cmpi::compare_steps();
    ordinary_negate::negate_steps();
    ordinary_rmw::rmw_steps();
    physical_rmw::rmw_steps(&mut bus);
    physical_negate::negate_steps(&mut bus);
    physical_from_sr::from_sr_steps(&mut bus);
    physical_status_register::status_register_steps();
    physical_tst_remainder::tst_remainder_steps();
}
