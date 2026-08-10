use zeff_gba_core::emulator::Emulator as GbaEmulator;

pub(super) fn gba_dma_channel_json(
    channel: u32,
    read_io: &impl Fn(u32) -> u16,
    dma: zeff_gba_core::hardware::dma::DmaChannel,
) -> serde_json::Value {
    let base = 0x0400_00B0 + channel * 12;
    let source = u32::from(read_io(base)) | (u32::from(read_io(base + 2)) << 16);
    let destination = u32::from(read_io(base + 4)) | (u32::from(read_io(base + 6)) << 16);
    let count = read_io(base + 8);
    let control = read_io(base + 10);
    serde_json::json!({
        "channel": channel,
        "source": source,
        "source_hex": format!("{source:08X}"),
        "destination": destination,
        "destination_hex": format!("{destination:08X}"),
        "count": count,
        "count_hex": format!("{count:04X}"),
        "control": control,
        "control_hex": format!("{control:04X}"),
        "active_source": dma.active_source,
        "active_source_hex": format!("{:08X}", dma.active_source),
        "active_destination": dma.active_destination,
        "active_destination_hex": format!("{:08X}", dma.active_destination),
        "active_count": dma.active_count,
        "active_count_hex": format!("{:04X}", dma.active_count),
        "enabled": control & 0x8000 != 0,
        "irq": control & (1 << 14) != 0,
        "start_timing": (control >> 12) & 0x3,
        "repeat": control & (1 << 9) != 0,
        "word": control & (1 << 10) != 0,
        "destination_mode": (control >> 5) & 0x3,
        "source_mode": (control >> 7) & 0x3,
    })
}

pub(super) fn gba_bg_layers_json(emulator: &GbaEmulator) -> serde_json::Value {
    serde_json::Value::Array(
        (0..4)
            .map(|bg| {
                let control = emulator.cpu_peek16(0x0400_0008 + bg * 2);
                let size = (control >> 14) & 0x3;
                let (width, height) = match size {
                    0 => (256, 256),
                    1 => (512, 256),
                    2 => (256, 512),
                    _ => (512, 512),
                };
                serde_json::json!({
                    "index": bg,
                    "enabled": emulator.cpu_peek16(0x0400_0000) & (1 << (8 + bg)) != 0,
                    "control": control,
                    "control_hex": format!("{control:04X}"),
                    "priority": control & 0x3,
                    "char_base": ((control >> 2) & 0x3) * 0x4000,
                    "screen_base": ((control >> 8) & 0x1F) * 0x800,
                    "color_256": control & (1 << 7) != 0,
                    "size": size,
                    "width": width,
                    "height": height,
                    "hofs": emulator.cpu_peek16(0x0400_0010 + bg * 4) & 0x01FF,
                    "vofs": emulator.cpu_peek16(0x0400_0012 + bg * 4) & 0x01FF,
                })
            })
            .collect(),
    )
}

pub(super) fn gba_oam_json(emulator: &GbaEmulator) -> serde_json::Value {
    let oam = emulator.oam_snapshot();
    let mut active_count = 0usize;
    let mut visible_count = 0usize;
    let mut visible_objects = Vec::new();

    for obj in 0..128usize {
        let base = obj * 8;
        let attr0 = read_le16_slice(oam, base);
        let attr1 = read_le16_slice(oam, base + 2);
        let attr2 = read_le16_slice(oam, base + 4);
        let affine = attr0 & (1 << 8) != 0;
        let disabled = !affine && attr0 & (1 << 9) != 0;
        if disabled {
            continue;
        }
        let mode = (attr0 >> 10) & 0x3;
        let shape = (attr0 >> 14) & 0x3;
        let size = (attr1 >> 14) & 0x3;
        let Some((width, height)) = gba_obj_dimensions(shape, size) else {
            continue;
        };
        active_count += 1;
        let double_size = affine && attr0 & (1 << 9) != 0;
        let draw_width = if double_size { width * 2 } else { width };
        let draw_height = if double_size { height * 2 } else { height };
        let raw_y = attr0 & 0x00FF;
        let raw_x = attr1 & 0x01FF;
        let y = gba_obj_y_coord(attr0 & 0x00FF);
        let x = gba_sign_obj_coord(attr1 & 0x01FF, 512);
        let visible =
            x < 240 && x + i32::from(draw_width) > 0 && y < 160 && y + i32::from(draw_height) > 0;
        if visible {
            visible_count += 1;
        }
        if !visible {
            continue;
        }
        let affine_index = affine.then_some((attr1 >> 9) & 0x1F);
        let affine_params = affine_index.map(|index| gba_obj_affine_params(oam, index));
        visible_objects.push(serde_json::json!({
            "index": obj,
            "attr0_hex": format!("{attr0:04X}"),
            "attr1_hex": format!("{attr1:04X}"),
            "attr2_hex": format!("{attr2:04X}"),
            "raw_x": raw_x,
            "raw_y": raw_y,
            "x": x,
            "y": y,
            "width": width,
            "height": height,
            "draw_width": draw_width,
            "draw_height": draw_height,
            "affine": affine,
            "affine_index": affine_index,
            "affine_params": affine_params.map(|(pa, pb, pc, pd)| {
                serde_json::json!({
                    "pa": pa,
                    "pb": pb,
                    "pc": pc,
                    "pd": pd,
                })
            }),
            "double_size": double_size,
            "mode": mode,
            "color_256": attr0 & (1 << 13) != 0,
            "shape": shape,
            "size": size,
            "hflip": !affine && attr1 & (1 << 12) != 0,
            "vflip": !affine && attr1 & (1 << 13) != 0,
            "tile": attr2 & 0x03FF,
            "priority": (attr2 >> 10) & 0x3,
            "palette": (attr2 >> 12) & 0xF,
        }));
    }

    serde_json::json!({
        "active_count": active_count,
        "visible_count": visible_count,
        "visible_sample": visible_objects.iter().take(24).cloned().collect::<Vec<_>>(),
        "visible_objects": visible_objects,
    })
}

fn gba_obj_dimensions(shape: u16, size: u16) -> Option<(u16, u16)> {
    match (shape, size) {
        (0, 0) => Some((8, 8)),
        (0, 1) => Some((16, 16)),
        (0, 2) => Some((32, 32)),
        (0, 3) => Some((64, 64)),
        (1, 0) => Some((16, 8)),
        (1, 1) => Some((32, 8)),
        (1, 2) => Some((32, 16)),
        (1, 3) => Some((64, 32)),
        (2, 0) => Some((8, 16)),
        (2, 1) => Some((8, 32)),
        (2, 2) => Some((16, 32)),
        (2, 3) => Some((32, 64)),
        _ => None,
    }
}

fn gba_sign_obj_coord(value: u16, range: i32) -> i32 {
    let value = i32::from(value);
    if value >= range / 2 {
        value - range
    } else {
        value
    }
}

fn gba_obj_y_coord(value: u16) -> i32 {
    let value = i32::from(value & 0x00FF);
    if value >= 160 { value - 256 } else { value }
}

fn gba_obj_affine_params(oam: &[u8], index: u16) -> (i16, i16, i16, i16) {
    let base = usize::from(index) * 0x20;
    (
        read_i16_slice(oam, base + 0x06),
        read_i16_slice(oam, base + 0x0E),
        read_i16_slice(oam, base + 0x16),
        read_i16_slice(oam, base + 0x1E),
    )
}

fn read_le16_slice(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}

fn read_i16_slice(data: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}
