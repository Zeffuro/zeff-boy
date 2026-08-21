use super::super::cd_media::CdTrack;
use super::*;

impl CdRom2 {
    pub(super) fn write_control(&mut self, value: u8) {
        self.data_ready_irq_enabled = value & 0x40 != 0;
        self.status_irq_enabled = value & 0x20 != 0;
        self.audio_end_irq_enabled = value & 0x08 != 0;
        self.audio_half_irq_enabled = value & 0x04 != 0;
        if self.auto_acknowledge {
            return;
        }
        let next_ack = value & 0x80 != 0;
        if next_ack && !self.acknowledge {
            self.acknowledge = true;
            if self.request {
                self.request = false;
                self.acknowledged_request = true;
            }
        } else if !next_ack && self.acknowledge {
            self.acknowledge = false;
            if self.acknowledged_request {
                self.acknowledged_request = false;
                match self.phase {
                    CdScsiPhase::Command => self.consume_command_byte(),
                    CdScsiPhase::DataIn | CdScsiPhase::Status | CdScsiPhase::MessageIn => {
                        self.consume_response_byte();
                    }
                    CdScsiPhase::BusFree | CdScsiPhase::Selection | CdScsiPhase::Busy => {}
                }
            } else if self.request_pending {
                self.raise_request();
            }
        }
    }

    fn consume_command_byte(&mut self) {
        self.command.push(self.output_latch);
        let expected = match self.command[0] {
            0xD8 | 0xD9 | 0xDA | 0xDD | 0xDE => 10,
            _ => 6,
        };
        if self.command.len() == expected {
            self.schedule(CdEvent::ExecuteCommand, PROVISIONAL_CDROM2_PHASE_TICKS);
        } else {
            self.schedule(CdEvent::RaiseRequest, PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS);
        }
    }

    fn execute_command(&mut self) {
        let command = std::mem::take(&mut self.command);
        let mut bytes = [0; 10];
        bytes[..command.len()].copy_from_slice(&command);
        if self.command_trace.len() == CD_COMMAND_TRACE_CAPACITY {
            self.command_trace.pop_front();
        }
        self.command_trace.push_back(CdCommandTrace {
            bytes,
            len: command.len() as u8,
        });
        self.status = 0;
        self.response.clear();
        self.response_index = 0;
        self.response_available = 0;
        match command[0] {
            0x00 => self.enter_status_after(0),
            0x03 => {
                self.response.resize(18, 0);
                self.response[0] = 0x70;
                self.response[2] = self.sense_key;
                self.response[7] = 10;
                self.response[12] = self.additional_sense_code;
                self.sense_key = 0;
                self.additional_sense_code = 0;
                self.enter_data_after(0);
            }
            0x08 => self.execute_read6(&command),
            0xD8 => self.execute_audio_start(&command),
            0xD9 => self.execute_audio_end(&command),
            0xDA => match self.audio_status {
                CdAudioStatus::Playing => {
                    self.audio_status = CdAudioStatus::Paused;
                    self.enter_status_after(0);
                }
                CdAudioStatus::Paused => self.enter_status_after(0),
                CdAudioStatus::Inactive | CdAudioStatus::Stopped => {
                    self.check_condition(0x05, 0x2C);
                }
            },
            0xDD => self.execute_read_subcode_q(),
            0xDE => self.execute_get_directory_info(&command),
            _ => self.check_condition(0x05, 0x20),
        }
    }

    fn execute_read6(&mut self, command: &[u8]) {
        self.stop_audio(CdAudioStatus::Inactive);
        let lba = (u32::from(command[1] & 0x1F) << 16)
            | (u32::from(command[2]) << 8)
            | u32::from(command[3]);
        let count = if command[4] == 0 {
            256
        } else {
            u16::from(command[4])
        };
        self.response
            .reserve(usize::from(count) * CD_USER_SECTOR_BYTES);
        for offset in 0..u32::from(count) {
            match self.disc.read_user_sector(lba + offset) {
                Ok(sector) => self.response.extend_from_slice(&sector),
                Err(_) => {
                    self.response.clear();
                    self.check_condition(0x05, 0x21);
                    return;
                }
            }
        }
        self.phase = CdScsiPhase::DataIn;
        self.request = false;
        self.response_index = 0;
        self.response_available = 0;
        self.sectors_pending = count;
        self.sector_arrival_ticks =
            Some(self.next_sector_delays(PROVISIONAL_CDROM2_READ_STARTUP_SECTORS));
    }

