pub(crate) mod audio;
mod image;
mod state_io;

use crate::hardware::cartridge::{Mapper, Mirroring};
use audio::FdsAudio;
pub use image::{FDS_HEADER_SIZE, FDS_SIDE_SIZE, FdsImage, FdsImageError};
use zeff_emu_common::media::{
    MediaEvent, MediaEventError, MediaObjectId, MediaSlotId, MediaSlotSnapshot, MediaSlotState,
};

pub const FDS_BIOS_SIZE: usize = 0x2000;
pub const FDS_DRIVE_SLOT_ID: &str = "fds.drive0";
const PRG_RAM_SIZE: usize = 0x8000;
const CHR_RAM_SIZE: usize = 0x2000;
const FDS_DISK_BIT_PERIOD_CPU_CYCLES: u32 = 24;
const FDS_DISK_BYTE_PERIOD_CPU_CYCLES: u16 = 192;
const FDS_LEAD_IN_GAP_BITS: u32 = 28_300;
const FDS_LEAD_IN_GAP_CPU_CYCLES: u32 = FDS_LEAD_IN_GAP_BITS * FDS_DISK_BIT_PERIOD_CPU_CYCLES;
const FDS_INTER_BLOCK_GAP_BITS: u32 = 976;
const FDS_INTER_BLOCK_GAP_CPU_CYCLES: u32 =
    FDS_INTER_BLOCK_GAP_BITS * FDS_DISK_BIT_PERIOD_CPU_CYCLES;
const FDS_DISK_INFO_BLOCK_LEN: usize = 56;
const FDS_FILE_AMOUNT_BLOCK_LEN: usize = 2;
const FDS_FILE_HEADER_BLOCK_LEN: usize = 16;
const FDS_DISK_INFO_BLOCK_CODE: u8 = 0x01;
const FDS_FILE_AMOUNT_BLOCK_CODE: u8 = 0x02;
const FDS_FILE_HEADER_BLOCK_CODE: u8 = 0x03;
const FDS_FILE_DATA_BLOCK_CODE: u8 = 0x04;
const FDS_SYNTHETIC_CRC_BYTE_COUNT: u8 = 2;
const FDS_SIDE_CHANGE_EJECT_CPU_CYCLES: u32 =
    (crate::hardware::constants::CPU_CYCLES_PER_FRAME as u32) * 12;
const FDS_SIDE_CHANGE_SETTLE_CPU_CYCLES: u32 =
    (crate::hardware::constants::CPU_CYCLES_PER_FRAME as u32) * 12;
const FDS_SIDE_CHANGE_TOTAL_CPU_CYCLES: u32 =
    FDS_SIDE_CHANGE_EJECT_CPU_CYCLES + FDS_SIDE_CHANGE_SETTLE_CPU_CYCLES;
const FDS_DRIVE_STATE_MARKER_V1: [u8; 8] = *b"FDSDRV1!";
const FDS_DRIVE_STATE_MARKER: [u8; 8] = *b"FDSDRV2!";
const FDS_SAVE_MAGIC: [u8; 8] = *b"ZBFDSSAV";
const FDS_SAVE_VERSION_V1: u32 = 1;
const FDS_SAVE_VERSION_V2: u32 = 2;
const FDS_SAVE_VERSION: u32 = 3;
const MAX_MEDIA_ID_LEN: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FdsDiskBlockPlan {
    end_position: usize,
    pending_file_data_len: Option<u16>,
}

pub struct Fds {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_ram: [u8; CHR_RAM_SIZE],
    mirroring: Mirroring,
    disk_image: Option<FdsImage>,
    source_disk_image: Option<FdsImage>,
    source_media_id: Option<MediaObjectId>,
    media_slot: MediaSlotState,

    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_repeat: bool,
    irq_pending: bool,

