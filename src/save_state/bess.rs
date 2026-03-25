// Specification: <https://github.com/LIJI32/SameBoy/blob/master/BESS.md>

use anyhow::{Result, anyhow, bail};
use std::time::{SystemTime, UNIX_EPOCH};

use super::StateWriter;
use crate::hardware::bus::Bus;
use crate::hardware::cpu::{CPU, Registers};
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::CPUState;
use crate::hardware::types::IMEState;
use crate::hardware::types::constants::{HRAM_SIZE, OAM_SIZE};
use crate::hardware::types::hardware_mode::HardwareMode;

const BESS_MAGIC: &[u8; 4] = b"BESS";
const BESS_MAJOR: u16 = 1;
const BESS_MINOR: u16 = 1;
const EMULATOR_NAME: &[u8] = b"zeff-boy";

const BLOCK_NAME: [u8; 4] = *b"NAME";
const BLOCK_INFO: [u8; 4] = *b"INFO";
const BLOCK_CORE: [u8; 4] = *b"CORE";
const BLOCK_MBC: [u8; 4] = *b"MBC ";
const BLOCK_RTC: [u8; 4] = *b"RTC ";
const BLOCK_END: [u8; 4] = *b"END ";

const CORE_BLOCK_LEN: u32 = 0xD0;
const INFO_BLOCK_LEN: u32 = 0x12;
const RTC_BLOCK_LEN: u32 = 0x30;

pub(crate) fn has_bess_footer(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[bytes.len() - 4..] == BESS_MAGIC
}

pub(crate) fn append_bess(
    writer: &mut StateWriter,
    cpu: &CPU,
    bus: &Bus,
    hardware_mode: HardwareMode,
) -> Result<()> {
    let rom = bus.cartridge.rom_bytes();
    let is_cgb = matches!(
        hardware_mode,
        HardwareMode::CGBNormal | HardwareMode::CGBDouble
    );

    let mbc_ram = bus.cartridge.mbc_ram_bytes();
    let mbc_ram_offset = writer.position() as u32;
    let mbc_ram_size = mbc_ram.len() as u32;
    writer.write_bytes(mbc_ram);

    let vram_offset = writer.position() as u32;
    let vram_size = bus.vram.len() as u32;
    writer.write_bytes(&bus.vram);

    let wram_offset = writer.position() as u32;
    let wram_size = bus.wram.len() as u32;
    writer.write_bytes(&bus.wram);

    let oam_offset = writer.position() as u32;
    writer.write_bytes(&bus.oam);

    let hram_offset = writer.position() as u32;
    writer.write_bytes(&bus.hram);

    let (bg_pal_offset, bg_pal_size, obj_pal_offset, obj_pal_size) = if is_cgb {
        let bg = writer.position() as u32;
        writer.write_bytes(bus.ppu_bg_palette_ram());
        let obj = writer.position() as u32;
        writer.write_bytes(bus.ppu_obj_palette_ram());
        (bg, 0x40u32, obj, 0x40u32)
    } else {
        (0u32, 0u32, 0u32, 0u32)
    };

    let first_block_offset = writer.position() as u32;

    write_block_header(writer, &BLOCK_NAME, EMULATOR_NAME.len() as u32);
    writer.write_bytes(EMULATOR_NAME);

    if rom.len() >= 0x150 {
        write_block_header(writer, &BLOCK_INFO, INFO_BLOCK_LEN);
        writer.write_bytes(&rom[0x134..0x144]); // title (16 bytes)
        writer.write_bytes(&rom[0x14E..0x150]); // global checksum (2 bytes)
    }

    write_block_header(writer, &BLOCK_CORE, CORE_BLOCK_LEN);
    write_core_body(
        writer,
        cpu,
        bus,
        hardware_mode,
        wram_offset,
        wram_size,
        vram_offset,
        vram_size,
        mbc_ram_offset,
        mbc_ram_size,
        oam_offset,
        hram_offset,
        bg_pal_offset,
        bg_pal_size,
        obj_pal_offset,
        obj_pal_size,
    );

    let mbc_writes = bus.cartridge.bess_mbc_writes();
    if !mbc_writes.is_empty() {
        write_block_header(writer, &BLOCK_MBC, (mbc_writes.len() * 3) as u32);
        for &(addr, val) in &mbc_writes {
            writer.write_u16(addr);
            writer.write_u8(val);
        }
    }

    if let Some((current, latched)) = bus.cartridge.bess_rtc_data() {
        write_block_header(writer, &BLOCK_RTC, RTC_BLOCK_LEN);
        for &val in &current {
            writer.write_u32(val as u32);
        }
        for &val in &latched {
            writer.write_u32(val as u32);
        }
        writer.write_u64(now_unix_seconds());
    }

    write_block_header(writer, &BLOCK_END, 0);

    writer.write_u32(first_block_offset);
    writer.write_bytes(BESS_MAGIC);

    Ok(())
}

pub(crate) struct BessImport {
    pub(crate) cpu: CPU,
    pub(crate) bus: Box<Bus>,
    pub(crate) hardware_mode: HardwareMode,
}

