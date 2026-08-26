use super::{
    FDS_DISK_BYTE_PERIOD_CPU_CYCLES, FDS_DRIVE_SLOT_ID, FDS_DRIVE_STATE_MARKER,
    FDS_DRIVE_STATE_MARKER_V1, FDS_DRIVE_STATE_MARKER_V2, FDS_INTER_BLOCK_GAP_CPU_CYCLES,
    FDS_LEAD_IN_GAP_CPU_CYCLES, FDS_SAVE_MAGIC, FDS_SAVE_VERSION, FDS_SAVE_VERSION_V1,
    FDS_SAVE_VERSION_V2, FDS_SIDE_CHANGE_TOTAL_CPU_CYCLES, FDS_SIDE_SIZE,
    FDS_SYNTHETIC_CRC_BYTE_COUNT, Fds, FdsImage, MAX_MEDIA_ID_LEN, PRG_RAM_SIZE,
    inserted_fds_media_slot,
};
use crate::save_state::{StateReader, StateWriter};
use zeff_emu_common::media::MediaSlotState;

impl Fds {
    pub(super) fn dump_persistent_data_inner(&self) -> Option<Vec<u8>> {
        let (Some(image), Some(media_id)) = (self.disk_image.as_ref(), self.source_media_id())
        else {
            return Some(self.prg_ram.clone());
        };
        let mut w = StateWriter::with_capacity(FDS_SAVE_MAGIC.len() + image.side_data_len());
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

    pub(super) fn load_persistent_data_inner(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if !bytes.starts_with(&FDS_SAVE_MAGIC) {
            return self.load_battery_data_inner(bytes);
        }

        let mut r = StateReader::new(&bytes[FDS_SAVE_MAGIC.len()..]);
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

    pub(super) fn write_mutable_media_state_inner(&self, w: &mut StateWriter) {
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

    pub(super) fn read_mutable_media_state_inner(
        &mut self,
        r: &mut StateReader,
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

    pub(super) fn reset_mutable_media_to_source_inner(&mut self) {
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

    pub(super) fn write_state_inner(&self, w: &mut StateWriter) {
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
        w.write_bool(self.audio_enabled);
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

    pub(super) fn read_state_inner(&mut self, r: &mut StateReader) -> anyhow::Result<()> {
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
        if matches!(
            marker,
            FDS_DRIVE_STATE_MARKER | FDS_DRIVE_STATE_MARKER_V2 | FDS_DRIVE_STATE_MARKER_V1
        ) {
            let has_current_drive_state = marker == FDS_DRIVE_STATE_MARKER;
            let has_modern_drive_state = marker != FDS_DRIVE_STATE_MARKER_V1;
            self.audio_enabled = if has_current_drive_state {
                r.read_bool()?
            } else {
                true
            };
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
            let inserted_tag = has_modern_drive_state.then(|| r.read_bool()).transpose()?;
            let side = if r.read_bool()? {
                Some(r.read_u8()?)
            } else {
                None
            };
            let inserted = inserted_tag.unwrap_or(side.is_some());
            let write_protected = if has_modern_drive_state {
                r.read_bool()?
            } else {
                false
            };
            self.disk_crc_accumulator = if has_modern_drive_state {
                r.read_u16()?
            } else {
                0
            };
            self.disk_write_in_gap = if has_modern_drive_state {
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
            self.audio_enabled = true;
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

    pub(super) fn load_battery_data_inner(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
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
}