    io_enabled: bool,
    disk_reg: u8,
    disk_control: u8,
    disk_data: u8,
    disk_position: usize,
    disk_lead_in_counter: u32,
    disk_inter_block_gap_counter: u32,
    disk_next_gap_position: Option<usize>,
    disk_pending_file_data_len: Option<u16>,
    disk_synthetic_crc_bytes_remaining: u8,
    disk_crc_accumulator: u16,
    disk_write_in_gap: bool,
    disk_cycle_counter: u16,
    disk_transfer_flag: bool,
    disk_irq_pending: bool,
    disk_end_of_head: bool,
    disk_media_change_counter: u32,

    audio: FdsAudio,
}

impl Fds {
    pub fn new(prg_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_SIZE],
            chr_ram: [0; CHR_RAM_SIZE],
            mirroring,
            disk_image: None,
            source_disk_image: None,
            source_media_id: None,
            media_slot: MediaSlotState::empty(FDS_DRIVE_SLOT_ID),
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_repeat: false,
            irq_pending: false,
            io_enabled: false,
            disk_reg: 0,
            disk_control: 0,
            disk_data: 0,
            disk_position: 0,
            disk_lead_in_counter: FDS_LEAD_IN_GAP_CPU_CYCLES,
            disk_inter_block_gap_counter: 0,
            disk_next_gap_position: None,
            disk_pending_file_data_len: None,
            disk_synthetic_crc_bytes_remaining: 0,
            disk_crc_accumulator: 0,
            disk_write_in_gap: true,
            disk_cycle_counter: FDS_DISK_BYTE_PERIOD_CPU_CYCLES,
            disk_transfer_flag: false,
            disk_irq_pending: false,
            disk_end_of_head: false,
            disk_media_change_counter: 0,
            audio: FdsAudio::new(),
        }
    }

    pub fn with_disk_image(prg_rom: Vec<u8>, disk_image: FdsImage, mirroring: Mirroring) -> Self {
        let media_id = disk_image.media_object_id();
        let mut fds = Self::new(prg_rom, mirroring);
        fds.media_slot = inserted_fds_media_slot(media_id.clone());
        fds.source_media_id = Some(media_id);
        fds.source_disk_image = Some(disk_image.clone());
        fds.disk_image = Some(disk_image);
        fds
    }

    pub fn disk_image(&self) -> Option<&FdsImage> {
        self.disk_image.as_ref()
    }

    pub fn media_slot_state(&self) -> &MediaSlotState {
        &self.media_slot
    }

    pub fn media_slot_snapshot(&self) -> MediaSlotSnapshot {
        MediaSlotSnapshot {
            state: self.media_slot.clone(),
            source_media_id: self.source_media_id(),
            side_count: self
                .disk_image
                .as_ref()
                .map_or(0, FdsImage::side_count)
                .try_into()
                .unwrap_or(u8::MAX),
        }
    }

    pub fn apply_media_event(&mut self, event: &MediaEvent) -> anyhow::Result<()> {
        if event.slot().as_ref() != FDS_DRIVE_SLOT_ID {
            return Err(media_event_error_to_anyhow(MediaEventError::WrongSlot {
                expected: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
                actual: event.slot().clone(),
            }));
        }

        match event {
            MediaEvent::SelectSide { side, .. } => self.select_side(*side),
            MediaEvent::Eject { .. } => {
                let mutation_counter = self.media_slot.mutation_counter;
                self.media_slot
                    .apply_event(event)
                    .map_err(media_event_error_to_anyhow)?;
                self.media_slot.mutation_counter = mutation_counter;
                self.reset_disk_scan();
                self.disk_media_change_counter = 0;
                Ok(())
            }
            MediaEvent::Insert {
                media_id,
                side,
                write_protected,
                ..
            } => {
                let expected_media_id = self.source_media_id().ok_or_else(|| {
                    anyhow::anyhow!("FDS drive has no source media available for insertion")
                })?;
                if media_id != &expected_media_id {
                    anyhow::bail!(
                        "FDS insert event references different media (expected {})",
                        expected_media_id.as_ref()
                    );
                }
                let side = side
                    .ok_or_else(|| anyhow::anyhow!("FDS insert event must select a disk side"))?;
                self.validate_side(side)?;

                let mutation_counter = self.media_slot.mutation_counter;
                self.media_slot
                    .apply_event(&MediaEvent::Insert {
                        slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
                        media_id: expected_media_id,
                        side: Some(side),
                        write_protected: *write_protected,
                    })
                    .map_err(media_event_error_to_anyhow)?;
                self.media_slot.mutation_counter = mutation_counter;
                self.reset_disk_scan();
                self.disk_media_change_counter = FDS_SIDE_CHANGE_TOTAL_CPU_CYCLES;
                Ok(())
            }
            MediaEvent::SetWriteProtected { .. } => self
                .media_slot
                .apply_event(event)
                .map_err(media_event_error_to_anyhow),
        }
    }

    pub fn select_side(&mut self, side: u8) -> anyhow::Result<()> {
        self.validate_side(side)?;
        if !self.media_slot.inserted() {
            anyhow::bail!("no FDS media inserted in {FDS_DRIVE_SLOT_ID}");
        }

        self.media_slot
            .apply_event(&MediaEvent::SelectSide {
                slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
                side,
            })
            .map_err(media_event_error_to_anyhow)?;
        self.reset_disk_scan();
        self.disk_media_change_counter = FDS_SIDE_CHANGE_TOTAL_CPU_CYCLES;
        Ok(())
    }

    fn validate_side(&self, side: u8) -> anyhow::Result<()> {
        let side_count = self.disk_image.as_ref().map_or(0, FdsImage::side_count);
        if usize::from(side) >= side_count {
            anyhow::bail!(
                "FDS side {} is not present; disk has {} side(s)",
                side_label(side),
                side_count
            );
        }

        Ok(())
    }

    fn source_media_id(&self) -> Option<MediaObjectId> {
        self.source_media_id.clone()
    }

    pub fn selected_side(&self) -> Option<u8> {
        self.media_slot.side
    }

    fn disk_status(&self) -> u8 {
        let mut status = 0;
        if self.irq_pending {
            status |= 0x01;
        }
        if self.mirroring == Mirroring::Horizontal {
            status |= 0x08;
        }
        if self.disk_end_of_head {
            status |= 0x40;
        }
        if self.disk_transfer_flag {
            status |= 0x80;
        }
        status
    }

    fn drive_status(&self) -> u8 {
        let inserted = self.disk_image.is_some()
            && self.media_slot.media_id.is_some()
            && self.media_slot.side.is_some();
        if !inserted {
            return 0x07;
        }

        if self.disk_media_change_counter > FDS_SIDE_CHANGE_SETTLE_CPU_CYCLES {
            return 0x07;
        }

        let mut status = 0;
        if self.disk_media_change_counter > 0 || self.disk_end_of_head {
            status |= 0x02;
        }
        if self.media_slot.write_protected {
            status |= 0x04;
        }
        status
    }

    fn acknowledge_disk_transfer(&mut self) {
        self.disk_irq_pending = false;
        self.disk_transfer_flag = false;
    }

    fn acknowledge_disk_irq(&mut self) {
        self.disk_irq_pending = false;
    }

    fn disk_motor_running(&self) -> bool {
        self.io_enabled && self.disk_control & 0x02 == 0
    }

    fn disk_scanning(&self) -> bool {
        self.disk_control & 0x01 != 0
    }

    fn disk_read_mode(&self) -> bool {
        self.disk_control & 0x04 != 0
    }

    fn disk_crc_control(&self) -> bool {
        self.disk_control & 0x10 != 0
    }

    fn disk_ready(&self) -> bool {
        self.disk_control & 0x40 != 0
    }

    fn disk_byte_irq_enabled(&self) -> bool {
        self.disk_control & 0x80 != 0
    }

    fn active_side(&self) -> Option<&[u8]> {
        let side = self.media_slot.side?;
        self.disk_image.as_ref()?.side(usize::from(side))
    }

    fn active_side_mut(&mut self) -> Option<&mut [u8]> {
        let side = self.media_slot.side?;
        self.disk_image.as_mut()?.side_mut(usize::from(side))
    }

    fn reset_disk_scan(&mut self) {
        self.disk_position = 0;
        self.disk_lead_in_counter = FDS_LEAD_IN_GAP_CPU_CYCLES;
        self.disk_inter_block_gap_counter = 0;
        self.disk_next_gap_position = None;
        self.disk_pending_file_data_len = None;
        self.disk_synthetic_crc_bytes_remaining = 0;
        self.disk_crc_accumulator = 0;
        self.disk_write_in_gap = true;
        self.disk_cycle_counter = FDS_DISK_BYTE_PERIOD_CPU_CYCLES;
        self.disk_end_of_head = false;
        self.acknowledge_disk_transfer();
    }

    fn update_disk_control(&mut self, val: u8) {
        let was_scanning = self.disk_scanning();
        self.disk_control = val;
        self.mirroring = if val & 0x08 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
        self.acknowledge_disk_irq();

        if !self.disk_scanning() {
            self.reset_disk_scan();
        } else if !was_scanning {
            self.disk_lead_in_counter = FDS_LEAD_IN_GAP_CPU_CYCLES;
            self.disk_inter_block_gap_counter = 0;
            self.disk_next_gap_position = None;
            self.disk_pending_file_data_len = None;
            self.disk_synthetic_crc_bytes_remaining = 0;
            self.disk_crc_accumulator = 0;
            self.disk_write_in_gap = true;
            self.disk_cycle_counter = FDS_DISK_BYTE_PERIOD_CPU_CYCLES;
            self.disk_end_of_head = false;
        }
    }

    fn schedule_current_disk_block_gap(&mut self) {
        if self.disk_next_gap_position.is_some() {
            return;
        }

        let Some(plan) = self.active_side().and_then(|side| {
            fds_disk_block_plan(side, self.disk_position, self.disk_pending_file_data_len)
        }) else {
            return;
        };

        self.disk_next_gap_position = Some(plan.end_position);
        self.disk_pending_file_data_len = plan.pending_file_data_len;
    }

    fn clock_disk_drive(&mut self) {
        if !self.disk_motor_running()
            || !self.disk_scanning()
            || self.disk_media_change_counter > 0
            || self.disk_end_of_head
        {
            return;
        }

        if self.disk_lead_in_counter > 0 {
            self.disk_lead_in_counter -= 1;
            return;
        }

        if self.disk_read_mode() && self.disk_inter_block_gap_counter > 0 {
            self.disk_inter_block_gap_counter -= 1;
            return;
        }

        if self.disk_cycle_counter > 0 {
            self.disk_cycle_counter -= 1;
            if self.disk_cycle_counter > 0 {
                return;
            }
        }
        self.disk_cycle_counter = FDS_DISK_BYTE_PERIOD_CPU_CYCLES;

        if self.disk_read_mode() && self.disk_next_gap_position == Some(self.disk_position) {
            self.disk_next_gap_position = None;
            self.disk_synthetic_crc_bytes_remaining = FDS_SYNTHETIC_CRC_BYTE_COUNT;
        }

        if self.disk_read_mode() && self.disk_synthetic_crc_bytes_remaining > 0 {
            let crc_byte = self.disk_crc_accumulator as u8;
            self.disk_crc_accumulator >>= 8;
            self.disk_synthetic_crc_bytes_remaining -= 1;
            if self.disk_synthetic_crc_bytes_remaining == 0 {
                self.disk_inter_block_gap_counter = FDS_INTER_BLOCK_GAP_CPU_CYCLES;
                self.disk_crc_accumulator = 0;
            }
            if self.disk_read_mode() {
                self.disk_data = crc_byte;
                self.signal_disk_byte_transfer();
            }
            return;
        }

        if !self.disk_read_mode() {
            self.clock_disk_write();
            return;
        }

        self.schedule_current_disk_block_gap();

        let Some((disk_data, side_len)) = self
            .active_side()
            .map(|side| (side.get(self.disk_position).copied(), side.len()))
        else {
            return;
        };
        let Some(disk_data) = disk_data else {
            self.disk_end_of_head = true;
            return;
        };

        self.disk_data = disk_data;
        self.update_disk_crc(disk_data);
        self.disk_position += 1;
        self.signal_disk_byte_transfer();
        if self.disk_position >= side_len {
            self.disk_end_of_head = true;
        }
    }

    fn clock_disk_write(&mut self) {
        let Some(side_len) = self.active_side().map(<[u8]>::len) else {
            return;
        };
        if self.disk_position >= side_len {
            self.disk_end_of_head = true;
            return;
        }

        if self.disk_crc_control() {
            self.disk_crc_accumulator >>= 8;
            self.disk_write_in_gap = true;
            return;
        }

        let position = self.disk_position;
        let value = if self.disk_ready() { self.disk_reg } else { 0 };
        if !self.disk_ready() {
            self.disk_crc_accumulator = 0;
            self.disk_write_in_gap = true;
            self.signal_disk_byte_transfer();
            return;
        }
        if self.disk_write_in_gap && matches!(value, 0x00 | 0x80) {
            if value == 0x80 {
                self.disk_write_in_gap = false;
            }
            self.signal_disk_byte_transfer();
            return;
        }
        self.disk_write_in_gap = false;
        self.update_disk_crc(value);
        let changed = if self.media_slot.write_protected {
            false
        } else {
            self.active_side_mut()
                .and_then(|side| side.get_mut(position))
                .is_some_and(|byte| {
                    if *byte == value {
                        false
                    } else {
                        *byte = value;
                        true
                    }
                })
        };
        if changed {
            self.media_slot
                .record_mutation()
                .expect("active writable FDS media should accept a mutation");
        }
        self.disk_position += 1;

        self.signal_disk_byte_transfer();
        if self.disk_position >= side_len {
            self.disk_end_of_head = true;
        }
    }

    fn update_disk_crc(&mut self, value: u8) {
        self.disk_crc_accumulator ^= u16::from(value);
        for _ in 0..8 {
            let carry = self.disk_crc_accumulator & 1;
            self.disk_crc_accumulator >>= 1;
            if carry != 0 {
                self.disk_crc_accumulator ^= 0x8408;
            }
        }
    }

    fn signal_disk_byte_transfer(&mut self) {
        self.disk_transfer_flag = true;
        if self.disk_byte_irq_enabled() {
            self.disk_irq_pending = true;
        }
    }
}

