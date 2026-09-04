use super::*;

impl PceMachine {
    pub(in super::super) fn validate_v1_state_target(&self) -> anyhow::Result<()> {
        if self.devices().cdrom2().is_some()
            && self.hardware_topology() != PceHardwareTopology::Base
        {
            bail!("PC Engine CD save-states require the base hardware topology");
        }
        if self.devices().arcade_card().is_some()
            && (self.devices().cdrom2().is_none()
                || self.hucard_board() != PceHuCardBoard::SystemCardV3)
        {
            bail!("Arcade Card save-states require a System Card v3 CD machine");
        }
        Ok(())
    }

    pub(in super::super) fn validate_v1_encode_state(&self) -> anyhow::Result<()> {
        self.validate_v1_state_target()?;
        if self.faulted {
            bail!("faulted PC Engine machines cannot be saved");
        }
        if !self.cpu.at_action_boundary() {
            bail!("PC Engine states can only be saved at CPU action boundaries");
        }
        self.devices().psg().validate_v1_state()?;
        if let Some(cdrom2) = self.devices().cdrom2() {
            cdrom2.validate_v1_state()?;
        }
        Ok(())
    }

    pub(in super::super) fn write_state(&self, writer: &mut StateWriter, state_version: u32) {
        write_section(writer, |section| {
            self.cpu.write_state(section, state_version)
        });
        write_section(writer, |section| self.bus.write_state(section));
        write_section(writer, |section| {
            self.bus.devices().vdc().write_state(section)
        });
        if let Some(supergrafx) = self.bus.devices().supergrafx_video() {
            write_section(writer, |section| supergrafx.vdc2().write_state(section));
            write_section(writer, |section| supergrafx.vpc().write_state(section));
        }
        write_section(writer, |section| {
            self.bus.devices().vce().write_state(section)
        });
        write_section(writer, |section| {
            self.bus.devices().psg().write_state(section)
        });
        write_section(writer, |section| {
            self.bus.devices().controller().write_state(section)
        });
        if let Some(cdrom2) = self.bus.devices().cdrom2() {
            write_section(writer, |section| cdrom2.write_state(section));
        }
        if let Some(arcade_card) = self.bus.devices().arcade_card() {
            write_section(writer, |section| arcade_card.write_state(section));
        }
        write_section(writer, |section| {
            section.write_u64(self.master_ticks);
            section.write_u64(self.vce_line_accumulator);
            section.write_u8(self.vdc_pixel_clock_remainder);
            section.write_u16(self.vce_line_index);
            section.write_u8(match self.vce_frame_length {
                VceFrameLength::Lines262 => 0,
                VceFrameLength::Lines263 => 1,
            });
            section.write_u8(match self.execution_state {
                PceExecutionState::Running => 0,
                PceExecutionState::Suspended => 1,
            });
            section.write_bool(self.suspend_after_instruction);
            if state_version >= 3 {
                section.write_u64(self.trace_frame);
            }
        });
        write_section(writer, |section| self.front_video.write_state(section));
        write_section(writer, |section| self.back_video.write_state(section));
    }

