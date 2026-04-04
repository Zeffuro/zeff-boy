use crate::hardware::cartridge::{Mapper, Mirroring};

const AY_PRESCALER_PERIOD: u16 = 16;

const AY_VOLUME_TABLE: [f32; 16] = [
    0.0, 0.0074, 0.0108, 0.0158, 0.0230, 0.0337, 0.0492, 0.0719, 0.1050, 0.1534, 0.2241,
    0.3273, 0.4781, 0.6984, 1.0, 1.0,
];

struct Sunsoft5BAudio {
    command: u8,
    regs: [u8; 16],
    channels: [Sunsoft5BChannel; 3],
    envelope: Sunsoft5BEnvelope,
    prescaler: u16,
}

impl Sunsoft5BAudio {
    fn new() -> Self {
        Self {
            command: 0,
            regs: [0; 16],
            channels: [
                Sunsoft5BChannel::new(),
                Sunsoft5BChannel::new(),
                Sunsoft5BChannel::new(),
            ],
            envelope: Sunsoft5BEnvelope::new(),
            prescaler: 0,
        }
    }

    fn write_command(&mut self, val: u8) {
        self.command = val & 0x0F;
    }

    fn write_data(&mut self, val: u8) {
        let reg = self.command as usize;
        if reg < 16 {
            self.regs[reg] = val;
        }
        match reg {
            0 => self.channels[0].period = (self.channels[0].period & 0xF00) | val as u16,
            1 => {
                self.channels[0].period =
                    (self.channels[0].period & 0x0FF) | ((val as u16 & 0x0F) << 8);
            }
            2 => self.channels[1].period = (self.channels[1].period & 0xF00) | val as u16,
            3 => {
                self.channels[1].period =
                    (self.channels[1].period & 0x0FF) | ((val as u16 & 0x0F) << 8);
            }
            4 => self.channels[2].period = (self.channels[2].period & 0xF00) | val as u16,
            5 => {
                self.channels[2].period =
                    (self.channels[2].period & 0x0FF) | ((val as u16 & 0x0F) << 8);
            }
            // R6: noise period, implement later but almost not used.
            7 => {
                self.channels[0].tone_disable = val & 0x01 != 0;
                self.channels[1].tone_disable = val & 0x02 != 0;
                self.channels[2].tone_disable = val & 0x04 != 0;
            }
            8 => {
                self.channels[0].use_envelope = val & 0x10 != 0;
                self.channels[0].volume = val & 0x0F;
            }
            9 => {
                self.channels[1].use_envelope = val & 0x10 != 0;
                self.channels[1].volume = val & 0x0F;
            }
            10 => {
                self.channels[2].use_envelope = val & 0x10 != 0;
                self.channels[2].volume = val & 0x0F;
            }
            11 => {
                self.envelope.period = (self.envelope.period & 0xFF00) | val as u16;
            }
            12 => {
                self.envelope.period = (self.envelope.period & 0x00FF) | ((val as u16) << 8);
            }
            13 => {
                self.envelope.trigger(val & 0x0F);
            }
            _ => {}
        }
    }

    fn tick(&mut self) {
        self.prescaler += 1;
        if self.prescaler < AY_PRESCALER_PERIOD {
            return;
        }
        self.prescaler = 0;

        for ch in &mut self.channels {
            ch.tick();
        }
        self.envelope.tick();
    }

    fn output(&self) -> f32 {
        let env_vol = self.envelope.output();
        let mut sum = 0.0f32;
        for ch in &self.channels {
            let vol_index = if ch.use_envelope {
                env_vol
            } else {
                ch.volume
            } as usize;
            let level = AY_VOLUME_TABLE[vol_index.min(15)];
            if ch.tone_disable || ch.output_high {
                sum += level;
            }
        }
        sum * 0.167
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.command);
        w.write_bytes(&self.regs);
        w.write_u16(self.prescaler);
        for ch in &self.channels {
            ch.write_state(w);
        }
        self.envelope.write_state(w);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.command = r.read_u8()?;
        r.read_exact(&mut self.regs)?;
        self.prescaler = r.read_u16()?;
        for ch in &mut self.channels {
            ch.read_state(r)?;
        }
        self.envelope.read_state(r)
    }
}