fn fds_disk_block_plan(
    side: &[u8],
    position: usize,
    pending_file_data_len: Option<u16>,
) -> Option<FdsDiskBlockPlan> {
    let block_code = *side.get(position)?;
    let mut next_pending_file_data_len = pending_file_data_len;

    let block_len = match block_code {
        FDS_DISK_INFO_BLOCK_CODE => FDS_DISK_INFO_BLOCK_LEN,
        FDS_FILE_AMOUNT_BLOCK_CODE => FDS_FILE_AMOUNT_BLOCK_LEN,
        FDS_FILE_HEADER_BLOCK_CODE => {
            let file_size =
                u16::from_le_bytes([*side.get(position + 13)?, *side.get(position + 14)?]);
            next_pending_file_data_len = Some(file_size);
            FDS_FILE_HEADER_BLOCK_LEN
        }
        FDS_FILE_DATA_BLOCK_CODE => {
            let file_size = pending_file_data_len?;
            next_pending_file_data_len = None;
            usize::from(file_size) + 1
        }
        _ => return None,
    };

    let end_position = position.checked_add(block_len)?.min(side.len());
    if end_position <= position {
        return None;
    }

    Some(FdsDiskBlockPlan {
        end_position,
        pending_file_data_len: next_pending_file_data_len,
    })
}