    fn execute_audio_start(&mut self, command: &[u8]) {
        let Some(lba) = self.audio_command_lba(command, self.audio_current_lba) else {
            self.check_condition(0x05, 0x21);
            return;
        };
        self.audio_start_lba = lba;
        self.audio_current_lba = lba;
        self.audio_current_sample = 0;
        self.audio_end_lba = self.disc.leadout_lba();
        self.audio_end_behavior = CdAudioEndBehavior::Stop;
        self.audio_status = if command[1] & 1 != 0 {
            CdAudioStatus::Playing
        } else {
            CdAudioStatus::Paused
        };
        self.audio_source_frames.clear();
        self.audio_resample_position = 0.0;
        self.phase = CdScsiPhase::Busy;
        self.request = false;
        self.schedule(CdEvent::CompleteAudioStart, PROVISIONAL_CDROM2_PHASE_TICKS);
    }

    fn execute_audio_end(&mut self, command: &[u8]) {
        let Some(lba) = self.audio_command_lba(command, self.disc.leadout_lba()) else {
            self.check_condition(0x05, 0x21);
            return;
        };
        match command[1] {
            0 => {
                self.stop_audio(CdAudioStatus::Stopped);
                self.enter_status_after(0);
            }
            1 => {
                self.audio_end_lba = lba;
                self.audio_end_behavior = CdAudioEndBehavior::Loop;
                self.audio_status = CdAudioStatus::Playing;
                self.phase = CdScsiPhase::Busy;
                self.request = false;
                self.event = None;
            }
            2 => {
                self.audio_end_lba = lba;
                self.audio_end_behavior = CdAudioEndBehavior::SignalCompletion;
                self.audio_status = CdAudioStatus::Playing;
                self.phase = CdScsiPhase::Busy;
                self.request = false;
                self.event = None;
            }
            3 => {
                self.audio_end_lba = lba;
                self.audio_end_behavior = CdAudioEndBehavior::Stop;
                self.audio_status = CdAudioStatus::Playing;
                self.enter_status_after(0);
            }
            4 => {
                self.audio_end_lba = lba;
                self.enter_status_after(0);
            }
            _ => self.check_condition(0x05, 0x22),
        }
    }

    fn execute_read_subcode_q(&mut self) {
        let lba = self.audio_current_lba;
        let track = self.disc.timeline_track_at_lba(lba);
        let track_number = track.map_or(0, CdTrack::number);
        let control = track.map_or(0, CdTrack::control);
        let index = track.map_or(1, |track| u8::from(lba >= track.index1_lba()));
        let relative = track.map_or(0, |track| {
            if index == 0 {
                track.index1_lba() - 1 - lba
            } else {
                lba - track.index1_lba()
            }
        });
        self.response.extend([
            self.audio_status as u8,
            (control << 4) | 1,
            to_bcd(track_number),
            index,
        ]);
        self.response.extend(frames_to_msf(relative));
        self.response.extend(lba_to_msf(lba));
        self.enter_data_after(0);
    }

    fn audio_command_lba(&self, command: &[u8], current_position: u32) -> Option<u32> {
        match command[9] & 0xC0 {
            0x00 => Some(
                (u32::from(command[3]) << 16)
                    | (u32::from(command[4]) << 8)
                    | u32::from(command[5]),
            ),
            0x40 => {
                let minutes = u32::from(checked_from_bcd(command[2])?);
                let seconds = u32::from(checked_from_bcd(command[3])?);
                let frames = u32::from(checked_from_bcd(command[4])?);
                (seconds < 60 && frames < 75)
                    .then_some((minutes * 60 + seconds) * 75 + frames)
                    .and_then(|absolute| absolute.checked_sub(150))
            }
            0x80 => {
                let track = checked_from_bcd(command[2])?;
                self.disc.track(track).map(CdTrack::index1_lba)
            }
            _ => Some(current_position),
        }
    }