    pub(in super::super) fn replace_from_state(
        &mut self,
        data: &[u8],
        identity: PceStateIdentity,
        state_version: u32,
    ) -> anyhow::Result<()> {
        let PceStateIdentity {
            board,
            topology,
            wiring,
            psg_revision,
            is_cd,
            has_arcade_card,
        } = identity;
        let runtime_audio = self.devices().psg().runtime_config();
        let runtime_cd_audio = self
            .devices()
            .cdrom2()
            .map(super::super::cdrom2::CdRom2::runtime_audio_config);
        let history_enabled = self.opcode_history.enabled;
        let trace_enabled = self.instruction_trace.is_enabled();
        let trace_capacity = self.instruction_trace.capacity();
        let cartridge = PceCartridgeDescriptor::default()
            .with_console_wiring(wiring)
            .with_required_hardware(match topology {
                PceHardwareTopology::Base => PceCartridgeHardware::Base,
                PceHardwareTopology::SuperGrafx => PceCartridgeHardware::SuperGrafx,
            })
            .with_hucard_board(board);
        let mut restored = if is_cd {
            if topology != PceHardwareTopology::Base {
                bail!("PC Engine CD save-state has an unsupported hardware topology");
            }
            let disc = self
                .devices()
                .cdrom2()
                .expect("CD state target has CD hardware")
                .disc()
                .clone();
            Self::with_cdrom2_system_card_controller_and_arcade_card(
                self.hucard_rom().to_vec(),
                board,
                disc,
                wiring,
                ControllerPort::default(),
                has_arcade_card,
            )
            .map_err(|error| anyhow::anyhow!(error))?
        } else {
            Self::with_topology(
                self.hucard_rom().to_vec(),
                cartridge,
                ControllerPort::default(),
                psg_revision,
                topology,
            )
            .map_err(|error| anyhow::anyhow!(error))?
        };
        restored.devices_mut().psg_mut().apply_runtime_config(
            runtime_audio.0,
            runtime_audio.1,
            runtime_audio.2,
            runtime_audio.3,
        );

        let mut reader = StateReader::new(data);
        read_section(&mut reader, 256, "CPU", |section| {
            restored.cpu.read_state(section, state_version)
        })?;
        if !restored.cpu.at_action_boundary() {
            bail!("PC Engine save-state is not at a CPU action boundary");
        }
        read_section(&mut reader, 256 * 1024, "bus", |section| {
            restored.bus.read_state(section)
        })?;
        read_section(&mut reader, 80 * 1024, "VDC", |section| {
            restored.bus.devices_mut().vdc_mut().read_state(section)
        })?;
        if topology == PceHardwareTopology::SuperGrafx {
            read_section(&mut reader, 80 * 1024, "VDC2", |section| {
                restored
                    .bus
                    .devices_mut()
                    .supergrafx_video_mut()
                    .expect("SuperGrafx state target has SuperGrafx devices")
                    .vdc2_mut()
                    .read_state(section)
            })?;
            read_section(&mut reader, 64, "VPC", |section| {
                restored
                    .bus
                    .devices_mut()
                    .supergrafx_video_mut()
                    .expect("SuperGrafx state target has SuperGrafx devices")
                    .vpc_mut()
                    .read_state(section)
            })?;
        }
        read_section(&mut reader, 2 * 1024, "VCE", |section| {
            restored.bus.devices_mut().vce_mut().read_state(section)
        })?;
        read_section(&mut reader, MAX_PSG_STATE_SECTION_BYTES, "PSG", |section| {
            restored.bus.devices_mut().psg_mut().read_state(section)
        })?;
        read_section(
            &mut reader,
            MAX_CONTROLLER_STATE_SECTION_BYTES,
            "controller",
            |section| {
                restored
                    .bus
                    .devices_mut()
                    .controller_mut()
                    .read_state(section)
            },
        )?;
        if is_cd {
            let (sample_rate, generation_enabled) =
                runtime_cd_audio.expect("CD state target has a retained CD audio configuration");
            let cdrom2 = restored
                .bus
                .devices_mut()
                .cdrom2_mut()
                .expect("restored CD state has CD hardware");
            cdrom2.set_sample_rate(sample_rate);
            cdrom2.set_sample_generation_enabled(generation_enabled);
            read_section(
                &mut reader,
                super::super::cdrom2::state::MAX_CDROM2_STATE_SECTION_BYTES,
                "CD-ROM2",
                |section| cdrom2.read_state(section),
            )?;
        }
        if has_arcade_card {
            read_section(
                &mut reader,
                super::super::arcade_card::MAX_ARCADE_CARD_STATE_SECTION_BYTES,
                "Arcade Card",
                |section| {
                    restored
                        .bus
                        .devices_mut()
                        .arcade_card_mut()
                        .expect("Arcade Card state target has Arcade Card hardware")
                        .read_state(section)
                },
            )?;
        }
        read_section(&mut reader, 64, "machine timing", |section| {
            restored.master_ticks = section.read_u64()?;
            restored.vce_line_accumulator = section.read_u64()?;
            if restored.vce_line_accumulator >= PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE {
                bail!(
                    "invalid machine VCE-line accumulator in save-state: {}",
                    restored.vce_line_accumulator
                );
            }
            restored.vdc_pixel_clock_remainder = section.read_u8()?;
            let pixel_divisor = restored.bus.devices().vce().pixel_clock().divisor();
            if restored.vdc_pixel_clock_remainder >= pixel_divisor {
                bail!(
                    "invalid machine VDC pixel-clock remainder in save-state: {}",
                    restored.vdc_pixel_clock_remainder
                );
            }
            restored.vce_line_index = section.read_u16()?;
            restored.vce_frame_length = match section.read_u8()? {
                0 => VceFrameLength::Lines262,
                1 => VceFrameLength::Lines263,
                tag => bail!("invalid VCE frame-length tag in save-state: {tag}"),
            };
            if restored.vce_line_index >= restored.vce_frame_length.scanlines() {
                bail!(
                    "invalid machine VCE line index in save-state: {}",
                    restored.vce_line_index
                );
            }
            restored.execution_state = match section.read_u8()? {
                0 => PceExecutionState::Running,
                1 => PceExecutionState::Suspended,
                tag => bail!("invalid machine execution-state tag in save-state: {tag}"),
            };
            restored.suspend_after_instruction = section.read_bool()?;
            if restored.execution_state == PceExecutionState::Suspended
                && restored.suspend_after_instruction
            {
                bail!("suspended PC Engine save-state has a pending debug step");
            }
            restored.trace_frame = if state_version >= 3 {
                section.read_u64()?
            } else {
                0
            };
            Ok(())
        })?;
        read_section(
            &mut reader,
            super::super::vdc_video::PCE_ACTIVE_FRAME_RGBA_BYTES + 5 * 512,
            "front video frame",
            |section| restored.front_video.read_state(section),
        )?;
        read_section(
            &mut reader,
            super::super::vdc_video::PCE_ACTIVE_FRAME_RGBA_BYTES + 5 * 512,
            "back video frame",
            |section| restored.back_video.read_state(section),
        )?;
        if !reader.is_exhausted() {
            bail!("PC Engine save-state payload has unexpected trailing data");
        }
        restored.validate_v1_encode_state()?;
        restored.opcode_history.enabled = history_enabled;
        restored.opcode_history.clear();
        restored.instruction_trace.set_capacity(trace_capacity);
        restored.instruction_trace.set_enabled(trace_enabled);
        restored.instruction_trace.clear();
        *self = restored;
        Ok(())
    }
}
