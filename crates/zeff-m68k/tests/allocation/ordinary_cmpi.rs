use super::*;
use zeff_m68k::Registers;
use zeff_m68k::timing::TimingFault;

const BUDGET: u64 = 4 + u32::MAX as u64;

struct Device {
    words: [u16; 0x4000],
    clock: u64,
    count: usize,
    host: Option<usize>,
    maximum: Option<usize>,
}

impl ClockedBus for Device {
    type Error = usize;

    fn read_cycle(&mut self, access: BusAccess, at: CpuClocks) -> CycleResult<u16, usize> {
        assert_eq!(at.0, self.clock);
        let index = self.count;
        self.count += 1;
        let wait = if self.maximum == Some(index) {
            u32::MAX
        } else {
            0
        };
        self.clock += 4 + u64::from(wait);
        CycleResult {
            result: if self.host == Some(index) {
                Err(index)
            } else {
                Ok(self.words[(access.bus_address as usize & 0x7fff) / 2])
            },
            wait_clocks: wait,
        }
    }

    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), usize> {
        panic!()
    }

    fn idle(&mut self, at: CpuClocks, clocks: u32) {
        assert_eq!(at.0, self.clock);
        self.clock += u64::from(clocks);
    }
}

#[derive(Clone, Copy)]
enum Path {
    Complete,
    Host(usize),
    Reserve(usize),
    Odd,
}

fn check(device: &mut Device, long: bool, field: (u16, usize), status: u16, path: Path) {
    let (mode, register) = field;
    let mut initial = Registers::new();
    initial.pc = 0x2000;
    initial.d = [0x1234_5678; 8];
    initial.d[6] = 4;
    initial.usp = 0x4000;
    initial.ssp = 0x5000;
    initial.set_status_register(status);
    for index in 0..8 {
        initial.set_address_register(index, 0x3008 + index as u32 * 8);
    }
    let odd = matches!(path, Path::Odd);
    if odd && mode != 7 {
        let value = initial.address_register(register).unwrap();
        initial.set_address_register(register, value | 1);
    }
    let base = initial.address_register(register).unwrap();
    let stride = if long { 4 } else { 2 };
    let (address, extensions, extra) = match (mode, register) {
        (2 | 3, _) => (base, 0, [0, 0]),
        (4, _) => (base - stride, 0, [0, 0]),
        (5, _) => (base + 16, 1, [16, 0]),
        (6, _) => (base + 4, 1, [0x6000, 0]),
        (7, 0) => (0x3010 | u32::from(odd), 1, [0x3010 | u16::from(odd), 0]),
        (7, 1) => (
            0x1234_3010 | u32::from(odd),
            2,
            [0x1234, 0x3010 | u16::from(odd)],
        ),
        _ => unreachable!(),
    };
    let opcode = 0x0c00 | if long { 0x80 } else { 0x40 } | mode << 3 | register as u16;
    let immediate_count = if long { 2 } else { 1 };
    let prefix = immediate_count + extensions;
    let count = prefix + if long { 2 } else { 1 } + 1;
    let idle_before = match mode {
        4 => Some(prefix),
        6 => Some(immediate_count),
        _ => None,
    };
    let start = if let Path::Reserve(index) = path {
        if index == 0 {
            u64::MAX - BUDGET + 1
        } else {
            let idle = u64::from(idle_before.is_some_and(|at| at <= index)) * 2;
            u64::MAX - (2 * BUDGET - 1 + (index as u64 - 1) * 4 + idle)
        }
    } else {
        0
    };
    device.words.fill(0x4e71);
    device.words[0x2000 / 2] = opcode;
    device.words[0x2002 / 2] = if long { 0 } else { 0x8000 };
    if long {
        device.words[0x2004 / 2] = 1;
    }
    for (index, word) in extra.into_iter().take(extensions).enumerate() {
        device.words[0x2002 / 2 + immediate_count + index] = word;
    }
    let canonical = 0x2002 + (prefix as u32 * 2);
    device.words[canonical as usize / 2] = 0x4e71;
    device.words[canonical as usize / 2 + 1] = 0x7201;
    let at = (address as usize & 0x7fff) / 2;
    device.words[at] = if long { 0x8000 } else { 0x7fff };
    if long {
        device.words[(at + 1) & 0x3fff] = 0;
    }
    let memory = device.words;
    let queue = [opcode, device.words[0x2002 / 2]];
    let mut expected = initial;
    let position = match path {
        Path::Complete => count,
        Path::Host(at) | Path::Reserve(at) => at,
        Path::Odd => prefix,
    };
    let updates = position >= prefix && (mode == 4 || mode == 3 && (!long || position > prefix));
    if updates {
        expected.set_address_register(
            register,
            if mode == 4 {
                base - stride
            } else {
                base + stride
            },
        );
    }
    if matches!(path, Path::Complete) || matches!(path, Path::Host(at) if at == count - 1) {
        expected.set_status_register(status & !15 | if long { 2 } else { 11 });
    }
    device.clock = start;
    device.count = 0;
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
    let mut cpu = PrefetchCpu::from_prefetched(initial, queue, CpuClocks(start)).unwrap();
    let result = black_box(cpu.step_instruction(device));
    match path {
        Path::Complete => {
            expected.pc = canonical;
            assert!(result.is_ok());
            assert_eq!(device.count, count);
            assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
        }
        Path::Host(at) => {
            assert!(matches!(result, Err(TimingFault::Bus { source, .. }) if source == at));
            assert_eq!(device.count, at + 1);
        }
        Path::Reserve(at) => {
            assert!(matches!(result, Err(TimingFault::ClockOverflow)));
            assert_eq!(device.count, at);
        }
        Path::Odd => {
            assert!(
                matches!(result, Err(TimingFault::AddressError { address: fault }) if fault == address)
            );
            assert_eq!(device.count, prefix);
        }
    }
    if !matches!(path, Path::Complete) {
        assert_eq!(cpu.queued_words(), Some(queue));
        let count = device.count;
        let clock = device.clock;
        let before = cpu.clone();
        assert!(matches!(
            cpu.step_instruction(device),
            Err(TimingFault::Frozen)
        ));
        assert_eq!(cpu, before);
        assert_eq!(device.count, count);
        assert_eq!(device.clock, clock);
    }
    assert_eq!(cpu.registers, expected);
    assert_eq!(device.words, memory);
    assert_eq!(cpu.clock().0, device.clock);
    assert!(!cpu.trace_pending());
}

pub(super) fn compare_steps() {
    let mut device = Device {
        words: [0; 0x4000],
        clock: 0,
        count: 0,
        host: None,
        maximum: None,
    };
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut cases = 0;
    for _ in 0..100 {
        for long in [false, true] {
            for mode in 2..=7 {
                for register in 0..if mode == 7 { 2 } else { 8 } {
                    let extra = match (mode, register) {
                        (5 | 6, _) | (7, 0) => 1,
                        (7, 1) => 2,
                        _ => 0,
                    };
                    let count = extra + if long { 5 } else { 3 };
                    for status in [0x051f, 0x251f] {
                        check(&mut device, long, (mode, register), status, Path::Complete);
                        cases += 1;
                        for at in 0..count {
                            for path in [Path::Host(at), Path::Reserve(at)] {
                                check(&mut device, long, (mode, register), status, path);
                                cases += 1;
                            }
                        }
                        if register == 0 || mode == 7 {
                            check(&mut device, long, (mode, register), status, Path::Odd);
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 169_200);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
