use zeff_gb_core::emulator::Emulator as GbEmulator;

#[derive(Debug, Clone)]
pub(super) struct GbMemoryTestStatus {
    pub(super) code: u8,
    pub(super) text: String,
}

pub(super) fn read_gb_memory_test_status(
    emulator: &GbEmulator,
    text_limit: u16,
) -> Option<GbMemoryTestStatus> {
    let signature = [
        emulator.peek_byte_raw(0xA001),
        emulator.peek_byte_raw(0xA002),
        emulator.peek_byte_raw(0xA003),
    ];
    if signature != [0xDE, 0xB0, 0x61] {
        return None;
    }

    let mut text_bytes = Vec::new();
    for offset in 0..text_limit {
        let byte = emulator.peek_byte_raw(0xA004u16.wrapping_add(offset));
        if byte == 0 {
            break;
        }
        text_bytes.push(byte);
    }

    Some(GbMemoryTestStatus {
        code: emulator.peek_byte_raw(0xA000),
        text: String::from_utf8_lossy(&text_bytes).to_string(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TestPassResult {
    Pass,
    Fail,
}

pub(super) fn serial_test_pass_result(serial_text: &str) -> Option<TestPassResult> {
    let lower = serial_text.to_ascii_lowercase();
    if lower.contains("failed") || lower.contains("error") {
        Some(TestPassResult::Fail)
    } else if lower.contains("passed") {
        Some(TestPassResult::Pass)
    } else {
        None
    }
}

pub(super) fn gb_screen_test_pass_result(
    emulator: &GbEmulator,
) -> Option<(TestPassResult, String)> {
    for tilemap_base in [0x9800u16, 0x9C00] {
        let text = read_gb_tilemap_ascii(emulator, tilemap_base);
        if let Some(result) = serial_test_pass_result(&text) {
            return Some((result, text));
        }
    }
    None
}

fn read_gb_tilemap_ascii(emulator: &GbEmulator, tilemap_base: u16) -> String {
    let mut text = String::with_capacity(32 * 32 + 31);
    for row in 0..32u16 {
        if row != 0 {
            text.push('\n');
        }
        for col in 0..32u16 {
            let tile = emulator.peek_byte_raw(tilemap_base + row * 32 + col);
            let ch = if tile.is_ascii_graphic() || tile == b' ' {
                tile as char
            } else {
                ' '
            };
            text.push(ch);
        }
    }
    text
}

pub(super) fn test_pass_breakpoint_result(
    emulator: &GbEmulator,
    pc: u16,
    op: u8,
) -> Option<TestPassResult> {
    if !matches!(op, 0x40 | 0xED) || pc < 0x0100 {
        return None;
    }

    let regs = [
        emulator.cpu_b(),
        emulator.cpu_c(),
        emulator.cpu_d(),
        emulator.cpu_e(),
        emulator.cpu_h(),
        emulator.cpu_l(),
    ];

    if regs == [3, 5, 8, 13, 21, 34] {
        Some(TestPassResult::Pass)
    } else if regs == [0x42; 6] {
        Some(TestPassResult::Fail)
    } else {
        None
    }
}
