use super::*;

pub(in crate::timing) struct Stream {
    pub(in crate::timing) cursor: u32,
    word: Option<u16>,
}

impl Stream {
    pub(in crate::timing) fn new(pipeline: Pipeline) -> Self {
        Self {
            cursor: pipeline.address.wrapping_add(2),
            word: Some(pipeline.words[1]),
        }
    }

    pub(in crate::timing) fn take<B: ClockedBus>(
        &mut self,
        cpu: &mut PrefetchCpu,
        bus: &mut B,
        delayed: bool,
    ) -> Result<u16, TimingFault<B::Error>> {
        let word = self.word.unwrap();
        let next = self.cursor.wrapping_add(2);
        self.word = if delayed {
            None
        } else {
            Some(cpu.fetch(bus, next)?)
        };
        self.cursor = next;
        Ok(word)
    }

    pub(in crate::timing) fn finish<B: ClockedBus>(
        &mut self,
        cpu: &mut PrefetchCpu,
        bus: &mut B,
    ) -> Result<[u16; 2], TimingFault<B::Error>> {
        let first = match self.word {
            Some(word) => word,
            None => cpu.fetch(bus, self.cursor)?,
        };
        Ok([first, cpu.fetch(bus, self.cursor.wrapping_add(2))?])
    }
}

fn indexed(registers: &Registers, base: u32, extension: u16) -> u32 {
    let register = usize::from((extension >> 12) & 7);
    let value = if extension & 0x8000 == 0 {
        registers.d[register]
    } else {
        registers.address_register(register).unwrap()
    };
    let value = if extension & 0x0800 == 0 {
        Size::Word.sign_extend(value)
    } else {
        value
    };
    base.wrapping_add(value)
        .wrapping_add_signed(i32::from(extension as u8 as i8))
}

pub(in crate::timing) fn extension_address<B: ClockedBus>(
    cpu: &mut PrefetchCpu,
    bus: &mut B,
    stream: &mut Stream,
    mode: u16,
    register: usize,
    delay_absolute_low: bool,
) -> Result<u32, TimingFault<B::Error>> {
    let base = if mode == 7 {
        stream.cursor
    } else {
        cpu.registers.address_register(register).unwrap()
    };
    match (mode, register) {
        (5, _) | (7, 2) => {
            let displacement = stream.take(cpu, bus, false)? as i16;
            Ok(base.wrapping_add_signed(i32::from(displacement)))
        }
        (6, _) | (7, 3) => {
            cpu.idle(bus, 2)?;
            let extension = stream.take(cpu, bus, false)?;
            Ok(indexed(&cpu.registers, base, extension))
        }
        (7, 0) => Ok(Size::Word.sign_extend(u32::from(stream.take(cpu, bus, false)?))),
        (7, 1) => {
            let high = stream.take(cpu, bus, false)?;
            let low = stream.take(cpu, bus, delay_absolute_low)?;
            Ok((u32::from(high) << 16) | u32::from(low))
        }
        _ => unreachable!(),
    }
}
