pub(crate) mod audio;

use crate::hardware::cartridge::{Mapper, Mirroring};
use audio::FdsAudio;

const PRG_RAM_SIZE: usize = 0x8000;
const CHR_RAM_SIZE: usize = 0x2000;

pub struct Fds {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_ram: [u8; CHR_RAM_SIZE],
    mirroring: Mirroring,

    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_repeat: bool,
    irq_pending: bool,

    io_enabled: bool,
    disk_reg: u8,

    audio: FdsAudio,
}

impl Fds {
    pub fn new(prg_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_SIZE],
            chr_ram: [0; CHR_RAM_SIZE],
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_repeat: false,
            irq_pending: false,
            io_enabled: false,
            disk_reg: 0,
            audio: FdsAudio::new(),
        }
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
                let val = if self.irq_pending { 0x01 } else { 0x00 };
                self.irq_pending = false;
                val
            }
            0x4031 => 0,
            0x4032 => 0x00,
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
            }
            0x4024 => {
                self.disk_reg = val;
            }
            0x4025 => {
                self.mirroring = if val & 0x08 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
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
        self.irq_pending
    }

    fn clock_cpu(&mut self) {
        self.audio.tick();

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
                "FDS battery data size mismatch: expected {}, got {}",
                self.prg_ram.len(),
                bytes.len()
            );
        }
        self.prg_ram[..bytes.len()].copy_from_slice(bytes);
        Ok(())
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
        self.audio.read_state(r)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fds() -> Fds {
        let bios = vec![0xFF; 0x2000];
        Fds::new(bios, Mirroring::Horizontal)
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
        let mut fds = make_fds();
        fds.cpu_write(0x6000, 0xAA);
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

        let mut fds2 = make_fds();
        let mut reader = crate::save_state::StateReader::new(&bytes);
        fds2.read_state(&mut reader).expect("read_state ok");

        assert_eq!(fds2.cpu_peek(0x6000), 0xAA);
        assert_eq!(fds2.cpu_peek(0x4040), 0x20);
    }
}