    fn execute_get_directory_info(&mut self, command: &[u8]) {
        match command[1] & 3 {
            0 => self.response.extend([
                to_bcd(self.disc.first_track()),
                to_bcd(self.disc.last_track()),
                0,
                0,
            ]),
            1 => {
                self.response.extend(lba_to_msf(self.disc.leadout_lba()));
                self.response.push(0);
            }
            mode @ (2 | 3) => {
                let Some(number) = checked_from_bcd(command[2]) else {
                    self.check_condition(0x05, 0x21);
                    return;
                };
                let Some(track) = self.disc.track(number) else {
                    self.check_condition(0x05, 0x21);
                    return;
                };
                if mode == 2 {
                    self.response.extend(lba_to_msf(track.index1_lba()));
                } else {
                    let [_, high, middle, low] = track.index1_lba().to_be_bytes();
                    self.response.extend([high, middle, low]);
                }
                self.response.push(track.control());
            }
            _ => unreachable!(),
        }
        self.enter_data_after(0);
    }

    fn check_condition(&mut self, sense_key: u8, additional_sense_code: u8) {
        self.sense_key = sense_key;
        self.additional_sense_code = additional_sense_code;
        self.status = 1;
        self.enter_status_after(0);
    }

    fn consume_response_byte(&mut self) {
        self.request = false;
        self.response_index += 1;
        if self.response_index < self.response_available {
            self.raise_request();
            return;
        }
        if self.response_index < self.response.len() {
            return;
        }
        match self.phase {
            CdScsiPhase::DataIn => self.enter_status_after(PROVISIONAL_CDROM2_PHASE_TICKS),
            CdScsiPhase::Status => {
                self.phase = CdScsiPhase::MessageIn;
                self.response.clear();
                self.response.push(0);
                self.response_index = 0;
                self.response_available = 1;
                self.schedule(CdEvent::EnterMessage, PROVISIONAL_CDROM2_PHASE_TICKS);
            }
            CdScsiPhase::MessageIn => {
                self.schedule(CdEvent::EnterBusFree, PROVISIONAL_CDROM2_PHASE_TICKS);
            }
            _ => {}
        }
    }

    fn enter_data_after(&mut self, delay: u64) {
        self.phase = CdScsiPhase::DataIn;
        self.request = false;
        self.response_index = 0;
        self.response_available = self.response.len();
        if delay == 0 {
            self.raise_request();
        } else {
            self.schedule(CdEvent::RaiseRequest, delay);
        }
    }

    pub(super) fn enter_status_after(&mut self, delay: u64) {
        self.phase = CdScsiPhase::Status;
        self.request = false;
        self.response.clear();
        self.response.push(self.status);
        self.response_index = 0;
        self.response_available = 1;
        if delay == 0 {
            self.raise_request();
        } else {
            self.schedule(CdEvent::EnterStatus, delay);
        }
    }

    pub(super) fn enter_bus_free(&mut self) {
        self.phase = CdScsiPhase::BusFree;
        self.request = false;
        self.acknowledge = false;
        self.auto_acknowledge = false;
        self.acknowledged_request = false;
        self.request_pending = false;
        self.command.clear();
        self.response.clear();
        self.response_index = 0;
        self.response_available = 0;
        self.event = None;
        self.sector_arrival_ticks = None;
        self.sectors_pending = 0;
    }

    pub(super) fn handle_event(&mut self, event: CdEvent) {
        match event {
            CdEvent::EnterCommand => {
                self.phase = CdScsiPhase::Command;
                self.command.clear();
                self.raise_request();
            }
            CdEvent::RaiseRequest | CdEvent::EnterStatus | CdEvent::EnterMessage => {
                self.raise_request();
            }
            CdEvent::ExecuteCommand => self.execute_command(),
            CdEvent::EnterBusFree => self.enter_bus_free(),
            CdEvent::CompleteAutoAck => {
                self.acknowledge = false;
                self.auto_acknowledge = false;
                self.consume_response_byte();
            }
            CdEvent::CompleteAudioStart => {
                self.status = 0;
                self.enter_status_after(0);
            }
        }
    }

