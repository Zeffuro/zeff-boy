use zeff_emu_common::time::{MachineTiming, MasterTicks, Reset};
use zeff_md_core::{Emulator, LoadConfig, Region, Vdp, VdpDmaSource};

const LINE_TICKS: u64 = 3420;
const PAL_FIELD_TICKS: u64 = 1_070_460;

pub fn actual_cpu_region_ports_and_clock() {
    let words: [u16; 20] = [
        0x1039, 0x00a1, 1, 0x13c0, 0x00ff, 0x100, 0x3239, 0x00c0, 4, 0x33c1, 0x00ff, 0x102, 0x3439,
        0x00c0, 8, 0x33c2, 0x00ff, 0x104, 0x60fe, 0x4e71,
    ];
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in words.into_iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent_sound in [false, true] {
            let mut machine = Emulator::new(
                image.clone(),
                LoadConfig {
                    region,
                    concurrent_sound,
                },
            )
            .unwrap();
            assert_eq!(machine.region(), region);
            for _ in 0..6 {
                machine.step_instruction().unwrap();
            }
            assert_eq!(
                machine.peek(0xff0100),
                Some(if region == Region::Pal { 0xe0 } else { 0xa0 })
            );
            assert_eq!(
                machine.peek(0xff0103).unwrap() & 1,
                u8::from(region == Region::Pal)
            );
            assert_eq!(machine.peek(0xff0104), Some(0));
            assert_eq!(
                machine.timing_snapshot().rate().numerator_hz(),
                region.master_hz()
            );
            let saved = machine.clone();
            let mut copy = saved.clone();
            assert_eq!(machine.step_instruction(), copy.step_instruction());
            assert_eq!(machine, copy);
            Reset::reset(&mut machine);
            assert_eq!(machine.region(), region);
            assert_eq!(
                machine.timing_snapshot().rate().numerator_hz(),
                region.master_hz()
            );
            assert_eq!(machine.timing_snapshot().now().get(), 56);
        }
    }
}

fn fixture(h40: bool, length: u16) -> Vdp {
    let mut device = Vdp::bootstrap_mode5();
    for (index, value) in [
        (12, if h40 { 0x81 } else { 0 }),
        (1, 0x14),
        (15, 2),
        (19, length as u8),
        (20, (length >> 8) as u8),
        (21, 0),
        (22, 0),
        (23, 0),
    ] {
        device
            .advance_to(MasterTicks::new(device.now().get() + 1), Region::Pal)
            .unwrap();
        let pending = device.queue_register(index, value, Region::Pal).unwrap();
        device.advance_to(pending.publish_at, Region::Pal).unwrap();
    }
    device
}

pub fn dma_crosses_field_and_preserves_source_failure() {
    let rom: Vec<_> = (0..256).map(|n| n as u8).collect();
    let source = VdpDmaSource::new(&rom, &[]);
    for h40 in [false, true] {
        for boundary in [262 * LINE_TICKS, PAL_FIELD_TICKS] {
            let mut device = fixture(h40, 128);
            device
                .advance_to(MasterTicks::new(boundary - 1000), Region::Pal)
                .unwrap();
            device.write_control(0x4400, Region::Pal).unwrap();
            device.write_control(0x80, Region::Pal).unwrap();
            let mut parts = device.clone();
            let release = device
                .external_dma_release(Region::Pal, source)
                .unwrap()
                .unwrap();
            assert!(release.get() > boundary);
            let end = MasterTicks::new(release.get() + LINE_TICKS);
            device
                .advance_with_source(end, Region::Pal, source)
                .unwrap();
            parts
                .advance_with_source(MasterTicks::new(boundary), Region::Pal, source)
                .unwrap();
            parts.advance_with_source(end, Region::Pal, source).unwrap();
            assert_eq!(device, parts);
            assert_eq!(&device.memory().vram()[0x400..0x500], &rom);
            assert_eq!(
                device.registers().registers()[19..23],
                [Some(0), Some(0), Some(128), Some(0)]
            );
        }
        let mut device = fixture(h40, 0);
        device.write_control(0xc000, Region::Pal).unwrap();
        device.write_control(0x80, Region::Pal).unwrap();
        let release = device
            .external_dma_release(Region::Pal, source)
            .unwrap()
            .unwrap();
        assert!(release.get() > PAL_FIELD_TICKS);
        device
            .advance_with_source(
                MasterTicks::new(release.get() + LINE_TICKS),
                Region::Pal,
                source,
            )
            .unwrap();
        assert_eq!(device.registers().registers()[19..24], [Some(0); 5]);
        assert!(device.memory().cram().iter().all(|&word| word == 0x0eee));
        let mut device = fixture(h40, 128);
        device
            .advance_to(MasterTicks::new(PAL_FIELD_TICKS - 1000), Region::Pal)
            .unwrap();
        device
            .advance_to(MasterTicks::new(device.now().get() + 1), Region::Pal)
            .unwrap();
        let pending = device.queue_register(23, 0x70, Region::Pal).unwrap();
        device.advance_to(pending.publish_at, Region::Pal).unwrap();
        device.write_control(0x4400, Region::Pal).unwrap();
        device.write_control(0x80, Region::Pal).unwrap();
        let saved = device.clone();
        assert!(
            device
                .advance_with_source(
                    MasterTicks::new(PAL_FIELD_TICKS + 2 * LINE_TICKS),
                    Region::Pal,
                    VdpDmaSource::new(&[], &[0x12; 40])
                )
                .is_err()
        );
        assert_eq!(device, saved);
    }
}
