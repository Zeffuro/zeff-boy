use zeff_gba_core::emulator::Emulator as GbaEmulator;

#[derive(Clone, Debug)]
pub(super) struct GbaTestStatus {
    pub(super) protocol: &'static str,
    pub(super) code: u32,
    pub(super) text: String,
    pub(super) result: GbaTestResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GbaTestResult {
    Pass,
    Fail,
    Running,
}

pub(super) fn read_gba_test_status(emulator: &GbaEmulator) -> Option<GbaTestStatus> {
    read_gba_memory_test_status(emulator)
        .or_else(|| read_mgba_suite_sram_status(emulator))
        .or_else(|| read_jsmolka_gba_screen_status(emulator))
}

fn read_gba_memory_test_status(emulator: &GbaEmulator) -> Option<GbaTestStatus> {
    const BASE: u32 = 0x0200_0000;
    const TEXT_LIMIT: u32 = 4096;

    let signature = [
        emulator.cpu_peek8(BASE + 1),
        emulator.cpu_peek8(BASE + 2),
        emulator.cpu_peek8(BASE + 3),
    ];
    if signature != [0xDE, 0xB0, 0x61] {
        return None;
    }

    let code = emulator.cpu_peek8(BASE);
    let mut text_bytes = Vec::new();
    for offset in 0..TEXT_LIMIT {
        let byte = emulator.cpu_peek8(BASE + 4 + offset);
        if byte == 0 {
            break;
        }
        text_bytes.push(byte);
    }
    let text = String::from_utf8_lossy(&text_bytes).to_string();
    let result = match code {
        0x00 => GbaTestResult::Pass,
        0x01..=0x7F => GbaTestResult::Fail,
        _ => GbaTestResult::Running,
    };

    Some(GbaTestStatus {
        protocol: "memory_status_02000000",
        code: u32::from(code),
        text,
        result,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MgbaSuiteActiveInfo {
    subtest_id: u16,
    test_id: u8,
    suite_id: u8,
}

impl MgbaSuiteActiveInfo {
    fn suite_finished(self) -> bool {
        self.suite_id != u8::MAX && self.test_id == u8::MAX
    }
}

fn read_mgba_suite_sram_status(emulator: &GbaEmulator) -> Option<GbaTestStatus> {
    let backup = emulator.dump_battery_sram()?;
    let text = mgba_suite_sram_log_text(&backup)?;
    mgba_suite_status_from_log_and_active_info(&text, read_mgba_suite_active_info(emulator))
}

fn read_mgba_suite_active_info(emulator: &GbaEmulator) -> Option<MgbaSuiteActiveInfo> {
    const ACTIVE_INFO_BASE: u32 = 0x0300_00AC;

    let magic = [
        emulator.cpu_peek8(ACTIVE_INFO_BASE),
        emulator.cpu_peek8(ACTIVE_INFO_BASE + 1),
        emulator.cpu_peek8(ACTIVE_INFO_BASE + 2),
        emulator.cpu_peek8(ACTIVE_INFO_BASE + 3),
    ];
    if magic != *b"Info" {
        return None;
    }

    let subtest_lo = u16::from(emulator.cpu_peek8(ACTIVE_INFO_BASE + 4));
    let subtest_hi = u16::from(emulator.cpu_peek8(ACTIVE_INFO_BASE + 5));
    Some(MgbaSuiteActiveInfo {
        subtest_id: subtest_lo | (subtest_hi << 8),
        test_id: emulator.cpu_peek8(ACTIVE_INFO_BASE + 6),
        suite_id: emulator.cpu_peek8(ACTIVE_INFO_BASE + 7),
    })
}

fn mgba_suite_sram_log_text(backup: &[u8]) -> Option<String> {
    let end = backup
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(backup.len());
    if end == 0 {
        return None;
    }

    Some(String::from_utf8_lossy(&backup[..end]).into_owned())
}

fn mgba_suite_status_from_log_and_active_info(
    text: &str,
    active_info: Option<MgbaSuiteActiveInfo>,
) -> Option<GbaTestStatus> {
    const HEADER: &str = "Game Boy Advance Test Suite\n===";
    if !text.starts_with(HEADER) {
        return None;
    }

    if let Some(line) = text.lines().find(|line| line.contains("FAIL")) {
        return Some(GbaTestStatus {
            protocol: "mgba_suite_sram_log",
            code: 1,
            text: line.to_string(),
            result: GbaTestResult::Fail,
        });
    }

    if active_info.is_some_and(MgbaSuiteActiveInfo::suite_finished) {
        let suite_id = active_info.map_or(u32::from(u8::MAX), |info| u32::from(info.suite_id));
        return Some(GbaTestStatus {
            protocol: "mgba_suite_sram_log",
            code: suite_id,
            text: format!("mGBA suite {suite_id} completed without SRAM failure log"),
            result: GbaTestResult::Pass,
        });
    }

    Some(GbaTestStatus {
        protocol: "mgba_suite_sram_log",
        code: 0x80,
        text: "mGBA suite still running or no suite selected".to_string(),
        result: GbaTestResult::Running,
    })
}

fn read_jsmolka_gba_screen_status(emulator: &GbaEmulator) -> Option<GbaTestStatus> {
    const TEXT_Y: usize = 76;
    const PASS_X: usize = 56;
    const FAIL_X: usize = 60;

    let vram = emulator.vram_snapshot();
    if gba_mode4_vram_matches_text(vram, PASS_X, TEXT_Y, "All tests passed") {
        return Some(GbaTestStatus {
            protocol: "jsmolka_mode4_text",
            code: 0,
            text: "All tests passed".to_string(),
            result: GbaTestResult::Pass,
        });
    }

    if gba_mode4_vram_matches_text(vram, FAIL_X, TEXT_Y, "Failed test ") {
        let digits_x = FAIL_X + "Failed test ".len() * 8;
        let digits = (0..3)
            .map(|digit| gba_mode4_vram_digit_at(vram, digits_x + digit * 8, TEXT_Y))
            .collect::<Option<String>>()
            .unwrap_or_else(|| "???".to_string());
        let code = digits.parse::<u32>().ok().unwrap_or(0x7FFF);
        return Some(GbaTestStatus {
            protocol: "jsmolka_mode4_text",
            code,
            text: format!("Failed test {digits}"),
            result: GbaTestResult::Fail,
        });
    }

    None
}

fn gba_mode4_vram_digit_at(vram: &[u8], x: usize, y: usize) -> Option<char> {
    ('0'..='9').find(|&digit| gba_mode4_vram_matches_char(vram, x, y, digit))
}

fn gba_mode4_vram_matches_text(vram: &[u8], x: usize, y: usize, text: &str) -> bool {
    text.chars()
        .enumerate()
        .all(|(index, ch)| gba_mode4_vram_matches_char(vram, x + index * 8, y, ch))
}

fn gba_mode4_vram_matches_char(vram: &[u8], x: usize, y: usize, ch: char) -> bool {
    let Some((upper, lower)) = gba_jsmolka_glyph_words(ch) else {
        return false;
    };

    if x + 8 > 240 || y + 8 > 160 {
        return false;
    }

    for py in 0..8 {
        let word = if py < 4 { upper } else { lower };
        let row = py % 4;
        for px in 0..8 {
            let bit = ((word >> (row * 8 + px)) & 1) as u8;
            let offset = (y + py) * 240 + x + px;
            if vram.get(offset).copied().unwrap_or(0) != bit {
                return false;
            }
        }
    }

    true
}

fn gba_jsmolka_glyph_words(ch: char) -> Option<(u32, u32)> {
    match ch {
        ' ' => Some((0x0000_0000, 0x0000_0000)),
        '0' => Some((0x7E76_663C, 0x003C_666E)),
        '1' => Some((0x181E_1C18, 0x0018_1818)),
        '2' => Some((0x3060_663C, 0x007E_0C18)),
        '3' => Some((0x3860_663C, 0x003C_6660)),
        '4' => Some((0x3336_3C38, 0x0030_307F)),
        '5' => Some((0x603E_067E, 0x003C_6660)),
        '6' => Some((0x3E06_0C38, 0x003C_6666)),
        '7' => Some((0x3060_607E, 0x0018_1818)),
        '8' => Some((0x3C66_663C, 0x003C_6666)),
        '9' => Some((0x7C66_663C, 0x001C_3060)),
        'A' => Some((0x7E66_663C, 0x0066_6666)),
        'F' => Some((0x1E06_067E, 0x0006_0606)),
        'a' => Some((0x603C_0000, 0x007C_667C)),
        'd' => Some((0x667C_6060, 0x007C_6666)),
        'e' => Some((0x663C_0000, 0x003C_067E)),
        'i' => Some((0x1818_0018, 0x0030_1818)),
        'l' => Some((0x1818_1818, 0x0030_1818)),
        'p' => Some((0x663E_0000, 0x0606_3E66)),
        's' => Some((0x063C_0000, 0x003E_603C)),
        't' => Some((0x0C3E_0C0C, 0x0038_0C0C)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GbaTestResult, MgbaSuiteActiveInfo, gba_jsmolka_glyph_words, gba_mode4_vram_matches_text,
        mgba_suite_status_from_log_and_active_info,
    };

    fn draw_char(vram: &mut [u8], x: usize, y: usize, ch: char) {
        let (upper, lower) = gba_jsmolka_glyph_words(ch).unwrap();
        for py in 0..8 {
            let word = if py < 4 { upper } else { lower };
            let row = py % 4;
            for px in 0..8 {
                let bit = ((word >> (row * 8 + px)) & 1) as u8;
                vram[(y + py) * 240 + x + px] = bit;
            }
        }
    }

    fn draw_text(vram: &mut [u8], x: usize, y: usize, text: &str) {
        for (index, ch) in text.chars().enumerate() {
            draw_char(vram, x + index * 8, y, ch);
        }
    }

    #[test]
    fn jsmolka_mode4_text_match_finds_pass_string() {
        let mut vram = vec![0; 0x18000];
        draw_text(&mut vram, 56, 76, "All tests passed");
        assert!(gba_mode4_vram_matches_text(
            &vram,
            56,
            76,
            "All tests passed"
        ));
        assert!(!gba_mode4_vram_matches_text(&vram, 60, 76, "Failed test "));
    }

    #[test]
    fn mgba_suite_sram_log_reports_first_failure() {
        let status = mgba_suite_status_from_log_and_active_info(
            "Game Boy Advance Test Suite\n===\nMemory test: ROM load\nDMA0: FAIL\n",
            Some(MgbaSuiteActiveInfo {
                subtest_id: u16::MAX,
                test_id: u8::MAX,
                suite_id: 0,
            }),
        )
        .unwrap();

        assert_eq!(status.protocol, "mgba_suite_sram_log");
        assert_eq!(status.result, GbaTestResult::Fail);
        assert_eq!(status.text, "DMA0: FAIL");
    }

    #[test]
    fn mgba_suite_sram_log_passes_when_suite_finished_without_failures() {
        let status = mgba_suite_status_from_log_and_active_info(
            "Game Boy Advance Test Suite\n===\nMemory test: ROM load\n",
            Some(MgbaSuiteActiveInfo {
                subtest_id: u16::MAX,
                test_id: u8::MAX,
                suite_id: 0,
            }),
        )
        .unwrap();

        assert_eq!(status.result, GbaTestResult::Pass);
    }

    #[test]
    fn mgba_suite_sram_log_passes_finished_suite_with_no_per_test_log() {
        let status = mgba_suite_status_from_log_and_active_info(
            "Game Boy Advance Test Suite\n===\n",
            Some(MgbaSuiteActiveInfo {
                subtest_id: 0,
                test_id: u8::MAX,
                suite_id: 1,
            }),
        )
        .unwrap();

        assert_eq!(status.result, GbaTestResult::Pass);
    }

    #[test]
    fn mgba_suite_sram_log_keeps_running_before_suite_finished() {
        let status = mgba_suite_status_from_log_and_active_info(
            "Game Boy Advance Test Suite\n===\nMemory test: ROM load\n",
            Some(MgbaSuiteActiveInfo {
                subtest_id: 3,
                test_id: 1,
                suite_id: 0,
            }),
        )
        .unwrap();

        assert_eq!(status.result, GbaTestResult::Running);
    }
}
