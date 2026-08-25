mod audio;
mod scsi;
pub(crate) mod state;

use std::collections::VecDeque;

use super::bus::OPEN_BUS_VALUE;
use super::cd_media::{CD_USER_SECTOR_BYTES, CdDisc};
use super::cpu::LineLevel;

pub const CDROM2_WORK_RAM_LEN: usize = 0x1_0000;
pub const CDROM2_BRAM_LEN: usize = 0x0800;
pub const CDROM2_ADPCM_RAM_LEN: usize = 0x1_0000;
pub const CDROM2_REGISTER_START: u32 = 0x1F_F800;
pub const CDROM2_REGISTER_END: u32 = 0x1F_F80F;
pub const SUPER_SYSTEM_CARD_ID_START: u32 = 0x1F_F8C0;
pub const SUPER_SYSTEM_CARD_ID_END: u32 = 0x1F_F8C3;
pub const CDROM2_WORK_RAM_START: u32 = 0x10_0000;
pub const CDROM2_WORK_RAM_END: u32 = 0x10_FFFF;
pub const CDROM2_BRAM_START: u32 = 0x1E_E000;
pub const CDROM2_BRAM_END: u32 = 0x1E_E7FF;
pub const PROVISIONAL_CDROM2_SELECTION_TICKS: u64 = 75_000;
pub const PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS: u64 = 3_000;
pub const PROVISIONAL_CDROM2_PHASE_TICKS: u64 = 1_500;
pub const PROVISIONAL_CDROM2_AUTO_ACK_TICKS: u64 = 21;
pub const PROVISIONAL_CDROM2_READ_STARTUP_SECTORS: u8 = 3;
pub const PROVISIONAL_CDROM2_ADPCM_MIX_GAIN: f32 = 0.5;
pub const PROVISIONAL_CDROM2_ADPCM_RATE_WRITE_PRESERVES_PHASE: bool = true;
pub const PROVISIONAL_CDROM2_ADPCM_STOP_AT_NEXT_NIBBLE_BOUNDARY: bool = true;
pub const PROVISIONAL_CDROM2_ADPCM_RESTART_REQUIRES_END_CLEAR_OR_D6_CLEAR: bool = true;
pub const PROVISIONAL_CDROM2_FADE_LONG_STEP_TICKS: u64 = 1_965;
pub const PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS: u64 = 819;

const CDROM2_SECTOR_TICKS_NUMERATOR: u64 = 3_150_000;
const CDROM2_SECTOR_TICKS_DENOMINATOR: u64 = 11;
const CDDA_TICK_NUMERATOR: u64 = 77;
const CDDA_TICK_DENOMINATOR: u64 = 37_500;
const CDDA_SOURCE_RATE: f64 = 44_100.0;
const CDDA_MIX_GAIN: f32 = 0.5;
const CDDA_SAMPLE_LATCH_TICKS: u64 = 700;
const CD_COMMAND_TRACE_CAPACITY: usize = 128;
const ADPCM_CLOCK_NUMERATOR: u64 = 176;
const ADPCM_CLOCK_DENOMINATOR: u64 = 118_125;
const CDROM2_MASTER_CLOCK_NUMERATOR: u64 = 315_000_000 * 6;
const CDROM2_MASTER_CLOCK_DENOMINATOR: u64 = 88;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CdCommandTrace {
    bytes: [u8; 10],
    len: u8,
}

