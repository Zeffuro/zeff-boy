use zeff_emu_common::time::MasterTicks;
use zeff_m68k::Registers;
use zeff_m68k::bus::BusAccess;
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu};
use zeff_md_core::{
    ControlCommand, Emulator, LoadConfig, Region, Vdp, VdpMemory, VdpPortError, VdpReadCache,
    VdpWrite, VdpWriteError, VdpWriteFifo,
};

const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];
const NO_EFFECT: [u8; 4] = [0, 0x10, 9, 0x19];

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}

fn control(vdp: &mut Vdp, address: u32, code: u8, region: Region) {
    vdp.write_control(
        (address as u16 & 0x3fff) | u16::from(code & 3) << 14,
        region,
    )
    .unwrap();
    vdp.write_control(
        ((address >> 14) as u16 & 7) | u16::from(code & 0x3c) << 2,
        region,
    )
    .unwrap();
    assert_eq!(vdp.command(), command(address, code));
}

fn register(vdp: &mut Vdp, index: u8, value: u8, region: Region) {
    vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
    let write = vdp.queue_register(index, value, region).unwrap();
    vdp.advance_to(write.publish_at, region).unwrap();
}

fn fixture(region: Region, h40: bool, active: bool) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    register(&mut vdp, 12, if h40 { 0x81 } else { 0 }, region);
    register(&mut vdp, 15, 2, region);
    register(&mut vdp, 5, 0, region);
    for (address, value) in [(0, 0x369c), (0x240, 0x1234), (0x242, 0xabcd)] {
        control(&mut vdp, address, 1, region);
        vdp.write_data(value, region).unwrap();
    }
    vdp.advance_to(tick(vdp.now().get() + 3420), region)
        .unwrap();
    if active {
        register(&mut vdp, 1, 0x44, region);
    }
    vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
    vdp
}

fn partial(vdp: &Vdp, region: Region) -> bool {
    if vdp.read_cache().is_ready()
        || vdp.command().code != 0
        || vdp.control().pending_word().is_some()
    {
        return false;
    }
    let Some(remaining) = vdp.data_read_wait(region).unwrap() else {
        return false;
    };
    let mut restarted = vdp.clone();
    control(&mut restarted, vdp.command().address, 0, region);
    let full = restarted.data_read_wait(region).unwrap().unwrap();
    assert!(remaining <= full);
    remaining < full
}

#[test]
fn first_command_word_cancels_before_partial_and_completed_fetch() {
    for region in REGIONS {
        for h40 in [false, true] {
            for active in [false, true] {
                let seeded = fixture(region, h40, active);
                for phase in 0..3 {
                    let mut vdp = seeded.clone();
                    control(&mut vdp, 0x240, 0, region);
                    let end = vdp.data_read_wait(region).unwrap().unwrap();
                    if phase == 1 {
                        while !partial(&vdp, region) && vdp.now() < end {
                            vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
                        }
                        assert!(partial(&vdp, region));
                    } else if phase == 2 {
                        vdp.advance_to(end, region).unwrap();
                        assert_eq!(vdp.read_cache().buffered_word(), Some(0x1234));
                        assert!(vdp.read_cache().is_ready());
                    }
                    let memory = vdp.memory().clone();
                    let sprites = *vdp.sprite_cache().unwrap();
                    let buffered = vdp.read_cache().buffered_word();
                    let now = vdp.now();
                    vdp.write_control(0x4000, region).unwrap();
                    assert_eq!(vdp.now(), now);
                    assert_eq!(vdp.control().pending_word(), Some(0x4000));
                    assert!(!vdp.read_cache().is_ready());
                    assert_eq!(vdp.read_cache().buffered_word(), buffered);
                    vdp.advance_to(tick(end.get() + 3420), region).unwrap();
                    assert_eq!(vdp.command(), command(0, 0x11));
                    assert_eq!(vdp.memory(), &memory);
                    assert_eq!(vdp.sprite_cache(), Ok(&sprites));
                    assert!(!vdp.read_cache().is_ready());
                    vdp.write_control(0, region).unwrap();
                    vdp.write_data(0xbeef, region).unwrap();
                    vdp.advance_to(tick(vdp.now().get() + 3420), region)
                        .unwrap();
                    control(&mut vdp, 0, 0, region);
                    let ready = vdp.data_read_wait(region).unwrap().unwrap();
                    vdp.advance_to(ready, region).unwrap();
                    assert_eq!(vdp.read_data(region), Ok(0xbeef));
                }
            }
        }
    }
}

