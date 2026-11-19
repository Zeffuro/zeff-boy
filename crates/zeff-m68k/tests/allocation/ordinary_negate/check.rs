use super::*;
use device::Event;
use plan::{Spec, access, negate};

pub(super) fn run(device: &mut device::Device, mut spec: Spec, path: Path) {
    let odd = matches!(path, Path::Odd);
    if odd && spec.mode != 7 {
        let old = spec.initial.address_register(spec.reg).unwrap();
        spec.initial.set_address_register(spec.reg, old | 1);
    }
    let (mut address, mut extra) = spec.operand();
    if odd && spec.mode == 7 {
        address |= 1;
        let word = usize::from(spec.reg == 1);
        extra[word] |= 1;
    }
    let queue = spec.seed(device);
    for (i, word) in extra.into_iter().take(spec.extensions()).enumerate() {
        device.word(spec.initial.pc + 2 + i as u32 * 2, word);
    }
    let queue = if spec.extensions() == 0 {
        queue
    } else {
        [spec.opcode(), extra[0]]
    };
    device.word(
        address,
        if spec.long {
            (spec.source >> 16) as u16
        } else {
            spec.source as u16
        },
    );
    if spec.long {
        device.word(address.wrapping_add(2), spec.source as u16);
    }
    let initial_idle = u64::from(matches!(spec.mode, 4 | 6)) * 2;
    let start = if let Path::Reserve(at) = path {
        u64::MAX
            - (BUDGET - 1
                + initial_idle
                + at as u64 * 4
                + if at == 0 { 0 } else { u64::from(u32::MAX) })
    } else {
        0
    };
    device.clock = start;
    device.host = if let Path::Host(at) = path {
        Some(at)
    } else {
        None
    };
    device.maximum = if let Path::Reserve(at) = path {
        at.checked_sub(1)
    } else {
        None
    };
    let mut memory = device.words;
    let mut expected = spec.initial;
    let mut events = [Event::Idle(0, 0); EVENTS];
    let mut length = 0;
    let mut clock = start;
    let mut called = 0;
    let mut error = None;
    if initial_idle != 0 {
        events[length] = Event::Idle(clock, 2);
        length += 1;
        clock += 2;
    }
    let base = spec.initial.address_register(spec.reg).unwrap();
    if spec.mode == 4 {
        expected.set_address_register(spec.reg, base.wrapping_sub(spec.stride()));
    }
    let reads = if spec.long { 2 } else { 1 };
    let f_index = spec.extensions() + reads;
    let (result, full_sr) = negate(
        spec.initial.status_register(),
        spec.source,
        spec.long,
        spec.family == 0x4000,
    );
    let (_, low_sr) = negate(
        spec.initial.status_register(),
        spec.source,
        false,
        spec.family == 0x4000,
    );
    for index in 0..spec.callbacks() {
        if spec.mode == 3 && index == spec.extensions() + usize::from(spec.long) {
            expected.set_address_register(spec.reg, base.wrapping_add(spec.stride()));
        }
        if odd && index == spec.extensions() {
            error = Some(TimingFault::AddressError { address });
            break;
        }
        if clock > u64::MAX - BUDGET {
            error = Some(TimingFault::ClockOverflow);
            break;
        }
        if index == f_index {
            expected.set_status_register(low_sr);
        }
        if spec.long && index == f_index + 1 {
            expected.set_status_register(full_sr);
        }
        let (at, kind, value) = if index < spec.extensions() {
            (
                spec.initial.pc + 4 + index as u32 * 2,
                AccessKind::Prefetch,
                None,
            )
        } else if index < f_index {
            (
                address.wrapping_add((index - spec.extensions()) as u32 * 2),
                AccessKind::Data,
                None,
            )
        } else if index == f_index {
            (spec.canonical() + 2, AccessKind::Prefetch, None)
        } else if spec.long && index == f_index + 1 {
            (
                address.wrapping_add(2),
                AccessKind::Data,
                Some(result as u16),
            )
        } else {
            (
                address,
                AccessKind::Data,
                Some(if spec.long {
                    (result >> 16) as u16
                } else {
                    result as u16
                }),
            )
        };
        let access = access(at, spec.initial.status_register(), kind);
        let wait = if device.maximum == Some(index) {
            u32::MAX
        } else {
            0
        };
        let word = (access.bus_address as usize & MEMORY_MASK) / 2;
        events[length] = if let Some(value) = value {
            memory[word] = value;
            Event::Write(access, clock, value, wait)
        } else {
            Event::Read(access, clock, memory[word], wait)
        };
        length += 1;
        called += 1;
        clock += 4 + u64::from(wait);
        if device.host == Some(index) {
            error = Some(TimingFault::Bus {
                access,
                direction: if value.is_some() {
                    Direction::Write
                } else {
                    Direction::Read
                },
                source: index,
            });
            break;
        }
    }
    let mut cpu = PrefetchCpu::from_prefetched(spec.initial, queue, CpuClocks(start)).unwrap();
    let actual = black_box(cpu.step_instruction(device));
    if let Some(error) = error {
        assert_eq!(actual, Err(error));
        assert_eq!(cpu.queued_words(), Some(queue));
        let frozen = cpu.clone();
        let saved = device.clone();
        let mut copied = frozen.clone();
        let mut copied_device = saved.clone();
        assert_eq!(
            copied.step_instruction(&mut copied_device),
            Err(TimingFault::Frozen)
        );
        assert_eq!(copied, frozen);
        assert_eq!(copied_device, saved);
        assert_eq!(cpu.step_instruction(device), Err(TimingFault::Frozen));
        assert_eq!(cpu, frozen);
        assert_eq!(*device, saved);
        assert_eq!(cpu.step_boundary(device), Err(TimingFault::Frozen));
        assert_eq!(cpu, frozen);
        assert_eq!(*device, saved);
        assert_eq!(cpu.prime(device), Err(TimingFault::Frozen));
        assert_eq!(cpu, frozen);
        assert_eq!(*device, saved);
        assert_eq!(cpu.service_pending_trace(device), Err(TimingFault::Frozen));
        assert_eq!(cpu, frozen);
        assert_eq!(*device, saved);
        assert_eq!(cpu.step(device), Err(TimingFault::Frozen));
        assert_eq!(cpu, frozen);
        assert_eq!(*device, saved);
    } else {
        expected.pc = spec.canonical();
        let observed = actual.unwrap();
        assert_eq!(observed.opcode, spec.opcode());
        assert_eq!(observed.clocks, clock - start);
        assert_eq!(observed.exception, None);
        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
        let expected_cpu =
            PrefetchCpu::from_prefetched(expected, [0x4e71, 0x7201], CpuClocks(clock)).unwrap();
        assert_eq!(cpu, expected_cpu);
    }
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.clock().0, clock);
    assert_eq!(device.clock, clock);
    assert_eq!(device.count, called);
    assert_eq!(device.length, length);
    assert_eq!(device.events, events);
    assert_eq!(device.words, memory);
    assert!(!cpu.trace_pending());
    assert!(!cpu.stopped());
    assert_eq!(cpu.interrupt_input().level(), 0);
    assert_eq!(
        cpu.physical_entry_state(),
        zeff_m68k::timing::physical::PhysicalEntryState::Idle
    );
}
