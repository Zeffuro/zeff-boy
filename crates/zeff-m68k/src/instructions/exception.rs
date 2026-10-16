use crate::bus::{AccessKind, FunctionCode, M68kBus, read_long, read_word, write_word};
use crate::constants::VECTOR_ENTRY_BYTES;
use crate::{Fault, Registers, SUPERVISOR, TRACE};

pub(crate) fn enter<B: M68kBus>(
    regs: &mut Registers,
    bus: &mut B,
    vector: u8,
    saved_pc: u32,
) -> Result<(), Fault<B::Error>> {
    let saved_sr = regs.status_register();
    let old_sp = regs.ssp;
    regs.set_status_register((saved_sr | SUPERVISOR) & !TRACE);
    regs.ssp = old_sp.wrapping_sub(6);
    // Short exception stacking interleaves PC words around the saved SR.
    for (address, value) in [
        (old_sp.wrapping_sub(2), saved_pc as u16),
        (old_sp.wrapping_sub(6), saved_sr),
        (old_sp.wrapping_sub(4), (saved_pc >> 16) as u16),
    ] {
        write_word(
            bus,
            address,
            value,
            FunctionCode::SupervisorData,
            AccessKind::Stack,
        )
        .map_err(Fault::Bus)?;
    }
    regs.pc = read_long(
        bus,
        u32::from(vector) * VECTOR_ENTRY_BYTES,
        FunctionCode::SupervisorData,
        AccessKind::Vector,
    )
    .map_err(Fault::Bus)?;
    Ok(())
}

pub(crate) fn return_from_exception<B: M68kBus>(
    regs: &mut Registers,
    bus: &mut B,
) -> Result<(), Fault<B::Error>> {
    let address = regs.ssp;
    let sr = read_word(
        bus,
        address,
        FunctionCode::SupervisorData,
        AccessKind::Stack,
    )
    .map_err(Fault::Bus)?;
    let pc = read_long(
        bus,
        address.wrapping_add(2),
        FunctionCode::SupervisorData,
        AccessKind::Stack,
    )
    .map_err(Fault::Bus)?;
    // Update SSP before restored S selects a different active A7 bank.
    regs.ssp = address.wrapping_add(6);
    regs.set_status_register(sr);
    regs.pc = pc;
    Ok(())
}
