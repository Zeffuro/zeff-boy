use zeff_sega8_core::hardware::bus::CpuAccessTraceEvent as Sega8BusTraceEvent;

pub(super) const SDSC_DEBUG_CONSOLE_COMMAND_PORT: u8 = 0xFC;
pub(super) const SDSC_DEBUG_CONSOLE_DATA_PORT: u8 = 0xFD;
pub(super) const SDSC_DEBUG_CONSOLE_SUSPEND_COMMAND: u8 = 0x01;
pub(super) const SDSC_DEBUG_CONSOLE_CLEAR_SCREEN_COMMAND: u8 = 0x02;
const SDSC_TEXT_PREVIEW_MAX_CHARS: usize = 512;

#[derive(Default)]
pub(super) struct Sega8SdscCapture {
    text: String,
    pub(super) command_count: u64,
    pub(super) suspend_seen: bool,
}

impl Sega8SdscCapture {
    pub(super) fn record_bus_event(&mut self, event: Sega8BusTraceEvent) {
        let Sega8BusTraceEvent::IoWrite { port, value } = event else {
            return;
        };

        match port {
            SDSC_DEBUG_CONSOLE_DATA_PORT => self.text.push(char::from(value)),
            SDSC_DEBUG_CONSOLE_COMMAND_PORT => {
                self.command_count = self.command_count.wrapping_add(1);
                match value {
                    SDSC_DEBUG_CONSOLE_SUSPEND_COMMAND => self.suspend_seen = true,
                    SDSC_DEBUG_CONSOLE_CLEAR_SCREEN_COMMAND => self.text.clear(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn preview(&self) -> String {
        let mut preview = String::new();
        for ch in self.text.chars().take(SDSC_TEXT_PREVIEW_MAX_CHARS) {
            if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
                preview.push(' ');
            } else {
                preview.push(ch);
            }
        }
        if self.text.chars().count() > SDSC_TEXT_PREVIEW_MAX_CHARS {
            preview.push_str("...");
        }
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sega8_sdsc_capture_collects_text_and_commands() {
        let mut capture = Sega8SdscCapture::default();

        for value in b"OK" {
            capture.record_bus_event(Sega8BusTraceEvent::IoWrite {
                port: SDSC_DEBUG_CONSOLE_DATA_PORT,
                value: *value,
            });
        }
        capture.record_bus_event(Sega8BusTraceEvent::IoWrite {
            port: SDSC_DEBUG_CONSOLE_COMMAND_PORT,
            value: SDSC_DEBUG_CONSOLE_SUSPEND_COMMAND,
        });

        assert_eq!(capture.text(), "OK");
        assert_eq!(capture.command_count, 1);
        assert!(capture.suspend_seen);
    }

    #[test]
    fn sega8_sdsc_clear_screen_command_clears_captured_text() {
        let mut capture = Sega8SdscCapture::default();
        capture.record_bus_event(Sega8BusTraceEvent::IoWrite {
            port: SDSC_DEBUG_CONSOLE_DATA_PORT,
            value: b'X',
        });
        capture.record_bus_event(Sega8BusTraceEvent::IoWrite {
            port: SDSC_DEBUG_CONSOLE_COMMAND_PORT,
            value: SDSC_DEBUG_CONSOLE_CLEAR_SCREEN_COMMAND,
        });
        capture.record_bus_event(Sega8BusTraceEvent::IoWrite {
            port: SDSC_DEBUG_CONSOLE_DATA_PORT,
            value: b'Y',
        });

        assert_eq!(capture.text(), "Y");
    }
}
