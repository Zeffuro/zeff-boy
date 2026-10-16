use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{Region, Vdp, VdpDmaError, VdpDmaSource, VdpScheduleError, VdpVerticalTiming};

const REGION: Region = Region::NtscUs;
const LINE_TICKS: u64 = 3420;
fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

pub fn fixture(h40: bool, length: u16, source: u16, high: u8, increment: u8) -> Vdp {
    let mut device = Vdp::bootstrap_mode5();
    for (index, value) in [
        (12, if h40 { 0x81 } else { 0 }),
        (1, 0x14),
        (15, increment),
        (19, length as u8),
        (20, (length >> 8) as u8),
        (21, source as u8),
        (22, (source >> 8) as u8),
        (23, high),
    ] {
        device
            .advance_to(tick(device.now().get() + 1), REGION)
            .unwrap();
        device
            .write_control(0x8000 | index << 8 | u16::from(value), REGION)
            .unwrap();
        let at = device.registers().pending().unwrap().publish_at;
        device.advance_to(at, REGION).unwrap();
    }
    device
}

pub fn arm(device: &mut Vdp, address: u32, code: u8) {
    device
        .write_control(
            (address as u16 & 0x3fff) | u16::from(code & 3) << 14,
            REGION,
        )
        .unwrap();
    device
        .write_control(
            ((address >> 14) as u16 & 7) | u16::from(code & 0x3c) << 2,
            REGION,
        )
        .unwrap();
}

pub fn fifo_acceptance_and_tail() {
    let rom = [0x12, 0x34, 0xab, 0xcd, 0x56, 0x78, 0x9a, 0xbc];
    let source = VdpDmaSource::new(&rom, &[]);
    for h40 in [false, true] {
        let mut device = fixture(h40, 4, 0, 0, 2);
        arm(&mut device, 0, 0x21);
        let before = device.clone();
        let release = device
            .external_dma_release(REGION, source)
            .unwrap()
            .unwrap();
        assert_eq!(device, before);
        let first = VdpVerticalTiming::at_counters_zero(tick(0))
            .next_memory_slot(device.now(), REGION, device.registers())
            .unwrap();
        device
            .advance_with_source(tick(first.get() - 1), REGION, source)
            .unwrap();
        assert!(device.fifo().is_empty());
        device.advance_with_source(first, REGION, source).unwrap();
        assert_eq!(device.fifo().len(), 1);
        assert_eq!(&device.memory().vram()[..2], &[0, 0]);
        assert_eq!(device.registers().registers()[19], Some(3));
        assert_eq!(device.registers().registers()[21], Some(1));
        assert_eq!(device.command().address, 2);
        let saved = device.clone();
        assert_eq!(
            device.advance_to(release, REGION),
            Err(VdpScheduleError::Dma(VdpDmaError::MissingSource))
        );
        assert_eq!(device, saved);
        device.advance_with_source(release, REGION, source).unwrap();
        assert_eq!(device.registers().registers()[19], Some(0));
        assert_eq!(device.registers().registers()[21], Some(4));
        assert_eq!(device.command().code & 0x20, 0);
        assert!(!device.fifo().is_empty());
        assert_eq!(device.external_dma_release(REGION, source), Ok(None));
        assert_ne!(&device.memory().vram()[..8], &rom);
        device
            .advance_with_source(tick(release.get() + LINE_TICKS), REGION, source)
            .unwrap();
        assert!(device.fifo().is_empty());
        assert_eq!(&device.memory().vram()[..8], &rom);
        let mut whole = before;
        whole
            .advance_with_source(device.now(), REGION, source)
            .unwrap();
        assert_eq!(device, whole);
    }
}

pub fn source_pages_and_ram_mirrors() {
    let mut rom = vec![0; 0x60000];
    rom[0x5fffe..].copy_from_slice(&[0x12, 0x34]);
    rom[0x40000..0x40002].copy_from_slice(&[0xab, 0xcd]);
    let mut ram = vec![0; 65536];
    ram[65534..].copy_from_slice(&[0x56, 0x78]);
    ram[..2].copy_from_slice(&[0x9a, 0xbc]);
    let source = VdpDmaSource::new(&rom, &ram);
    for (high, expected) in [
        (2, [0x34, 0x12, 0xcd, 0xab]),
        (0x7f, [0x78, 0x56, 0xbc, 0x9a]),
    ] {
        let mut device = fixture(true, 2, 65535, high, 2);
        arm(&mut device, 1, 0x21);
        let release = device
            .external_dma_release(REGION, source)
            .unwrap()
            .unwrap();
        device
            .advance_with_source(tick(release.get() + LINE_TICKS), REGION, source)
            .unwrap();
        assert_eq!(&device.memory().vram()[..4], &expected);
        assert_eq!(device.registers().registers()[21], Some(1));
        assert_eq!(device.registers().registers()[22], Some(0));
        assert_eq!(device.registers().registers()[23], Some(high));
    }
}

