use super::logic::{read_bits, write_bits};
use super::*;

impl Board {
    pub fn write_state(&self, writer: &mut StateWriter) -> Result<()> {
        self.validate_state()?;
        writer.write_u8(4);
        writer.write_u64(self.half);
        writer.write_bool(self.clock_high);
        writer.write_bool(self.main.is_some());
        if let Some(main) = &self.main {
            main.write_state(writer);
        }
        self.grant.write_state(writer);
        self.arbiter.write_state(writer);
        write_bits(writer, self.outputs.bits());
        writer.write_u32(self.address);
        writer.write_u16(self.data);
        writer.write_u16(self.data_mask);
        write_bits(writer, self.held_controls);
        write_companion(writer, self.companion);
        write_companion(writer, self.last_companion);
        self.write_sound(writer);
        self.psg.write_state(writer);
        self.write_protocol(writer);
        Ok(())
    }

    pub fn read_state(reader: &mut StateReader<'_>, half: u64) -> Result<Self> {
        ensure!(
            reader.read_u8()? == 4 && reader.read_u64()? == half,
            "invalid MAIN board state frontier"
        );
        let clock_high = reader.read_bool()?;
        let main = if reader.read_bool()? {
            Some(main_bus::MainBus::read_state(reader)?)
        } else {
            None
        };
        let phase = main
            .as_ref()
            .map_or(main_bus::Phase::Idle, |main| main.snapshot().phase);
        let mut value = Self {
            half,
            next_pass: 0,
            clock_high,
            main,
            grant: grant::Grant::read_state(reader, clock_high, phase)?,
            arbiter: arbiter::Arbiter::read_state(reader, half)?,
            outputs: Outputs::from_bits(read_bits(reader)?),
            address: reader.read_u32()?,
            data: reader.read_u16()?,
            data_mask: reader.read_u16()?,
            instruction_origin: Default::default(),
            held_controls: read_bits(reader)?,
            companion: read_companion(reader)?,
            last_companion: read_companion(reader)?,
            sound: sound::SoundState::origin(),
            psg: psg::Psg::origin(),
            stage: None,
            protocol: None,
            protocol_seen: false,
            protocol_drive: None,
        };
        value.read_sound(reader)?;
        value.psg = psg::Psg::read_state(reader)?;
        value.read_protocol(reader)?;
        value.validate_state()?;
        Ok(value)
    }

