use super::*;
use crate::ControlCommand;

use crate::constants::VDP_LINE_TICKS as LINE_TICKS;

fn advance(machine: &mut Emulator, ticks: u64) {
    if machine.concurrent_sound {
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .advance_machine(ticks)
        .unwrap();
    } else {
        machine.bus.advance_machine(ticks).unwrap();
    }
}

fn command(machine: &mut Emulator, address: u32, code: u8) {
    let region = machine.region();
    machine
        .bus
        .vdp
        .write_control(
            (address as u16 & 0x3fff) | (u16::from(code & 3) << 14),
            region,
        )
        .unwrap();
    machine
        .bus
        .vdp
        .write_control(
            ((address >> 14) as u16 & 7) | (u16::from(code & 0x3c) << 2),
            region,
        )
        .unwrap();
}

fn machine(region: Region) -> Emulator {
    let mut bytes = vec![0; 0x200];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in [0x7080_u16, 0x7201, 0x60fa, 0x4e71].into_iter().enumerate() {
        bytes[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut machine = Emulator::new(
        bytes,
        LoadConfig {
            region,
            concurrent_sound: region == Region::Pal,
        },
    )
    .unwrap();
    for (index, value) in [
        (0, 0x12),
        (1, if region == Region::Pal { 0x7c } else { 0x74 }),
        (12, if region == Region::Pal { 0x81 } else { 0 }),
        (15, 2),
        (19, 2),
        (22, 0x80),
        (23, 0xc0),
    ] {
        machine
            .bus
            .vdp
            .write_control(0x8000 | (index << 8) | value, region)
            .unwrap();
        advance(&mut machine, 20);
    }
    command(&mut machine, 0, 3);
    machine.bus.vdp.write_data(0x000e, region).unwrap();
    let at = region.frame_ticks() + 20;
    let ticks = at - machine.bus.now.get();
    advance(&mut machine, ticks);
    assert_eq!(machine.frame_count(), 1);
    command(&mut machine, 0, 3);
    machine.bus.vdp.write_data(0x00e0, region).unwrap();
    let at = region.frame_ticks() + 72 * LINE_TICKS + 123;
    let ticks = at - machine.bus.now.get();
    advance(&mut machine, ticks);
    machine.clear_frame_ready();
    let dimensions = if region == Region::Pal {
        (320, 240)
    } else {
        (256, 224)
    };
    assert_eq!(machine.framebuffer_dimensions(), dimensions);
    assert_eq!(&machine.framebuffer()[..4], &[0, 255, 0, 255]);
    assert_eq!(
        &machine.framebuffer()[machine.framebuffer().len() - 4..],
        &[255, 0, 0, 255]
    );
    assert!(machine.vdp().interrupts().horizontal_pending());
    assert!(machine.vdp().interrupts().vertical_pending());
    assert_eq!(machine.video_error(), None);
    machine
}

fn assert_continuation(mut source: Emulator) {
    let saved = source.encode_state().unwrap();
    let frame = source.framebuffer().to_vec();
    let hv = source.vdp().read_hv(source.region()).unwrap();
    let mut target = source.clone();
    advance(&mut target, 2 * LINE_TICKS);
    assert_ne!(target.encode_state().unwrap(), saved);
    target.decode_state(&saved).unwrap();
    assert_eq!(target.encode_state().unwrap(), saved);
    assert_eq!(target.framebuffer(), frame);
    assert_eq!(target.vdp().read_hv(target.region()).unwrap(), hv);
    source.set_sample_rate(source.sample_rate());
    for _ in 0..127 {
        assert_eq!(
            target.step_instruction().unwrap(),
            source.step_instruction().unwrap()
        );
    }
    for ticks in [1, 499, LINE_TICKS + 1, source.region().frame_ticks()] {
        advance(&mut source, ticks);
        advance(&mut target, ticks);
        assert_eq!(
            target.encode_state().unwrap(),
            source.encode_state().unwrap()
        );
        assert_eq!(target.framebuffer(), source.framebuffer());
        assert_eq!(target.video_error(), None);
    }
    assert!(source.frame_count() >= 2 && source.frame_ready());
    assert!(
        source
            .framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 255, 0, 255])
    );
    assert_eq!(target.drain_audio_samples(), source.drain_audio_samples());
}

#[test]
fn partial_visible_frame_with_half_fifo_word_restores_and_continues() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut source = machine(region);
        command(&mut source, 0x8000, 1);
        source.bus.vdp.write_data(0xabcd, region).unwrap();
        for _ in 0..LINE_TICKS {
            if source.vdp().memory().vram()[0x8001] == 0xcd {
                break;
            }
            advance(&mut source, 1);
        }
        assert_eq!(&source.vdp().memory().vram()[0x8000..0x8002], &[0, 0xcd]);
        assert_eq!(source.vdp().fifo().len(), 1);
        assert_continuation(source);
    }
}

#[test]
fn partial_visible_frame_with_prefetched_dma_restores_and_continues() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut source = machine(region);
        command(&mut source, 0x8000, 1);
        source.bus.vdp.write_data(0xabcd, region).unwrap();
        advance(&mut source, 2 * LINE_TICKS);
        source
            .bus
            .vdp
            .resume_dma_from_known_phase(
                ControlCommand {
                    address: 0x9000,
                    code: 0x30,
                },
                None,
                region,
            )
            .unwrap();
        let mut probe = source.clone();
        for _ in 0..2 * LINE_TICKS {
            if probe.vdp().memory().vram()[0x9001] == 0xcd {
                break;
            }
            advance(&mut probe, 1);
        }
        assert_eq!(probe.vdp().memory().vram()[0x9001], 0xcd);
        let ticks = probe.bus.now.get() - source.bus.now.get() - 1;
        advance(&mut source, ticks);
        assert_eq!(source.vdp().memory().vram()[0x9001], 0);
        assert_continuation(source);
    }
}
