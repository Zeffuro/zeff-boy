use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

#[path = "internal_clock_tests/cpu_cases.rs"]
mod cpu_cases;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn fixture(region: Region, display: bool, old: u8, at: u64) -> Vdp {
    let mut image = [0; 24];
    image[1] = 0x14 | if display { 0x40 } else { 0 };
    image[10] = 255;
    image[12] = old;
    image[15] = 2;
    let registers = VdpRegisterPipeline::clock_fixture(tick(0), tick(0), image);
    let interrupts = VdpInterrupts::at_counters_zero(tick(0), 255, false, false);
    let mut vdp = Vdp::from_stable_display_state(interrupts, registers, region).unwrap();
    vdp.advance_to(tick(at), region).unwrap();
    vdp
}

fn clock(vdp: &mut Vdp, region: Region, value: u8) -> VdpRegisterWrite {
    vdp.write_control(0x8c00 | u16::from(value), region)
        .unwrap();
    vdp.registers().pending().unwrap()
}

fn command(vdp: &mut Vdp, region: Region, address: u16, code: u8) {
    vdp.write_control((address & 0x3fff) | (u16::from(code & 3) << 14), region)
        .unwrap();
    vdp.write_control((address >> 14) | (u16::from(code & !3) << 2), region)
        .unwrap();
}

fn restore(vdp: &Vdp, region: Region) -> Vdp {
    let mut writer = StateWriter::new();
    vdp.write_state_in_region(&mut writer, region).unwrap();
    let bytes = writer.into_bytes();
    let mut reader = StateReader::new(&bytes);
    let mut target = Vdp::bootstrap_mode5();
    target.read_state(&mut reader, region).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(target, *vdp);
    target
}

#[test]
fn published_internal_clock_keeps_old_line_then_retires_new_fifo_slots_once() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for old in [0, 0x81] {
            let mut vdp = fixture(region, false, old, 20 * 3420 + 3399);
            let write = clock(&mut vdp, region, 1);
            assert_eq!(write.publish_at.get(), 20 * 3420 + 3400);
            vdp.advance_to(write.publish_at, region).unwrap();
            assert_eq!(vdp.effective_line_ticks(), 3420);
            command(&mut vdp, region, 0x100, VRAM_WRITE_CODE);
            vdp.write_data(0xcafe, region).unwrap();
            let mut whole = vdp.clone();
            let mut restored = restore(&vdp, region);
            let end = 21 * 3420 + 32;
            for at in [21 * 3420 - 1, 21 * 3420, 21 * 3420 + 16, end] {
                vdp.advance_to(tick(at), region).unwrap();
                restored.advance_to(tick(at), region).unwrap();
                assert_eq!(vdp, restored);
            }
            whole.advance_to(tick(end), region).unwrap();
            assert_eq!(vdp, whole);
            assert_eq!(vdp.effective_line_ticks(), 3360);
            assert_eq!(&vdp.memory().vram()[0x100..0x102], &[0xca, 0xfe]);
            assert!(vdp.fifo().is_empty());
            assert!(!vdp.progressive_skip_allowed());
            assert!(vdp.interval_hint.is_none());
        }
    }
}

#[test]
fn newly_started_read_and_dma_cross_the_latch_and_restore_exactly() {
    for mode in [0, DMA_FILL_MODE, DMA_COPY_MODE] {
        let region = Region::NtscUs;
        let mut vdp = fixture(region, false, 0x81, 20 * 3420 + 3399);
        vdp.memory
            .write_word(
                ControlCommand {
                    address: 0,
                    code: VRAM_WRITE_CODE,
                },
                0xa55a,
            )
            .unwrap();
        let write = clock(&mut vdp, region, 1);
        vdp.advance_to(write.publish_at, region).unwrap();
        if mode == 0 {
            command(&mut vdp, region, 0, VRAM_READ_CODE);
        } else {
            let mut image = vdp.registers().registers().map(Option::unwrap);
            image[19] = 2;
            image[20] = 0;
            image[21] = 0;
            image[22] = 0;
            image[23] = mode;
            vdp.state.registers = VdpRegisterPipeline::clock_fixture(tick(0), vdp.now(), image);
            if mode == DMA_FILL_MODE {
                command(&mut vdp, region, 0x100, VRAM_WRITE_CODE | COMMAND_CD5);
                vdp.write_data(0xbeef, region).unwrap();
            } else {
                command(
                    &mut vdp,
                    region,
                    0x100,
                    VRAM_WRITE_CODE | COMMAND_CD4 | COMMAND_CD5,
                );
            }
        }
        let mut copy = restore(&vdp, region);
        let mut whole = vdp.clone();
        for at in [21 * 3420 - 1, 21 * 3420, 21 * 3420 + 16, 21 * 3420 + 512] {
            vdp.advance_to(tick(at), region).unwrap();
            copy.advance_to(tick(at), region).unwrap();
            assert_eq!(copy, vdp);
        }
        whole.advance_to(vdp.now(), region).unwrap();
        assert_eq!(whole, vdp);
        if mode == 0 {
            assert_eq!(vdp.read_data(region).unwrap(), 0xa55a);
        } else {
            assert!(vdp.state.dma.is_none());
            assert!(vdp.fifo().is_empty());
            assert_ne!(&vdp.memory().vram()[0x100..0x104], &[0; 4]);
        }
    }
}

