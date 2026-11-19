use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::timing::ControlStackBank;

fn validate(frame: &CallStackFrame) -> Result<()> {
    ensure!(
        (frame.source_pc
            | frame.target_pc
            | frame.return_pc
            | frame.stack.before
            | frame.stack.after
            | frame.stack.pc_slot)
            & 1
            == 0,
        "odd captured call-stack coordinate"
    );
    let exception = matches!(
        frame.kind,
        ControlFlowEntryKind::Exception(_) | ControlFlowEntryKind::Interrupt { .. }
    );
    ensure!(
        !exception || frame.stack.bank == ControlStackBank::Supervisor,
        "exception stack bank mismatch"
    );
    let bytes = if exception { 6 } else { 4 };
    ensure!(
        frame.stack.after == frame.stack.before.wrapping_sub(bytes)
            && frame.stack.pc_slot == frame.stack.before.wrapping_sub(4),
        "captured call-stack slot mismatch"
    );
    if let ControlFlowEntryKind::Interrupt { level, .. } = frame.kind {
        ensure!((1..=7).contains(&level), "invalid captured interrupt level");
    }
    Ok(())
}

impl Tracker {
    pub(in crate::emulator) fn write_owned(&self) -> Result<Vec<u8>> {
        let mut writer = StateWriter::new();
        writer.write_u8(1);
        writer.write_bool(self.enabled());
        if let Some(recording) = &self.recording {
            ensure!(
                recording.frames.len() <= MAX_FRAMES,
                "captured call-stack count exceeds bound"
            );
            ensure!(
                !recording.unknown || recording.frames.is_empty() && !recording.truncated,
                "invalid unknown call stack"
            );
            writer.write_bool(recording.unknown);
            writer.write_bool(recording.truncated);
            writer.write_u16(recording.frames.len() as u16);
            for frame in &recording.frames {
                validate(frame)?;
                match frame.kind {
                    ControlFlowEntryKind::Jsr => writer.write_u8(0),
                    ControlFlowEntryKind::Bsr => writer.write_u8(1),
                    ControlFlowEntryKind::Debugger => writer.write_u8(2),
                    ControlFlowEntryKind::Exception(vector) => {
                        writer.write_u8(3);
                        writer.write_u8(vector);
                    }
                    ControlFlowEntryKind::Interrupt { level, vector } => {
                        writer.write_u8(4);
                        writer.write_u8(level);
                        writer.write_u8(vector);
                    }
                }
                writer.write_u32(frame.source_pc);
                writer.write_u32(frame.target_pc);
                writer.write_u32(frame.return_pc);
                writer.write_u8(match frame.stack.bank {
                    ControlStackBank::User => 0,
                    ControlStackBank::Supervisor => 1,
                });
                writer.write_u32(frame.stack.before);
                writer.write_u32(frame.stack.after);
                writer.write_u32(frame.stack.pc_slot);
            }
        }
        let bytes = writer.into_bytes();
        ensure!(
            bytes.len() <= MAX_CALL_STACK_BYTES,
            "captured call stack exceeds byte bound"
        );
        Ok(bytes)
    }

    pub(in crate::emulator) fn read_owned(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_CALL_STACK_BYTES,
            "captured call stack exceeds byte bound"
        );
        let mut reader = StateReader::new(bytes);
        ensure!(
            reader.read_u8()? == 1,
            "unsupported captured call-stack version"
        );
        let recording = if reader.read_bool()? {
            let unknown = reader.read_bool()?;
            let truncated = reader.read_bool()?;
            let count = usize::from(reader.read_u16()?);
            ensure!(
                count <= MAX_FRAMES,
                "captured call-stack count exceeds bound"
            );
            ensure!(
                !unknown || count == 0 && !truncated,
                "invalid unknown call stack"
            );
            let mut frames = VecDeque::with_capacity(count);
            for _ in 0..count {
                let kind = match reader.read_u8()? {
                    0 => ControlFlowEntryKind::Jsr,
                    1 => ControlFlowEntryKind::Bsr,
                    2 => ControlFlowEntryKind::Debugger,
                    3 => ControlFlowEntryKind::Exception(reader.read_u8()?),
                    4 => ControlFlowEntryKind::Interrupt {
                        level: reader.read_u8()?,
                        vector: reader.read_u8()?,
                    },
                    _ => anyhow::bail!("invalid captured call-stack kind"),
                };
                let source_pc = reader.read_u32()?;
                let target_pc = reader.read_u32()?;
                let return_pc = reader.read_u32()?;
                let bank = match reader.read_u8()? {
                    0 => ControlStackBank::User,
                    1 => ControlStackBank::Supervisor,
                    _ => anyhow::bail!("invalid captured stack bank"),
                };
                let frame = CallStackFrame {
                    kind,
                    source_pc,
                    target_pc,
                    return_pc,
                    stack: ControlStack {
                        bank,
                        before: reader.read_u32()?,
                        after: reader.read_u32()?,
                        pc_slot: reader.read_u32()?,
                    },
                };
                validate(&frame)?;
                frames.push_back(frame);
            }
            Some(Box::new(Recording {
                frames,
                unknown,
                truncated,
            }))
        } else {
            None
        };
        ensure!(reader.is_exhausted(), "trailing captured call-stack data");
        Ok(Self { recording })
    }
}
