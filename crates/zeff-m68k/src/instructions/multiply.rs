use crate::bus::M68kBus;
use crate::constants::{VECTOR_CHK, VECTOR_ZERO_DIVIDE};
use crate::ea::Size;
use crate::instruction::{Context, data_source};
use crate::{CARRY, Fault, NEGATIVE, OVERFLOW, Registers, ZERO};

pub(crate) fn execute<B: M68kBus>(
    ctx: &mut Context<'_, B>,
) -> Result<Option<u32>, Fault<B::Error>> {
    let family = ctx.opcode & 0xf1c0;
    if !matches!(family, 0xc0c0 | 0xc1c0 | 0x80c0 | 0x81c0 | 0x4180) {
        return Ok(None);
    }
    let mode = (ctx.opcode >> 3) & 7;
    let reg = ctx.opcode & 7;
    if !data_source(mode, reg) {
        return Err(ctx.unsupported());
    }
    let operand = ctx.resolve(mode, reg, Size::Word)?;
    let source = ctx.read(operand, Size::Word)? as u16;
    let ea_cycles = ctx.ea_cycles(mode, reg, Size::Word)?;
    let destination = usize::from((ctx.opcode >> 9) & 7);
    let calculation = calculate(ctx.regs, family, destination, source);
    if let Some(vector) = calculation.exception {
        ctx.exception(vector, ctx.cursor)?;
    }
    Ok(Some(calculation.cycles + ea_cycles))
}

pub(crate) struct Calculation {
    pub cycles: u32,
    pub exception: Option<u8>,
}

pub(crate) fn calculate(
    regs: &mut Registers,
    family: u16,
    destination: usize,
    source: u16,
) -> Calculation {
    let dividend = regs.d[destination];
    let cycles = match family {
        0xc0c0 | 0xc1c0 => {
            let signed = family == 0xc1c0;
            let result = if signed {
                (i32::from(dividend as i16) * i32::from(source as i16)) as u32
            } else {
                u32::from(dividend as u16) * u32::from(source)
            };
            regs.d[destination] = result;
            crate::alu::logic(regs, Size::Long, result);
            multiply_cycles(source, signed)
        }
        0x4180 => {
            let value = dividend as i16;
            // CHK sets N/Z from Dn and clears V/C before its comparisons.
            crate::alu::logic(regs, Size::Word, dividend);
            let difference = i32::from(source as i16) - i32::from(value);
            // Bounds failure precedes the negative-Dn check and traps two clocks sooner.
            if !(0..=32767).contains(&difference) {
                return Calculation {
                    cycles: 38,
                    exception: Some(VECTOR_CHK),
                };
            } else if value < 0 {
                return Calculation {
                    cycles: 40,
                    exception: Some(VECTOR_CHK),
                };
            } else {
                10
            }
        }
        0x80c0 | 0x81c0 => {
            let signed = family == 0x81c0;
            if source == 0 {
                // Zero divide selects flags from zero (signed) or the dividend high word.
                crate::alu::logic(regs, Size::Word, if signed { 0 } else { dividend >> 16 });
                return Calculation {
                    cycles: 38,
                    exception: Some(VECTOR_ZERO_DIVIDE),
                };
            } else if signed {
                let numerator = i64::from(dividend as i32);
                let denominator = i64::from(source as i16);
                let quotient = numerator / denominator;
                if !(-32768..=32767).contains(&quotient) {
                    division_overflow(regs);
                } else {
                    let remainder = numerator % denominator;
                    regs.d[destination] =
                        (u32::from(remainder as u16) << 16) | u32::from(quotient as u16);
                    crate::alu::logic(regs, Size::Word, quotient as u32);
                }
                signed_divide_cycles(dividend as i32, source as i16)
            } else {
                let denominator = u32::from(source);
                let quotient = dividend / denominator;
                if quotient > 0xffff {
                    division_overflow(regs);
                } else {
                    regs.d[destination] = ((dividend % denominator) << 16) | quotient;
                    crate::alu::logic(regs, Size::Word, quotient);
                }
                unsigned_divide_cycles(dividend, source)
            }
        }
        _ => unreachable!(),
    };
    Calculation {
        cycles,
        exception: None,
    }
}

fn division_overflow(regs: &mut Registers) {
    // Division overflow selects N=1 and Z=0.
    regs.set_status_register(
        (regs.status_register() & !(NEGATIVE | ZERO | OVERFLOW | CARRY)) | NEGATIVE | OVERFLOW,
    );
}

pub(crate) fn multiply_cycles(source: u16, signed: bool) -> u32 {
    let events = if signed {
        (source ^ source.wrapping_shl(1)).count_ones()
    } else {
        source.count_ones()
    };
    38 + 2 * events
}

pub(crate) fn unsigned_divide_cycles(dividend: u32, divisor: u16) -> u32 {
    let divisor = u32::from(divisor);
    if dividend >> 16 >= divisor {
        return 10;
    }
    let mut remainder = dividend >> 16;
    let mut cycles = 76;
    // Each of the first fifteen restoring-division stages costs 4, 6, or 8 clocks.
    for bit in (1..=15).rev() {
        let carried = remainder & 0x8000 != 0;
        let trial = (remainder << 1) | ((dividend >> bit) & 1);
        if !carried {
            cycles += if trial >= divisor { 2 } else { 4 };
        }
        remainder = if trial >= divisor {
            trial - divisor
        } else {
            trial
        };
    }
    cycles
}

pub(crate) fn signed_divide_cycles(dividend: i32, divisor: i16) -> u32 {
    let magnitude = dividend.unsigned_abs();
    let denominator = u32::from(divisor.unsigned_abs());
    let negate_dividend = u32::from(dividend < 0) * 2;
    if magnitude >> 16 >= denominator {
        return 16 + negate_dividend;
    }
    let quotient = magnitude / denominator;
    let zero_stages = 15 - (quotient >> 1).count_ones();
    let sign_correction = if divisor < 0 {
        2
    } else {
        u32::from(dividend < 0) * 4
    };
    120 + negate_dividend + sign_correction + 2 * zero_stages
}
