use super::{Meter, Pattern, ReadError, ReadResult, checked};

pub(super) fn validate_zero_patterns<M: Meter>(
    channels: usize,
    orders: &[u8],
    restart: usize,
    patterns: &[Pattern],
    meter: &mut M,
) -> ReadResult<()> {
    for pattern in patterns {
        checked(meter, pattern.cells.len() <= channels * 256)?;
        for cell in &pattern.cells {
            checked(meter, cell.effect != 0x0d)?;
            checked(
                meter,
                cell.effect != 0x0e || !(0x61..=0x6f).contains(&cell.parameter),
            )?;
            if cell.effect == 0x0b {
                let target = orders
                    .get(usize::from(cell.parameter))
                    .ok_or(ReadError::Invalid)?;
                checked(meter, !patterns[usize::from(*target)].cells.is_empty())?;
            }
        }
    }
    let mut seen = [false; 256];
    let mut pending = [0; 256];
    let mut count = 0;
    let enqueue =
        |order: usize, seen: &mut [bool; 256], pending: &mut [usize; 256], count: &mut usize| {
            if !seen[order] {
                seen[order] = true;
                pending[*count] = order;
                *count += 1;
            }
        };
    enqueue(0, &mut seen, &mut pending, &mut count);
    enqueue(restart, &mut seen, &mut pending, &mut count);
    while count != 0 {
        meter.charge().map_err(ReadError::Stop)?;
        count -= 1;
        let order = pending[count];
        let pattern = &patterns[usize::from(orders[order])];
        checked(meter, !pattern.cells.is_empty())?;
        let mut last_row_jumps = false;
        for row in pattern.cells.chunks_exact(channels) {
            last_row_jumps = false;
            for cell in row {
                meter.charge().map_err(ReadError::Stop)?;
                if cell.effect == 0x0b {
                    last_row_jumps = true;
                    enqueue(
                        usize::from(cell.parameter),
                        &mut seen,
                        &mut pending,
                        &mut count,
                    );
                }
            }
        }
        // Every row jump is a possible edge; only an unconditional final jump removes fallthrough.
        if !last_row_jumps {
            let next = if order + 1 < orders.len() {
                order + 1
            } else {
                restart
            };
            enqueue(next, &mut seen, &mut pending, &mut count);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
