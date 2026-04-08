use anyhow::{Result, anyhow, bail};

use super::{
    BESS_MAJOR, CORE_BLOCK_LEN, RTC_BLOCK_LEN, bess_model_to_mode, copy_buffer, now_unix_seconds,
    read_u16_le, read_u32_le,
};
use crate::hardware::bus::Bus;
use crate::hardware::cpu::{Cpu, Registers};
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::CpuState;
use crate::hardware::types::ImeState;
use crate::hardware::types::hardware_mode::HardwareMode;

pub struct BessImport {
    pub cpu: Cpu,
    pub bus: Bus,
    pub hardware_mode: HardwareMode,
}

pub fn import_bess(bytes: &[u8], rom: &[u8], header: &RomHeader) -> Result<BessImport> {
    if !super::has_bess_footer(bytes) {
        bail!("file does not contain BESS footer");
    }

    let footer_start = bytes.len() - 8;
    let first_offset = read_u32_le(&bytes[footer_start..]) as usize;
    if first_offset >= footer_start {
        bail!("BESS first-block offset out of range");
    }

    let mut core_data: Option<&[u8]> = None;
    let mut mbc_data: Option<&[u8]> = None;
    let mut rtc_data: Option<&[u8]> = None;

    let mut pos = first_offset;
    loop {
        if pos + 8 > footer_start {
            bail!("BESS block header truncated");
        }
        let id = &bytes[pos..pos + 4];
        let len = read_u32_le(&bytes[pos + 4..]) as usize;
        let data_start = pos + 8;
        let data_end = data_start + len;
        if data_end > bytes.len() {
            bail!("BESS block data truncated");
        }

        if id == b"END " {
            if len != 0 {
                bail!("BESS END block has non-zero length");
            }
            break;
        }

        match id {
            b"CORE" => {
                if core_data.is_some() {
                    bail!("duplicate BESS CORE block");
                }
                core_data = Some(&bytes[data_start..data_end]);
            }
            b"MBC " => mbc_data = Some(&bytes[data_start..data_end]),
            b"RTC " => rtc_data = Some(&bytes[data_start..data_end]),
            _ => {}
        }

        pos = data_end;
    }

    let core = core_data.ok_or_else(|| anyhow!("BESS file missing CORE block"))?;
    if core.len() < CORE_BLOCK_LEN as usize {
        bail!(
            "BESS CORE block too short ({} bytes, need {})",
            core.len(),
            CORE_BLOCK_LEN
        );
    }

    let major = read_u16_le(&core[0x00..]);
    if major != BESS_MAJOR {
        bail!("unsupported BESS major version {major}");
    }

    let model = &core[0x04..0x08];
    let hardware_mode = bess_model_to_mode(model, core)?;

    let pc = read_u16_le(&core[0x08..]);
    let af = read_u16_le(&core[0x0A..]);
    let bc = read_u16_le(&core[0x0C..]);
    let de = read_u16_le(&core[0x0E..]);
    let hl = read_u16_le(&core[0x10..]);
    let sp = read_u16_le(&core[0x12..]);
    let ime_val = core[0x14];
    let ie = core[0x15];
    let exec_state = core[0x16];

    let io_regs = &core[0x18..0x98];

    let ram_size = read_u32_le(&core[0x98..]) as usize;
    let ram_offset = read_u32_le(&core[0x9C..]) as usize;
    let vram_size = read_u32_le(&core[0xA0..]) as usize;
    let vram_offset = read_u32_le(&core[0xA4..]) as usize;
    let mbc_ram_size = read_u32_le(&core[0xA8..]) as usize;
    let mbc_ram_offset = read_u32_le(&core[0xAC..]) as usize;
    let oam_size = read_u32_le(&core[0xB0..]) as usize;
    let oam_offset = read_u32_le(&core[0xB4..]) as usize;
    let hram_size = read_u32_le(&core[0xB8..]) as usize;
    let hram_offset = read_u32_le(&core[0xBC..]) as usize;
    let bg_pal_size = read_u32_le(&core[0xC0..]) as usize;
    let bg_pal_offset = read_u32_le(&core[0xC4..]) as usize;
    let obj_pal_size = read_u32_le(&core[0xC8..]) as usize;
    let obj_pal_offset = read_u32_le(&core[0xCC..]) as usize;

    let cpu = Cpu {
        pc,
        sp,
        regs: Registers {
            a: (af >> 8) as u8,
            f: (af & 0xF0) as u8,
            b: (bc >> 8) as u8,
            c: (bc & 0xFF) as u8,
            d: (de >> 8) as u8,
            e: (de & 0xFF) as u8,
            h: (hl >> 8) as u8,
            l: (hl & 0xFF) as u8,
        },
        ime: if ime_val != 0 {
            ImeState::Enabled
        } else {
            ImeState::Disabled
        },
        running: match exec_state {
            1 => CpuState::Halted,
            2 => CpuState::Stopped,
            _ => CpuState::Running,
        },
        cycles: 0,
        last_step_cycles: 0,
        timed_cycles_accounted: 0,
        halt_bug_active: false,
    };

    let mut bus = Bus::new(rom.to_vec(), header, hardware_mode)?;

    copy_buffer(bytes, vram_offset, vram_size, &mut bus.vram);
    copy_buffer(bytes, ram_offset, ram_size, &mut bus.wram);
    copy_buffer(bytes, oam_offset, oam_size, &mut bus.oam);
    copy_buffer(bytes, hram_offset, hram_size, &mut bus.hram);

    if mbc_ram_size > 0 && mbc_ram_offset + mbc_ram_size <= bytes.len() {
        bus.cartridge
            .load_sram(&bytes[mbc_ram_offset..mbc_ram_offset + mbc_ram_size]);
    }

    let is_cgb = matches!(
        hardware_mode,
        HardwareMode::CGBNormal | HardwareMode::CGBDouble
    );
    if is_cgb {
        copy_buffer(
            bytes,
            bg_pal_offset,
            bg_pal_size,
            bus.ppu_bg_palette_ram_mut(),
        );
        copy_buffer(
            bytes,
            obj_pal_offset,
            obj_pal_size,
            bus.ppu_obj_palette_ram_mut(),
        );
    }

    if let Some(mbc) = mbc_data {
        if mbc.len() % 3 != 0 {
            bail!("BESS MBC block length not a multiple of 3");
        }
        for chunk in mbc.chunks_exact(3) {
            let addr = read_u16_le(chunk);
            let val = chunk[2];
            if addr <= 0x7FFF || (0xA000..=0xBFFF).contains(&addr) {
                bus.cartridge.write_rom(addr, val);
            }
        }
    }

    if mbc_ram_size > 0 && mbc_ram_offset + mbc_ram_size <= bytes.len() {
        bus.cartridge
            .load_sram(&bytes[mbc_ram_offset..mbc_ram_offset + mbc_ram_size]);
    }

    apply_bess_io_registers(&mut bus, io_regs, hardware_mode);
    bus.ie = ie;

    if let Some(rtc) = rtc_data {
        apply_bess_rtc(&mut bus, rtc)?;
    }

    Ok(BessImport {
        cpu,
        bus,
        hardware_mode,
    })
}