#[test]
fn no_effect_fifo_preserves_memory_history_and_following_read() {
    for code in NO_EFFECT {
        for address in [0, 1, 0xffff, 0x1ffff] {
            let mut memory = VdpMemory::default();
            memory.write_word(command(0, 1), 0x369c).unwrap();
            memory.write_word(command(0, 3), 0x0ace).unwrap();
            memory.write_word(command(0, 5), 0x456).unwrap();
            let before = memory.clone();
            let mut fifo = VdpWriteFifo::default();
            let mut current = command(address, code);
            let entry = fifo.push_data(&mut current, Some(2), 0xdead).unwrap();
            assert_eq!(current, command((address + 2) & 0x1ffff, code));
            let next = VdpWrite {
                command: command(2, 1),
                data: 0x5678,
            };
            fifo.push(next).unwrap();
            let history = fifo.next_slot_data();
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
            assert_eq!(fifo.len(), 2);
            assert_eq!(fifo.front(), Some(entry));
            assert_eq!(fifo.next_slot_data(), history);
            assert_eq!(memory, before);
            let mut copied = (fifo.clone(), memory.clone());
            assert_eq!(copied.0.complete_write(&mut copied.1), Ok(Some(entry)));
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(entry)));
            assert_eq!((&fifo, &memory), (&copied.0, &copied.1));
            assert_eq!(memory, before);
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(next)));
            let mut cache = VdpReadCache::default();
            let mut read = command(2, 0);
            assert_eq!(
                cache.complete_read(&memory, &fifo, &mut read, Some(2)),
                Ok(0x5678)
            );
            assert_eq!(current, command((address + 2) & 0x1ffff, code));
            assert_eq!(memory.known_cram().unwrap(), before.known_cram().unwrap());
            assert_eq!(memory.vsram(), before.vsram());
        }
    }
}