#[test]
fn internal_clock_write_rejects_busy_transfers_and_interlace_atomically() {
    let region = Region::NtscUs;
    for transfer in 0..4 {
        let mut vdp = fixture(region, false, 0, 20 * 3420 + 3001);
        match transfer {
            0 => {
                command(&mut vdp, region, 0x100, VRAM_WRITE_CODE);
                vdp.write_data(0xcafe, region).unwrap();
            }
            1 => command(&mut vdp, region, 0x100, VRAM_READ_CODE),
            2 => vdp.state.dma = Some(dma::Phase::FillWaiting),
            _ => {
                vdp.state.interrupts = {
                    let mut bytes = StateWriter::new();
                    vdp.state.interrupts.write_state(&mut bytes);
                    let mut raw = bytes.into_bytes();
                    raw[32] = 1;
                    VdpInterrupts::read_state(&mut StateReader::new(&raw), region).unwrap()
                }
            }
        }
        let before = vdp.clone();
        assert!(
            vdp.write_control(0x8c01, region).is_err(),
            "transfer={transfer}"
        );
        assert_eq!(vdp, before);
    }
    let mut vdp = fixture(region, false, 0, 1001);
    for value in [0x80, 3, 7] {
        let before = vdp.clone();
        assert!(vdp.write_control(0x8c00 | value, region).is_err());
        assert_eq!(vdp, before);
    }
}

#[test]
fn external_dma_release_tracks_final_fetch_before_fifo_retirement() {
    let rom = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0];
    let ram_data = rom.map(|byte| byte ^ 0xff);
    let mut ram = vec![0; 65536];
    ram[0xdddc..0xdde4].copy_from_slice(&ram_data);
    for address in [0, 0xcddddc] {
        let source = super::super::super::VdpDmaSource::new(&rom, &ram);
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for display in [false, true] {
                for stable in [false, true] {
                    let mut vdp = fixture(region, display, 0x81, 20 * 3420 + 3399);
                    let write = clock(&mut vdp, region, 1);
                    vdp.advance_to(write.publish_at, region).unwrap();
                    if stable {
                        vdp.advance_to(tick(21 * 3420 + 101), region).unwrap();
                    }
                    let mut image = vdp.registers().registers().map(Option::unwrap);
                    image[19] = 4;
                    let word = address >> 1;
                    image[21] = word as u8;
                    image[22] = (word >> 8) as u8;
                    image[23] = (word >> 16) as u8;
                    vdp.state.registers =
                        VdpRegisterPipeline::clock_fixture(tick(0), vdp.now(), image);
                    command(&mut vdp, region, 0x100, VRAM_WRITE_CODE | COMMAND_CD5);
                    let release = vdp.external_dma_release(region, source).unwrap().unwrap();
                    let mut copy = restore(&vdp, region);
                    let mut whole = vdp.clone();
                    vdp.advance_with_source(tick(release.get() - 1), region, source)
                        .unwrap();
                    assert_eq!(vdp.state.dma, Some(dma::Phase::External));
                    vdp.advance_with_source(release, region, source).unwrap();
                    copy.advance_with_source(release, region, source).unwrap();
                    assert_eq!(vdp, copy);
                    assert!(vdp.state.dma.is_none());
                    assert!(!vdp.fifo().is_empty());
                    assert_eq!(vdp.external_dma_release(region, source).unwrap(), None);
                    let end = tick(release.get() + 3 * 3360);
                    vdp.advance_with_source(end, region, source).unwrap();
                    whole.advance_with_source(end, region, source).unwrap();
                    assert_eq!(vdp, whole);
                    assert_eq!(
                        &vdp.memory().vram()[0x100..0x108],
                        if address == 0 { &rom } else { &ram_data }
                    );
                    assert!(vdp.fifo().is_empty());
                }
            }
        }
    }
}