fn inserted_fds_media_slot(media_id: MediaObjectId) -> MediaSlotState {
    MediaSlotState {
        slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
        media_id: Some(media_id),
        side: Some(0),
        write_protected: false,
        mutation_counter: 0,
    }
}

impl Mapper for Fds {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x4040..=0x407F => self.audio.read(addr),
            0x4090 | 0x4092 => self.audio.read(addr),
            0x6000..=0xDFFF => {
                let offset = (addr as usize) - 0x6000;
                if offset < self.prg_ram.len() {
                    self.prg_ram[offset]
                } else {
                    0
                }
            }
            0xE000..=0xFFFF => {
                let offset = (addr as usize) - 0xE000;
                if offset < self.prg_rom.len() {
                    self.prg_rom[offset]
                } else {
                    0xFF
                }
            }
            _ => 0,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4030 => {
                let val = self.disk_status();
                self.irq_pending = false;
                val
            }
            0x4031 => {
                let val = self.disk_data;
                self.acknowledge_disk_transfer();
                val
            }
            0x4032 => {
                let val = self.drive_status();
                self.acknowledge_disk_irq();
                val
            }
            0x4033 => 0x80,
            _ => self.cpu_peek(addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x4020 => {
                self.irq_latch = (self.irq_latch & 0xFF00) | val as u16;
            }
            0x4021 => {
                self.irq_latch = (self.irq_latch & 0x00FF) | ((val as u16) << 8);
            }
            0x4022 => {
                self.irq_repeat = val & 0x01 != 0;
                self.irq_enabled = val & 0x02 != 0;
                if self.irq_enabled {
                    self.irq_counter = self.irq_latch;
                }
                self.irq_pending = false;
            }
            0x4023 => {
                self.io_enabled = val & 0x01 != 0;
                if !self.io_enabled {
                    self.irq_enabled = false;
                    self.irq_pending = false;
                    self.acknowledge_disk_transfer();
                }
            }
            0x4024 => {
                self.disk_reg = val;
                self.acknowledge_disk_transfer();
            }
            0x4025 => {
                self.update_disk_control(val);
            }
            0x4026 => {}
            0x4040..=0x408A => {
                self.audio.write(addr, val);
            }
            0x6000..=0xDFFF => {
                let offset = (addr as usize) - 0x6000;
                if offset < self.prg_ram.len() {
                    self.prg_ram[offset] = val;
                }
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.chr_ram[(addr as usize) & (CHR_RAM_SIZE - 1)]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        self.chr_ram[(addr as usize) & (CHR_RAM_SIZE - 1)] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending || self.disk_irq_pending
    }

    fn clock_cpu(&mut self) {
        self.audio.tick();
        self.disk_media_change_counter = self.disk_media_change_counter.saturating_sub(1);
        self.clock_disk_drive();

        if self.irq_enabled {
            if self.irq_counter == 0 {
                self.irq_pending = true;
                if self.irq_repeat {
                    self.irq_counter = self.irq_latch;
                } else {
                    self.irq_enabled = false;
                }
            } else {
                self.irq_counter -= 1;
            }
        }
    }

    fn audio_output(&self) -> f32 {
        self.audio.output()
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        Some(self.prg_ram.clone())
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.load_battery_data_inner(bytes)
    }

    fn dump_persistent_data(&self) -> Option<Vec<u8>> {
        self.dump_persistent_data_inner()
    }

    fn load_persistent_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.load_persistent_data_inner(bytes)
    }

    fn write_mutable_media_state(&self, w: &mut crate::save_state::StateWriter) {
        self.write_mutable_media_state_inner(w);
    }

    fn read_mutable_media_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        self.read_mutable_media_state_inner(r)
    }

    fn reset_mutable_media_to_source(&mut self) {
        self.reset_mutable_media_to_source_inner();
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        self.write_state_inner(w);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.read_state_inner(r)
    }
}

fn side_label(side: u8) -> char {
    char::from(b'A'.saturating_add(side))
}

fn media_event_error_to_anyhow(err: MediaEventError) -> anyhow::Error {
    match err {
        MediaEventError::WrongSlot { expected, actual } => anyhow::anyhow!(
            "FDS media event targeted wrong slot: expected {}, got {}",
            expected.as_ref(),
            actual.as_ref()
        ),
        MediaEventError::NoMediaInserted { slot } => {
            anyhow::anyhow!("no FDS media inserted in {}", slot.as_ref())
        }
        MediaEventError::WriteProtected { slot } => {
            anyhow::anyhow!("FDS media in {} is write-protected", slot.as_ref())
        }
    }
}

#[cfg(test)]
mod tests;
