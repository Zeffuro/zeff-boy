use super::*;

impl Z80Bus for CandidateBus<'_> {
    fn cpu_read(&self, address: u16) -> u8 {
        #[cfg(feature = "sound-shadow-observer")]
        observer::access(address, Direction::Read);
        self.record();
        #[cfg(feature = "first-failure-diagnostic")]
        self.diagnostic_access(address, Direction::Read, None);
        if let Some(value) = self.memory.read(address) {
            return value;
        }
        if (SOUND_OPN2_START..=SOUND_OPN2_END).contains(&address) {
            return self.read_opn2((address & 3) as u8);
        }
        if address >= SOUND_BANK_WINDOW {
            if !svp_access_allowed(self.cartridge, self.bank_address(address), Direction::Read) {
                self.fail(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Read,
                });
                return UNPOPULATED_ROM_BYTE;
            }
            if self.cartridge.sf004.is_some()
                || self.cartridge.extended_ssf.is_some()
                || self.cartridge.realtec.is_some()
            {
                let target = self.bank_address(address);
                if self.cartridge.realtec.is_some() && target & !1 == 0x100 {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    });
                    return UNPOPULATED_ROM_BYTE;
                }
                let Some(value) = self
                    .cartridge
                    .rom_source_offset(target)
                    .and_then(|offset| self.cartridge.source_byte(offset))
                else {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    });
                    return UNPOPULATED_ROM_BYTE;
                };
                self.cartridge_read.set(Some(address));
                self.timed_read.set(true);
                return self.bank_read(target, value);
            }
            let target = self.bank_address(address);
            if self.cartridge.download.is_some() && target <= CARTRIDGE_END {
                self.cartridge_read.set(Some(address));
                self.timed_read.set(true);
                return self.bank_read(target, self.cartridge.read(target).unwrap());
            }
            if self.cartridge.expansion_save_contains(target) {
                self.fail(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Read,
                });
                return UNPOPULATED_ROM_BYTE;
            }
            if self.cartridge.parallel_adapter_domain(target)
                || self.cartridge.bounded_split_hole(target)
                || self.cartridge.bounded_odd_guarded(target)
                || self.cartridge.bounded_switched_guarded(target)
            {
                self.fail(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Read,
                });
                return UNPOPULATED_ROM_BYTE;
            }
            if self.cartridge.linear_rom.is_some() && target <= CARTRIDGE_UNUSED_END {
                let Some(value) = self.cartridge.read(target) else {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    });
                    return UNPOPULATED_ROM_BYTE;
                };
                return self.bank_read(target, value);
            }
            if let Some(port) = Self::bank_opn2_port(target) {
                return self.bank_read(target, self.read_opn2(port));
            }
            if self.cartridge.sf002.is_some() && target <= CARTRIDGE_END {
                if self.cartridge.mode_sensitive_read(target)
                    || (0x3c0000..=0x3c3fff).contains(&target)
                {
                    self.cartridge_read.set(Some(address));
                    self.timed_read.set(true);
                }
                let Some(value) = self.sf002_writes.read(self.cartridge, target) else {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    });
                    return UNPOPULATED_ROM_BYTE;
                };
                return self.bank_read(target, value);
            }
            if self.cartridge.sf001.is_some() && target <= CARTRIDGE_UNUSED_END {
                self.cartridge_read.set(Some(address));
                self.timed_read.set(true);
                return self.bank_read(target, self.sf001_writes.read(self.cartridge, target));
            }
            if let Some(profile) = self.cartridge.protection {
                if let Some(value) = profile.read(target) {
                    return self.bank_read(target, value);
                }
                if profile.rejects_missing_byte(target) {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    });
                    return UNPOPULATED_ROM_BYTE;
                }
            }
            if let Some(registers) = &self.cartridge.registers
                && registers.contains(target)
            {
                if let Some(value) = registers.read(target) {
                    self.cartridge_read.set(Some(address));
                    self.timed_read.set(true);
                    return self.bank_read(target, value);
                }
                self.fail(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Read,
                });
                return UNPOPULATED_ROM_BYTE;
            }
            if target <= CARTRIDGE_END {
                if self
                    .cartridge
                    .registers
                    .as_ref()
                    .is_some_and(|board| board.is_banked())
                {
                    if self.cartridge.mode_sensitive_read(target) {
                        self.cartridge_read.set(Some(address));
                        self.timed_read.set(true);
                    }
                    let Some(value) = self.cartridge.read(target) else {
                        self.fail(SoundFault::UnsupportedAccess {
                            address,
                            direction: Direction::Read,
                        });
                        return UNPOPULATED_ROM_BYTE;
                    };
                    return self.bank_read(target, value);
                }
                if self.cartridge.ssf2.is_some() {
                    if self.cartridge.mode_sensitive_read(target) {
                        self.cartridge_read.set(Some(address));
                        self.timed_read.set(true);
                    }
                    let Some(value) = self.cartridge.read(target) else {
                        self.fail(SoundFault::UnsupportedAccess {
                            address,
                            direction: Direction::Read,
                        });
                        return UNPOPULATED_ROM_BYTE;
                    };
                    return self.bank_read(target, value);
                }
                if self
                    .cartridge
                    .save
                    .as_ref()
                    .is_some_and(|save| save.contains(target))
                {
                    if self.cartridge.save.as_ref().unwrap().banked_readable() {
                        self.cartridge_read.set(Some(address));
                        self.timed_read.set(true);
                        return self.bank_read(target, self.cartridge.read(target).unwrap());
                    }
                    // Mutable bank reads need publication-time sampling.
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    });
                    return UNPOPULATED_ROM_BYTE;
                }
                if target >= 0x200000
                    && (self.cartridge.lock_on.is_some() || self.cartridge.lock_on_save.is_some())
                {
                    if self.cartridge.mode_sensitive_read(target) {
                        self.cartridge_read.set(Some(address));
                        self.timed_read.set(true);
                    }
                    let Some(value) = self.cartridge.read(target) else {
                        self.fail(SoundFault::UnsupportedAccess {
                            address,
                            direction: Direction::Read,
                        });
                        return UNPOPULATED_ROM_BYTE;
                    };
                    return self.bank_read(target, value);
                }
                if self.cartridge.save_latch.is_some() {
                    let Some(value) = self.cartridge.read(target) else {
                        self.fail(SoundFault::UnsupportedAccess {
                            address,
                            direction: Direction::Read,
                        });
                        return UNPOPULATED_ROM_BYTE;
                    };
                    return self.bank_read(target, value);
                }
                if self.cartridge.svp.is_some() {
                    return self.svp_rom_read(address, target);
                }
                return self.bank_read(
                    target,
                    self.cartridge
                        .source_byte(target as usize)
                        .unwrap_or(UNPOPULATED_ROM_BYTE),
                );
            }
            if (CARTRIDGE_END + 1..=CARTRIDGE_UNUSED_END).contains(&target) {
                return self.bank_read(target, SOUND_DISCONNECTED_BYTE);
            }
            if (WORK_RAM_START..=BUS_ADDRESS_MASK).contains(&target) {
                return self.bank_read(target, SOUND_DISCONNECTED_BYTE);
            }
        }
        self.fail(SoundFault::UnsupportedAccess {
            address,
            direction: Direction::Read,
        });
        UNPOPULATED_ROM_BYTE
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        #[cfg(feature = "sound-shadow-observer")]
        observer::access(address, Direction::Write);
        self.record();
        #[cfg(feature = "first-failure-diagnostic")]
        self.diagnostic_access(address, Direction::Write, Some(value));
        if self.memory.write(address, value) {
            return;
        }
        if (SOUND_OPN2_START..=SOUND_OPN2_END).contains(&address)
            && self.opn2_writes.push((address & 3) as u8, value)
        {
            return;
        }
        if (SOUND_PSG_START..=SOUND_PSG_END).contains(&address)
            && (address & 1 == 0 || self.psg_writes.push(value))
        {
            return;
        }
        if address >= SOUND_BANK_WINDOW {
            let target = self.bank_address(address);
            if !svp_access_allowed(self.cartridge, target, Direction::Write) {
                self.fail(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                });
                return;
            }
            if self.cartridge.sf004.is_some()
                || self.cartridge.extended_ssf.is_some()
                || self.cartridge.realtec.is_some()
            {
                self.fail(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                });
                return;
            }
            let target = self.bank_address(address);
            if self.cartridge.download.is_some()
                && (target <= CARTRIDGE_END || crate::cartridge::download::time(target))
            {
                if crate::cartridge::download::memory(target) {
                    self.timed_read.set(true);
                    self.capture_bank(target, Direction::Write, value);
                } else {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Write,
                    });
                }
                return;
            }
            if self.cartridge.expansion_save_contains(target) {
                self.fail(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                });
                return;
            }
            if self.cartridge.parallel_adapter_domain(target)
                || self.cartridge.bounded_split_hole(target)
                || self.cartridge.bounded_odd_guarded(target)
                || self.cartridge.bounded_switched_guarded(target)
            {
                self.fail(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                });
                return;
            }
            if self.cartridge.linear_rom.is_some() && target <= CARTRIDGE_UNUSED_END {
                self.fail(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                });
                return;
            }
            if self
                .cartridge
                .lock_on_save
                .as_ref()
                .is_some_and(|save| save.contains(target))
            {
                self.fail(SoundFault::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                });
                return;
            }
            if let Some(port) = Self::bank_opn2_port(target)
                && self.opn2_writes.push(port, value)
            {
                self.capture_bank(target, Direction::Write, value);
                return;
            }
            if self.cartridge.sf002.is_some()
                && (target <= CARTRIDGE_END
                    || crate::cartridge::sf002::Sf002::rejects_time_write(target))
            {
                if !self.sf002_writes.push(self.cartridge, target, value) {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Write,
                    });
                    return;
                }
                self.capture_bank(target, Direction::Write, value);
                return;
            }
            if self.cartridge.sf001.is_some() && Sf001Writes::accepts(self.cartridge, target) {
                if !self.sf001_writes.push(target, value) {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Write,
                    });
                    return;
                }
                self.capture_bank(target, Direction::Write, value);
                return;
            }
            if let Some(profile) = self.cartridge.protection
                && profile.contains(target)
            {
                if profile.rejects_write(target) {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Write,
                    });
                    return;
                }
                self.capture_bank(target, Direction::Write, value);
                return;
            }
            if let Some(registers) = &self.cartridge.registers
                && registers.contains(target)
            {
                if registers.is_banked() {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Write,
                    });
                    return;
                }
                if let Some(index) = crate::cartridge::registers::CartridgeRegisters::index(target)
                    && !self.register_writes.push(index, value)
                {
                    self.fail(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Write,
                    });
                    return;
                }
                self.capture_bank(target, Direction::Write, value);
                return;
            }
            if target <= CARTRIDGE_END
                && !self
                    .cartridge
                    .save
                    .as_ref()
                    .is_some_and(|save| save.contains(target))
            {
                self.capture_bank(target, Direction::Write, value);
                return;
            }
            if target & VDP_DECODE_MASK == VDP_DECODE_VALUE
                && (PSG_PORT_START..=PSG_PORT_END).contains(&(target & VDP_PORT_MASK))
                && (target & 1 == 0 || self.psg_writes.push(value))
            {
                self.capture_bank(target, Direction::Write, value);
                return;
            }
            if (WORK_RAM_START..=BUS_ADDRESS_MASK).contains(&target)
                && self.writes.push(target, value)
            {
                self.capture_bank(target, Direction::Write, value);
                return;
            }
        }
        self.fail(SoundFault::UnsupportedAccess {
            address,
            direction: Direction::Write,
        });
    }
    fn io_read(&mut self, _: u8) -> u8 {
        self.record();
        SOUND_DISCONNECTED_BYTE
    }
    fn io_write(&mut self, _: u8, _: u8) {
        self.record();
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.memory.irq_asserted
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        false
    }
}
