use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use crate::hardware::vdp::{ControlCommand, VdpSprite, VdpWriteFifo};
use zeff_emu_common::time::MasterTicks;

fn bank(values: [Option<u8>; REGISTER_COUNT as usize]) -> VdpRegisterPipeline {
    let mut writer = StateWriter::new();
    writer.write_u64(0);
    writer.write_u64(0);
    writer.write_bool(false);
    for value in values {
        writer.write_bool(value.is_some());
        if let Some(value) = value {
            writer.write_u8(value);
        }
    }
    let bytes = writer.into_bytes();
    VdpRegisterPipeline::read_state(&mut StateReader::new(&bytes)).unwrap()
}

fn values(display: u8) -> [Option<u8>; REGISTER_COUNT as usize] {
    let mut values = [Some(0); REGISTER_COUNT as usize];
    for (index, value) in [
        (MODE_REGISTER, MODE5_ENABLE | DISPLAY_ENABLE),
        (DISPLAY_SIZE_REGISTER, display),
        (PLANE_A_TABLE_REGISTER, 0x30),
        (PLANE_B_TABLE_REGISTER, 7),
        (WINDOW_TABLE_REGISTER, 0x2e),
        (HSCROLL_TABLE_REGISTER, 0x3f),
        (BACKDROP_REGISTER, 23),
        (WINDOW_X_REGISTER, 0x88),
    ] {
        values[usize::from(index)] = Some(value);
    }
    values
}

fn word(memory: &mut VdpMemory, code: u8, address: u32, data: u16) {
    memory
        .write_word(ControlCommand { address, code }, data)
        .unwrap();
}

fn memory() -> VdpMemory {
    let mut memory = VdpMemory::default();
    let mut noise = 0x1234_5678u32;
    for address in 0..VRAM_SIZE {
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        memory.write_dma_byte(address as u16, noise as u8);
    }
    for index in 0..CRAM_WORDS {
        word(
            &mut memory,
            CRAM_WRITE_CODE,
            index as u32 * 2,
            index as u16 * 54,
        );
    }
    for index in 0..VSRAM_WORDS {
        word(
            &mut memory,
            VSRAM_WRITE_CODE,
            index as u32 * 2,
            index as u16 * 53,
        );
    }
    for row in 0..TILE_HEIGHT {
        for offset in [0, 2] {
            word(
                &mut memory,
                VRAM_WRITE_CODE,
                32 + u32::from(row) * 4 + offset,
                0xeeee,
            );
            word(
                &mut memory,
                VRAM_WRITE_CODE,
                64 + u32::from(row) * 4 + offset,
                0xffff,
            );
        }
    }
    memory
}

fn sprites(memory: &VdpMemory, mode: HorizontalMode, y: u16) -> VdpSpriteLine {
    let patterns: Vec<_> = [
        (0, 0x6001),
        (23, 0xe002),
        (111, 0x2803),
        (248, 0x9004),
        (311, 0x6002),
    ]
    .into_iter()
    .map(|(x, name)| {
        VdpSprite {
            x,
            y: y as i16,
            width: 8,
            height: 8,
            link: 0,
            name,
        }
        .pattern_row(y)
        .unwrap()
    })
    .collect();
    memory.compose_raster_sprite_line(&patterns, mode).unwrap()
}

fn scalar(
    memory: &VdpMemory,
    registers: &VdpRegisterPipeline,
    sprites: &VdpSpriteLine,
    colors: &mut [u16],
    y: u16,
) -> Result<(), VdpRenderError> {
    for (x, color) in colors.iter_mut().enumerate() {
        *color = memory.raster_sprite_line_color(registers, sprites, x as u16, y)?;
    }
    Ok(())
}

fn compare(
    memory: &VdpMemory,
    registers: &VdpRegisterPipeline,
    sprites: &VdpSpriteLine,
    width: usize,
    y: u16,
) -> Result<(), VdpRenderError> {
    let mut expected = vec![0xdead; width];
    let mut actual = expected.clone();
    let reference = scalar(memory, registers, sprites, &mut expected, y);
    let result = memory.render_raster_sprite_line(registers, sprites, &mut actual, y);
    assert_eq!(
        result,
        reference,
        "row {y}, registers {:?}",
        registers.registers()
    );
    assert_eq!(
        actual,
        expected,
        "row {y}, registers {:?}",
        registers.registers()
    );
    result
}

#[test]
fn raster_context_matches_scalar_rows_with_scroll_window_attributes_and_intensity() {
    let memory = memory();
    for display in [
        0,
        SHADOW_HIGHLIGHT_ENABLE,
        H40_CLOCK_MASK,
        H40_CLOCK_MASK | SHADOW_HIGHLIGHT_ENABLE,
    ] {
        let mode = HorizontalMode::from_register(display).unwrap();
        for size in [0, 1, 3, 0x10, 0x11, 0x30] {
            for scroll in [0, 2, 3, 4, 6, 7] {
                for (window_x, window_y) in [(0, 0), (8, 0), (0x88, 0), (8, 0x90), (0x9f, 0x9f)] {
                    let mut values = values(display);
                    values[usize::from(PLANE_SIZE_REGISTER)] = Some(size);
                    values[usize::from(SCROLL_MODE_REGISTER)] = Some(scroll);
                    values[usize::from(WINDOW_X_REGISTER)] = Some(window_x);
                    values[usize::from(WINDOW_Y_REGISTER)] = Some(window_y);
                    let registers = bank(values);
                    for y in [0, 7, 8, 127, 128, 223, 239, 255] {
                        let sprites = sprites(&memory, mode, y);
                        assert!(
                            RasterContext::prepare(
                                &memory,
                                &registers,
                                &sprites,
                                &vec![0; mode.width()],
                                y
                            )
                            .is_some()
                        );
                        compare(&memory, &registers, &sprites, mode.width(), y).unwrap();
                    }
                }
            }
        }
    }
}

