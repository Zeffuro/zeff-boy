use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, bail, ensure};

impl MainBus {
    pub fn write_state(&self, w: &mut StateWriter) {
        let s = self.snapshot();
        w.write_u8(2);
        w.write_u64(s.origin_rise_half);
        w.write_u64(s.now_half);
        w.write_u64(s.last_edge_half);
        w.write_bool(s.clock_high);
        w.write_u8(s.enable.phase);
        optional_u64(w, s.enable.last_fall_half);
        w.write_u8(s.function_code as u8);
        w.write_u8(s.phase as u8);
        w.write_bool(s.pending.is_some());
        if let Some(p) = s.pending {
            write_request(w, p.request);
            w.write_u64(p.start_half);
            w.write_u32(p.waits);
            optional_u64(w, p.sampled_ack_half);
            optional_word(w, p.read_word);
            write_vpa(w, p.vpa);
        }
        w.write_bool(s.completed.is_some());
        if let Some(p) = s.completed {
            write_request(w, p.request);
            w.write_u64(p.start_half);
            w.write_u64(p.end_half);
            w.write_u64(p.sampled_ack_half);
            w.write_u32(p.waits);
            optional_word(w, p.read_word);
            write_vpa(w, p.vpa);
            w.write_bool(p.autovector.is_some());
            if let Some(vector) = p.autovector {
                w.write_u8(vector);
            }
        }
    }

    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        ensure!(r.read_u8()? == 2, "unsupported MAIN sequencer state");
        let origin_rise_half = r.read_u64()?;
        let now_half = r.read_u64()?;
        let last_edge_half = r.read_u64()?;
        let clock_high = r.read_bool()?;
        let enable = EnableClock {
            phase: r.read_u8()?,
            last_fall_half: read_optional_u64(r)?,
        };
        let function_code = read_fc(r)?;
        let phase = match r.read_u8()? {
            0 => Phase::Idle,
            1 => Phase::S0,
            2 => Phase::S1,
            3 => Phase::S2,
            4 => Phase::S3,
            5 => Phase::S4,
            6 => Phase::WaitLow,
            7 => Phase::WaitHigh,
            8 => Phase::S5,
            9 => Phase::S6,
            10 => Phase::S7,
            11 => Phase::VpaLow,
            12 => Phase::VpaHigh,
            _ => bail!("invalid MAIN sequencer phase"),
        };
        let pending = if r.read_bool()? {
            Some(Pending {
                request: read_request(r)?,
                start_half: r.read_u64()?,
                waits: r.read_u32()?,
                sampled_ack_half: if r.read_bool()? {
                    Some(r.read_u64()?)
                } else {
                    None
                },
                read_word: read_optional_word(r)?,
                vpa: read_vpa(r)?,
            })
        } else {
            None
        };
        let completed = if r.read_bool()? {
            Some(Receipt {
                request: read_request(r)?,
                start_half: r.read_u64()?,
                end_half: r.read_u64()?,
                sampled_ack_half: r.read_u64()?,
                waits: r.read_u32()?,
                read_word: read_optional_word(r)?,
                vpa: read_vpa(r)?,
                autovector: if r.read_bool()? {
                    Some(r.read_u8()?)
                } else {
                    None
                },
            })
        } else {
            None
        };
        Self::from_snapshot(Snapshot {
            origin_rise_half,
            now_half,
            last_edge_half,
            clock_high,
            enable,
            function_code,
            phase,
            pending,
            completed,
        })
        .map_err(|error| anyhow::anyhow!("invalid MAIN sequencer state: {error:?}"))
    }
}

fn write_request(w: &mut StateWriter, request: Request) {
    let a = request.access;
    w.write_u32(a.logical_address);
    w.write_u32(a.bus_address);
    w.write_u8(a.function_code as u8);
    w.write_u8(match a.kind {
        AccessKind::Opcode => 0,
        AccessKind::Extension => 1,
        AccessKind::ResetVector => 2,
        AccessKind::Vector => 3,
        AccessKind::Stack => 4,
        AccessKind::Data => 5,
        AccessKind::Discard => 6,
        AccessKind::Prefetch => 7,
        AccessKind::InterruptAcknowledge => 8,
    });
    w.write_u8(match a.lanes {
        ByteLanes::Upper => 0,
        ByteLanes::Lower => 1,
        ByteLanes::Both => 2,
    });
    match request.operation {
        Operation::Read => w.write_u8(0),
        Operation::Write(word) => {
            w.write_u8(1);
            w.write_u16(word);
        }
        Operation::InterruptAcknowledge => w.write_u8(2),
    }
}

fn read_fc(r: &mut StateReader<'_>) -> Result<FunctionCode> {
    Ok(match r.read_u8()? {
        1 => FunctionCode::UserData,
        2 => FunctionCode::UserProgram,
        5 => FunctionCode::SupervisorData,
        6 => FunctionCode::SupervisorProgram,
        7 => FunctionCode::CpuSpace,
        _ => bail!("invalid MAIN function code"),
    })
}

fn read_request(r: &mut StateReader<'_>) -> Result<Request> {
    let logical_address = r.read_u32()?;
    let bus_address = r.read_u32()?;
    let function_code = read_fc(r)?;
    let kind = match r.read_u8()? {
        0 => AccessKind::Opcode,
        1 => AccessKind::Extension,
        2 => AccessKind::ResetVector,
        3 => AccessKind::Vector,
        4 => AccessKind::Stack,
        5 => AccessKind::Data,
        6 => AccessKind::Discard,
        7 => AccessKind::Prefetch,
        8 => AccessKind::InterruptAcknowledge,
        _ => bail!("invalid MAIN access kind"),
    };
    let lanes = match r.read_u8()? {
        0 => ByteLanes::Upper,
        1 => ByteLanes::Lower,
        2 => ByteLanes::Both,
        _ => bail!("invalid MAIN byte lanes"),
    };
    let operation = match r.read_u8()? {
        0 => Operation::Read,
        1 => Operation::Write(r.read_u16()?),
        2 => Operation::InterruptAcknowledge,
        _ => bail!("invalid MAIN operation"),
    };
    Ok(Request {
        access: BusAccess {
            logical_address,
            bus_address,
            function_code,
            kind,
            lanes,
        },
        operation,
    })
}

fn optional_u64(w: &mut StateWriter, value: Option<u64>) {
    w.write_bool(value.is_some());
    if let Some(value) = value {
        w.write_u64(value);
    }
}

fn optional_word(w: &mut StateWriter, value: Option<u16>) {
    w.write_bool(value.is_some());
    if let Some(value) = value {
        w.write_u16(value);
    }
}

fn read_optional_word(r: &mut StateReader<'_>) -> Result<Option<u16>> {
    Ok(if r.read_bool()? {
        Some(r.read_u16()?)
    } else {
        None
    })
}

fn read_optional_u64(r: &mut StateReader<'_>) -> Result<Option<u64>> {
    Ok(if r.read_bool()? {
        Some(r.read_u64()?)
    } else {
        None
    })
}

fn write_vpa(w: &mut StateWriter, sync: Option<VpaSync>) {
    w.write_bool(sync.is_some());
    if let Some(sync) = sync {
        w.write_u64(sync.recognition_half);
        w.write_u64(sync.rise_half);
        w.write_u64(sync.fall_half);
    }
}

fn read_vpa(r: &mut StateReader<'_>) -> Result<Option<VpaSync>> {
    Ok(if r.read_bool()? {
        Some(VpaSync {
            recognition_half: r.read_u64()?,
            rise_half: r.read_u64()?,
            fall_half: r.read_u64()?,
        })
    } else {
        None
    })
}