#[test]
fn device_full_fifo_waits_for_two_slots_without_sprite_or_forwarding_effects() {
    for region in REGIONS {
        for h40 in [false, true] {
            for active in [false, true] {
                for code in NO_EFFECT {
                    let mut vdp = fixture(region, h40, active);
                    let before = vdp.memory().clone();
                    let sprites = *vdp.sprite_cache().unwrap();
                    control(&mut vdp, 0, code, region);
                    for value in [0xdead, 0xbeef, 0x1357, 0x2468] {
                        vdp.write_data(value, region).unwrap();
                    }
                    assert_eq!(vdp.command().address, 8);
                    let saved = vdp.clone();
                    assert_eq!(
                        vdp.write_data(0xffff, region),
                        Err(VdpPortError::Data(VdpWriteError::Full))
                    );
                    assert_eq!(vdp, saved);
                    let free = vdp.data_write_wait(region).unwrap().unwrap();
                    vdp.advance_to(tick(free.get() - 1), region).unwrap();
                    assert_eq!(vdp.fifo().len(), 4);
                    assert_eq!(vdp.fifo().next_slot_data(), Some(0xdead));
                    vdp.advance_to(free, region).unwrap();
                    assert_eq!(vdp.fifo().len(), 3);
                    assert_eq!(vdp.command().address, 8);
                    vdp.write_data(0xaaaa, region).unwrap();
                    assert_eq!(vdp.command().address, 10);
                    let mut batched = vdp.clone();
                    control(&mut vdp, 0x240, 0, region);
                    control(&mut batched, 0x240, 0, region);
                    let ready = vdp.data_read_wait(region).unwrap().unwrap();
                    for at in vdp.now().get() + 1..=ready.get() {
                        vdp.advance_to(tick(at), region).unwrap();
                    }
                    batched.advance_to(ready, region).unwrap();
                    assert_eq!(vdp, batched);
                    assert_eq!(vdp.memory(), &before);
                    assert_eq!(vdp.sprite_cache(), Ok(&sprites));
                    assert_eq!(vdp.read_data(region), Ok(0x1234));
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CpuBus {
    vdp: Vdp,
    region: Region,
    image: Vec<u8>,
    epoch: u64,
    clock: CpuClocks,
    writes: Vec<(u32, u16, u64)>,
    overlapped: bool,
}

impl CpuBus {
    fn advance(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(start, self.clock);
        self.clock.0 += u64::from(clocks);
        self.vdp
            .advance_to(tick(self.epoch + self.clock.0 * 7), self.region)
            .unwrap();
    }
}

impl ClockedBus for CpuBus {
    type Error = VdpPortError;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, Self::Error> {
        self.advance(start, 4);
        let at = access.bus_address as usize;
        CycleResult::ready(u16::from_be_bytes([self.image[at], self.image[at + 1]]))
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        self.advance(start, 4);
        self.writes
            .push((access.bus_address, value, self.vdp.now().get()));
        if access.bus_address == 0xc00006 {
            self.overlapped |= partial(&self.vdp, self.region);
        }
        let result = if access.bus_address & 4 == 0 {
            self.vdp.write_data(value, self.region)
        } else {
            self.vdp.write_control(value, self.region).map(|_| ())
        };
        CycleResult {
            result,
            wait_clocks: 0,
        }
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.advance(start, clocks);
    }
}

fn guest() -> Vec<u8> {
    let words: [u16; 20] = [
        0x23fc, 0xc000, 0, 0x00c0, 4, 0x33fc, 0, 0x00c0, 0, 0x23fc, 0x4100, 0, 0x00c0, 4, 0x33fc,
        0xbeef, 0x00c0, 0, 0x4e71, 0x4e71,
    ];
    let mut image = vec![0; 0x4000];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in words.into_iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    image
}

#[test]
fn actual_m68k_longword_cancels_partial_fetch_then_recovers() {
    for region in REGIONS {
        for h40 in [false, true] {
            for active in [false, true] {
                let seeded = fixture(region, h40, active);
                let mut witnessed = false;
                for offset in (0..3420).step_by(7) {
                    let mut vdp = seeded.clone();
                    vdp.advance_to(tick(vdp.now().get() + offset), region)
                        .unwrap();
                    vdp.write_control(0x0240, region).unwrap();
                    let epoch = vdp.now().get();
                    let memory = vdp.memory().clone();
                    let mut bus = CpuBus {
                        vdp,
                        region,
                        image: guest(),
                        epoch,
                        clock: CpuClocks(0),
                        writes: Vec::new(),
                        overlapped: false,
                    };
                    let mut registers = Registers::new();
                    registers.pc = 0x100;
                    registers.ssp = 0x1000;
                    registers.set_status_register(0x2700);
                    let mut cpu =
                        PrefetchCpu::from_prefetched(registers, [0x23fc, 0xc000], CpuClocks(0))
                            .unwrap();
                    let initial = (cpu.clone(), bus.clone());
                    cpu.step_boundary(&mut bus).unwrap();
                    assert_eq!(bus.writes.len(), 2);
                    assert_eq!((bus.writes[0].0, bus.writes[0].1), (0xc00004, 0xc000));
                    assert_eq!((bus.writes[1].0, bus.writes[1].1), (0xc00006, 0));
                    assert_eq!(bus.writes[1].2 - bus.writes[0].2, 28);
                    assert_eq!(bus.vdp.command(), command(0, 0x10));
                    assert_eq!(bus.vdp.control().pending_word(), Some(0));
                    cpu.step_boundary(&mut bus).unwrap();
                    assert_eq!(bus.vdp.control().pending_word(), None);
                    assert_eq!(bus.vdp.command(), command(2, 0x10));
                    assert_eq!(bus.vdp.memory(), &memory);
                    let mut copied = (cpu.clone(), bus.clone());
                    for _ in 0..2 {
                        assert_eq!(
                            cpu.step_boundary(&mut bus),
                            copied.0.step_boundary(&mut copied.1)
                        );
                    }
                    assert_eq!((&cpu, &bus), (&copied.0, &copied.1));
                    bus.vdp
                        .advance_to(tick(bus.vdp.now().get() + 3420), region)
                        .unwrap();
                    assert_eq!(&bus.vdp.memory().vram()[0x100..0x102], &[0xbe, 0xef]);
                    assert_eq!(
                        &bus.vdp.memory().vram()[0x240..0x244],
                        &[0x12, 0x34, 0xab, 0xcd]
                    );
                    if bus.overlapped {
                        let (mut register_cpu, mut register_bus) = initial;
                        register_bus.image[0x104..0x106].copy_from_slice(&0x8f02_u16.to_be_bytes());
                        register_cpu.step_boundary(&mut register_bus).unwrap();
                        assert_eq!(register_bus.writes.len(), 2);
                        assert!(register_bus.overlapped);
                        assert_eq!(register_bus.vdp.command(), command(0xf02, 0x12));
                        assert_eq!(register_bus.vdp.control().pending_word(), None);
                        assert_eq!(register_bus.vdp.memory(), &memory);
                        assert!(!register_bus.vdp.read_cache().is_ready());
                        let mut resumed = (register_cpu.clone(), register_bus.clone());
                        register_cpu.step_boundary(&mut register_bus).unwrap();
                        resumed.0.step_boundary(&mut resumed.1).unwrap();
                        assert_eq!((register_cpu, register_bus), resumed);
                        witnessed = true;
                        break;
                    }
                }
                assert!(witnessed, "{region:?} h40={h40} active={active}");
            }
        }
    }
}

#[test]
fn actual_machine_pending_commands_and_fifo_restore_in_both_schedulers() {
    for region in REGIONS {
        for concurrent_sound in [false, true] {
            let mut image = guest();
            let words: [u16; 25] = [
                0x33fc, 0x8c81, 0x00c0, 4, 0x33fc, 0x8144, 0x00c0, 4, 0x33fc, 0x8f02, 0x00c0, 4,
                0x23fc, 0x4000, 0x20, 0x00c0, 4, 0x33fc, 0xdead, 0x00c0, 0, 0x4e71, 0x4e71, 0x60fe,
                0x4e71,
            ];
            for (index, word) in words.into_iter().enumerate() {
                image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
            }
            let mut machine = Emulator::new(
                image,
                LoadConfig {
                    region,
                    concurrent_sound,
                },
            )
            .unwrap();
            let mut pending = false;
            for _ in 0..8 {
                machine.step_instruction().unwrap();
                pending |= !machine.vdp().fifo().is_empty();
                let bytes = machine.encode_state().unwrap();
                assert_eq!(&bytes[8..12], &1_u32.to_le_bytes());
                let mut restored = machine.clone();
                restored.decode_state(&bytes).unwrap();
                assert_eq!(bytes, restored.encode_state().unwrap());
                assert_eq!(machine.step_instruction(), restored.step_instruction());
                assert_eq!(
                    machine.encode_state().unwrap(),
                    restored.encode_state().unwrap()
                );
            }
            assert!(pending);
            assert!(
                machine
                    .vdp()
                    .memory()
                    .vram()
                    .iter()
                    .all(|&value| value == 0)
            );
        }
    }
}

#[test]
fn actual_machine_overlapping_longword_and_fresh_write_in_both_schedulers() {
    for region in REGIONS {
        for h40 in [false, true] {
            for concurrent_sound in [false, true] {
                for padding in 0..8 {
                    let mut words: Vec<u16> = vec![
                        0x33fc,
                        if h40 { 0x8c81 } else { 0x8c00 },
                        0x00c0,
                        4,
                        0x33fc,
                        0x8144,
                        0x00c0,
                        4,
                        0x33fc,
                        0x8f02,
                        0x00c0,
                        4,
                    ];
                    words.extend(std::iter::repeat_n(0x4e71, padding));
                    words.extend([0x33fc, 0x0240, 0x00c0, 4]);
                    let mut image = guest();
                    let tail = image[0x100..0x128].to_vec();
                    for (index, word) in words.iter().enumerate() {
                        image[0x100 + index * 2..0x102 + index * 2]
                            .copy_from_slice(&word.to_be_bytes());
                    }
                    image[0x100 + words.len() * 2..0x128 + words.len() * 2].copy_from_slice(&tail);
                    let end = 0x128 + words.len() * 2;
                    image[end..end + 2].copy_from_slice(&0x60fe_u16.to_be_bytes());
                    let mut machine = Emulator::new(
                        image,
                        LoadConfig {
                            region,
                            concurrent_sound,
                        },
                    )
                    .unwrap();
                    for _ in 0..4 + padding {
                        machine.step_instruction().unwrap();
                    }
                    assert_eq!(machine.vdp().control().pending_word(), Some(0x0240));
                    let bytes = machine.encode_state().unwrap();
                    let mut restored = machine.clone();
                    restored.decode_state(&bytes).unwrap();
                    for expected in [
                        command(0, 0x10),
                        command(2, 0x10),
                        command(0x100, 1),
                        command(0x102, 1),
                    ] {
                        assert_eq!(machine.step_instruction(), restored.step_instruction());
                        assert_eq!(machine.vdp().command(), expected);
                        assert_eq!(
                            machine.encode_state().unwrap(),
                            restored.encode_state().unwrap()
                        );
                    }
                    for _ in 0..32 {
                        machine.step_instruction().unwrap();
                    }
                    assert_eq!(&machine.vdp().memory().vram()[0x100..0x102], &[0xbe, 0xef]);
                    assert!(
                        machine.vdp().memory().vram()[..0x100]
                            .iter()
                            .all(|&byte| byte == 0)
                    );
                }
            }
        }
    }
}