    pub fn validate_state(&self) -> Result<()> {
        ensure!(
            self.next_pass == 0 && self.stage.is_none(),
            "MAIN board state inside a pass"
        );
        ensure!(
            self.address <= 0xfffffe && self.address & 1 == 0,
            "invalid held MAIN address"
        );
        self.arbiter.validate(self.half)?;
        ensure!(
            self.observed_clocks()[0] == self.clock_high,
            "MAIN and arbiter clock frontier differ"
        );
        self.validate_protocol()?;
        self.validate_psg()?;
        let first = Self::canonical_first_rise()?;
        if let Some(main) = &self.main {
            main.validate()
                .map_err(|error| anyhow::anyhow!("invalid MAIN phase state: {error:?}"))?;
            let snapshot = main.snapshot();
            ensure!(
                self.main_reset_released()
                    || snapshot.phase == main_bus::Phase::Idle
                        && snapshot.pending.is_none()
                        && snapshot.completed.is_none(),
                "MAIN transfer precedes ASIC RESET release"
            );
            ensure!(
                snapshot.origin_rise_half == first
                    && snapshot.now_half == self.half
                    && snapshot.clock_high == self.clock_high,
                "invalid MAIN clock origin or frontier"
            );
            if self.protocol.is_none() {
                if let Some(pending) = snapshot.pending {
                    self.validate_owner(pending.request)?;
                }
                if let Some(completed) = snapshot.completed {
                    self.validate_owner(completed.request)?;
                }
            }
            self.grant.validate(self.clock_high, snapshot.phase)?;
        } else {
            ensure!(
                self.half < first
                    && !self.clock_high
                    && self.grant == grant::Grant::software_origin(),
                "missing MAIN clock prefix history"
            );
        }
        if let Some(companion) = self.companion {
            self.validate_sound_continuation(companion)?;
            ensure!(
                companion.pass == 0 && Some(companion.half) == self.half.checked_add(1),
                "invalid future SOUND qualification"
            );
        }
        if self.half == 0 {
            let mut origin = Self::software_origin();
            origin.companion = self.companion;
            ensure!(*self == origin, "invalid MAIN board software origin");
        } else {
            let input = self.arbiter.held_inputs();
            ensure!(
                self.last_companion
                    .is_some_and(|companion| companion.half == self.half
                        && companion.pass == 1
                        && companion.bank == input.sound_bank
                        && companion
                            .pins
                            .address
                            .is_none_or(|a| a == self.sound.address)
                        && self.sound.address == input.sound_address
                        && companion.pins.bus_ack != self.arbiter.sound_acknowledge_level()
                        && companion.pins.m1 != input.m1
                        && companion.pins.mreq != input.sound_memory_level
                        && companion.pins.read != input.sound_read_level
                        && companion.pins.write != input.sound_write_level
                        && companion.pins.iorq != input.sound_io_level),
                "missing SOUND companion provenance"
            );
            if let Some(companion) = self.last_companion {
                companion.validate()?;
            }
            let zv = self.arbiter.sound_to_main_level();
            let vz = self.arbiter.main_to_sound_level();
            let driven = !((zv || input.cas) && (vz || !input.cas));
            let shift = if self.sound.address & 1 == 0 { 8 } else { 0 };
            let known = if driven {
                (self.sound.main_mask >> shift) as u8
            } else {
                0
            };
            ensure!(
                self.sound.response
                    == SoundResponse {
                        wait: !self.arbiter.wait_level,
                        request: self.arbiter.sound_request(),
                        reset: self.arbiter.sound_reset(),
                        data: ((input.data >> shift) as u8) & known,
                        known,
                    },
                "invalid SOUND byte-bridge output"
            );
        }
        Ok(())
    }

    pub fn canonical_first_rise() -> Result<u64> {
        let mut clock = super::super::clock::ClockCone::software_origin(false);
        let mut feedback = false;
        // The MAIN divider and its reset chain do not consume EDCLK, PAL, or R12.
        for half in 1..=14 {
            for _ in 0..2 {
                if feedback {
                    return Ok(half);
                }
                feedback = clock.cpu_clk1();
                clock.eval_mclk((half + 3) & 1 != 0, false, false, 0);
            }
        }
        anyhow::bail!("MAIN origin divider did not produce its first rise")
    }
}

impl Outputs {
    fn bits(self) -> [bool; 12] {
        [
            self.request_level,
            self.acknowledge_level,
            self.grant_acknowledge_level,
            self.vpa_level,
            self.cart_enable_level,
            self.strobe_direction,
            self.read_direction,
            self.address_strobe_level,
            self.upper_level,
            self.lower_level,
            self.low_address_direction,
            self.high_address_direction,
        ]
    }

    fn from_bits(bits: [bool; 12]) -> Self {
        let [
            request_level,
            acknowledge_level,
            grant_acknowledge_level,
            vpa_level,
            cart_enable_level,
            strobe_direction,
            read_direction,
            address_strobe_level,
            upper_level,
            lower_level,
            low_address_direction,
            high_address_direction,
        ] = bits;
        Self {
            request_level,
            acknowledge_level,
            grant_acknowledge_level,
            vpa_level,
            cart_enable_level,
            strobe_direction,
            read_direction,
            address_strobe_level,
            upper_level,
            lower_level,
            low_address_direction,
            high_address_direction,
        }
    }
}

fn write_companion(writer: &mut StateWriter, companion: Option<Companion>) {
    writer.write_bool(companion.is_some());
    if let Some(companion) = companion {
        writer.write_u64(companion.half);
        writer.write_u8(companion.pass);
        sound::write_pins(writer, companion.pins);
        writer.write_u16(companion.bank);
    }
}

fn read_companion(reader: &mut StateReader<'_>) -> Result<Option<Companion>> {
    Ok(if reader.read_bool()? {
        Some(Companion {
            half: reader.read_u64()?,
            pass: reader.read_u8()?,
            pins: sound::read_pins(reader)?,
            bank: reader.read_u16()?,
        })
    } else {
        None
    })
}