fn apply_bess_io_registers(bus: &mut Bus, io: &[u8], mode: HardwareMode) {
    let is_cgb = matches!(mode, HardwareMode::CGBNormal | HardwareMode::CGBDouble);

    bus.io_bank.copy_from_slice(&io[..0x80]);

    bus.write_joypad_p1(io[0x00]);

    bus.apply_bess_timer_serial_registers(io, mode);

    bus.if_reg = io[0x0F] & 0x1F;

    bus.apply_bess_apu_io(io);

    bus.apply_bess_ppu_registers(io, is_cgb);

    if is_cgb {
        bus.key1 = io[0x4D];

        bus.vram_bank = io[0x4F] & 0x01;

        bus.hdma1 = io[0x51];
        bus.hdma2 = io[0x52];
        bus.hdma3 = io[0x53];
        bus.hdma4 = io[0x54];
        bus.hdma5 = io[0x55];

        let svbk = io[0x70] & 0x07;
        bus.wram_bank = if svbk == 0 { 1 } else { svbk };
    }
}

fn apply_bess_rtc(bus: &mut Bus, data: &[u8]) -> Result<()> {
    if data.len() < RTC_BLOCK_LEN as usize {
        bail!("BESS RTC block too short");
    }

    let mut current = [0u8; 5];
    let mut latched = [0u8; 5];
    for i in 0..5 {
        current[i] = data[i * 4];
    }
    for i in 0..5 {
        latched[i] = data[0x14 + i * 4];
    }

    let unix_ts = u64::from_le_bytes(data[0x28..0x30].try_into()?);
    let now = now_unix_seconds();
    let elapsed = now.saturating_sub(unix_ts);

    bus.cartridge.apply_bess_rtc(current, latched, elapsed);

    Ok(())
}