    pub(super) fn handle_sector_arrival(&mut self) {
        self.response_available =
            (self.response_available + CD_USER_SECTOR_BYTES).min(self.response.len());
        self.sectors_pending -= 1;
        if self.sectors_pending != 0 {
            self.sector_arrival_ticks = Some(self.next_sector_delay());
        }
        self.service_adpcm_dma();
        if self.phase == CdScsiPhase::DataIn
            && self.response_index < self.response_available
            && !self.request
        {
            self.raise_request();
        }
    }

    fn raise_request(&mut self) {
        if self.acknowledge {
            self.request_pending = true;
        } else {
            self.request = true;
            self.request_pending = false;
        }
    }

    fn next_sector_delay(&mut self) -> u64 {
        let total = self.sector_tick_remainder + CDROM2_SECTOR_TICKS_NUMERATOR;
        self.sector_tick_remainder = total % CDROM2_SECTOR_TICKS_DENOMINATOR;
        total / CDROM2_SECTOR_TICKS_DENOMINATOR
    }

    fn next_sector_delays(&mut self, count: u8) -> u64 {
        (0..count).map(|_| self.next_sector_delay()).sum()
    }

    #[inline]
    pub(super) fn schedule(&mut self, event: CdEvent, ticks: u64) {
        self.event = Some((event, ticks));
    }

    #[inline]
    pub(super) fn current_input_data(&self) -> u8 {
        if self.request {
            self.response.get(self.response_index).copied().unwrap_or(0)
        } else {
            0
        }
    }

    #[inline]
    pub(super) fn data_register(&self) -> u8 {
        if matches!(
            self.phase,
            CdScsiPhase::BusFree
                | CdScsiPhase::Selection
                | CdScsiPhase::Command
                | CdScsiPhase::Busy
        ) {
            self.output_latch
        } else {
            self.current_input_data()
        }
    }

    #[inline]
    pub(super) fn data_ready_condition(&self) -> bool {
        self.request && self.phase == CdScsiPhase::DataIn
    }

    #[inline]
    pub(super) fn status_condition(&self) -> bool {
        self.request && matches!(self.phase, CdScsiPhase::Status | CdScsiPhase::MessageIn)
    }

    pub(super) fn bus_status(&self) -> u8 {
        let busy = !matches!(self.phase, CdScsiPhase::BusFree | CdScsiPhase::Selection);
        let message = self.phase == CdScsiPhase::MessageIn;
        let command_or_status = matches!(
            self.phase,
            CdScsiPhase::Command | CdScsiPhase::Status | CdScsiPhase::MessageIn
        );
        let input = matches!(
            self.phase,
            CdScsiPhase::DataIn | CdScsiPhase::Status | CdScsiPhase::MessageIn
        );
        u8::from(busy) << 7
            | u8::from(self.request) << 6
            | u8::from(message) << 5
            | u8::from(command_or_status) << 4
            | u8::from(input) << 3
    }
}

fn lba_to_msf(lba: u32) -> [u8; 3] {
    let absolute = lba + 150;
    [
        to_bcd((absolute / (75 * 60)) as u8),
        to_bcd(((absolute / 75) % 60) as u8),
        to_bcd((absolute % 75) as u8),
    ]
}

fn frames_to_msf(frames: u32) -> [u8; 3] {
    [
        to_bcd((frames / (75 * 60)) as u8),
        to_bcd(((frames / 75) % 60) as u8),
        to_bcd((frames % 75) as u8),
    ]
}

#[inline]
const fn to_bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

#[inline]
const fn from_bcd(value: u8) -> u8 {
    (value >> 4) * 10 + (value & 15)
}

#[inline]
const fn checked_from_bcd(value: u8) -> Option<u8> {
    if value >> 4 > 9 || value & 15 > 9 {
        None
    } else {
        Some(from_bcd(value))
    }
}