#[derive(Clone)]
struct Sunsoft5BChannel {
    period: u16,
    timer: u16,
    output_high: bool,
    tone_disable: bool,
    volume: u8,
    use_envelope: bool,
}

impl Sunsoft5BChannel {
    fn new() -> Self {
        Self {
            period: 0,
            timer: 0,
            output_high: false,
            tone_disable: false,
            volume: 0,
            use_envelope: false,
        }
    }

    fn tick(&mut self) {
        if self.timer == 0 {
            self.timer = self.period.max(1);
            self.output_high = !self.output_high;
        } else {
            self.timer -= 1;
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u16(self.period);
        w.write_u16(self.timer);
        w.write_bool(self.output_high);
        w.write_bool(self.tone_disable);
        w.write_u8(self.volume);
        w.write_bool(self.use_envelope);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.period = r.read_u16()?;
        self.timer = r.read_u16()?;
        self.output_high = r.read_bool()?;
        self.tone_disable = r.read_bool()?;
        self.volume = r.read_u8()?;
        self.use_envelope = r.read_bool()?;
        Ok(())
    }
}

struct Sunsoft5BEnvelope {
    period: u16,
    timer: u16,
    step: u8,
    shape: u8,
    holding: bool,
    output: u8,
    attack: bool,
    alternate: bool,
    hold: bool,
    cont: bool,
}

impl Sunsoft5BEnvelope {
    fn new() -> Self {
        Self {
            period: 0,
            timer: 0,
            step: 0,
            shape: 0,
            holding: false,
            output: 0,
            attack: false,
            alternate: false,
            hold: false,
            cont: false,
        }
    }

    fn trigger(&mut self, shape: u8) {
        self.shape = shape;
        self.cont = shape & 0x08 != 0;
        self.attack = shape & 0x04 != 0;
        self.alternate = shape & 0x02 != 0;
        self.hold = shape & 0x01 != 0;
        self.step = 0;
        self.holding = false;
        self.update_output();
    }

    fn tick(&mut self) {
        if self.holding {
            return;
        }
        if self.timer == 0 {
            self.timer = self.period.max(1);
            self.step += 1;
            if self.step > 15 {
                if !self.cont {
                    self.output = 0;
                    self.holding = true;
                    return;
                }
                if self.hold {
                    self.holding = true;
                    if self.alternate {
                        self.output = if self.attack { 0 } else { 15 };
                    }
                    return;
                }
                if self.alternate {
                    self.attack = !self.attack;
                }
                self.step = 0;
            }
            self.update_output();
        } else {
            self.timer -= 1;
        }
    }

    fn update_output(&mut self) {
        self.output = if self.attack { self.step } else { 15 - self.step };
    }

    fn output(&self) -> u8 {
        self.output
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u16(self.period);
        w.write_u16(self.timer);
        w.write_u8(self.step);
        w.write_u8(self.shape);
        w.write_bool(self.holding);
        w.write_u8(self.output);
        w.write_bool(self.attack);
        w.write_bool(self.alternate);
        w.write_bool(self.hold);
        w.write_bool(self.cont);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.period = r.read_u16()?;
        self.timer = r.read_u16()?;
        self.step = r.read_u8()?;
        self.shape = r.read_u8()?;
        self.holding = r.read_bool()?;
        self.output = r.read_u8()?;
        self.attack = r.read_bool()?;
        self.alternate = r.read_bool()?;
        self.hold = r.read_bool()?;
        self.cont = r.read_bool()?;
        Ok(())
    }
}

const SN5B_AUDIO_MARKER: &[u8; 4] = b"SN5B";

pub struct Fme7 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: Vec<u8>,
    has_battery: bool,

    command: u8,
    chr_banks: [u8; 8],
    prg_6000_bank: u8,
    prg_8000_bank: u8,
    prg_a000_bank: u8,
    prg_c000_bank: u8,
    prg_ram_select: bool,
    prg_ram_enable: bool,

    mirroring: Mirroring,

