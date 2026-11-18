use super::*;

#[derive(Clone, Copy)]
pub(super) struct ReadContext<'a> {
    pub region: Region,
    pub input: &'a Input,
    pub prefetch: Prefetch,
}

impl ReadContext<'_> {
    pub(super) fn read(
        self,
        access: BusAccess,
        target: ProtocolTarget,
        now: u64,
        sound: &SoundOwner,
        audio: &Audio,
    ) -> Result<(u16, u16)> {
        let bus_ack = sound.pins().bus_ack;
        let sound = sound.memory();
        let address = access.bus_address;
        let word = match target {
            ProtocolTarget::Io => {
                let byte = match address {
                    IO_VERSION..=IO_VERSION_END => self.region.io_version(),
                    IO_DATA_START..=IO_DATA_END => {
                        let port = ((address - IO_DATA_START) / IO_REGISTER_STRIDE) as usize;
                        ensure!(self.input.data_read_allowed(port), "unqualified input read");
                        self.input.read_data(port, now, self.region)
                    }
                    IO_CONTROL_START..=IO_CONTROL_END => self
                        .input
                        .read_control(((address - IO_CONTROL_START) / IO_REGISTER_STRIDE) as usize),
                    IO_SERIAL_START..=IO_SERIAL_END => {
                        let register = ((address - IO_SERIAL_START) / IO_REGISTER_STRIDE) as usize;
                        ensure!(
                            self.input.serial_read_allowed(register),
                            "unqualified serial read"
                        );
                        self.input.read_serial(register, now, self.region)
                    }
                    _ => anyhow::bail!("unowned IO read {address:06x}"),
                };
                u16::from_be_bytes([byte; 2])
            }
            ProtocolTarget::SoundControl => {
                let word = self
                    .prefetch
                    .sample(access.lanes)
                    .ok_or_else(|| anyhow::anyhow!("SOUND control has no surviving prefetch"))?;
                if address & SOUND_CONTROL_ADDRESS_MASK == SOUND_BUS_REQUEST
                    && access.lanes != ByteLanes::Lower
                {
                    return Ok((word & !0x100, lane_mask(access.lanes) & !0x100));
                }
                word
            }
            ProtocolTarget::SoundRam => {
                if !bus_ack || sound.reset_asserted {
                    self.prefetch
                        .sample(access.lanes)
                        .ok_or_else(|| anyhow::anyhow!("closed SOUND aperture has no prefetch"))?
                } else {
                    let address = ((address + u32::from(access.lanes == ByteLanes::Lower))
                        & SOUND_MAIN_MASK) as u16;
                    let byte = crate::hardware::vdp::owner::sound_cpu::local_byte(
                        sound, audio, now, address,
                    )?;
                    u16::from_be_bytes([byte; 2])
                }
            }
            _ => anyhow::bail!("unowned MAIN protocol read {target:?} {address:06x}"),
        };
        let mask = lane_mask(access.lanes);
        Ok((word & mask, mask))
    }
}

impl<S: PhysicalCapture> Context<'_, S> {
    pub(super) fn protocol_write(
        &mut self,
        access: BusAccess,
        target: ProtocolTarget,
        word: u16,
        begin: u64,
        now: u64,
    ) -> Result<()> {
        let address = access.bus_address;
        match target {
            ProtocolTarget::Io => {
                if access.lanes == ByteLanes::Upper {
                    return Ok(());
                }
                let byte = word as u8;
                let accepted = match address {
                    IO_VERSION => true,
                    IO_DATA_START..=IO_DATA_END => self.input.write_data(
                        ((address - IO_DATA_START) / IO_REGISTER_STRIDE) as usize,
                        byte,
                        now,
                        self.region,
                    ),
                    IO_CONTROL_START..=IO_CONTROL_END => self.input.write_control(
                        ((address - IO_CONTROL_START) / IO_REGISTER_STRIDE) as usize,
                        byte,
                        now,
                        self.region,
                    ),
                    IO_SERIAL_START..=IO_SERIAL_END => {
                        let register = ((address - IO_SERIAL_START) / IO_REGISTER_STRIDE) as usize;
                        if matches!(
                            super::super::input_timed::serial_register(access),
                            Some(0 | 3)
                        ) {
                            self.input.write_serial_at_range(
                                register,
                                byte,
                                begin,
                                now,
                                self.region,
                            )
                        } else {
                            self.input.write_serial(register, byte, now, self.region)
                        }
                    }
                    _ => false,
                };
                ensure!(accepted, "unqualified IO write {address:06x}");
            }
            ProtocolTarget::SoundControl | ProtocolTarget::SoundRam | ProtocolTarget::Psg => {
                self.write_sound_protocol(access, target, word, now)?
            }
            ProtocolTarget::Time => {
                let save = self
                    .cartridge
                    .save
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("unowned TIME write"))?;
                ensure!(
                    save.accepts_control_address(address),
                    "unqualified TIME save control"
                );
                let accepted = if access.lanes == ByteLanes::Upper {
                    access.logical_address == crate::cartridge::ram::SAVE_CONTROL
                        && save.unchanged_upper_control((word >> 8) as u8)
                } else {
                    save.control(word as u8, access.lanes == ByteLanes::Both)
                };
                ensure!(accepted, "unqualified TIME save value");
            }
        }
        Ok(())
    }
}

fn lane_mask(lanes: ByteLanes) -> u16 {
    match lanes {
        ByteLanes::Upper => 0xff00,
        ByteLanes::Lower => 0xff,
        ByteLanes::Both => 0xffff,
    }
}
