use super::{
    CompositedPixel, DisplayCompositionError, DisplayLayerLine, HuC6260, SpriteBackgroundPriority,
    SpritePixel, VcePort, VpcPixelSource, VpcVdcPixel, compose_vdc_output_scanline,
};

fn port(offset: u8) -> VcePort {
    VcePort::from_offset(offset)
}

fn write_color(vce: &mut HuC6260, index: u16, raw: u16) {
    vce.write_port(port(2), index as u8);
    vce.write_port(port(3), (index >> 8) as u8);
    vce.write_port(port(4), raw as u8);
    vce.write_port(port(5), (raw >> 8) as u8);
}

fn sprite(index: u16, priority: SpriteBackgroundPriority) -> Option<SpritePixel> {
    Some(SpritePixel::new(index, priority, 0))
}

#[test]
fn active_pixels_follow_the_complete_priority_chain() {
    let vce = HuC6260::new();
    let background = [5, 5, 6, 0, 0];
    let sprites = [
        sprite(0x111, SpriteBackgroundPriority::Sprite),
        None,
        sprite(0x113, SpriteBackgroundPriority::Background),
        sprite(0x114, SpriteBackgroundPriority::Background),
        None,
    ];
    let mut output = [CompositedPixel::default(); 5];

    vce.compose_scanline(
        DisplayLayerLine::Rendered(&background),
        DisplayLayerLine::Rendered(&sprites),
        &mut output,
    )
    .unwrap();

    assert_eq!(
        output.map(CompositedPixel::palette_index),
        [0x111, 5, 6, 0x114, 0]
    );
}

#[test]
fn raw_vdc_output_preserves_background_and_sprite_bus_sources() {
    let background = [5, 6, 0];
    let sprites = [
        sprite(0x111, SpriteBackgroundPriority::Sprite),
        sprite(0x112, SpriteBackgroundPriority::Background),
        sprite(0x113, SpriteBackgroundPriority::Background),
    ];
    let mut output = [VpcVdcPixel::new(0); 3];

    compose_vdc_output_scanline(
        DisplayLayerLine::Rendered(&background),
        DisplayLayerLine::Rendered(&sprites),
        &mut output,
    )
    .unwrap();

    assert_eq!(output.map(VpcVdcPixel::palette_index), [0x111, 6, 0x113]);
    assert_eq!(output[0].source(), VpcPixelSource::Sprite);
    assert_eq!(output[1].source(), VpcPixelSource::Background);
    assert_eq!(output[2].source(), VpcPixelSource::Sprite);
}

#[test]
fn disabled_layers_never_reuse_rendered_buffers() {
    let vce = HuC6260::new();
    let background = [5];
    let sprites = [sprite(0x111, SpriteBackgroundPriority::Background)];
    let mut output = [CompositedPixel::default()];

    vce.compose_scanline(
        DisplayLayerLine::Rendered(&background),
        DisplayLayerLine::Rendered(&sprites),
        &mut output,
    )
    .unwrap();
    assert_eq!(output[0].palette_index(), 5);

    vce.compose_scanline(
        DisplayLayerLine::Disabled,
        DisplayLayerLine::Disabled,
        &mut output,
    )
    .unwrap();
    assert_eq!(output[0].palette_index(), 0);

    vce.compose_scanline(
        DisplayLayerLine::Disabled,
        DisplayLayerLine::Rendered(&sprites),
        &mut output,
    )
    .unwrap();
    assert_eq!(output[0].palette_index(), 0x111);

    vce.compose_scanline(
        DisplayLayerLine::Rendered(&background),
        DisplayLayerLine::Disabled,
        &mut output,
    )
    .unwrap();
    assert_eq!(output[0].palette_index(), 5);
}

#[test]
fn backdrop_uses_index_zero_and_never_the_blanking_index() {
    let mut vce = HuC6260::new();
    write_color(&mut vce, 0, 0x0038);
    write_color(&mut vce, 0x100, 0x0007);
    let mut output = [CompositedPixel::default()];

    vce.compose_scanline(
        DisplayLayerLine::Disabled,
        DisplayLayerLine::Disabled,
        &mut output,
    )
    .unwrap();

    assert_eq!(output[0].palette_index(), 0);
    assert_eq!(output[0].rgb8(), [255, 0, 0]);
}

#[test]
fn selected_indices_resolve_through_the_current_vce_palette() {
    let mut vce = HuC6260::new();
    write_color(&mut vce, 5, 0x01C0);
    write_color(&mut vce, 0x111, 0x0007);
    let background = [5, 0];
    let sprites = [None, sprite(0x111, SpriteBackgroundPriority::Sprite)];
    let mut output = [CompositedPixel::default(); 2];

    vce.compose_scanline(
        DisplayLayerLine::Rendered(&background),
        DisplayLayerLine::Rendered(&sprites),
        &mut output,
    )
    .unwrap();

    assert_eq!(output[0].rgb8(), [0, 255, 0]);
    assert_eq!(output[1].rgb8(), [0, 0, 255]);
}

#[test]
fn monochrome_control_applies_during_base_composition() {
    let mut vce = HuC6260::new();
    write_color(&mut vce, 5, 0x00D1);
    vce.write_port(port(0), 0x80);
    let background = [5];
    let mut output = [CompositedPixel::default()];

    vce.compose_scanline(
        DisplayLayerLine::Rendered(&background),
        DisplayLayerLine::Disabled,
        &mut output,
    )
    .unwrap();

    assert_eq!(output[0].palette_index(), 5);
    assert_eq!(output[0].rgb8(), [89; 3]);
}

#[test]
fn layer_length_errors_are_transactional() {
    let vce = HuC6260::new();
    let sentinel = CompositedPixel::new(0x1FF, [1, 2, 3]);
    let mut output = [sentinel; 2];
    let background = [1];

    assert_eq!(
        vce.compose_scanline(
            DisplayLayerLine::Rendered(&background),
            DisplayLayerLine::Disabled,
            &mut output,
        ),
        Err(DisplayCompositionError::BackgroundLengthMismatch {
            expected: 2,
            actual: 1,
        })
    );
    assert_eq!(output, [sentinel; 2]);

    let sprites = [None];
    assert_eq!(
        vce.compose_scanline(
            DisplayLayerLine::Disabled,
            DisplayLayerLine::Rendered(&sprites),
            &mut output,
        ),
        Err(DisplayCompositionError::SpriteLengthMismatch {
            expected: 2,
            actual: 1,
        })
    );
    assert_eq!(output, [sentinel; 2]);

    assert_eq!(
        vce.compose_scanline(
            DisplayLayerLine::Rendered(&background),
            DisplayLayerLine::Rendered(&sprites),
            &mut output,
        ),
        Err(DisplayCompositionError::BackgroundLengthMismatch {
            expected: 2,
            actual: 1,
        })
    );
    assert_eq!(output, [sentinel; 2]);
}

#[test]
fn vce_control_bits_do_not_change_digital_palette_composition() {
    let mut vce = HuC6260::new();
    write_color(&mut vce, 5, 0x01FF);
    let background = [5];
    let mut before = [CompositedPixel::default()];
    let mut after = [CompositedPixel::default()];

    vce.compose_scanline(
        DisplayLayerLine::Rendered(&background),
        DisplayLayerLine::Disabled,
        &mut before,
    )
    .unwrap();
    vce.write_port(port(0), 0xFF);
    vce.compose_scanline(
        DisplayLayerLine::Rendered(&background),
        DisplayLayerLine::Disabled,
        &mut after,
    )
    .unwrap();

    assert_eq!(before, after);
    assert_eq!(vce.control(), 0x87);
}