pub(crate) fn import_bess(bytes: &[u8], rom: &[u8], header: &RomHeader) -> Result<BessImport> {
    if !has_bess_footer(bytes) {
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

    let cpu = CPU {
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
            IMEState::Enabled
        } else {
            IMEState::Disabled
        },
        running: match exec_state {
            1 => CPUState::Halted,
            2 => CPUState::Stopped,
            _ => CPUState::Running,
        },
        cycles: 0,
        last_step_cycles: 0,
        timed_cycles_accounted: 0,
        halt_bug_active: false,
    };

    let mut bus = *Bus::new(rom.to_vec(), header, hardware_mode)?;

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
        copy_buffer(bytes, bg_pal_offset, bg_pal_size, bus.ppu_bg_palette_ram_mut());
        copy_buffer(bytes, obj_pal_offset, obj_pal_size, bus.ppu_obj_palette_ram_mut());
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
        bus: Box::new(bus),
        hardware_mode,
    })
}

#[allow(clippy::too_many_arguments)]
fn write_core_body(
    writer: &mut StateWriter,
    cpu: &CPU,
    bus: &Bus,
    hardware_mode: HardwareMode,
    ram_offset: u32,
    ram_size: u32,
    vram_offset: u32,
    vram_size: u32,
    mbc_ram_offset: u32,
    mbc_ram_size: u32,
    oam_offset: u32,
    hram_offset: u32,
    bg_pal_offset: u32,
    bg_pal_size: u32,
    obj_pal_offset: u32,
    obj_pal_size: u32,
) {
    let is_cgb = matches!(
        hardware_mode,
        HardwareMode::CGBNormal | HardwareMode::CGBDouble
    );

    writer.write_u16(BESS_MAJOR);
    writer.write_u16(BESS_MINOR);

    writer.write_bytes(&mode_to_bess_model(hardware_mode));

    writer.write_u16(cpu.pc);
    writer.write_u16(((cpu.regs.a as u16) << 8) | cpu.regs.f as u16); // AF
    writer.write_u16(((cpu.regs.b as u16) << 8) | cpu.regs.c as u16); // BC
    writer.write_u16(((cpu.regs.d as u16) << 8) | cpu.regs.e as u16); // DE
    writer.write_u16(((cpu.regs.h as u16) << 8) | cpu.regs.l as u16); // HL
    writer.write_u16(cpu.sp);

    writer.write_u8(match cpu.ime {
        IMEState::Enabled | IMEState::PendingEnable => 1,
        IMEState::Disabled => 0,
    });

    writer.write_u8(bus.ie);

    writer.write_u8(match cpu.running {
        CPUState::Running | CPUState::InterruptHandling => 0,
        CPUState::Halted => 1,
        CPUState::Stopped | CPUState::Reset | CPUState::Suspended => 2,
    });

    writer.write_u8(0);

    let mut io_snapshot = bus.collect_io_register_snapshot();

    if is_cgb {
        io_snapshot[0x4C] = if bus.ppu_cgb_mode() { 0x80 } else { 0x04 }; // KEY0
    }
    io_snapshot[0x50] = 0x01;
    writer.write_bytes(&io_snapshot);

    writer.write_u32(ram_size);
    writer.write_u32(ram_offset);
    writer.write_u32(vram_size);
    writer.write_u32(vram_offset);
    writer.write_u32(mbc_ram_size);
    writer.write_u32(mbc_ram_offset);
    writer.write_u32(OAM_SIZE as u32);
    writer.write_u32(oam_offset);
    writer.write_u32(HRAM_SIZE as u32);
    writer.write_u32(hram_offset);
    writer.write_u32(bg_pal_size);
    writer.write_u32(bg_pal_offset);
    writer.write_u32(obj_pal_size);
    writer.write_u32(obj_pal_offset);
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

fn write_block_header(writer: &mut StateWriter, id: &[u8; 4], len: u32) {
    writer.write_bytes(id);
    writer.write_u32(len);
}

fn mode_to_bess_model(mode: HardwareMode) -> [u8; 4] {
    match mode {
        HardwareMode::DMG => *b"GD  ",
        HardwareMode::SGB1 => *b"SN  ",
        HardwareMode::SGB2 => *b"S2  ",
        HardwareMode::CGBNormal | HardwareMode::CGBDouble => *b"CC  ",
    }
}

fn bess_model_to_mode(model: &[u8], core: &[u8]) -> Result<HardwareMode> {
    match model[0] {
        b'G' => Ok(HardwareMode::DMG),
        b'S' => {
            if model.len() >= 2 && model[1] == b'2' {
                Ok(HardwareMode::SGB2)
            } else {
                Ok(HardwareMode::SGB1)
            }
        }
        b'C' => {
            let key1 = core[0x18 + 0x4D];
            if key1 & 0x80 != 0 {
                Ok(HardwareMode::CGBDouble)
            } else {
                Ok(HardwareMode::CGBNormal)
            }
        }
        _ => bail!("unknown BESS model family '{}'", char::from(model[0])),
    }
}

fn copy_buffer(file: &[u8], offset: usize, size: usize, dest: &mut [u8]) {
    if size == 0 || offset + size > file.len() {
        return;
    }
    let copy_len = size.min(dest.len());
    dest[..copy_len].copy_from_slice(&file[offset..offset + copy_len]);

    for b in &mut dest[copy_len..] {
        *b = 0;
    }
}

fn read_u16_le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn read_u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
