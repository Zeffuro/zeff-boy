use super::*;
use crate::emulator::rollback::memory::{MdAllocationKind as K, MdRetainedAllocations};
use zeff_emu_common::save_state::StateWriter;

pub(super) fn write_registers(registers: &Registers, writer: &mut StateWriter) {
    for value in registers.d.into_iter().chain(registers.a) {
        writer.write_u32(value);
    }
    writer.write_u32(registers.usp);
    writer.write_u32(registers.ssp);
    writer.write_u32(registers.pc);
    writer.write_u16(registers.status_register());
}
fn write_text(writer: &mut StateWriter, value: &str) -> anyhow::Result<()> {
    writer.write_u64(u64::try_from(value.len())?);
    writer.write_bytes(value.as_bytes());
    Ok(())
}
impl Cartridge {
    pub(crate) fn write_rollback_metadata(
        &self,
        logical: &mut StateWriter,
        local: &mut StateWriter,
    ) -> anyhow::Result<()> {
        write_registers(&self.initial, logical);
        self.rom_overlay.write_rollback_metadata(local);
        local.write_bool(self.header.is_some());
        if let Some(header) = &self.header {
            let CartridgeHeader {
                console,
                copyright,
                domestic_title,
                international_title,
                product,
                checksum,
                calculated_checksum,
                peripherals,
                rom_range,
                work_ram_range,
                save_declaration,
                countries,
            } = header.as_ref();
            for text in [
                console,
                copyright,
                domestic_title,
                international_title,
                product,
                peripherals,
                countries,
            ] {
                write_text(local, text)?;
            }
            local.write_u16(*checksum);
            local.write_u16(*calculated_checksum);
            for value in [rom_range.0, rom_range.1, work_ram_range.0, work_ram_range.1] {
                local.write_u32(value);
            }
            local.write_bytes(save_declaration);
        }
        Ok(())
    }

    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        let Self {
            bytes,
            svp: _,
            initial: _,
            header,
            save: _,
            save_latch: _,
            download: _,
            linear_rom: _,
            unprotected_rom: _,
            parallel_adapter: _,
            registers: _,
            protection: _,
            sf001: _,
            sf002: _,
            sf004: _,
            extended_ssf: _,
            realtec: _,
            ssf2: _,
            lock_on: _,
            lock_on_save: _,
            rom_overlay,
            digest_memo,
        } = self;
        into.arc(bytes, K::Rom)?;
        if into.arc(digest_memo, K::RomMemo)? {
            let RomDigestMemo {
                seal,
                factory_save: _,
                factory_timed_save: _,
                factory_lock_on: _,
                factory_registers: _,
                factory_protection: _,
                digest: _,
            } = digest_memo.as_ref();
            into.arc(seal, K::Rom)?;
        }
        if let Some(header) = header
            && into.arc(header, K::Header)?
        {
            let CartridgeHeader {
                console,
                copyright,
                domestic_title,
                international_title,
                product,
                checksum: _,
                calculated_checksum: _,
                peripherals,
                rom_range: _,
                work_ram_range: _,
                save_declaration: _,
                countries,
            } = header.as_ref();
            for text in [
                console,
                copyright,
                domestic_title,
                international_title,
                product,
                peripherals,
                countries,
            ] {
                into.text(text, K::HeaderText)?;
            }
        }
        rom_overlay.visit_retained_allocations(into)
    }
}
