use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{ControlCommand, Region, Vdp};

pub const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];

pub fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

pub fn control(vdp: &mut Vdp, address: u32, code: u8, region: Region) {
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
    assert_eq!(vdp.command(), ControlCommand { address, code });
}

pub fn register(vdp: &mut Vdp, index: u8, value: u8, region: Region) {
    vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
    let write = vdp.queue_register(index, value, region).unwrap();
    vdp.advance_to(write.publish_at, region).unwrap();
}

pub fn fixture(region: Region, h40: bool, active: bool) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    register(&mut vdp, 12, if h40 { 0x81 } else { 0 }, region);
    register(&mut vdp, 15, 2, region);
    for (address, value) in [
        (0, 0x369c),
        (0x240, 0x12ab),
        (0x242, 0xcd56),
        (0x3ffe, 0xe781),
    ] {
        control(&mut vdp, address, 1, region);
        vdp.write_data(value, region).unwrap();
    }
    vdp.advance_to(tick(vdp.now().get() + 3420), region)
        .unwrap();
    if active {
        register(&mut vdp, 1, 0x44, region);
    }
    vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
    control(&mut vdp, 0, 0, region);
    let end = vdp.data_read_wait(region).unwrap().unwrap();
    vdp.advance_to(end, region).unwrap();
    assert_eq!(vdp.read_cache().buffered_word(), Some(0x369c));
    vdp
}

pub fn first_slot(vdp: &Vdp, region: Region) -> MasterTicks {
    assert!(vdp.fifo().is_empty());
    let mut probe = vdp.clone();
    control(&mut probe, 0, 3, region);
    for word in [0x0123, 0x0456, 0x0789, 0x0abc] {
        probe.write_data(word, region).unwrap();
    }
    // A CRAM FIFO word occupies one memory slot, independently of read progress.
    probe.data_write_wait(region).unwrap().unwrap()
}

pub fn phase(vdp: &Vdp, region: Region) -> u8 {
    if vdp.read_cache().is_ready() {
        return 2;
    }
    let remaining = vdp.data_read_wait(region).unwrap().unwrap();
    let mut restarted = vdp.clone();
    control(&mut restarted, vdp.command().address, 0, region);
    let full = restarted.data_read_wait(region).unwrap().unwrap();
    assert!(remaining <= full);
    u8::from(remaining < full)
}

pub fn fresh_read(vdp: &mut Vdp, region: Region, increment: u8) {
    control(vdp, 0x240, 0, region);
    let start = vdp.now();
    let first = first_slot(vdp, region);
    let end = vdp.data_read_wait(region).unwrap().unwrap();
    assert!(start < first && first < end);
    assert!(!vdp.read_cache().is_ready());
    vdp.advance_to(first, region).unwrap();
    assert!(!vdp.read_cache().is_ready());
    assert_eq!(vdp.command().address, 0x240);
    vdp.advance_to(tick(end.get() - 1), region).unwrap();
    assert!(!vdp.read_cache().is_ready());
    vdp.advance_to(end, region).unwrap();
    assert_eq!(vdp.command().address, 0x240 + u32::from(increment));
    assert_eq!(vdp.read_data(region), Ok(0x12ab));
}

pub fn image(words: &[u16]) -> Vec<u8> {
    let mut image = vec![0; 0x4000];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in words.iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    image
}