impl CdCommandTrace {
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CdScsiPhase {
    #[default]
    BusFree,
    Selection,
    Command,
    DataIn,
    Busy,
    Status,
    MessageIn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CdEvent {
    EnterCommand,
    RaiseRequest,
    ExecuteCommand,
    EnterStatus,
    EnterMessage,
    EnterBusFree,
    CompleteAutoAck,
    CompleteAudioStart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdProtocolEventDebugKind {
    EnterCommand,
    RaiseRequest,
    ExecuteCommand,
    EnterStatus,
    EnterMessage,
    EnterBusFree,
    CompleteAutoAck,
    CompleteAudioStart,
}

impl CdProtocolEventDebugKind {
    const fn from_event(event: CdEvent) -> Self {
        match event {
            CdEvent::EnterCommand => Self::EnterCommand,
            CdEvent::RaiseRequest => Self::RaiseRequest,
            CdEvent::ExecuteCommand => Self::ExecuteCommand,
            CdEvent::EnterStatus => Self::EnterStatus,
            CdEvent::EnterMessage => Self::EnterMessage,
            CdEvent::EnterBusFree => Self::EnterBusFree,
            CdEvent::CompleteAutoAck => Self::CompleteAutoAck,
            CdEvent::CompleteAudioStart => Self::CompleteAudioStart,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CdProtocolEventDebugSnapshot {
    pub kind: CdProtocolEventDebugKind,
    pub ticks_remaining: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum CdAudioStatus {
    Playing = 0,
    #[default]
    Inactive = 1,
    Paused = 2,
    Stopped = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdAudioEndMode {
    Stop,
    Loop,
    SignalCompletion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdAudioFadeTarget {
    Cdda,
    Adpcm,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CdAudioDebugSnapshot {
    pub status: CdAudioStatus,
    pub start_lba: u32,
    pub end_lba: u32,
    pub current_lba: u32,
    pub current_sample: usize,
    pub end_mode: CdAudioEndMode,
    pub tick_accumulator: u64,
    pub queued_source_frames: usize,
    pub fade_control: u8,
    pub fade_target: Option<CdAudioFadeTarget>,
    pub fade_level_q16: u32,
    pub fade_step_ticks: u64,
    pub fade_ticks_to_next: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CdRom2DebugSnapshot {
    pub phase: CdScsiPhase,
    pub bus_status: u8,
    pub request: bool,
    pub acknowledge: bool,
    pub auto_acknowledge: bool,
    pub acknowledged_request: bool,
    pub request_pending: bool,
    pub reset_asserted: bool,
    pub output_latch: u8,
    pub current_input_data: u8,
    pub data_register: u8,
    pub command: Vec<u8>,
    pub response: Vec<u8>,
    pub response_index: usize,
    pub response_available: usize,
    pub status: u8,
    pub sense_key: u8,
    pub additional_sense_code: u8,
    pub pending_event: Option<CdProtocolEventDebugSnapshot>,
    pub sector_arrival_ticks: Option<u64>,
    pub sectors_pending: u16,
    pub sector_tick_remainder: u64,
    pub data_ready_irq_enabled: bool,
    pub status_irq_enabled: bool,
    pub audio_end_irq_enabled: bool,
    pub audio_half_irq_enabled: bool,
    pub data_ready_condition: bool,
    pub status_condition: bool,
    pub irq2_asserted: bool,
    pub bram_unlocked: bool,
    pub recent_commands: Vec<CdCommandTrace>,
    pub audio: CdAudioDebugSnapshot,
    pub audio_left_sample: i16,
    pub audio_right_sample: i16,
    pub audio_sample_latch: i16,
    pub audio_sample_latch_right: bool,
    pub audio_sample_rate: u32,
    pub audio_sample_generation_enabled: bool,
    pub adpcm_address_latch: u16,
    pub adpcm_read_address: u16,
    pub adpcm_write_address: u16,
    pub adpcm_read_buffer: u8,
    pub adpcm_dma_control: u8,
    pub adpcm_address_control: u8,
    pub adpcm_playback_rate: u8,
    pub adpcm_length: u32,
    pub adpcm_playing: bool,
    pub adpcm_stop_pending: bool,
    pub adpcm_high_nibble_next: bool,
    pub adpcm_clock_accumulator: u64,
    pub adpcm_predictor: u16,
    pub adpcm_step_index: u8,
    pub adpcm_end_irq: bool,
    pub adpcm_half_irq: bool,
    pub adpcm_buffered_samples: usize,
}

fn formatted_bram() -> Box<[u8; CDROM2_BRAM_LEN]> {
    let mut bram = Box::new([0; CDROM2_BRAM_LEN]);
    bram[..8].copy_from_slice(b"HUBM\x00\xA0\x10\x80");
    bram
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CdAudioEndBehavior {
    #[default]
    Stop,
    Loop,
    SignalCompletion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CdFadeTarget {
    Cdda,
    Adpcm,
}

#[derive(Debug)]
pub struct CdRom2 {
    disc: CdDisc,
    super_system_card: bool,
    work_ram: Box<[u8; CDROM2_WORK_RAM_LEN]>,
    bram: Box<[u8; CDROM2_BRAM_LEN]>,
    adpcm_ram: Box<[u8; CDROM2_ADPCM_RAM_LEN]>,
    bram_unlocked: bool,
    reset_asserted: bool,
    phase: CdScsiPhase,
    request: bool,
    acknowledge: bool,
    auto_acknowledge: bool,
    acknowledged_request: bool,
    request_pending: bool,
    data_ready_irq_enabled: bool,
    status_irq_enabled: bool,
    audio_end_irq_enabled: bool,
    audio_half_irq_enabled: bool,
    output_latch: u8,
    command: Vec<u8>,
    command_trace: VecDeque<CdCommandTrace>,
    response: Vec<u8>,
    response_index: usize,
    response_available: usize,
    status: u8,
    sense_key: u8,
    additional_sense_code: u8,
    event: Option<(CdEvent, u64)>,
    sector_arrival_ticks: Option<u64>,
    sectors_pending: u16,
    sector_tick_remainder: u64,
    audio_status: CdAudioStatus,
    audio_start_lba: u32,
    audio_end_lba: u32,
    audio_current_lba: u32,
    audio_current_sample: usize,
    // Derived cache: never part of persistent emulation state.
    audio_track_index: Option<usize>,
    audio_end_behavior: CdAudioEndBehavior,
    audio_tick_accumulator: u64,
    audio_left_sample: i16,
    audio_right_sample: i16,
    audio_sample_latch: i16,
    audio_sample_latch_right: bool,
    audio_sample_latch_clock: u64,
    audio_sample_latch_last_clock: u64,
    audio_source_frames: VecDeque<[f32; 2]>,
    audio_resample_position: f64,
    audio_sample_rate: u32,
    audio_sample_generation_enabled: bool,
    adpcm_address_latch: u16,
    adpcm_read_address: u16,
    adpcm_write_address: u16,
    adpcm_read_buffer: u8,
    adpcm_dma_control: u8,
    adpcm_address_control: u8,
    adpcm_playback_rate: u8,
    adpcm_fade_control: u8,
    audio_fade_target: Option<CdFadeTarget>,
    audio_fade_level_q16: u32,
    audio_fade_step_ticks: u64,
    audio_fade_ticks_to_next: u64,
    adpcm_length: u32,
    adpcm_playing: bool,
    adpcm_stop_pending: bool,
    adpcm_high_nibble_next: bool,
    adpcm_clock_accumulator: u64,
    adpcm_predictor: u16,
    adpcm_step_index: u8,
    adpcm_end_irq: bool,
    adpcm_half_irq: bool,
    adpcm_resampler: audio::MonoBlipResampler,
    adpcm_audio_samples: Vec<i16>,
    debug_adpcm_dma_completed: bool,
}

impl CdRom2 {
    pub fn new(disc: CdDisc) -> Self {
        Self::with_super_system_card(disc, false)
    }

    pub(crate) fn with_super_system_card(disc: CdDisc, super_system_card: bool) -> Self {
        Self {
            disc,
            super_system_card,
            work_ram: Box::new([0; CDROM2_WORK_RAM_LEN]),
            bram: formatted_bram(),
            adpcm_ram: Box::new([0; CDROM2_ADPCM_RAM_LEN]),
            bram_unlocked: false,
            reset_asserted: false,
            phase: CdScsiPhase::BusFree,
            request: false,
            acknowledge: false,
            auto_acknowledge: false,
            acknowledged_request: false,
            request_pending: false,
            data_ready_irq_enabled: false,
            status_irq_enabled: false,
            audio_end_irq_enabled: false,
            audio_half_irq_enabled: false,
            output_latch: 0,
            command: Vec::with_capacity(10),
            command_trace: VecDeque::with_capacity(CD_COMMAND_TRACE_CAPACITY),
            response: Vec::new(),
            response_index: 0,
            response_available: 0,
            status: 0,
            sense_key: 0,
            additional_sense_code: 0,
            event: None,
            sector_arrival_ticks: None,
            sectors_pending: 0,
            sector_tick_remainder: 0,
            audio_status: CdAudioStatus::Inactive,
            audio_start_lba: 0,
            audio_end_lba: 0,
            audio_current_lba: 0,
            audio_current_sample: 0,
            audio_track_index: None,
            audio_end_behavior: CdAudioEndBehavior::Stop,
            audio_tick_accumulator: 0,
            audio_left_sample: 0,
            audio_right_sample: 0,
            audio_sample_latch: 0,
            audio_sample_latch_right: false,
            audio_sample_latch_clock: 0,
            audio_sample_latch_last_clock: 0,
            audio_source_frames: VecDeque::new(),
            audio_resample_position: 0.0,
            audio_sample_rate: 44_100,
            audio_sample_generation_enabled: true,
            adpcm_address_latch: 0,
            adpcm_read_address: 0,
            adpcm_write_address: 0,
            adpcm_read_buffer: 0,
            adpcm_dma_control: 0,
            adpcm_address_control: 0,
            adpcm_playback_rate: 0x0F,
            adpcm_fade_control: 0,
            audio_fade_target: None,
            audio_fade_level_q16: 0x1_0000,
            audio_fade_step_ticks: 0,
            audio_fade_ticks_to_next: 0,
            adpcm_length: 0,
            adpcm_playing: false,
            adpcm_stop_pending: false,
            adpcm_high_nibble_next: true,
            adpcm_clock_accumulator: 0,
            adpcm_predictor: audio::ADPCM_RESET_PREDICTOR,
            adpcm_step_index: 0,
            adpcm_end_irq: false,
            adpcm_half_irq: false,
            adpcm_resampler: audio::MonoBlipResampler::new(44_100),
            adpcm_audio_samples: Vec::new(),
            debug_adpcm_dma_completed: false,
        }
    }

    pub fn reset(&mut self) {
        self.work_ram.fill(0);
        self.bram_unlocked = false;
        self.reset_asserted = false;
        self.data_ready_irq_enabled = false;
        self.status_irq_enabled = false;
        self.audio_end_irq_enabled = false;
        self.audio_half_irq_enabled = false;
        self.sector_tick_remainder = 0;
        self.stop_audio(CdAudioStatus::Inactive);
        self.audio_tick_accumulator = 0;
        self.audio_sample_latch = 0;
        self.audio_sample_latch_right = false;
        self.audio_sample_latch_clock = 0;
        self.audio_sample_latch_last_clock = 0;
        self.audio_source_frames.clear();
        self.audio_resample_position = 0.0;
        self.adpcm_address_latch = 0;
        self.adpcm_read_address = 0;
        self.adpcm_write_address = 0;
        self.adpcm_read_buffer = 0;
        self.adpcm_dma_control = 0;
        self.adpcm_address_control = 0;
        self.adpcm_playback_rate = 0x0F;
        self.adpcm_fade_control = 0;
        self.audio_fade_target = None;
        self.audio_fade_level_q16 = 0x1_0000;
        self.audio_fade_step_ticks = 0;
        self.audio_fade_ticks_to_next = 0;
        self.adpcm_length = 0;
        self.adpcm_playing = false;
        self.adpcm_stop_pending = false;
        self.adpcm_high_nibble_next = true;
        self.adpcm_clock_accumulator = 0;
        self.adpcm_predictor = audio::ADPCM_RESET_PREDICTOR;
        self.adpcm_step_index = 0;
        self.adpcm_end_irq = false;
        self.adpcm_half_irq = false;
        self.adpcm_resampler = audio::MonoBlipResampler::new(self.audio_sample_rate);
        self.adpcm_audio_samples.clear();
        self.debug_adpcm_dma_completed = false;
        self.command_trace.clear();
        self.enter_bus_free();
    }

    pub(crate) fn take_debug_dma_completed(&mut self) -> bool {
        std::mem::take(&mut self.debug_adpcm_dma_completed)
    }

    #[inline]
    pub const fn phase(&self) -> CdScsiPhase {
        self.phase
    }

    #[inline]
    pub const fn bram_unlocked(&self) -> bool {
        self.bram_unlocked
    }

    #[inline]
    pub const fn audio_status(&self) -> CdAudioStatus {
        self.audio_status
    }

    #[inline]
    pub const fn disc(&self) -> &CdDisc {
        &self.disc
    }

    #[inline]
    pub fn command_trace(&self) -> &VecDeque<CdCommandTrace> {
        &self.command_trace
    }

    pub fn debug_snapshot(&self) -> CdRom2DebugSnapshot {
        CdRom2DebugSnapshot {
            phase: self.phase,
            bus_status: self.bus_status(),
            request: self.request,
            acknowledge: self.acknowledge,
            auto_acknowledge: self.auto_acknowledge,
            acknowledged_request: self.acknowledged_request,
            request_pending: self.request_pending,
            reset_asserted: self.reset_asserted,
            output_latch: self.output_latch,
            current_input_data: self.current_input_data(),
            data_register: self.data_register(),
            command: self.command.clone(),
            response: self.response.clone(),
            response_index: self.response_index,
            response_available: self.response_available,
            status: self.status,
            sense_key: self.sense_key,
            additional_sense_code: self.additional_sense_code,
            pending_event: self.event.map(|(event, ticks_remaining)| {
                CdProtocolEventDebugSnapshot {
                    kind: CdProtocolEventDebugKind::from_event(event),
                    ticks_remaining,
                }
            }),
            sector_arrival_ticks: self.sector_arrival_ticks,
            sectors_pending: self.sectors_pending,
            sector_tick_remainder: self.sector_tick_remainder,
            data_ready_irq_enabled: self.data_ready_irq_enabled,
            status_irq_enabled: self.status_irq_enabled,
            audio_end_irq_enabled: self.audio_end_irq_enabled,
            audio_half_irq_enabled: self.audio_half_irq_enabled,
            data_ready_condition: self.data_ready_condition(),
            status_condition: self.status_condition(),
            irq2_asserted: self.irq2_level() == LineLevel::Low,
            bram_unlocked: self.bram_unlocked,
            recent_commands: self.command_trace.iter().copied().collect(),
            audio: self.audio_debug_snapshot(),
            audio_left_sample: self.audio_left_sample,
            audio_right_sample: self.audio_right_sample,
            audio_sample_latch: self.audio_sample_latch,
            audio_sample_latch_right: self.audio_sample_latch_right,
            audio_sample_rate: self.audio_sample_rate,
            audio_sample_generation_enabled: self.audio_sample_generation_enabled,
            adpcm_address_latch: self.adpcm_address_latch,
            adpcm_read_address: self.adpcm_read_address,
            adpcm_write_address: self.adpcm_write_address,
            adpcm_read_buffer: self.adpcm_read_buffer,
            adpcm_dma_control: self.adpcm_dma_control,
            adpcm_address_control: self.adpcm_address_control,
            adpcm_playback_rate: self.adpcm_playback_rate,
            adpcm_length: self.adpcm_length,
            adpcm_playing: self.adpcm_playing,
            adpcm_stop_pending: self.adpcm_stop_pending,
            adpcm_high_nibble_next: self.adpcm_high_nibble_next,
            adpcm_clock_accumulator: self.adpcm_clock_accumulator,
            adpcm_predictor: self.adpcm_predictor,
            adpcm_step_index: self.adpcm_step_index,
            adpcm_end_irq: self.adpcm_end_irq,
            adpcm_half_irq: self.adpcm_half_irq,
            adpcm_buffered_samples: self.adpcm_audio_samples.len(),
        }
    }

    pub fn audio_debug_snapshot(&self) -> CdAudioDebugSnapshot {
        CdAudioDebugSnapshot {
            status: self.audio_status,
            start_lba: self.audio_start_lba,
            end_lba: self.audio_end_lba,
            current_lba: self.audio_current_lba,
            current_sample: self.audio_current_sample,
            end_mode: match self.audio_end_behavior {
                CdAudioEndBehavior::Stop => CdAudioEndMode::Stop,
                CdAudioEndBehavior::Loop => CdAudioEndMode::Loop,
                CdAudioEndBehavior::SignalCompletion => CdAudioEndMode::SignalCompletion,
            },
            tick_accumulator: self.audio_tick_accumulator,
            queued_source_frames: self.audio_source_frames.len(),
            fade_control: self.adpcm_fade_control,
            fade_target: self.audio_fade_target.map(|target| match target {
                CdFadeTarget::Cdda => CdAudioFadeTarget::Cdda,
                CdFadeTarget::Adpcm => CdAudioFadeTarget::Adpcm,
            }),
            fade_level_q16: self.audio_fade_level_q16,
            fade_step_ticks: self.audio_fade_step_ticks,
            fade_ticks_to_next: self.audio_fade_ticks_to_next,
        }
    }

    #[cfg(test)]
    pub(super) fn audio_transport_for_test(&self) -> (u32, u32, u32, usize, u8) {
        let behavior = match self.audio_end_behavior {
            CdAudioEndBehavior::Stop => 0,
            CdAudioEndBehavior::Loop => 1,
            CdAudioEndBehavior::SignalCompletion => 2,
        };
        (
            self.audio_start_lba,
            self.audio_end_lba,
            self.audio_current_lba,
            self.audio_current_sample,
            behavior,
        )
    }

    #[inline]
    pub fn work_ram(&self) -> &[u8; CDROM2_WORK_RAM_LEN] {
        &self.work_ram
    }

    #[inline]
    pub fn bram(&self) -> &[u8; CDROM2_BRAM_LEN] {
        &self.bram
    }

    #[inline]
    pub fn bram_mut(&mut self) -> &mut [u8; CDROM2_BRAM_LEN] {
        &mut self.bram
    }

    #[inline]
    pub fn adpcm_ram(&self) -> &[u8; CDROM2_ADPCM_RAM_LEN] {
        &self.adpcm_ram
    }

    #[inline]
    pub fn irq2_level(&self) -> LineLevel {
        if (self.data_ready_irq_enabled && self.data_ready_condition())
            || (self.status_irq_enabled && self.status_condition())
            || (self.audio_end_irq_enabled && self.adpcm_end_irq)
            || (self.audio_half_irq_enabled && self.adpcm_half_irq)
        {
            LineLevel::Low
        } else {
            LineLevel::High
        }
    }

    pub fn peek_physical(&self, physical_addr: u32) -> Option<u8> {
        match physical_addr {
            CDROM2_WORK_RAM_START..=CDROM2_WORK_RAM_END => {
                Some(self.work_ram[(physical_addr - CDROM2_WORK_RAM_START) as usize])
            }
            CDROM2_BRAM_START..=CDROM2_BRAM_END => Some(if self.bram_unlocked {
                self.bram[(physical_addr - CDROM2_BRAM_START) as usize]
            } else {
                OPEN_BUS_VALUE
            }),
            CDROM2_REGISTER_START..=CDROM2_REGISTER_END => {
                Some(self.peek_register((physical_addr - CDROM2_REGISTER_START) as u8))
            }
            SUPER_SYSTEM_CARD_ID_START..=SUPER_SYSTEM_CARD_ID_END => {
                Some(self.system_card_id(physical_addr))
            }
            _ => None,
        }
    }

    pub fn read_physical(&mut self, physical_addr: u32) -> Option<u8> {
        match physical_addr {
            CDROM2_WORK_RAM_START..=CDROM2_WORK_RAM_END => {
                Some(self.work_ram[(physical_addr - CDROM2_WORK_RAM_START) as usize])
            }
            CDROM2_BRAM_START..=CDROM2_BRAM_END => Some(if self.bram_unlocked {
                self.bram[(physical_addr - CDROM2_BRAM_START) as usize]
            } else {
                OPEN_BUS_VALUE
            }),
            CDROM2_REGISTER_START..=CDROM2_REGISTER_END => {
                Some(self.read_register((physical_addr - CDROM2_REGISTER_START) as u8))
            }
            SUPER_SYSTEM_CARD_ID_START..=SUPER_SYSTEM_CARD_ID_END => {
                Some(self.system_card_id(physical_addr))
            }
            _ => None,
        }
    }

    pub fn write_physical(&mut self, physical_addr: u32, value: u8) -> bool {
        match physical_addr {
            CDROM2_WORK_RAM_START..=CDROM2_WORK_RAM_END => {
                self.work_ram[(physical_addr - CDROM2_WORK_RAM_START) as usize] = value;
                true
            }
            CDROM2_BRAM_START..=CDROM2_BRAM_END => {
                if self.bram_unlocked {
                    self.bram[(physical_addr - CDROM2_BRAM_START) as usize] = value;
                }
                true
            }
            CDROM2_REGISTER_START..=CDROM2_REGISTER_END => {
                self.write_register((physical_addr - CDROM2_REGISTER_START) as u8, value);
                true
            }
            _ => false,
        }
    }

    pub fn advance_master_ticks(&mut self, mut ticks: u64) {
        loop {
            if self.event.is_some_and(|(_, remaining)| remaining == 0) {
                let (event, _) = self.event.take().unwrap();
                self.handle_event(event);
                continue;
            }
            if self.sector_arrival_ticks == Some(0) {
                self.sector_arrival_ticks = None;
                self.handle_sector_arrival();
                continue;
            }
            if ticks == 0 {
                return;
            }

            let protocol_ticks = self.event.map(|(_, remaining)| remaining);
            let next = match (protocol_ticks, self.sector_arrival_ticks) {
                (Some(protocol), Some(sector)) => protocol.min(sector),
                (Some(protocol), None) => protocol,
                (None, Some(sector)) => sector,
                (None, None) => ticks,
            };
            let step = ticks.min(next);
            self.advance_audio(step);
            ticks -= step;
            if let Some((event, remaining)) = self.event {
                self.event = Some((event, remaining - step));
            }
            if let Some(remaining) = self.sector_arrival_ticks {
                self.sector_arrival_ticks = Some(remaining - step);
            }
        }
    }

    #[inline]
    fn system_card_id(&self, physical_addr: u32) -> u8 {
        if self.super_system_card {
            [0x00, 0xAA, 0x55, 0x03][(physical_addr - SUPER_SYSTEM_CARD_ID_START) as usize]
        } else {
            OPEN_BUS_VALUE
        }
    }

    fn peek_register(&self, offset: u8) -> u8 {
        match offset {
            0 => self.bus_status(),
            1 => self.data_register(),
            2 => {
                u8::from(self.acknowledge) << 7
                    | u8::from(self.data_ready_irq_enabled) << 6
                    | u8::from(self.status_irq_enabled) << 5
                    | u8::from(self.audio_end_irq_enabled) << 3
                    | u8::from(self.audio_half_irq_enabled) << 2
            }
            3 => {
                u8::from(self.data_ready_condition()) << 6
                    | u8::from(self.status_condition()) << 5
                    | u8::from(self.adpcm_end_irq) << 3
                    | u8::from(self.adpcm_half_irq) << 2
            }
            4 => u8::from(self.reset_asserted) << 1,
            5 => self.audio_sample_latch.to_le_bytes()[0],
            6 => self.audio_sample_latch.to_le_bytes()[1],
            7 => u8::from(self.bram_unlocked) << 7,
            8 => self.current_input_data(),
            9 => self.adpcm_address_latch.to_le_bytes()[1],
            10 => self.adpcm_read_buffer,
            11 => self.adpcm_dma_control,
            12 => {
                u8::from(self.adpcm_playing) << 3
                    | u8::from(!self.adpcm_playing) << 1
                    | u8::from(self.adpcm_end_irq)
            }
            13 => self.adpcm_address_control,
            14 => self.adpcm_playback_rate,
            15 => self.adpcm_fade_control,
            _ => 0,
        }
    }

    fn read_register(&mut self, offset: u8) -> u8 {
        let value = self.peek_register(offset);
        match offset {
            3 => self.bram_unlocked = false,
            8 if self.phase == CdScsiPhase::DataIn && self.request => {
                self.acknowledge = true;
                self.auto_acknowledge = true;
                self.request = false;
                self.schedule(CdEvent::CompleteAutoAck, PROVISIONAL_CDROM2_AUTO_ACK_TICKS);
            }
            10 => {
                let value = self.adpcm_read_buffer;
                self.adpcm_read_buffer = self.adpcm_ram[usize::from(self.adpcm_read_address)];
                self.adpcm_read_address = self.adpcm_read_address.wrapping_add(1);
                return value;
            }
            _ => {}
        }
        value
    }

    fn write_register(&mut self, offset: u8, value: u8) {
        match offset {
            // D9 modes 1/2 keep the bus busy during playback.
            0 if !self.reset_asserted
                && matches!(self.phase, CdScsiPhase::BusFree | CdScsiPhase::Busy) =>
            {
                self.phase = CdScsiPhase::Selection;
                self.schedule(CdEvent::EnterCommand, PROVISIONAL_CDROM2_SELECTION_TICKS);
            }
            1 => self.output_latch = value,
            2 => self.write_control(value),
            3 => {}
            4 => {
                self.reset_asserted = value & 2 != 0;
                if self.reset_asserted {
                    self.enter_bus_free();
                }
            }
            5 => self.latch_cdda_sample(),
            6 => {}
            7 => self.bram_unlocked = value & 0x80 != 0,
            8 => {
                self.adpcm_address_latch = (self.adpcm_address_latch & 0xFF00) | u16::from(value);
                self.reload_adpcm_length_if_held();
            }
            9 => {
                self.adpcm_address_latch =
                    (self.adpcm_address_latch & 0x00FF) | (u16::from(value) << 8);
                self.reload_adpcm_length_if_held();
            }
            10 => {
                self.complete_adpcm_write(value);
            }
            11 => {
                self.adpcm_dma_control = value & 3;
                self.service_adpcm_dma();
            }
            12 => {}
            13 => self.write_adpcm_address_control(value),
            14 => self.write_adpcm_playback_rate(value),
            15 => self.write_audio_fade_control(value),
            _ => {}
        }
    }
}
