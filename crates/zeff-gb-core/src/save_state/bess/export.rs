use anyhow::Result;

use super::{
    BESS_MAGIC, BESS_MAJOR, BESS_MINOR, BLOCK_CORE, BLOCK_END, BLOCK_INFO, BLOCK_MBC, BLOCK_NAME,
    BLOCK_RTC, CORE_BLOCK_LEN, EMULATOR_NAME, INFO_BLOCK_LEN, RTC_BLOCK_LEN, mode_to_bess_model,
    now_unix_seconds, write_block_header,
};
use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::types::CpuState;
use crate::hardware::types::ImeState;
use crate::hardware::types::constants::{HRAM_SIZE, OAM_SIZE};
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::save_state::StateWriter;

struct BessMemoryLayout {
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
}

pub fn append_bess(
    writer: &mut StateWriter,
    cpu: &Cpu,
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
        writer.write_bytes(&rom[0x134..0x144]);
        writer.write_bytes(&rom[0x14E..0x150]);
    }

    write_block_header(writer, &BLOCK_CORE, CORE_BLOCK_LEN);
    write_core_body(
        writer,
        cpu,
        bus,
        hardware_mode,
        &BessMemoryLayout {
            ram_offset: wram_offset,
            ram_size: wram_size,
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
        },
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

fn write_core_body(
    writer: &mut StateWriter,
    cpu: &Cpu,
    bus: &Bus,
    hardware_mode: HardwareMode,
    mem: &BessMemoryLayout,
) {
    let is_cgb = matches!(
        hardware_mode,
        HardwareMode::CGBNormal | HardwareMode::CGBDouble
    );

    writer.write_u16(BESS_MAJOR);
    writer.write_u16(BESS_MINOR);

    writer.write_bytes(&mode_to_bess_model(hardware_mode));

    writer.write_u16(cpu.pc);
    writer.write_u16(((cpu.regs.a as u16) << 8) | cpu.regs.f as u16);
    writer.write_u16(((cpu.regs.b as u16) << 8) | cpu.regs.c as u16);
    writer.write_u16(((cpu.regs.d as u16) << 8) | cpu.regs.e as u16);
    writer.write_u16(((cpu.regs.h as u16) << 8) | cpu.regs.l as u16);
    writer.write_u16(cpu.sp);

    writer.write_u8(match cpu.ime {
        ImeState::Enabled | ImeState::PendingEnable => 1,
        ImeState::Disabled => 0,
    });

    writer.write_u8(bus.ie);

    writer.write_u8(match cpu.running {
        CpuState::Running | CpuState::InterruptHandling => 0,
        CpuState::Halted => 1,
        CpuState::Stopped | CpuState::Reset | CpuState::Suspended => 2,
    });

    writer.write_u8(0);

    let mut io_snapshot = bus.collect_io_register_snapshot();

    if is_cgb {
        io_snapshot[0x4C] = if bus.ppu_cgb_mode() { 0x80 } else { 0x04 };
    }
    io_snapshot[0x50] = 0x01;
    writer.write_bytes(&io_snapshot);

    writer.write_u32(mem.ram_size);
    writer.write_u32(mem.ram_offset);
    writer.write_u32(mem.vram_size);
    writer.write_u32(mem.vram_offset);
    writer.write_u32(mem.mbc_ram_size);
    writer.write_u32(mem.mbc_ram_offset);
    writer.write_u32(OAM_SIZE as u32);
    writer.write_u32(mem.oam_offset);
    writer.write_u32(HRAM_SIZE as u32);
    writer.write_u32(mem.hram_offset);
    writer.write_u32(mem.bg_pal_size);
    writer.write_u32(mem.bg_pal_offset);
    writer.write_u32(mem.obj_pal_size);
    writer.write_u32(mem.obj_pal_offset);
}
