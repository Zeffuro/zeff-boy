use super::*;

const SIZES: [u8; 16] = [
    0, 1, 2, 3, 0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23, 0x30, 0x31, 0x32, 0x33,
];

fn scalar_range(
    memory: &VdpMemory,
    registers: &VdpRegisterPipeline,
    line: &VdpSpriteLine,
    geometry: RasterGeometry,
    start: usize,
    output: &mut [u16],
) {
    for (offset, color) in output.iter_mut().enumerate() {
        *color = memory
            .raster_sprite_line_color_with_geometry(
                registers,
                line,
                (start + offset) as u16,
                geometry,
            )
            .unwrap();
    }
}

#[test]
fn every_layout_span_matches_scalar_with_fine_scroll_fields_and_partial_ranges() {
    let mut memory = fixture();
    for display in [0, H40_CLOCK_MASK] {
        for size in SIZES {
            for geometry in [
                RasterGeometry::progressive(0),
                RasterGeometry::progressive(255),
                RasterGeometry::new(0, true, false),
                RasterGeometry::new(0, true, true),
                RasterGeometry::new(255, true, false),
                RasterGeometry::new(255, true, true),
            ] {
                for scroll in [0, 2, 3, 4, 6, 7] {
                    for horizontal in (0..16).chain([1023, 1024]) {
                        let mut raw = values(display | if geometry.interlaced { 6 } else { 0 });
                        raw[usize::from(PLANE_SIZE_REGISTER)] = Some(size);
                        raw[usize::from(SCROLL_MODE_REGISTER)] = Some(scroll);
                        raw[usize::from(WINDOW_X_REGISTER)] = Some([0, 8, 0x88][horizontal % 3]);
                        let registers = bank(raw);
                        let line = empty_sprites(&memory, &registers);
                        let physical = usize::from(geometry.physical_y);
                        let row = match scroll & 3 {
                            0 => 0,
                            2 => physical / 8 * 8,
                            _ => physical,
                        };
                        let address = 0xfc00 + row * 4;
                        word(
                            &mut memory,
                            VRAM_WRITE_CODE,
                            address as u32,
                            horizontal as u16,
                        );
                        word(
                            &mut memory,
                            VRAM_WRITE_CODE,
                            address as u32 + 2,
                            (1024 - horizontal) as u16,
                        );
                        for pair in 0..20 {
                            for plane in 0..2 {
                                word(
                                    &mut memory,
                                    VSRAM_WRITE_CODE,
                                    (pair * 4 + plane * 2) as u32,
                                    [
                                        7, 8, 15, 16, 255, 256, 511, 512, 767, 768, 1023, 1024,
                                        1535, 1536, 2047,
                                    ][(pair + plane) % 15],
                                );
                            }
                        }
                        let width = line.pixels().len();
                        let mut actual = vec![0; width];
                        let mut expected = vec![0; width];
                        scalar_range(&memory, &registers, &line, geometry, 0, &mut expected);
                        memory
                            .render_raster_sprite_line_with_geometry(
                                &registers,
                                &line,
                                &mut actual,
                                geometry,
                            )
                            .unwrap();
                        assert_eq!(
                            actual, expected,
                            "size {size:02x}, scroll {scroll}, horizontal {horizontal}, geometry {geometry:?}"
                        );
                        let context = RasterContext::prepare_with_geometry(
                            &memory, &registers, &line, &actual, geometry,
                        )
                        .unwrap();
                        for (start, end) in [
                            (1, 7),
                            (7, 17),
                            (8, 16),
                            (15, 129),
                            (16, 128),
                            (127, 129),
                            (128, width),
                            (width - 1, width),
                        ] {
                            let mut partial = vec![0xdead; end - start];
                            context
                                .render_spans::<true>(
                                    &memory,
                                    &registers,
                                    &line,
                                    &mut partial,
                                    start,
                                )
                                .unwrap();
                            assert_eq!(partial, expected[start..end]);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn pending_r16_publication_rederives_layout_between_partial_invocations() {
    let memory = fixture();
    for display in [0, H40_CLOCK_MASK] {
        for next in SIZES {
            let mut raw = values(display);
            raw[usize::from(PLANE_SIZE_REGISTER)] = Some(0x30);
            raw[usize::from(WINDOW_X_REGISTER)] = Some(0);
            let mut registers = bank(raw);
            let line = empty_sprites(&memory, &registers);
            registers.advance_to(MasterTicks::new(1)).unwrap();
            let pending = registers.queue_register(PLANE_SIZE_REGISTER, next).unwrap();
            for at in [
                pending.publish_at.get() - 1,
                pending.publish_at.get(),
                pending.publish_at.get() + 1,
            ] {
                registers.advance_to(MasterTicks::new(at)).unwrap();
                assert_eq!(
                    registers.registers()[usize::from(PLANE_SIZE_REGISTER)],
                    Some(if at < pending.publish_at.get() {
                        0x30
                    } else {
                        next
                    })
                );
                for split in [1, 7, 8, 15, 16, 127, 128, line.pixels().len() - 1] {
                    let geometry = RasterGeometry::progressive(223);
                    let mut expected = vec![0; line.pixels().len()];
                    let mut actual = expected.clone();
                    scalar_range(&memory, &registers, &line, geometry, 0, &mut expected);
                    let context = RasterContext::prepare_with_geometry(
                        &memory, &registers, &line, &actual, geometry,
                    )
                    .unwrap();
                    context
                        .render_spans::<true>(&memory, &registers, &line, &mut actual[..split], 0)
                        .unwrap();
                    context
                        .render_spans::<true>(
                            &memory,
                            &registers,
                            &line,
                            &mut actual[split..],
                            split,
                        )
                        .unwrap();
                    assert_eq!(actual, expected);
                }
            }
            for split in [7, 16, 127, 128] {
                let geometry = RasterGeometry::progressive(223);
                let mut before = values(display);
                before[usize::from(PLANE_SIZE_REGISTER)] = Some(0x30);
                before[usize::from(WINDOW_X_REGISTER)] = Some(0);
                let before = bank(before);
                let mut expected = vec![0; line.pixels().len()];
                let mut actual = expected.clone();
                scalar_range(&memory, &before, &line, geometry, 0, &mut expected[..split]);
                let context = RasterContext::prepare_with_geometry(
                    &memory, &before, &line, &actual, geometry,
                )
                .unwrap();
                context
                    .render_spans::<true>(&memory, &before, &line, &mut actual[..split], 0)
                    .unwrap();
                scalar_range(
                    &memory,
                    &registers,
                    &line,
                    geometry,
                    split,
                    &mut expected[split..],
                );
                let context = RasterContext::prepare_with_geometry(
                    &memory, &registers, &line, &actual, geometry,
                )
                .unwrap();
                context
                    .render_spans::<true>(&memory, &registers, &line, &mut actual[split..], split)
                    .unwrap();
                assert_eq!(actual, expected);
            }
        }
    }
}
