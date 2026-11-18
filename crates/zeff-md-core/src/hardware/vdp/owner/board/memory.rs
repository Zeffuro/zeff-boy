use super::*;

impl Board {
    pub(super) fn respond_memory<P: previous::TagSink, H: HalfCapture>(
        &mut self,
        pass: (Stage, BoardView),
        storage: &impl Storage,
        previous: &mut P,
        physical: &mut H,
        #[cfg(test)] witness: Option<&mut Option<work_witness::Pulse>>,
    ) -> Result<Vec<Effect>> {
        let (stage, view) = pass;
        let mut mask = self.data_mask;
        let mut sources = previous::Sources::default();
        let mut sound_sources = sound_source::Sources::default();
        let sound_source = VdpSource::Sound {
            logical: stage.companion.pins.address.unwrap_or(0),
            bank: stage.companion.bank,
            write: stage.companion.pins.shared() && stage.companion.pins.write,
        };
        #[cfg(test)]
        let mut ram_read_lanes = [false; 2];
        #[cfg(test)]
        let mut cartridge_drive = false;
        if let Some(address) = stage.preview.drives.address {
            ensure!(
                address <= 0xffffff && address & 1 == 0,
                "invalid VDP MAIN address drive"
            );
            self.address = address;
            physical.vdp(VdpEvent::Drive {
                slot: VdpSlot::Address,
                mask: 0xffffff,
                source: VdpSource::Foreign,
            });
            #[cfg(test)]
            physical.vdp_raw(VdpRaw::Drive {
                driver: VdpDriver::Vdp,
                slot: VdpSlot::Address,
                mask: 0xffffff,
                value: address,
            });
        }
        if let Some(word) = stage.preview.data {
            self.instruction_origin.0.clear_latch();
            self.data = word;
            physical.vdp(VdpEvent::Drive {
                slot: VdpSlot::Data,
                mask: 0xffff,
                source: VdpSource::Foreign,
            });
            #[cfg(test)]
            physical.vdp_raw(VdpRaw::Drive {
                driver: VdpDriver::Vdp,
                slot: VdpSlot::Data,
                mask: 0xffff,
                value: u32::from(self.data),
            });
            mask = 0xffff;
        }
        if self.arbiter.bus_status_read {
            self.instruction_origin.0.clear_latch();
            self.data = (self.data & !0x100)
                | if stage.arbiter.sound_bus_acknowledge_level {
                    0x100
                } else {
                    0
                };
            physical.vdp(VdpEvent::Drive {
                slot: VdpSlot::Data,
                mask: 0x100,
                source: VdpSource::Foreign,
            });
            #[cfg(test)]
            physical.vdp_raw(VdpRaw::Drive {
                driver: VdpDriver::Status,
                slot: VdpSlot::Data,
                mask: 0x100,
                value: u32::from(self.data),
            });
            mask |= 0x100;
        }
        let (sound_data_drive, sound_low_address) = self.sound_bridge(stage);
        if sound_low_address {
            self.address = self.address & !0xfe | u32::from(self.sound.address & 0xfe);
            physical.vdp(VdpEvent::Drive {
                slot: VdpSlot::Address,
                mask: 0xfe,
                source: sound_source,
            });
            #[cfg(test)]
            physical.vdp_raw(VdpRaw::Drive {
                driver: VdpDriver::SoundLow,
                slot: VdpSlot::Address,
                mask: 0xfe,
                value: self.address,
            });
            if H::ENABLED {
                sound_sources.address |= 0xfe;
            }
        }
        let high = u32::from(self.arbiter.sound_address_high()) << 8;
        if !self.arbiter.low_address_direction() {
            self.address &= !0x1fff00;
            self.address |= high & 0x1fff00;
            physical.vdp(VdpEvent::Drive {
                slot: VdpSlot::Address,
                mask: 0x1fff00,
                source: VdpSource::ArbiterHigh,
            });
            #[cfg(test)]
            physical.vdp_raw(VdpRaw::Drive {
                driver: VdpDriver::SoundHigh,
                slot: VdpSlot::Address,
                mask: 0x1fff00,
                value: self.address,
            });
            if H::ENABLED {
                sound_sources.address |= 0x1fff00;
            }
        }
        if !self.arbiter.high_address_direction() {
            self.address &= !0xe00000;
            self.address |= high & 0xe00000;
            physical.vdp(VdpEvent::Drive {
                slot: VdpSlot::Address,
                mask: 0xe00000,
                source: VdpSource::ArbiterHigh,
            });
            #[cfg(test)]
            physical.vdp_raw(VdpRaw::Drive {
                driver: VdpDriver::SoundHigh,
                slot: VdpSlot::Address,
                mask: 0xe00000,
                value: self.address,
            });
            if H::ENABLED {
                sound_sources.address |= 0xe00000;
            }
        }
        if sound_data_drive {
            self.instruction_origin.0.clear_latch();
            if let Some(data) = stage.companion.pins.data {
                self.data = u16::from(data) * 0x101;
                physical.vdp(VdpEvent::Drive {
                    slot: VdpSlot::Data,
                    mask: 0xffff,
                    source: sound_source,
                });
                #[cfg(test)]
                physical.vdp_raw(VdpRaw::Drive {
                    driver: VdpDriver::SoundData,
                    slot: VdpSlot::Data,
                    mask: 0xffff,
                    value: u32::from(self.data),
                });
                if H::ENABLED {
                    sound_sources.data = [true; 2];
                }
                mask = 0xffff;
            } else {
                mask = 0;
                physical.vdp(VdpEvent::Drive {
                    slot: VdpSlot::Data,
                    mask: 0xffff,
                    source: VdpSource::Unknown,
                });
                #[cfg(test)]
                physical.vdp_raw(VdpRaw::Drive {
                    driver: VdpDriver::Undriven,
                    slot: VdpSlot::Data,
                    mask: 0xffff,
                    value: u32::from(self.data),
                });
            }
        }
        let released = self.grant.tristate() && !stage.pins.address_strobe;
        if !released {
            if let Some(address) = stage.pins.address {
                self.address = address;
                physical.vdp(VdpEvent::Drive {
                    slot: VdpSlot::Address,
                    mask: 0xffffff,
                    source: VdpSource::Foreign,
                });
                #[cfg(test)]
                physical.vdp_raw(VdpRaw::Drive {
                    driver: VdpDriver::Main,
                    slot: VdpSlot::Address,
                    mask: 0xffffff,
                    value: self.address,
                });
                if H::ENABLED {
                    sound_sources.address = 0;
                }
                if P::ENABLED {
                    sources.address = true;
                }
            }
            if let Some(word) = stage.pins.write_word {
                self.instruction_origin.0.clear_latch();
                self.data = word;
                physical.vdp(VdpEvent::Drive {
                    slot: VdpSlot::Data,
                    mask: 0xffff,
                    source: VdpSource::Foreign,
                });
                #[cfg(test)]
                physical.vdp_raw(VdpRaw::Drive {
                    driver: VdpDriver::Main,
                    slot: VdpSlot::Data,
                    mask: 0xffff,
                    value: u32::from(self.data),
                });
                if H::ENABLED {
                    sound_sources.data = [false; 2];
                }
                if P::ENABLED {
                    sources.data = [true; 2];
                }
                mask = 0xffff;
            }
        }
        let protocol_mask = self.apply_protocol_drive(stage, &mut mask)?;
        if H::ENABLED {
            sound_sources.clear_data_mask(protocol_mask);
        }
        physical.vdp(VdpEvent::Drive {
            slot: VdpSlot::Data,
            mask: u32::from(protocol_mask),
            source: VdpSource::Foreign,
        });
        #[cfg(test)]
        physical.vdp_raw(VdpRaw::Drive {
            driver: VdpDriver::Protocol,
            slot: VdpSlot::Data,
            mask: u32::from(protocol_mask),
            value: u32::from(self.data),
        });
        let word_address = self.address >> 1;
        let mut low_word = word_address & 0x5fff;
        if self.arbiter.ram_address14 {
            low_word |= 0x2000;
        }
        low_word ^= 0x2000;
        let high = ((word_address & 0x7fff) * 2) as u16;
        let low = (low_word * 2 + 1) as u16;
        if view.drives.levels.row_strobe {
            if !self.arbiter.lower_oe_level {
                if H::ENABLED {
                    sound_sources.data[1] = false;
                }
                if P::ENABLED {
                    sources.data[1] = false;
                }
                #[cfg(test)]
                {
                    ram_read_lanes[1] = true;
                }
                self.instruction_origin.0.clear_latch();
                self.data =
                    (self.data & 0xff00) | u16::from(storage.main_byte(0xff0000 | u32::from(low))?);
                physical.vdp(VdpEvent::Drive {
                    slot: VdpSlot::Data,
                    mask: 0xff,
                    source: VdpSource::Foreign,
                });
                #[cfg(test)]
                physical.vdp_raw(VdpRaw::Drive {
                    driver: VdpDriver::Ram,
                    slot: VdpSlot::Data,
                    mask: 0xff,
                    value: u32::from(self.data),
                });
                mask |= 0xff;
            }
            if !self.arbiter.upper_oe_level {
                if H::ENABLED {
                    sound_sources.data[0] = false;
                }
                if P::ENABLED {
                    sources.data[0] = false;
                }
                #[cfg(test)]
                {
                    ram_read_lanes[0] = true;
                }
                self.instruction_origin.0.clear_latch();
                self.data = (self.data & 0xff)
                    | u16::from(storage.main_byte(0xff0000 | u32::from(high))?) << 8;
                physical.vdp(VdpEvent::Drive {
                    slot: VdpSlot::Data,
                    mask: 0xff00,
                    source: VdpSource::Foreign,
                });
                #[cfg(test)]
                physical.vdp_raw(VdpRaw::Drive {
                    driver: VdpDriver::Ram,
                    slot: VdpSlot::Data,
                    mask: 0xff00,
                    value: u32::from(self.data),
                });
                mask |= 0xff00;
            }
        }
        if !self.outputs.cart_enable_level && stage.preview.drives.levels.column_strobe {
            if H::ENABLED {
                sound_sources.data = [false; 2];
            }
            if P::ENABLED {
                sources.data = [false; 2];
            }
            #[cfg(test)]
            {
                cartridge_drive = true;
            }
            // The selected fixed ROM wires only the low 22 byte-address bits.
            let address = self.address & 0x3fffff;
            let bytes = [
                storage.main_byte_sampled(address)?,
                storage.main_byte_sampled(address + 1)?,
            ];
            self.data = u16::from_be_bytes(bytes.map(|byte| byte.value));
            physical.vdp(VdpEvent::Drive {
                slot: VdpSlot::Data,
                mask: 0xffff,
                source: VdpSource::Foreign,
            });
            #[cfg(test)]
            physical.vdp_raw(VdpRaw::Drive {
                driver: VdpDriver::Cartridge,
                slot: VdpSlot::Data,
                mask: 0xffff,
                value: u32::from(self.data),
            });
            let key = self.cart_read_key(stage, released);
            self.instruction_origin.0.drive(key, bytes);
            mask = 0xffff;
        }
        self.data_mask = mask;
        let mut effects = Vec::new();
        if view.pass == 1 && view.drives.levels.row_strobe {
            if view.drives.levels.lower_write {
                if P::ENABLED
                    && let Some(tag) = sources.tag(
                        self,
                        stage,
                        released,
                        (view.half, effects.len()),
                        (1, low, self.data as u8),
                    )
                {
                    previous.record(tag);
                }
                if H::ENABLED
                    && let Some(tag) = sound_sources.tag(
                        stage,
                        (
                            crate::sound_trace::physical::Stamp {
                                half: view.half,
                                pass: view.pass,
                            },
                            effects.len(),
                        ),
                        (1, low, self.data as u8),
                    )
                {
                    physical.work_tag(tag);
                }
                effects.push(Effect::WorkRam {
                    address: low,
                    value: self.data as u8,
                });
            }
            if view.drives.levels.upper_write {
                if P::ENABLED
                    && let Some(tag) = sources.tag(
                        self,
                        stage,
                        released,
                        (view.half, effects.len()),
                        (0, high, (self.data >> 8) as u8),
                    )
                {
                    previous.record(tag);
                }
                if H::ENABLED
                    && let Some(tag) = sound_sources.tag(
                        stage,
                        (
                            crate::sound_trace::physical::Stamp {
                                half: view.half,
                                pass: view.pass,
                            },
                            effects.len(),
                        ),
                        (0, high, (self.data >> 8) as u8),
                    )
                {
                    physical.work_tag(tag);
                }
                effects.push(Effect::WorkRam {
                    address: high,
                    value: (self.data >> 8) as u8,
                });
            }
        }
        #[cfg(test)]
        if let Some(witness) = witness
            && !effects.is_empty()
        {
            *witness = Some(work_witness::Pulse {
                half: view.half,
                pass: view.pass,
                main: self.snapshot(),
                pins: stage.pins,
                companion: stage.companion,
                released,
                grant_tristate: self.grant.tristate(),
                main_address_selected: !released && stage.pins.address.is_some(),
                main_data_selected: !released && stage.pins.write_word.is_some(),
                protocol: self.protocol.is_some(),
                protocol_drive: self.protocol_drive.is_some(),
                ram_read_lanes,
                cartridge_drive,
                sound_data_drive,
                sound_low_address,
                address: self.address,
                data: self.data,
                ram_address14: self.arbiter.ram_address14,
                high,
                low,
                row_strobe: view.drives.levels.row_strobe,
                write_strobes: [
                    view.drives.levels.upper_write,
                    view.drives.levels.lower_write,
                ],
                effects: effects
                    .iter()
                    .map(|effect| match *effect {
                        Effect::WorkRam { address, value } => (address, value),
                        _ => unreachable!(),
                    })
                    .collect(),
            });
        }
        Ok(effects)
    }
}
