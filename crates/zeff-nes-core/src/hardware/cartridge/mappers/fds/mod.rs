pub(crate) mod audio;
mod image;

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
                self.acknowledge_disk_transfer();
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
        if bytes.len() > self.prg_ram.len() {
            anyhow::bail!(
                "FDS battery data size mismatch: expected at most {}, got {}",
                self.prg_ram.len(),
                bytes.len()
            );
        }
        self.prg_ram[..bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    fn dump_persistent_data(&self) -> Option<Vec<u8>> {
        let (Some(image), Some(media_id)) = (self.disk_image.as_ref(), self.source_media_id())
        else {
            return Some(self.prg_ram.clone());
        };
        let mut w = crate::save_state::StateWriter::with_capacity(
            FDS_SAVE_MAGIC.len() + image.side_data_len(),
        );
        w.write_bytes(&FDS_SAVE_MAGIC);
        w.write_u32(FDS_SAVE_VERSION);
        w.write_vec(media_id.as_ref().as_bytes());
        w.write_u64(self.media_slot.mutation_counter);
        w.write_u8(image.side_count() as u8);
        for side in image.sides() {
            w.write_vec(side);
        }
        Some(w.into_bytes())
    }

    fn load_persistent_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if !bytes.starts_with(&FDS_SAVE_MAGIC) {
            return self.load_battery_data(bytes);
        }

        let mut r = crate::save_state::StateReader::new(&bytes[FDS_SAVE_MAGIC.len()..]);
        let version = r.read_u32()?;
        if !matches!(
            version,
            FDS_SAVE_VERSION_V1 | FDS_SAVE_VERSION_V2 | FDS_SAVE_VERSION
        ) {
            anyhow::bail!(
                "unsupported FDS save version {version}; expected {FDS_SAVE_VERSION_V1}, {FDS_SAVE_VERSION_V2}, or {FDS_SAVE_VERSION}"
            );
        }
        let media_id = r.read_vec(MAX_MEDIA_ID_LEN)?;
        let expected_media_id = self
            .source_media_id()
            .ok_or_else(|| anyhow::anyhow!("cannot load FDS save without source media"))?;
        if media_id != expected_media_id.as_ref().as_bytes() {
            anyhow::bail!(
                "FDS save belongs to different source media (expected {})",
                expected_media_id.as_ref()
            );
        }

        let mutation_counter = if version >= FDS_SAVE_VERSION_V2 {
            r.read_u64()?
        } else {
            0
        };

        if version <= FDS_SAVE_VERSION_V2 {
            let legacy_prg_ram = r.read_vec(PRG_RAM_SIZE)?;
            if legacy_prg_ram.len() != PRG_RAM_SIZE {
                anyhow::bail!(
                    "FDS save PRG RAM size mismatch: expected {PRG_RAM_SIZE}, got {}",
                    legacy_prg_ram.len()
                );
            }
        }
        let side_count = usize::from(r.read_u8()?);
        let expected_side_count = self.disk_image.as_ref().map_or(0, FdsImage::side_count);
        if side_count != expected_side_count {
            anyhow::bail!(
                "FDS save side count mismatch: expected {expected_side_count}, got {side_count}"
            );
        }
        let mut sides = Vec::with_capacity(side_count);
        for _ in 0..side_count {
            let side = r.read_vec(FDS_SIDE_SIZE)?;
            if side.len() != FDS_SIDE_SIZE {
                anyhow::bail!(
                    "FDS save side size mismatch: expected {FDS_SIDE_SIZE}, got {}",
                    side.len()
                );
            }
            sides.push(side);
        }
        if !r.is_exhausted() {
            anyhow::bail!("FDS save has unexpected trailing data");
        }

        self.disk_image
            .as_mut()
            .expect("validated FDS media should remain inserted")
            .replace_sides(sides)?;
        self.media_slot.mutation_counter = mutation_counter;
        Ok(())
    }

    fn write_mutable_media_state(&self, w: &mut crate::save_state::StateWriter) {
        let image = self
            .disk_image
            .as_ref()
            .expect("FDS mapper should retain its inserted disk image");
        w.write_u64(self.media_slot.mutation_counter);
        w.write_u8(image.side_count() as u8);
        for side in image.sides() {
            w.write_vec(side);
        }
    }

    fn read_mutable_media_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        let mutation_counter = r.read_u64()?;
        let side_count = usize::from(r.read_u8()?);
        let expected_side_count = self.disk_image.as_ref().map_or(0, FdsImage::side_count);
        if side_count != expected_side_count {
            anyhow::bail!(
                "FDS state side count mismatch: expected {expected_side_count}, got {side_count}"
            );
        }
        let mut sides = Vec::with_capacity(side_count);
        for _ in 0..side_count {
            let side = r.read_vec(FDS_SIDE_SIZE)?;
            if side.len() != FDS_SIDE_SIZE {
                anyhow::bail!(
                    "FDS state side size mismatch: expected {FDS_SIDE_SIZE}, got {}",
                    side.len()
                );
            }
            sides.push(side);
        }
        self.disk_image
            .as_mut()
            .expect("FDS mapper should retain its inserted disk image")
            .replace_sides(sides)?;
        self.media_slot.mutation_counter = mutation_counter;
        Ok(())
    }

    fn reset_mutable_media_to_source(&mut self) {
        let source_side_count = self
            .source_disk_image
            .as_ref()
            .map_or(0, FdsImage::side_count);
        let selected_side = self
            .media_slot
            .side
            .filter(|&side| usize::from(side) < source_side_count)
            .unwrap_or(0);
        self.disk_image.clone_from(&self.source_disk_image);
        self.media_slot = self.source_media_id().map_or_else(
            || MediaSlotState::empty(FDS_DRIVE_SLOT_ID),
            |media_id| MediaSlotState {
                slot: FDS_DRIVE_SLOT_ID.into(),
                media_id: Some(media_id),
                side: Some(selected_side),
                write_protected: false,
                mutation_counter: 0,
            },
        );
        self.reset_disk_scan();
        self.disk_media_change_counter = 0;
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_vec(&self.prg_ram);
        w.write_bytes(&self.chr_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u16(self.irq_latch);
        w.write_u16(self.irq_counter);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_repeat);
        w.write_bool(self.irq_pending);
        w.write_bool(self.io_enabled);
        w.write_u8(self.disk_reg);
        w.write_bytes(&FDS_DRIVE_STATE_MARKER);
        w.write_u8(self.disk_control);
        w.write_u8(self.disk_data);
        w.write_u32(self.disk_position as u32);
        w.write_u32(self.disk_lead_in_counter);
        w.write_u32(self.disk_inter_block_gap_counter);
        w.write_bool(self.disk_next_gap_position.is_some());
        if let Some(position) = self.disk_next_gap_position {
            w.write_u32(position as u32);
        }
        w.write_bool(self.disk_pending_file_data_len.is_some());
        if let Some(len) = self.disk_pending_file_data_len {
            w.write_u16(len);
        }
        w.write_u8(self.disk_synthetic_crc_bytes_remaining);
        w.write_u16(self.disk_cycle_counter);
        w.write_bool(self.disk_transfer_flag);
        w.write_bool(self.disk_irq_pending);
        w.write_bool(self.disk_end_of_head);
        w.write_u32(self.disk_media_change_counter);
        w.write_bool(self.media_slot.inserted());
        w.write_bool(self.media_slot.side.is_some());
        if let Some(side) = self.media_slot.side {
            w.write_u8(side);
        }
        w.write_bool(self.media_slot.write_protected);
        w.write_u16(self.disk_crc_accumulator);
        w.write_bool(self.disk_write_in_gap);
        self.audio.write_state(w);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.prg_ram = r.read_vec(PRG_RAM_SIZE * 2)?;
        r.read_exact(&mut self.chr_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.irq_latch = r.read_u16()?;
        self.irq_counter = r.read_u16()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_repeat = r.read_bool()?;
        self.irq_pending = r.read_bool()?;
        self.io_enabled = r.read_bool()?;
        self.disk_reg = r.read_u8()?;
        let marker_pos = r.position();
        let mut marker = [0; FDS_DRIVE_STATE_MARKER.len()];
        r.read_exact(&mut marker)?;
        self.disk_media_change_counter = 0;
        if marker == FDS_DRIVE_STATE_MARKER || marker == FDS_DRIVE_STATE_MARKER_V1 {
            self.disk_control = r.read_u8()?;
            self.disk_data = r.read_u8()?;
            self.disk_position = (r.read_u32()? as usize).min(FDS_SIDE_SIZE);
            self.disk_lead_in_counter = r.read_u32()?.min(FDS_LEAD_IN_GAP_CPU_CYCLES);
            self.disk_inter_block_gap_counter = r.read_u32()?.min(FDS_INTER_BLOCK_GAP_CPU_CYCLES);
            self.disk_next_gap_position = if r.read_bool()? {
                Some((r.read_u32()? as usize).min(FDS_SIDE_SIZE))
            } else {
                None
            };
            self.disk_pending_file_data_len = if r.read_bool()? {
                Some(r.read_u16()?)
            } else {
                None
            };
            self.disk_synthetic_crc_bytes_remaining =
                r.read_u8()?.min(FDS_SYNTHETIC_CRC_BYTE_COUNT);
            self.disk_cycle_counter = r.read_u16()?.clamp(1, FDS_DISK_BYTE_PERIOD_CPU_CYCLES);
            self.disk_transfer_flag = r.read_bool()?;
            self.disk_irq_pending = r.read_bool()?;
            self.disk_end_of_head = r.read_bool()?;
            self.disk_media_change_counter = r.read_u32()?.min(FDS_SIDE_CHANGE_TOTAL_CPU_CYCLES);
            let inserted_tag = (marker == FDS_DRIVE_STATE_MARKER)
                .then(|| r.read_bool())
                .transpose()?;
            let side = if r.read_bool()? {
                Some(r.read_u8()?)
            } else {
                None
            };
            let inserted = inserted_tag.unwrap_or(side.is_some());
            let write_protected = if marker == FDS_DRIVE_STATE_MARKER {
                r.read_bool()?
            } else {
                false
            };
            self.disk_crc_accumulator = if marker == FDS_DRIVE_STATE_MARKER {
                r.read_u16()?
            } else {
                0
            };
            self.disk_write_in_gap = if marker == FDS_DRIVE_STATE_MARKER {
                r.read_bool()?
            } else {
                true
            };
            if inserted != side.is_some() {
                anyhow::bail!("invalid FDS drive state: inserted media must select a side");
            }
            if let Some(side) = side {
                self.validate_side(side)?;
                self.media_slot.media_id = self.source_media_id();
                self.media_slot.side = Some(side);
                self.media_slot.write_protected = write_protected;
            } else {
                self.media_slot.media_id = None;
                self.media_slot.side = None;
                self.media_slot.write_protected = false;
            }
        } else {
            r.set_position(marker_pos);
            self.disk_control = 0;
            self.disk_data = 0;
            self.media_slot = self
                .source_media_id()
                .map(inserted_fds_media_slot)
                .unwrap_or_else(|| MediaSlotState::empty(FDS_DRIVE_SLOT_ID));
            self.reset_disk_scan();
        }
        self.audio.read_state(r)?;
        Ok(())
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
mod tests {
    use super::*;

    fn make_fds() -> Fds {
        let bios = vec![0xFF; FDS_BIOS_SIZE];
        Fds::new(bios, Mirroring::Horizontal)
    }

    fn side(fill: u8) -> Vec<u8> {
        vec![fill; FDS_SIDE_SIZE]
    }

    fn clock_until_next_disk_byte(fds: &mut Fds) {
        let cycles = u64::from(fds.disk_media_change_counter)
            + u64::from(fds.disk_lead_in_counter)
            + u64::from(FDS_DISK_BYTE_PERIOD_CPU_CYCLES);
        for _ in 0..cycles {
            fds.clock_cpu();
        }
    }

    fn clock_until_side_change_settles(fds: &mut Fds) {
        let cycles = u64::from(fds.disk_media_change_counter);
        for _ in 0..cycles {
            fds.clock_cpu();
        }
    }

    #[test]
    fn selecting_disk_side_reports_disk_change_transition() {
        let mut side_a = vec![0; FDS_SIDE_SIZE];
        side_a[0] = 0xA1;
        let mut side_b = vec![0; FDS_SIDE_SIZE];
        side_b[0] = 0xB2;
        let mut image_bytes = side_a;
        image_bytes.extend_from_slice(&side_b);
        let image = FdsImage::parse(&image_bytes).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

        fds.select_side(1).expect("side B should be selectable");

        assert_eq!(
            fds.cpu_read(0x4032) & 0x03,
            0x03,
            "side changes should first look like the disk was removed"
        );

        for _ in 0..FDS_SIDE_CHANGE_EJECT_CPU_CYCLES {
            fds.clock_cpu();
        }
        assert_eq!(
            fds.cpu_read(0x4032) & 0x03,
            0x02,
            "after reinsertion the disk should still need to settle"
        );

        clock_until_side_change_settles(&mut fds);
        assert_eq!(
            fds.cpu_read(0x4032) & 0x03,
            0x00,
            "side changes should eventually report disk present and ready"
        );
    }

    #[test]
    fn disk_image_constructor_initializes_media_slot() {
        let image = FdsImage::parse(&side(0x34)).expect("FDS image should parse");
        let media_id = image.media_object_id();
        let fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

        assert_eq!(fds.disk_image().unwrap().side_count(), 1);
        assert_eq!(fds.media_slot_state().slot.as_ref(), FDS_DRIVE_SLOT_ID);
        assert_eq!(fds.media_slot_state().media_id.as_ref(), Some(&media_id));
        assert_eq!(fds.media_slot_state().side, Some(0));
        assert!(!fds.media_slot_state().write_protected);
    }

    #[test]
    fn media_events_preserve_mutated_disk_across_eject_and_reinsert() {
        let mut bytes = side(0x11);
        bytes.extend_from_slice(&side(0x22));
        let image = FdsImage::parse(&bytes).unwrap();
        let media_id = image.media_object_id();
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        fds.active_side_mut().unwrap()[123] = 0xA5;
        fds.media_slot.record_mutation().unwrap();

        fds.apply_media_event(&MediaEvent::Eject {
            slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
        })
        .unwrap();
        assert!(!fds.media_slot_snapshot().inserted());
        assert_eq!(fds.media_slot.mutation_counter, 1);
        assert_eq!(fds.drive_status(), 0x07);
        let ejected_persistent = fds.dump_persistent_data().unwrap();
        assert!(ejected_persistent.starts_with(&FDS_SAVE_MAGIC));
        let image = FdsImage::parse(&bytes).unwrap();
        let mut reloaded =
            Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        reloaded.load_persistent_data(&ejected_persistent).unwrap();
        assert_eq!(reloaded.disk_image().unwrap().side(0).unwrap()[123], 0xA5);
        assert_eq!(reloaded.media_slot.mutation_counter, 1);
        assert!(
            fds.apply_media_event(&MediaEvent::SelectSide {
                slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
                side: 1,
            })
            .is_err()
        );
        assert!(
            fds.apply_media_event(&MediaEvent::SetWriteProtected {
                slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
                write_protected: true,
            })
            .is_err()
        );

        fds.apply_media_event(&MediaEvent::Insert {
            slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
            media_id,
            side: Some(1),
            write_protected: true,
        })
        .unwrap();
        assert!(fds.media_slot_snapshot().inserted());
        assert_eq!(fds.selected_side(), Some(1));
        assert!(fds.media_slot.write_protected);
        assert_eq!(fds.media_slot.mutation_counter, 1);
        assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[123], 0xA5);
        assert_eq!(fds.drive_status(), 0x07, "inserted media must settle");
    }

    #[test]
    fn media_insert_rejects_wrong_slot_source_and_side() {
        let image = FdsImage::parse(&side(0x11)).unwrap();
        let media_id = image.media_object_id();
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        fds.apply_media_event(&MediaEvent::Eject {
            slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
        })
        .unwrap();

        for event in [
            MediaEvent::Insert {
                slot: MediaSlotId::from("other"),
                media_id: media_id.clone(),
                side: Some(0),
                write_protected: false,
            },
            MediaEvent::Insert {
                slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
                media_id: MediaObjectId::from("sha256:other"),
                side: Some(0),
                write_protected: false,
            },
            MediaEvent::Insert {
                slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
                media_id,
                side: Some(1),
                write_protected: false,
            },
        ] {
            assert!(fds.apply_media_event(&event).is_err());
        }
    }

    #[test]
    fn prg_ram_read_write() {
        let mut fds = make_fds();
        fds.cpu_write(0x6000, 0x42);
        assert_eq!(fds.cpu_peek(0x6000), 0x42);
        fds.cpu_write(0xDFFF, 0xAB);
        assert_eq!(fds.cpu_peek(0xDFFF), 0xAB);
    }

    #[test]
    fn chr_ram_read_write() {
        let mut fds = make_fds();
        fds.chr_write(0x0000, 0x55);
        assert_eq!(fds.chr_read(0x0000), 0x55);
        fds.chr_write(0x1FFF, 0xAA);
        assert_eq!(fds.chr_read(0x1FFF), 0xAA);
    }

    #[test]
    fn irq_counter_fires() {
        let mut fds = make_fds();
        fds.cpu_write(0x4020, 0x02);
        fds.cpu_write(0x4021, 0x00);
        fds.cpu_write(0x4022, 0x02);
        assert!(!fds.irq_pending());
        fds.clock_cpu();
        fds.clock_cpu();
        fds.clock_cpu();
        assert!(fds.irq_pending());
    }

    #[test]
    fn mirroring_toggle() {
        let mut fds = make_fds();
        assert_eq!(fds.mirroring(), Mirroring::Horizontal);
        fds.cpu_write(0x4025, 0x00);
        assert_eq!(fds.mirroring(), Mirroring::Vertical);
        fds.cpu_write(0x4025, 0x08);
        assert_eq!(fds.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn inserted_disk_reports_ready_and_present_status() {
        let image = FdsImage::parse(&side(0x34)).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

        assert_eq!(fds.cpu_read(0x4032) & 0x07, 0x00);
    }

    #[test]
    fn selecting_disk_side_switches_active_side_and_rewinds_scan() {
        let mut side_a = vec![0; FDS_SIDE_SIZE];
        side_a[0] = 0xA1;
        let mut side_b = vec![0; FDS_SIDE_SIZE];
        side_b[0] = 0xB2;
        let mut image_bytes = side_a;
        image_bytes.extend_from_slice(&side_b);
        let image = FdsImage::parse(&image_bytes).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

        fds.cpu_write(0x4023, 0x01);
        fds.cpu_write(0x4025, 0xE5);
        clock_until_next_disk_byte(&mut fds);
        assert_eq!(fds.cpu_read(0x4031), 0xA1);

        fds.select_side(1).expect("side B should be selectable");
        assert_eq!(fds.selected_side(), Some(1));
        clock_until_next_disk_byte(&mut fds);
        assert_eq!(fds.cpu_read(0x4031), 0xB2);
    }

    #[test]
    fn selecting_missing_disk_side_is_rejected() {
        let image = FdsImage::parse(&side(0x34)).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

        let err = fds.select_side(1).unwrap_err().to_string();

        assert!(err.contains("side B is not present"), "{err}");
        assert_eq!(fds.selected_side(), Some(0));
    }

    #[test]
    fn disk_scan_streams_side_bytes_and_raises_transfer_irq() {
        let mut side = vec![0; FDS_SIDE_SIZE];
        side[0] = 0x01;
        side[1] = 0x02;
        let image = FdsImage::parse(&side).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

        fds.cpu_write(0x4023, 0x01);
        fds.cpu_write(0x4025, 0xE5);
        clock_until_next_disk_byte(&mut fds);

        assert!(fds.irq_pending());
        assert_eq!(fds.cpu_read(0x4030) & 0x80, 0x80);
        assert!(!fds.irq_pending());

        clock_until_next_disk_byte(&mut fds);
        assert!(fds.irq_pending());
        assert_eq!(fds.cpu_read(0x4031), 0x02);
        assert!(!fds.irq_pending());
    }

    #[test]
    fn disk_scan_reset_rewinds_to_start_of_side() {
        let mut side = vec![0; FDS_SIDE_SIZE];
        side[0] = 0xAA;
        side[1] = 0xBB;
        let image = FdsImage::parse(&side).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

        fds.cpu_write(0x4023, 0x01);
        fds.cpu_write(0x4025, 0xE5);
        clock_until_next_disk_byte(&mut fds);
        assert_eq!(fds.cpu_read(0x4031), 0xAA);

        fds.cpu_write(0x4025, 0xE4);
        fds.cpu_write(0x4025, 0xE5);
        clock_until_next_disk_byte(&mut fds);
        assert_eq!(fds.cpu_read(0x4031), 0xAA);
    }

    #[test]
    fn audio_wavetable_write_read() {
        let mut fds = make_fds();
        fds.cpu_write(0x4089, 0x80);
        fds.cpu_write(0x4040, 0x20);
        fds.cpu_write(0x4041, 0x3F);
        assert_eq!(fds.cpu_peek(0x4040), 0x20);
        assert_eq!(fds.cpu_peek(0x4041), 0x3F);
    }

    #[test]
    fn battery_data_roundtrip() {
        let mut fds = make_fds();
        fds.cpu_write(0x6000, 0x42);
        fds.cpu_write(0x7FFF, 0xBE);
        let data = fds.dump_battery_data().expect("should dump");
        let mut fds2 = make_fds();
        fds2.load_battery_data(&data).expect("should load");
        assert_eq!(fds2.cpu_peek(0x6000), 0x42);
        assert_eq!(fds2.cpu_peek(0x7FFF), 0xBE);
    }

    #[test]
    fn disk_write_transport_mutates_only_selected_side() {
        let mut side_a = side(0x11);
        side_a[0] = FDS_DISK_INFO_BLOCK_CODE;
        let mut side_b = side(0x22);
        side_b[0] = FDS_DISK_INFO_BLOCK_CODE;
        let mut image_bytes = side_a;
        image_bytes.extend_from_slice(&side_b);
        let image = FdsImage::parse(&image_bytes).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        fds.select_side(1).unwrap();
        clock_until_side_change_settles(&mut fds);
        fds.cpu_write(0x4023, 0x01);
        fds.cpu_write(0x4025, 0xC1);
        fds.disk_position = 10;
        fds.disk_lead_in_counter = 0;
        fds.disk_cycle_counter = 1;
        fds.disk_next_gap_position = Some(FDS_DISK_INFO_BLOCK_LEN);

        fds.cpu_write(0x4024, 0xA5);
        fds.clock_cpu();

        let image = fds.disk_image().unwrap();
        assert_eq!(image.side(0).unwrap()[10], 0x11);
        assert_eq!(image.side(1).unwrap()[10], 0xA5);
        assert_eq!(fds.disk_position, 11);
        assert_eq!(fds.media_slot_state().mutation_counter, 1);
        assert!(fds.irq_pending());
        assert_eq!(fds.cpu_read(0x4030) & 0x80, 0x80);
    }

    #[test]
    fn write_protected_disk_signals_status_without_mutating() {
        let mut disk = side(0x33);
        disk[0] = FDS_DISK_INFO_BLOCK_CODE;
        let image = FdsImage::parse(&disk).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        fds.media_slot.write_protected = true;
        fds.cpu_write(0x4023, 0x01);
        fds.cpu_write(0x4025, 0xC1);
        fds.disk_position = 10;
        fds.disk_lead_in_counter = 0;
        fds.disk_cycle_counter = 1;
        fds.disk_next_gap_position = Some(FDS_DISK_INFO_BLOCK_LEN);

        fds.cpu_write(0x4024, 0xA5);
        fds.clock_cpu();

        assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[10], 0x33);
        assert_eq!(fds.media_slot_state().mutation_counter, 0);
        assert_eq!(fds.disk_position, 11);
        assert_eq!(fds.cpu_read(0x4032) & 0x04, 0x04);
    }

    #[test]
    fn disk_ready_clear_clocks_compact_gap_without_mutation() {
        let mut disk = side(0x66);
        disk[0] = FDS_DISK_INFO_BLOCK_CODE;
        let image = FdsImage::parse(&disk).unwrap();
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        fds.cpu_write(0x4023, 0x01);
        fds.cpu_write(0x4025, 0x81);
        fds.disk_position = 10;
        fds.disk_lead_in_counter = 0;
        fds.disk_cycle_counter = 1;
        fds.disk_next_gap_position = Some(FDS_DISK_INFO_BLOCK_LEN);

        fds.cpu_write(0x4024, 0xA5);
        fds.clock_cpu();

        assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[10], 0x66);
        assert_eq!(fds.disk_position, 10);
        assert_eq!(fds.media_slot_state().mutation_counter, 0);
        assert!(fds.irq_pending());
    }

    #[test]
    fn crc_control_clocks_physical_crc_without_mutating_compact_media() {
        let mut disk = side(0x77);
        disk[0] = FDS_DISK_INFO_BLOCK_CODE;
        let image = FdsImage::parse(&disk).unwrap();
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        fds.cpu_write(0x4023, 0x01);
        fds.cpu_write(0x4025, 0xD1);
        fds.disk_position = 10;
        fds.disk_lead_in_counter = 0;
        fds.disk_cycle_counter = 1;
        fds.disk_next_gap_position = Some(10);
        fds.disk_crc_accumulator = 0xBEEF;

        fds.cpu_write(0x4024, 0xA5);
        fds.clock_cpu();

        assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[10], 0x77);
        assert_eq!(fds.disk_position, 10);
        assert_eq!(fds.disk_crc_accumulator, 0x00BE);
        assert!(fds.disk_write_in_gap);
        assert_eq!(fds.media_slot_state().mutation_counter, 0);
        assert!(!fds.irq_pending());
        assert_eq!(fds.disk_status() & 0x80, 0);
    }

    #[test]
    fn write_gap_and_sync_do_not_overwrite_compact_block_bytes() {
        let mut disk = side(0x66);
        disk[0] = FDS_DISK_INFO_BLOCK_CODE;
        let image = FdsImage::parse(&disk).unwrap();
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        fds.cpu_write(0x4023, 0x01);
        fds.disk_position = 10;
        fds.disk_lead_in_counter = 0;

        for (control, value) in [(0x81, 0x00), (0xC1, 0x80)] {
            fds.cpu_write(0x4025, control);
            fds.cpu_write(0x4024, value);
            fds.disk_lead_in_counter = 0;
            fds.disk_cycle_counter = 1;
            fds.clock_cpu();
            assert_eq!(fds.disk_position, 10);
            assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[10], 0x66);
        }

        fds.cpu_write(0x4024, 0x02);
        fds.disk_cycle_counter = 1;
        fds.clock_cpu();
        assert_eq!(fds.disk_position, 11);
        assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[10], 0x02);
        assert_eq!(fds.media_slot.mutation_counter, 1);
    }

    #[test]
    fn fds_save_container_roundtrips_mutated_sides_without_volatile_ram() {
        let mut disk = side(0x44);
        disk[0] = FDS_DISK_INFO_BLOCK_CODE;
        let image = FdsImage::parse(&disk).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(
            vec![0xFF; FDS_BIOS_SIZE],
            image.clone(),
            Mirroring::Horizontal,
        );
        fds.cpu_write(0x6000, 0x5A);
        fds.disk_image.as_mut().unwrap().side_mut(0).unwrap()[12] = 0xC7;
        fds.media_slot.record_mutation().unwrap();

        let data = fds.dump_persistent_data().expect("FDS save should dump");
        assert!(data.starts_with(&FDS_SAVE_MAGIC));

        let mut restored =
            Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        restored
            .load_persistent_data(&data)
            .expect("FDS save should load");
        assert_eq!(restored.cpu_peek(0x6000), 0x00);
        assert_eq!(restored.disk_image().unwrap().side(0).unwrap()[12], 0xC7);
        assert_eq!(restored.media_slot_state().mutation_counter, 1);
    }

    #[test]
    fn legacy_raw_fds_ram_save_still_loads() {
        let image = FdsImage::parse(&side(0x44)).unwrap();
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        let mut legacy = vec![0; PRG_RAM_SIZE];
        legacy[0] = 0x42;
        legacy[PRG_RAM_SIZE - 1] = 0xBE;

        fds.load_persistent_data(&legacy).unwrap();

        assert_eq!(fds.cpu_peek(0x6000), 0x42);
        assert_eq!(fds.cpu_peek(0xDFFF), 0xBE);
        assert_eq!(fds.media_slot_state().mutation_counter, 0);
    }

    #[test]
    fn version_one_fds_container_without_mutation_counter_still_loads() {
        let image = FdsImage::parse(&side(0x44)).unwrap();
        let source = Fds::with_disk_image(
            vec![0xFF; FDS_BIOS_SIZE],
            image.clone(),
            Mirroring::Horizontal,
        );
        let mut version_one = source.dump_persistent_data().unwrap();
        version_one[FDS_SAVE_MAGIC.len()..FDS_SAVE_MAGIC.len() + 4]
            .copy_from_slice(&FDS_SAVE_VERSION_V1.to_le_bytes());
        let media_id_len_offset = FDS_SAVE_MAGIC.len() + 4;
        let media_id_len = u32::from_le_bytes(
            version_one[media_id_len_offset..media_id_len_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        let counter_offset = media_id_len_offset + 4 + media_id_len;
        let mut legacy_prg_ram = Vec::with_capacity(4 + PRG_RAM_SIZE);
        legacy_prg_ram.extend_from_slice(&(PRG_RAM_SIZE as u32).to_le_bytes());
        legacy_prg_ram.resize(4 + PRG_RAM_SIZE, 0);
        version_one.splice(counter_offset + 8..counter_offset + 8, legacy_prg_ram);
        version_one.drain(counter_offset..counter_offset + 8);

        let mut restored =
            Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        restored.load_persistent_data(&version_one).unwrap();

        assert_eq!(restored.disk_image().unwrap().side(0).unwrap()[0], 0x44);
        assert_eq!(restored.media_slot_state().mutation_counter, 0);
    }

    #[test]
    fn version_two_fds_container_loads_media_without_restoring_volatile_ram() {
        let image = FdsImage::parse(&side(0x44)).unwrap();
        let source = Fds::with_disk_image(
            vec![0xFF; FDS_BIOS_SIZE],
            image.clone(),
            Mirroring::Horizontal,
        );
        let mut version_two = source.dump_persistent_data().unwrap();
        version_two[FDS_SAVE_MAGIC.len()..FDS_SAVE_MAGIC.len() + 4]
            .copy_from_slice(&FDS_SAVE_VERSION_V2.to_le_bytes());
        let media_id_len_offset = FDS_SAVE_MAGIC.len() + 4;
        let media_id_len = u32::from_le_bytes(
            version_two[media_id_len_offset..media_id_len_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        let counter_offset = media_id_len_offset + 4 + media_id_len;
        let mut legacy_prg_ram = Vec::with_capacity(4 + PRG_RAM_SIZE);
        legacy_prg_ram.extend_from_slice(&(PRG_RAM_SIZE as u32).to_le_bytes());
        legacy_prg_ram.resize(4 + PRG_RAM_SIZE, 0x5A);
        version_two.splice(counter_offset + 8..counter_offset + 8, legacy_prg_ram);

        let mut restored =
            Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        restored.load_persistent_data(&version_two).unwrap();

        assert_eq!(restored.cpu_peek(0x6000), 0x00);
        assert_eq!(restored.disk_image().unwrap().side(0).unwrap()[0], 0x44);
    }

    #[test]
    fn fds_save_container_rejects_different_source_media() {
        let source = FdsImage::parse(&side(0x44)).unwrap();
        let other = FdsImage::parse(&side(0x55)).unwrap();
        let source = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], source, Mirroring::Horizontal);
        let data = source.dump_persistent_data().unwrap();
        let mut other =
            Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], other, Mirroring::Horizontal);

        let err = other.load_persistent_data(&data).unwrap_err();
        assert!(err.to_string().contains("different source media"));
    }

    #[test]
    fn audio_produces_output_when_playing() {
        let mut fds = make_fds();
        fds.cpu_write(0x4089, 0x80);
        for i in 0..64u8 {
            fds.cpu_write(0x4040 + i as u16, (i & 0x3F) | 0x20);
        }
        fds.cpu_write(0x4089, 0x00);
        fds.cpu_write(0x4080, 0x80 | 0x1F);
        fds.cpu_write(0x4082, 0xFF);
        fds.cpu_write(0x4083, 0x03);
        for _ in 0..1000 {
            fds.clock_cpu();
        }
        let out = fds.audio_output();
        assert!(out > 0.0, "expected audio output > 0, got {out}");
    }

    #[test]
    fn audio_silent_when_halted() {
        let mut fds = make_fds();
        fds.cpu_write(0x4083, 0x80);
        for _ in 0..100 {
            fds.clock_cpu();
        }
        assert_eq!(fds.audio_output(), 0.0);
    }

    #[test]
    fn irq_repeat_mode() {
        let mut fds = make_fds();
        fds.cpu_write(0x4020, 0x01);
        fds.cpu_write(0x4021, 0x00);
        fds.cpu_write(0x4022, 0x03);
        fds.clock_cpu();
        fds.clock_cpu();
        assert!(fds.irq_pending());
        let _ = fds.cpu_read(0x4030);
        assert!(!fds.irq_pending());
        fds.clock_cpu();
        fds.clock_cpu();
        assert!(fds.irq_pending());
    }

    #[test]
    fn irq_no_repeat_disables_after_fire() {
        let mut fds = make_fds();
        fds.cpu_write(0x4020, 0x01);
        fds.cpu_write(0x4021, 0x00);
        fds.cpu_write(0x4022, 0x02);
        fds.clock_cpu();
        fds.clock_cpu();
        assert!(fds.irq_pending());
        let _ = fds.cpu_read(0x4030);
        assert!(!fds.irq_pending());
        for _ in 0..10 {
            fds.clock_cpu();
        }
        assert!(!fds.irq_pending());
    }

    #[test]
    fn mod_table_writes_only_when_halted() {
        let mut fds = make_fds();
        fds.cpu_write(0x4087, 0x80);
        fds.cpu_write(0x4088, 0x03);
        assert_eq!(fds.cpu_peek(0x4040), 0);
        fds.cpu_write(0x4087, 0x00);
        fds.cpu_write(0x4088, 0x05);
    }

    #[test]
    fn volume_gain_read() {
        let mut fds = make_fds();
        fds.cpu_write(0x4080, 0x80 | 0x20);
        let val = fds.cpu_peek(0x4090);
        assert_eq!(val & 0x3F, 0x20);
    }

    #[test]
    fn save_state_roundtrip() {
        let mut side = vec![0; FDS_SIDE_SIZE];
        side[0] = 0x11;
        side[1] = 0x22;
        let image = FdsImage::parse(&side).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        fds.cpu_write(0x6000, 0xAA);
        fds.cpu_write(0x4023, 0x01);
        fds.cpu_write(0x4025, 0xE5);
        clock_until_next_disk_byte(&mut fds);
        assert_eq!(fds.cpu_read(0x4031), 0x11);
        fds.cpu_write(0x4089, 0x80);
        fds.cpu_write(0x4040, 0x20);
        fds.cpu_write(0x4089, 0x00);
        fds.cpu_write(0x4080, 0x80 | 0x1F);
        fds.cpu_write(0x4082, 0xFF);
        fds.cpu_write(0x4083, 0x03);
        for _ in 0..100 {
            fds.clock_cpu();
        }

        let mut writer = crate::save_state::StateWriter::new();
        fds.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let image = FdsImage::parse(&side).expect("FDS image should parse");
        let mut fds2 =
            Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        let mut reader = crate::save_state::StateReader::new(&bytes);
        fds2.read_state(&mut reader).expect("read_state ok");

        assert_eq!(fds2.cpu_peek(0x6000), 0xAA);
        assert_eq!(fds2.cpu_peek(0x4040), 0x20);
        clock_until_next_disk_byte(&mut fds2);
        assert_eq!(fds2.cpu_read(0x4031), 0x22);
    }

    #[test]
    fn save_state_roundtrip_preserves_selected_side() {
        let mut side_a = vec![0; FDS_SIDE_SIZE];
        side_a[0] = 0xA1;
        let mut side_b = vec![0; FDS_SIDE_SIZE];
        side_b[0] = 0xB2;
        let mut image_bytes = side_a;
        image_bytes.extend_from_slice(&side_b);
        let image = FdsImage::parse(&image_bytes).expect("FDS image should parse");
        let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        fds.select_side(1).expect("side B should be selectable");

        let mut writer = crate::save_state::StateWriter::new();
        fds.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let image = FdsImage::parse(&image_bytes).expect("FDS image should parse");
        let mut fds2 =
            Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        let mut reader = crate::save_state::StateReader::new(&bytes);
        fds2.read_state(&mut reader).expect("read_state ok");

        assert_eq!(fds2.selected_side(), Some(1));
        fds2.cpu_write(0x4023, 0x01);
        fds2.cpu_write(0x4025, 0xE5);
        clock_until_next_disk_byte(&mut fds2);
        assert_eq!(fds2.cpu_read(0x4031), 0xB2);
    }

    #[test]
    fn legacy_drive_marker_restores_selected_side_unprotected() {
        let mut bytes = side(0x11);
        bytes.extend_from_slice(&side(0x22));
        let image = FdsImage::parse(&bytes).unwrap();
        let mut source = Fds::with_disk_image(
            vec![0xFF; FDS_BIOS_SIZE],
            image.clone(),
            Mirroring::Horizontal,
        );
        source.select_side(1).unwrap();
        let mut writer = crate::save_state::StateWriter::new();
        source.write_state(&mut writer);
        let mut state = writer.into_bytes();
        let marker = state
            .windows(FDS_DRIVE_STATE_MARKER.len())
            .position(|window| window == FDS_DRIVE_STATE_MARKER)
            .unwrap();
        state[marker..marker + FDS_DRIVE_STATE_MARKER.len()]
            .copy_from_slice(&FDS_DRIVE_STATE_MARKER_V1);

        let inserted_offset = marker + FDS_DRIVE_STATE_MARKER.len() + 26;
        state.remove(inserted_offset);
        let after_side = inserted_offset + 2;
        state.drain(after_side..after_side + 4);

        let mut restored =
            Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
        restored
            .read_state(&mut crate::save_state::StateReader::new(&state))
            .unwrap();
        assert_eq!(restored.selected_side(), Some(1));
        assert!(restored.media_slot.inserted());
        assert!(!restored.media_slot.write_protected);
    }
}
