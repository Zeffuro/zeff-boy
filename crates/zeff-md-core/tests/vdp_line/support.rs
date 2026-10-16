use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{Region, Vdp, VdpInterrupts, VdpRegisterPipeline, VdpVerticalTiming};

pub const REGION: Region = Region::NtscUs;
pub const ROW: u16 = 64;
pub const ROW_AT: u64 = 64 * 3420 + 1;

pub fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

pub fn publish(bank: &mut VdpRegisterPipeline, index: u8, value: u8) {
    bank.advance_to(tick(bank.now().get() + 1)).unwrap();
    let write = bank.queue_register(index, value).unwrap();
    bank.advance_to(write.publish_at).unwrap();
}

pub fn configured(mut bank: VdpRegisterPipeline) -> VdpRegisterPipeline {
    for (index, value) in [
        (0, 0),
        (1, 0x44),
        (2, 0x30),
        (3, 0x1c),
        (4, 5),
        (5, 0x10),
        (7, 15),
        (10, 0xff),
        (11, 0),
        (12, 0),
        (13, 0x3c),
        (15, 2),
        (16, 0),
        (17, 0),
        (18, 0),
    ] {
        publish(&mut bank, index, value);
    }
    bank
}

pub fn from_bank(mut bank: VdpRegisterPipeline, now: u64, stable: bool) -> Vdp {
    let mut irq = VdpInterrupts::at_counters_zero(tick(0), 0xff, false, false);
    irq.advance_to(bank.now(), REGION, &bank).unwrap();
    while irq
        .advance_registers_to(tick(now), REGION, &mut bank)
        .unwrap()
        .is_some()
    {}
    if stable {
        Vdp::from_stable_display_state(irq, bank, REGION).unwrap()
    } else {
        Vdp::from_known_state(irq, bank, REGION).unwrap()
    }
}

pub fn device_at(now: u64) -> Vdp {
    let bank = configured(*Vdp::bootstrap_mode5().registers());
    from_bank(bank, now, true)
}

pub fn command(device: &mut Vdp, address: u32, code: u8) {
    device
        .write_control(
            (address as u16 & 0x3fff) | (u16::from(code & 3) << 14),
            REGION,
        )
        .unwrap();
    device
        .write_control(
            ((address >> 14) as u16 & 7) | (u16::from(code & 0x3c) << 2),
            REGION,
        )
        .unwrap();
}

pub fn drain(device: &mut Vdp) {
    while !device.fifo().is_empty() {
        let next = device.data_write_wait(REGION).unwrap().unwrap_or_else(|| {
            VdpVerticalTiming::at_counters_zero(tick(0))
                .next_memory_slot(device.now(), REGION, device.registers())
                .unwrap()
        });
        device.advance_to(tick(next.get() + 1), REGION).unwrap();
    }
}

pub fn words(device: &mut Vdp, address: u32, code: u8, words: &[u16]) {
    command(device, address, code);
    for chunk in words.chunks(4) {
        for &word in chunk {
            device.write_data(word, REGION).unwrap();
        }
        drain(device);
    }
}

pub fn register(device: &mut Vdp, index: u8, value: u8) {
    let write = device.queue_register(index, value, REGION).unwrap();
    device
        .advance_to(tick(write.publish_at.get() + 1), REGION)
        .unwrap();
}

pub fn color(index: u16) -> u16 {
    ((index & 7) << 1) | (((index >> 3) & 7) << 5) | (6 << 9)
}

pub fn palette(device: &mut Vdp) {
    let colors: [u16; 64] = std::array::from_fn(|index| color(index as u16));
    words(device, 0, 3, &colors);
}

pub fn sat(device: &mut Vdp, base: u32, index: u8, entry: [u16; 4]) {
    words(device, base + u32::from(index) * 8, 1, &entry);
}

pub fn fixture() -> Vdp {
    let mut device = device_at(301);
    palette(&mut device);
    words(&mut device, 32, 1, &[0x1234, 0x5678]);
    words(&mut device, 60, 1, &[0x1020, 0x3040]);
    words(&mut device, 64, 1, &[0x2222, 0x2222]);
    words(&mut device, 96, 1, &[0x3030, 0x3030]);
    words(&mut device, 128, 1, &[0x4567, 0x89ab]);
    words(&mut device, 0xc200, 1, &[1, 0x0801, 0x9001, 1]);
    words(&mut device, 0xa200, 1, &[0xa002, 0x2002, 0xa002, 0xa002]);
    words(&mut device, 0x7200, 1, &[0x8001]);
    sat(&mut device, 0x2000, 0, [192, 1, 0x4003, 128]);
    sat(&mut device, 0x2000, 1, [192, 0, 0xe004, 152]);
    sat(&mut device, 0x4000, 0, [0, 0, 0xe004, 168]);
    sat(&mut device, 0x4000, 1, [0, 0, 0xe004, 176]);
    assert!(device.now().get() < ROW_AT);
    device.advance_to(tick(ROW_AT), REGION).unwrap();
    device
}

pub fn expected(x: usize) -> u16 {
    let index = match x {
        0..=7 => 18,
        8..=15 => 16 - x as u16,
        16..=23 => [1, 18, 2, 18, 3, 18, 4, 18][x - 16],
        24..=31 => 52 + (x - 24) as u16,
        _ => 15,
    };
    color(index)
}
