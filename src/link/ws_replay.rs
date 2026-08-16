use zeff_emu_common::replay::{ReplayEvent, ReplayWonderSwanLinkEvent};
use zeff_ws_core::emulator::Emulator as WonderSwanEmulator;

use crate::link::LinkSessionError;

pub(crate) struct WonderSwanReplayLink {
    events: Vec<ReplayWonderSwanLinkRecord>,
}

#[derive(Clone, Copy)]
struct ReplayWonderSwanLinkRecord {
    frame: u64,
    tick: u64,
    event: ReplayWonderSwanLinkEvent,
    delivered: bool,
}

impl WonderSwanReplayLink {
    pub(crate) fn try_new(
        events: Vec<ReplayEvent>,
        base_frame: u64,
        replay_start_tick: Option<u64>,
        playback_start_tick: u64,
    ) -> anyhow::Result<Self> {
        let base_tick = replay_start_tick.map(|_| playback_start_tick).unwrap_or(0);
        let mut records = Vec::new();
        for event in events {
            let ReplayEvent::WonderSwanLink {
                frame,
                session_cycle,
                event,
            } = event
            else {
                continue;
            };
            let frame = base_frame.checked_add(frame).ok_or_else(|| {
                anyhow::anyhow!("replay WonderSwan event frame overflows playback timeline")
            })?;
            let tick = base_tick.checked_add(session_cycle).ok_or_else(|| {
                anyhow::anyhow!("replay WonderSwan event tick overflows playback timeline")
            })?;
            records.push(ReplayWonderSwanLinkRecord {
                frame,
                tick,
                event,
                delivered: false,
            });
        }
        records.sort_by_key(|record| (record.frame, record.tick));
        Ok(Self { events: records })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub(crate) fn poll_emulator(
        &mut self,
        emulator: &mut WonderSwanEmulator,
    ) -> Result<(), LinkSessionError> {
        for record in &mut self.events {
            if record.delivered
                || record.frame > emulator.frame_count()
                || record.tick > emulator.cpu_cycles()
            {
                continue;
            }
            record.delivered = true;
            match record.event {
                ReplayWonderSwanLinkEvent::RemoteByte { byte, .. } => {
                    emulator.receive_wonder_swan_link_byte(byte);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn debug_summary(&self) -> String {
        let delivered = self.events.iter().filter(|record| record.delivered).count();
        format!("delivered={delivered}/{}", self.events.len())
    }
}

#[cfg(test)]
mod tests {
    use super::WonderSwanReplayLink;
    use zeff_emu_common::replay::{ReplayEvent, ReplayWonderSwanLinkEvent};
    use zeff_ws_core::emulator::Emulator as WonderSwanEmulator;

    const SERIAL_CONTROL_PORT: u16 = 0x00B3;
    const SERIAL_DATA_PORT: u16 = 0x00B1;
    const SERIAL_CONTROL_ENABLE: u8 = 0x80;
    const SERIAL_STATUS_RX_READY: u8 = 0x01;

    #[test]
    fn rejects_overflowed_playback_timestamps() {
        let event = ReplayEvent::WonderSwanLink {
            frame: 1,
            session_cycle: 1,
            event: ReplayWonderSwanLinkEvent::RemoteByte {
                generation: 0,
                baud_bps: 9_600,
                byte: 0x42,
            },
        };
        assert!(
            WonderSwanReplayLink::try_new(vec![event.clone()], u64::MAX, None, 0)
                .err()
                .expect("frame overflow should fail")
                .to_string()
                .contains("frame")
        );
        assert!(
            WonderSwanReplayLink::try_new(vec![event], 0, Some(0), u64::MAX)
                .err()
                .expect("tick overflow should fail")
                .to_string()
                .contains("tick")
        );
    }

    #[test]
    fn applies_remote_byte_at_its_recorded_cycle() {
        let mut emulator = wonder_swan_emulator();
        emulator.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
        let mut link = WonderSwanReplayLink::try_new(
            vec![ReplayEvent::WonderSwanLink {
                frame: 0,
                session_cycle: 20,
                event: ReplayWonderSwanLinkEvent::RemoteByte {
                    generation: 3,
                    baud_bps: 9_600,
                    byte: 0x5A,
                },
            }],
            0,
            Some(0),
            0,
        )
        .expect("event timestamps should be valid");

        link.poll_emulator(&mut emulator).unwrap();
        assert_eq!(
            emulator.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            0
        );
        step_until_cycle(&mut emulator, 20);
        link.poll_emulator(&mut emulator).unwrap();

        assert_eq!(
            emulator.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            SERIAL_STATUS_RX_READY
        );
        assert_eq!(emulator.io_peek8(SERIAL_DATA_PORT), 0x5A);
    }

    fn step_until_cycle(emulator: &mut WonderSwanEmulator, target_cycle: u64) {
        while emulator.cpu_cycles() < target_cycle {
            emulator
                .step_instruction()
                .expect("minimal WonderSwan test ROM should keep running");
        }
    }

    fn wonder_swan_emulator() -> WonderSwanEmulator {
        let mut rom = vec![0x90; 0x10000];
        rom[0] = 0x90;
        rom[1] = 0xEB;
        rom[2] = 0xFC;
        let reset_vector = rom.len() - 16;
        rom[reset_vector..reset_vector + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
        let footer = rom.len() - 10;
        rom[footer] = 0x01;
        rom[footer + 2] = 0x23;
        rom[footer + 4] = 0x01;
        let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        WonderSwanEmulator::from_rom_data(&rom).expect("minimal WonderSwan ROM should initialize")
    }
}