#[test]
fn incomplete_and_unsupported_inputs_keep_scalar_error_and_partial_output() {
    let memory = VdpMemory::default();
    let sprites = memory.compose_sprite_line(&[]).unwrap();
    for index in [2, 3, 4, 7, 11, 12, 13, 16, 17, 18] {
        let mut values = values(0);
        values[index] = None;
        let registers = bank(values);
        assert!(
            RasterContext::prepare(&memory, &registers, &sprites, &[0; H32_SCREEN_WIDTH], 0)
                .is_none()
        );
        let result = compare(&memory, &registers, &sprites, H32_SCREEN_WIDTH, 0);
        if index == 3 {
            assert_eq!(
                result,
                Err(VdpRenderError::UnknownRegister(WINDOW_TABLE_REGISTER))
            );
        }
    }
    for index in [
        DISPLAY_SIZE_REGISTER,
        SCROLL_MODE_REGISTER,
        PLANE_SIZE_REGISTER,
    ] {
        for value in 0..=u8::MAX {
            let mut values = values(0);
            values[usize::from(index)] = Some(value);
            let registers = bank(values);
            let _ = compare(&memory, &registers, &sprites, H32_SCREEN_WIDTH, 0);
        }
    }
    let registers = bank(values(0));
    assert_eq!(
        compare(&memory, &registers, &sprites, H32_SCREEN_WIDTH, 256),
        Err(VdpRenderError::CoordinateOutOfBounds { x: 0, y: 256 })
    );
    let _ = compare(&memory, &registers, &sprites, H32_SCREEN_WIDTH + 1, 0);
    compare(&memory, &registers, &sprites, H32_SCREEN_WIDTH - 1, 0).unwrap();
    let h40 = bank(values(H40_CLOCK_MASK));
    assert_eq!(
        compare(&memory, &h40, &sprites, H40_SCREEN_WIDTH, 0),
        Err(VdpRenderError::CoordinateOutOfBounds { x: 256, y: 0 })
    );
}

#[test]
fn raster_accepts_fine_column_scroll_without_changing_debug_guards() {
    let mut memory = memory();
    word(&mut memory, VRAM_WRITE_CODE, 0xfc00, 17);
    word(&mut memory, VRAM_WRITE_CODE, 0xfc02, 1023);
    let mut values = values(H40_CLOCK_MASK | SHADOW_HIGHLIGHT_ENABLE);
    values[usize::from(SCROLL_MODE_REGISTER)] = Some(VSCROLL_COLUMN_MODE);
    values[usize::from(WINDOW_X_REGISTER)] = Some(8);
    let registers = bank(values);
    let sprites = sprites(&memory, HorizontalMode::H40, 0);
    assert!(
        RasterContext::prepare(&memory, &registers, &sprites, &[0; H40_SCREEN_WIDTH], 0).is_some()
    );
    compare(&memory, &registers, &sprites, H40_SCREEN_WIDTH, 0).unwrap();
    assert_eq!(
        memory.sample_scrolled_plane(&registers, VdpPlane::A, 128, 0),
        Err(VdpRenderError::UnsupportedScrollCombination)
    );
    assert_eq!(
        memory.sample_background(&registers, 128, 0),
        Err(VdpRenderError::UnsupportedWindowScroll)
    );
}

#[test]
fn each_raster_call_observes_register_and_completed_fifo_publications() {
    let mut memory = memory();
    let mut registers = bank(values(0));
    let mut fifo = VdpWriteFifo::default();
    for (code, address, data) in [
        (VRAM_WRITE_CODE, 0xfc00, 17),
        (VSRAM_WRITE_CODE, 0, 13),
        (CRAM_WRITE_CODE, 46, 0xeee),
        (VRAM_WRITE_CODE, 0xc000, 0xe001),
    ] {
        let sprites = sprites(&memory, HorizontalMode::H32, 0);
        compare(&memory, &registers, &sprites, H32_SCREEN_WIDTH, 0).unwrap();
        let mut command = ControlCommand { address, code };
        fifo.push_data(&mut command, Some(2), data).unwrap();
        compare(&memory, &registers, &sprites, H32_SCREEN_WIDTH, 0).unwrap();
        fifo.complete_write(&mut memory).unwrap();
        compare(&memory, &registers, &sprites, H32_SCREEN_WIDTH, 0).unwrap();
    }
    registers.advance_to(MasterTicks::new(1)).unwrap();
    let pending = registers.queue_register(WINDOW_X_REGISTER, 0x80).unwrap();
    let sprites = sprites(&memory, HorizontalMode::H32, 0);
    compare(&memory, &registers, &sprites, H32_SCREEN_WIDTH, 0).unwrap();
    registers.advance_to(pending.publish_at).unwrap();
    compare(&memory, &registers, &sprites, H32_SCREEN_WIDTH, 0).unwrap();
}

mod interlace;
