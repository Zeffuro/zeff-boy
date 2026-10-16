use super::*;
use crate::hardware::vdp::VdpDmaSource;

const REGION: Region = Region::NtscUs;
const DESTINATION: usize = 0x400;
const SOURCE: usize = 0x8000;

fn register(machine: &mut Emulator, index: u8, value: u8) {
    machine.bus.advance_machine(1).unwrap();
    let write = machine
        .bus
        .vdp
        .queue_register(index, value, REGION)
        .unwrap();
    machine
        .bus
        .advance_machine(write.publish_at.get() - machine.bus.now.get())
        .unwrap();
}

fn fixture(concurrent: bool, h40: bool) -> Emulator {
    let mut machine = super::vdp_line::seed(concurrent, 0x4e71);
    for (index, value) in [
        (12, if h40 { 0x81 } else { 0 }),
        (1, 0x54),
        (19, 17),
        (20, 0),
        (21, 0),
        (22, 0xc0),
        (23, 0x7f),
    ] {
        register(&mut machine, index, value);
    }
    machine.bus.ram[SOURCE..SOURCE + 34].fill(0x44);
    machine.bus.vdp.write_control(0x4400, REGION).unwrap();
    machine.bus.vdp.write_control(0x80, REGION).unwrap();
    machine
}

fn release(machine: &Emulator) -> u64 {
    machine
        .vdp()
        .external_dma_release(
            REGION,
            VdpDmaSource::new(&machine.bus.cartridge.bytes, &machine.bus.ram),
        )
        .unwrap()
        .unwrap()
        .get()
}

fn sound_write(machine: &mut Emulator) {
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x1ff;
    machine.bus.sound.ram[..8].copy_from_slice(&[0x3e, 0x5a, 0x32, 0, 0x80, 0x76, 0, 0]);
}

pub fn reserved_source_and_capture_with(
    mut advance: impl FnMut(&mut Emulator, u64) -> Result<(), BusError>,
) {
    for h40 in [false, true] {
        let mut machine = fixture(true, h40);
        sound_write(&mut machine);
        let end = release(&machine);
        let start = machine.bus.now.get();
        let first = MasterTicks::new(start + 60);
        machine.queue_vdp_capture(first).unwrap();
        advance(&mut machine, first.get() - start).unwrap();
        assert!(machine.take_vdp_capture().unwrap().is_err());
        assert_eq!(machine.bus.ram[SOURCE], 0x44);
        let mut copy = machine.clone();
        for candidate in [&mut machine, &mut copy] {
            let delta = end - 1 - candidate.bus.now.get();
            advance(candidate, delta).unwrap();
            assert_eq!(candidate.bus.ram[SOURCE], 0x44);
            assert!(candidate.sound_cpu.cycles() > 0);
            let pending = candidate.sound_schedule.pending.as_ref().unwrap();
            assert!(pending.start >= end);
            advance(candidate, 2000).unwrap();
            assert_eq!(candidate.bus.ram[SOURCE], 0x5a);
            assert_eq!(
                &candidate.vdp().memory().vram()[DESTINATION..DESTINATION + 34],
                &[0x44; 34]
            );
            assert_eq!(candidate.vdp().registers().registers()[19], Some(0));
            assert_eq!(candidate.sound_fault(), None);
        }
        assert_eq!(machine, copy);
    }
}

pub fn reserved_source_and_capture() {
    reserved_source_and_capture_with(super::vdp_row_delivery::advance);
}

pub fn active_restart_rollback_and_reset() {
    for concurrent in [false, true] {
        for h40 in [false, true] {
            let mut machine = fixture(concurrent, h40);
            if concurrent {
                sound_write(&mut machine);
            }
            let first = MasterTicks::new(machine.bus.now.get() + 60);
            machine.queue_vdp_capture(first).unwrap();
            machine.bus.cartridge.initial.pc = 0xc0000e;
            let before = machine.clone();
            let pointer = machine.vdp().memory().vram().as_ptr();
            assert!(machine.reset_cpu().is_err());
            assert_eq!(machine, before);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            machine.bus.cartridge.initial.pc = 0x100;
            machine.reset_cpu().unwrap();
            assert_eq!(machine.registers().pc, 0x100);
            assert_eq!(machine.vdp().registers().registers()[19], Some(0));
            super::vdp_row_delivery::advance(&mut machine, 2000).unwrap();
            assert_eq!(
                &machine.vdp().memory().vram()[DESTINATION..DESTINATION + 34],
                &[0x44; 34]
            );
            Reset::reset(&mut machine);
            assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
            assert!(machine.vdp().memory().vram().iter().all(|&byte| byte == 0));
            assert!(machine.vdp().fifo().is_empty());
            assert_eq!(machine.sound_deadline(), None);
        }
    }
    let mut machine = fixture(true, false);
    sound_write(&mut machine);
    let pointer = machine.vdp().memory().vram().as_ptr();
    Reset::reset(&mut machine);
    assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
    assert!(machine.vdp().fifo().is_empty());
    assert_eq!(machine.sound_deadline(), None);
    assert_eq!(machine.vdp().registers().registers()[19], Some(0));
}

#[test]
fn source_reservation() {
    reserved_source_and_capture();
}

#[test]
fn restart_domains() {
    active_restart_rollback_and_reset();
}