pub fn whole_horizon_source_fault_is_atomic() {
    let ram = [0x12; 40];
    let source = VdpDmaSource::new(&[], &ram);
    let mut device = fixture(false, 32, 0, 0x70, 2);
    arm(&mut device, 0, 0x21);
    let before = device.clone();
    let error = VdpScheduleError::Dma(VdpDmaError::UnsupportedSource(0xe00028));
    assert_eq!(device.external_dma_release(REGION, source), Err(error));
    assert_eq!(device, before);
    assert_eq!(
        device.advance_with_source(tick(device.now().get() + LINE_TICKS), REGION, source),
        Err(error)
    );
    assert_eq!(device, before);
    let mut pal = device.clone();
    let at = tick(device.now().get() + 1);
    assert_eq!(
        pal.advance_with_source(at, Region::Pal, source),
        device.advance_with_source(at, REGION, source)
    );
    assert_eq!(pal, device);
}

pub fn active_fifo_prefix_and_sprite_retirement() {
    let rom = [0, 0x80, 1, 0, 0, 1, 0, 0x80];
    let source = VdpDmaSource::new(&rom, &[]);
    for h40 in [false, true] {
        let mut device = fixture(h40, 4, 0, 0, 2);
        device
            .advance_to(tick(device.now().get() + 1), REGION)
            .unwrap();
        let pending = device.queue_register(1, 0x54, REGION).unwrap();
        device.advance_to(pending.publish_at, REGION).unwrap();
        arm(&mut device, 0x400, 1);
        for word in [0x1122, 0x3344, 0x5566, 0x7788] {
            device.write_data(word, REGION).unwrap();
        }
        arm(&mut device, 0, 0x21);
        let start = device.clone();
        let calendar = VdpVerticalTiming::at_counters_zero(tick(0));
        let first = calendar
            .next_memory_slot(device.now(), REGION, device.registers())
            .unwrap();
        device.advance_with_source(first, REGION, source).unwrap();
        assert_eq!(device.fifo().len(), 4);
        assert_eq!(device.registers().registers()[19], Some(4));
        assert_eq!(&device.memory().vram()[0x400..0x402], &[0, 0x22]);
        let second = calendar
            .next_memory_slot(first, REGION, device.registers())
            .unwrap();
        device.advance_with_source(second, REGION, source).unwrap();
        assert_eq!(device.fifo().len(), 4);
        assert_eq!(device.registers().registers()[19], Some(3));
        assert_eq!(&device.memory().vram()[0x400..0x402], &[0x11, 0x22]);
        assert_eq!(&device.memory().vram()[..8], &[0; 8]);
        let release = device
            .external_dma_release(REGION, source)
            .unwrap()
            .unwrap();
        device.advance_with_source(release, REGION, source).unwrap();
        assert_eq!(device.registers().registers()[19], Some(0));
        assert!(!device.fifo().is_empty());
        device
            .advance_with_source(tick(release.get() + 4 * LINE_TICKS), REGION, source)
            .unwrap();
        assert_eq!(&device.memory().vram()[..8], &rom);
        assert_eq!(
            &device.memory().vram()[0x400..0x408],
            &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]
        );
        assert_eq!(
            device.sprite_cache().unwrap().entry_words(0).unwrap(),
            [Some(0x80), Some(0x100)]
        );
        let mut whole = start;
        whole
            .advance_with_source(device.now(), REGION, source)
            .unwrap();
        assert_eq!(device, whole);
    }
}

pub fn zero_length_and_partitioned_transfer() {
    let mut rom = vec![0; 2 * 65536];
    for (index, word) in rom.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        *word = (index as u16).wrapping_mul(73).to_be_bytes();
    }
    let source = VdpDmaSource::new(&rom, &[]);
    let mut whole = fixture(false, 0, 0, 0, 2);
    arm(&mut whole, 0, 0x23);
    let mut parts = whole.clone();
    let release = whole.external_dma_release(REGION, source).unwrap().unwrap();
    whole
        .advance_with_source(tick(release.get() + LINE_TICKS), REGION, source)
        .unwrap();
    let end = whole.now();
    while parts.now() < end {
        let next = tick((parts.now().get() + 20003).min(end.get()));
        parts.advance_with_source(next, REGION, source).unwrap();
    }
    assert_eq!(whole, parts);
    assert_eq!(whole.registers().registers()[19..24], [Some(0); 5]);
    assert_eq!(whole.command().address, 0);
    assert!(whole.fifo().is_empty());
    for (index, &word) in whole.memory().cram().iter().enumerate() {
        assert_eq!(word, (65472u16 + index as u16).wrapping_mul(73) & 0x0eee);
    }
}