    irq_counter_enable: bool,
    irq_enable: bool,
    irq_counter: u16,
    irq_pending: bool,

    audio: Sunsoft5BAudio,
}

impl Fme7 {
    pub fn new(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        prg_ram_size: usize,
        has_battery: bool,
    ) -> Self {
        let ram_len = if prg_ram_size == 0 {
            0x2000
        } else {
            prg_ram_size
        };
        Self {
            prg_rom,
            chr,
            prg_ram: vec![0; ram_len],
            has_battery,
            command: 0,
            chr_banks: [0; 8],
            prg_6000_bank: 0,
            prg_8000_bank: 0,
            prg_a000_bank: 1,
            prg_c000_bank: 2,
            prg_ram_select: false,
            prg_ram_enable: false,
            mirroring,
            irq_counter_enable: false,
            irq_enable: false,
            irq_counter: 0,
            irq_pending: false,
            audio: Sunsoft5BAudio::new(),
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn map_chr_bank(&self, addr: u16) -> usize {
        let slot = ((addr as usize) >> 10) & 0x07;
        (self.chr_banks[slot] as usize) % self.chr_bank_count_1k()
    }

    fn prg_rom_read_8k(&self, bank: u8, addr: u16) -> u8 {
        let bank = (bank as usize) % self.prg_bank_count_8k();
        let offset = (addr as usize) & 0x1FFF;
        self.prg_rom[bank * 0x2000 + offset]
    }

    fn prg_ram_read(&self, bank: u8, addr: u16) -> u8 {
        if self.prg_ram.is_empty() {
            return 0;
        }
        let bank_count = (self.prg_ram.len() / 0x2000).max(1);
        let bank = (bank as usize) % bank_count;
        let offset = (addr as usize) & 0x1FFF;
        self.prg_ram[bank * 0x2000 + offset]
    }

    fn prg_ram_write(&mut self, bank: u8, addr: u16, val: u8) {
        if self.prg_ram.is_empty() {
            return;
        }
        let bank_count = (self.prg_ram.len() / 0x2000).max(1);
        let bank = (bank as usize) % bank_count;
        let offset = (addr as usize) & 0x1FFF;
        self.prg_ram[bank * 0x2000 + offset] = val;
    }

    fn write_parameter(&mut self, val: u8) {
        match self.command & 0x0F {
            0x0..=0x7 => self.chr_banks[(self.command & 0x07) as usize] = val,
            0x8 => {
                self.prg_ram_enable = val & 0x80 != 0;
                self.prg_ram_select = val & 0x40 != 0;
                self.prg_6000_bank = val & 0x3F;
            }
            0x9 => self.prg_8000_bank = val & 0x3F,
            0xA => self.prg_a000_bank = val & 0x3F,
            0xB => self.prg_c000_bank = val & 0x3F,
            0xC => {
                self.mirroring = match val & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenLower,
                    3 => Mirroring::SingleScreenUpper,
                    _ => Mirroring::Horizontal,
                };
            }
            0xD => {
                self.irq_pending = false;
                self.irq_enable = val & 0x01 != 0;
                self.irq_counter_enable = val & 0x80 != 0;
            }
            0xE => {
                self.irq_counter = (self.irq_counter & 0xFF00) | (val as u16);
            }
            0xF => {
                self.irq_counter = (self.irq_counter & 0x00FF) | ((val as u16) << 8);
            }
            _ => {}
        }
    }
}

impl Mapper for Fme7 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select {
                    if self.prg_ram_enable {
                        self.prg_ram_read(self.prg_6000_bank, addr)
                    } else {
                        0
                    }
                } else {
                    self.prg_rom_read_8k(self.prg_6000_bank, addr)
                }
            }
            0x8000..=0x9FFF => self.prg_rom_read_8k(self.prg_8000_bank, addr),
            0xA000..=0xBFFF => self.prg_rom_read_8k(self.prg_a000_bank, addr),
            0xC000..=0xDFFF => self.prg_rom_read_8k(self.prg_c000_bank, addr),
            0xE000..=0xFFFF => {
                let last = (self.prg_bank_count_8k() - 1) as u8;
                self.prg_rom_read_8k(last, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select && self.prg_ram_enable {
                    self.prg_ram_write(self.prg_6000_bank, addr, val);
                }
            }
            0x8000..=0x9FFF => {
                self.command = val & 0x0F;
            }
            0xA000..=0xBFFF => {
                self.write_parameter(val);
            }
            0xC000..=0xDFFF => {
                self.audio.write_command(val);
            }
            0xE000..=0xFFFF => {
                self.audio.write_data(val);
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let bank = self.map_chr_bank(addr);
        let offset = (addr as usize) & 0x03FF;
        self.chr[(bank * 0x0400 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let bank = self.map_chr_bank(addr);
        let offset = (addr as usize) & 0x03FF;
        let idx = (bank * 0x0400 + offset) % self.chr.len();
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clock_cpu(&mut self) {
        self.audio.tick();

        if !self.irq_counter_enable {
            return;
        }

        let prev = self.irq_counter;
        self.irq_counter = self.irq_counter.wrapping_sub(1);
        if prev == 0 && self.irq_enable {
            self.irq_pending = true;
        }
    }

    fn audio_output(&self) -> f32 {
        self.audio.output()
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        if self.has_battery && !self.prg_ram.is_empty() {
            Some(self.prg_ram.clone())
        } else {
            None
        }
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if self.prg_ram.is_empty() {
            return Ok(());
        }
        let copy_len = self.prg_ram.len().min(bytes.len());
        self.prg_ram[..copy_len].copy_from_slice(&bytes[..copy_len]);
        if copy_len < self.prg_ram.len() {
            self.prg_ram[copy_len..].fill(0);
        }
        Ok(())
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.command);
        w.write_bytes(&self.chr_banks);
        w.write_u8(self.prg_6000_bank);
        w.write_u8(self.prg_8000_bank);
        w.write_u8(self.prg_a000_bank);
        w.write_u8(self.prg_c000_bank);
        w.write_bool(self.prg_ram_select);
        w.write_bool(self.prg_ram_enable);

        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));

        w.write_bool(self.irq_counter_enable);
        w.write_bool(self.irq_enable);
        w.write_u16(self.irq_counter);
        w.write_bool(self.irq_pending);

        w.write_bool(self.has_battery);
        w.write_vec(&self.prg_ram);
        w.write_vec(&self.chr);

        // Sunsoft 5B audio state (new — preceded by marker for detection)
        w.write_bytes(SN5B_AUDIO_MARKER);
        self.audio.write_state(w);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.command = r.read_u8()?;
        r.read_exact(&mut self.chr_banks)?;
        self.prg_6000_bank = r.read_u8()?;
        self.prg_8000_bank = r.read_u8()?;
        self.prg_a000_bank = r.read_u8()?;
        self.prg_c000_bank = r.read_u8()?;
        self.prg_ram_select = r.read_bool()?;
        self.prg_ram_enable = r.read_bool()?;

        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;

        self.irq_counter_enable = r.read_bool()?;
        self.irq_enable = r.read_bool()?;
        self.irq_counter = r.read_u16()?;
        self.irq_pending = r.read_bool()?;

        self.has_battery = r.read_bool()?;

        let prg_ram = r.read_vec(512 * 1024)?;
        if prg_ram.len() != self.prg_ram.len() {
            anyhow::bail!(
                "FME-7 PRG RAM size mismatch: expected {}, got {}",
                self.prg_ram.len(),
                prg_ram.len()
            );
        }
        self.prg_ram = prg_ram;

        let chr = r.read_vec(512 * 1024)?;
        if chr.len() != self.chr.len() {
            anyhow::bail!(
                "FME-7 CHR size mismatch: expected {}, got {}",
                self.chr.len(),
                chr.len()
            );
        }
        self.chr = chr;

        let saved_pos = r.position();
        let mut marker = [0u8; 4];
        match r.read_exact(&mut marker) {
            Ok(()) if &marker == SN5B_AUDIO_MARKER => {
                self.audio.read_state(r)?;
            }
            _ => {
                r.set_position(saved_pos);
                self.audio = Sunsoft5BAudio::new();
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;
