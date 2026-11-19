use super::*;
use crate::ControlCommand;

use crate::constants::VDP_LINE_TICKS as LINE_TICKS;
mod cram_provenance;
mod raster_failures;

fn advance(machine: &mut Emulator, ticks: u64) {
    if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn partial_visible_frame_with_half_fifo_word_restores_and_continues() {
    cram_provenance::cold_fill_presentations_restore();
    for region in [Region::NtscUs, Region::Pal] {
        assert_scroll_fetch_boundaries(region);
    }
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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn partial_visible_frame_with_prefetched_dma_restores_and_continues() {
    raster_failures::availability_after_render_failure();
    for region in [Region::NtscUs, Region::Pal] {
        assert_unqualified_scroll_cache(region);
        assert_scroll_publication_and_preparation(region);
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

fn assert_unqualified_scroll_cache(region: Region) {
    for (width, phase) in [(0, 3190), (0x81, 3197)] {
        for interlace in [2, 6] {
            for clear_display in [false, true] {
                let mut source = scroll_fixture(region, width | interlace, 3);
                let field = source.vdp().field_position(region).unwrap();
                assert!(field.active_mode.interlaced());
                source.bus.vdp.write_control(0x8b03, region).unwrap();
                let fetch = field.start.get() + 9 * field.line_ticks + phase;
                let remaining = fetch - source.bus.now.get();
                advance(&mut source, remaining);
                assert_eq!(
                    source.bus.video.horizontal_scroll(field.id, 10),
                    Some([8, 0])
                );
                if clear_display {
                    source
                        .bus
                        .vdp
                        .write_control(0x8c00 | u16::from(width), region)
                        .unwrap();
                    advance(&mut source, 20);
                    assert_eq!(source.vdp().registers().registers()[12], Some(width));
                    assert!(
                        source
                            .vdp()
                            .field_position(region)
                            .unwrap()
                            .active_mode
                            .interlaced()
                    );
                }
                source.bus.vdp.write_control(0x8b01, region).unwrap();
                advance(&mut source, 20);
                assert_eq!(source.video_error(), None);
                let saved = source.encode_state().unwrap();
                let front = source.encode_frontend_state().unwrap();
                let mut raw = source.clone();
                raw.decode_state(&saved).unwrap();
                let mut restored = source.clone();
                restored.decode_frontend_state(&front).unwrap();
                let mut raw_reference = source.clone();
                raw_reference.set_sample_rate(raw_reference.sample_rate());
                for machine in [&mut source, &mut raw, &mut restored, &mut raw_reference] {
                    let finish = field.start.get() + 11 * field.line_ticks;
                    let remaining = finish - machine.bus.now.get();
                    advance(machine, remaining);
                    assert_eq!(
                        machine.video_error(),
                        Some(crate::VdpLineError::Render(
                            crate::VdpRenderError::UnsupportedScrollMode(1)
                        ))
                    );
                }
                assert!(
                    raw_reference == raw,
                    "raw {region:?} width={width} interlace={interlace} clear={clear_display}"
                );
                assert!(
                    source == restored,
                    "frontend {region:?} width={width} interlace={interlace} clear={clear_display}"
                );
            }
        }
    }
}

fn scroll_fixture(region: Region, width: u8, scroll: u8) -> Emulator {
    let mut bytes = vec![0; 0x200];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    bytes[0x100..0x104].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    let mut source = Emulator::new(
        bytes,
        LoadConfig {
            region,
            concurrent_sound: false,
        },
    )
    .unwrap();
    for (index, value) in [
        (1, 4),
        (12, width),
        (2, 0x30),
        (4, 7),
        (5, 0x7c),
        (15, 2),
        (13, 0x3f),
    ] {
        source
            .bus
            .vdp
            .write_control(0x8000 | (index << 8) | u16::from(value), region)
            .unwrap();
        advance(&mut source, 20);
    }
    for (address, values, code) in [
        (0, vec![0, 0x000e, 0x00e0], 3),
        (32, vec![0x1111; 16], 1),
        (64, vec![0x2222; 16], 1),
        (0xc040, (0..32).map(|column| 1 + column % 2).collect(), 1),
        (
            if scroll == 1 { 0xfc08 } else { 0xfc28 },
            vec![8, 0, 8, 0],
            1,
        ),
    ] {
        command(&mut source, address, code);
        for value in values {
            source.bus.vdp.write_data(value, region).unwrap();
            advance(&mut source, 80);
        }
    }
    source.bus.vdp.write_control(0x8144, region).unwrap();
    advance(&mut source, 20);
    let field = source.bus.vdp.field_position(region).unwrap();
    let at = field.end().unwrap().get() + 9 * field.line_ticks;
    let remaining = at - source.bus.now.get();
    advance(&mut source, remaining);
    source
}

fn assert_scroll_fetch_boundaries(region: Region) {
    for scroll in [1, 3] {
        for (width, phase) in [(0, 3190), (0x81, 3197), (1, 3152)] {
            let base = scroll_fixture(region, width, scroll);
            let field = base.bus.vdp.field_position(region).unwrap();
            let fetch = base.bus.now.get() + phase;
            let finish = field.line_start.get() + 2 * field.line_ticks;
            for (offset, pair, color) in [
                (-21, [8, 0], [0, 255, 0, 255]),
                (-1, [0, 0], [255, 0, 0, 255]),
                (0, [0, 0], [255, 0, 0, 255]),
                (1, [0, 0], [255, 0, 0, 255]),
            ] {
                let mut source = base.clone();
                let at = fetch.checked_add_signed(offset).unwrap();
                let remaining = at - source.bus.now.get();
                advance(&mut source, remaining);
                source
                    .bus
                    .vdp
                    .write_control(0x8b00 | u16::from(scroll), region)
                    .unwrap();
                let saved = source.encode_state().unwrap();
                let front = source.encode_frontend_state().unwrap();
                let mut raw = base.clone();
                raw.decode_state(&saved).unwrap();
                let mut restored = base.clone();
                restored.decode_frontend_state(&front).unwrap();
                for machine in [&mut source, &mut raw, &mut restored] {
                    let remaining = finish - machine.bus.now.get();
                    advance(machine, remaining);
                    assert_eq!(
                        machine.bus.video.horizontal_scroll(field.id, 10),
                        Some(pair)
                    );
                    let row = 10 * machine.framebuffer_dimensions().0 * 4;
                    assert_eq!(
                        &machine.framebuffer()[row..row + 4],
                        &color,
                        "{region:?} width={width} offset={offset}"
                    );
                    advance(machine, field.line_ticks);
                    let row = 11 * machine.framebuffer_dimensions().0 * 4;
                    assert_eq!(&machine.framebuffer()[row..row + 4], &[0, 255, 0, 255]);
                }
                assert_eq!(source.encode_state().unwrap(), raw.encode_state().unwrap());
                assert_eq!(
                    source.encode_frontend_state().unwrap(),
                    restored.encode_frontend_state().unwrap()
                );
            }

            let mut source = base.clone();
            source
                .bus
                .vdp
                .write_control(0x8b00 | u16::from(scroll), region)
                .unwrap();
            let remaining = fetch - source.bus.now.get();
            advance(&mut source, remaining);
            assert_eq!(
                source.bus.video.horizontal_scroll(field.id, 10),
                Some([8, 0])
            );
            source.bus.vdp.write_control(0x8d3e, region).unwrap();
            let remaining = finish - source.bus.now.get();
            advance(&mut source, remaining);
            assert_eq!(
                source.bus.video.horizontal_scroll(field.id, 10),
                Some([8, 0])
            );
            let row = 10 * source.framebuffer_dimensions().0 * 4;
            assert_eq!(&source.framebuffer()[row..row + 4], &[0, 255, 0, 255]);
            advance(&mut source, field.line_ticks);
            let row = 11 * source.framebuffer_dimensions().0 * 4;
            assert_eq!(&source.framebuffer()[row..row + 4], &[255, 0, 0, 255]);

            let mut source = base;
            source
                .bus
                .vdp
                .write_control(0x8b00 | u16::from(scroll), region)
                .unwrap();
            let remaining = fetch - source.bus.now.get();
            advance(&mut source, remaining);
            source.bus.vdp.write_control(0x8104, region).unwrap();
            advance(&mut source, 20);
            source.bus.vdp.write_control(0x8d3e, region).unwrap();
            let missed = fetch + field.line_ticks + 1;
            let remaining = missed - source.bus.now.get();
            advance(&mut source, remaining);
            assert_eq!(
                source.bus.video.horizontal_scroll(field.id, 11),
                Some([8, 0])
            );
            source.bus.vdp.write_control(0x8144, region).unwrap();
            let end = field.line_start.get() + 3 * field.line_ticks;
            let remaining = end - source.bus.now.get();
            advance(&mut source, remaining);
            let row = 11 * source.framebuffer_dimensions().0 * 4;
            assert_eq!(&source.framebuffer()[row..row + 4], &[0, 255, 0, 255]);
        }
    }
}

fn assert_scroll_publication_and_preparation(region: Region) {
    for scroll in [1, 3] {
        for (width, phase) in [(0, 3190), (0x81, 3197), (1, 3152)] {
            let mut source = scroll_fixture(region, width, scroll);
            let position = source.bus.vdp.field_position(region).unwrap();
            command(&mut source, if scroll == 1 { 0xfc1c } else { 0xfffc }, 1);
            source.bus.vdp.write_data(0x0123, region).unwrap();
            advance(&mut source, 200);
            source
                .bus
                .vdp
                .write_control(0x8b00 | u16::from(scroll), region)
                .unwrap();
            let prep = position.end().unwrap().get() - 2 * position.line_ticks + phase;
            let remaining = prep - source.bus.now.get();
            advance(&mut source, remaining);
            source.bus.vdp.write_control(0x8104, region).unwrap();
            let saved = source.encode_state().unwrap();
            let mut restored = source.clone();
            restored.decode_state(&saved).unwrap();
            let finish = position.end().unwrap().get() + position.line_ticks;
            for machine in [&mut source, &mut restored] {
                let remaining = finish - machine.bus.now.get();
                advance(machine, remaining);
                assert_eq!(
                    machine.bus.video.horizontal_scroll(position.id + 1, 0),
                    Some([0x0123, 0]),
                    "preparation pair survives disabled row0: {region:?} width={width}"
                );
            }
            assert_eq!(
                source.encode_state().unwrap(),
                restored.encode_state().unwrap()
            );
        }
    }
    for (from, to) in [(0, 0x81_u8), (0x81, 0)] {
        let mut source = scroll_fixture(region, from, 3);
        let position = source.bus.vdp.field_position(region).unwrap();
        source.bus.vdp.write_control(0x8b03, region).unwrap();
        advance(&mut source, 3160);
        source
            .bus
            .vdp
            .write_control(0x8c00 | u16::from(to), region)
            .unwrap();
        let mut split = source.clone();
        advance(&mut source, 2 * position.line_ticks);
        for _ in 0..2 * position.line_ticks {
            advance(&mut split, 1);
        }
        assert_eq!(
            source.encode_state().unwrap(),
            split.encode_state().unwrap()
        );
        assert_eq!(
            source.bus.video.horizontal_scroll(position.id, 11),
            Some([8, 0])
        );
    }

    let mut source = scroll_fixture(region, 0, 3);
    let position = source.bus.vdp.field_position(region).unwrap();
    command(&mut source, 0xfc28, 1);
    source.bus.vdp.write_data(0x0108, region).unwrap();
    advance(&mut source, 200);
    source.bus.vdp.write_control(0x8b03, region).unwrap();
    let remaining = position.line_start.get() + 3169 - source.bus.now.get();
    advance(&mut source, remaining);
    command(&mut source, 0xfc28, 1);
    source.bus.vdp.write_data(0x0200, region).unwrap();
    advance(&mut source, 21);
    assert_eq!(&source.vdp().memory().vram()[0xfc28..0xfc2a], &[1, 0]);
    assert_eq!(
        source.bus.video.horizontal_scroll(position.id, 10),
        Some([0x0100, 0])
    );
    let saved = source.encode_state().unwrap();
    let mut restored = source.clone();
    restored.decode_state(&saved).unwrap();
    for machine in [&mut source, &mut restored] {
        let remaining = position.line_start.get() + 2 * position.line_ticks - machine.bus.now.get();
        advance(machine, remaining);
        assert_eq!(&machine.vdp().memory().vram()[0xfc28..0xfc2a], &[2, 0]);
        assert_eq!(
            machine.bus.video.horizontal_scroll(position.id, 10),
            Some([0x0100, 0])
        );
    }
    assert_eq!(
        source.encode_state().unwrap(),
        restored.encode_state().unwrap()
    );

    if region == Region::Pal {
        let mut source = scroll_fixture(region, 0, 3);
        let position = source.bus.vdp.field_position(region).unwrap();
        source.bus.vdp.write_control(0x8b03, region).unwrap();
        command(&mut source, 0xff7c, 1);
        source.bus.vdp.write_data(0x0155, region).unwrap();
        let fetch = position.start.get() + 222 * position.line_ticks + 3190;
        let remaining = fetch - source.bus.now.get();
        advance(&mut source, remaining);
        let remaining = position.line_ticks;
        advance(&mut source, remaining);
        assert_eq!(
            source.bus.video.horizontal_scroll(position.id, 224),
            Some([0x0155, 0])
        );
        source.bus.vdp.write_control(0x814c, region).unwrap();
        advance(&mut source, 20);
        source.bus.vdp.write_control(0x8d3e, region).unwrap();
        advance(&mut source, position.line_ticks);
        assert_eq!(
            source.bus.video.horizontal_scroll(position.id, 224),
            Some([0x0155, 0])
        );
    }
}
