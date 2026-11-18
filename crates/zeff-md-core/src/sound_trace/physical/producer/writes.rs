use super::fold::Fold;
use super::*;
use zeff_emu_common::debug::{TraceWrite, TraceWriteKind, TraceWriteWidth};
use zeff_z80::Cpu;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Local,
    Work { bank: u16, address: u16 },
    Psg { bank: u16 },
    Vdp { kind: vdp::Kind, address: u32 },
}
#[derive(Clone, Copy)]
pub(super) struct ObservedWrite {
    pub stamp: Stamp,
    pub ordinal: usize,
    pub write: TraceWrite,
    target: Target,
}
#[derive(Clone, Copy)]
pub(super) struct Link {
    pub id: Occurrence,
    pub row: Option<u64>,
    pub expected: bool,
    pub accepted: Option<ObservedWrite>,
}

impl Fold {
    pub(super) fn activation(
        &mut self,
        event: &Event,
        _: Binding,
        output: &mut InstructionTraceStore,
    ) {
        let Some(id) = event.occurrence else {
            self.gap(output);
            return;
        };
        if self.links[15].is_some_and(|link| link.expected && link.accepted.is_none()) {
            self.gap(output);
        }
        self.links.rotate_right(1);
        self.links[0] = Some(Link {
            id,
            row: self.current,
            expected: false,
            accepted: None,
        });
    }
    pub(super) fn bind_retirement(
        &mut self,
        occurrence: Option<Occurrence>,
        row: u64,
        output: &mut InstructionTraceStore,
    ) {
        let Some(id) = occurrence else {
            return;
        };
        let Some(link) = self.links.iter_mut().flatten().find(|link| link.id == id) else {
            self.gap(output);
            return;
        };
        if link.row.is_some_and(|owner| owner != row) {
            self.gap(output);
            return;
        }
        link.row = Some(row);
    }
    pub(super) fn expected(&mut self, event: &Event, output: &mut InstructionTraceStore) {
        let Some(id) = event.occurrence else {
            self.gap(output);
            return;
        };
        if let Some(link) = self.links.iter_mut().flatten().find(|link| link.id == id) {
            link.expected = true;
        } else {
            self.gap(output);
        }
    }
    fn accept_write(
        &mut self,
        event: &Event,
        write: ObservedWrite,
        cpu: &Cpu,
        output: &mut InstructionTraceStore,
    ) {
        let Some(id) = event.occurrence else {
            self.gap(output);
            return;
        };
        let Some(index) = self
            .links
            .iter()
            .position(|link| link.is_some_and(|link| link.id == id))
        else {
            self.gap(output);
            return;
        };
        let link = self.links[index].unwrap();
        if let Some(first) = link.accepted {
            if first.write != write.write || first.target != write.target {
                self.gap(output);
            }
            return;
        }
        let row = match link.row {
            Some(row) => row,
            None => match self.suffix(cpu, output) {
                Some(row) => row,
                None => return,
            },
        };
        let Some(owner) = self.row_mut(row) else {
            self.gap(output);
            return;
        };
        if let Some(slot) = owner.writes.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(write);
        } else {
            owner.record.write_overflow = owner.record.write_overflow.saturating_add(1);
        }
        self.links[index] = Some(Link {
            row: Some(row),
            expected: true,
            accepted: Some(write),
            ..link
        });
    }
    pub(super) fn local(
        &mut self,
        event: &Event,
        transfer: (u16, u8),
        cpu: &Cpu,
        output: &mut InstructionTraceStore,
    ) {
        self.accept_write(
            event,
            observed(event.stamp, 0, transfer, Target::Local),
            cpu,
            output,
        );
    }
    pub(super) fn work(
        &mut self,
        event: &Event,
        tag: WorkTag,
        cpu: &Cpu,
        output: &mut InstructionTraceStore,
    ) {
        let address = crate::hardware::vdp::owner::Companion::mapped_address(tag.logical, tag.bank);
        if tag.bank >= 512 || address < 0xe00000 || address as u16 != tag.address {
            self.gap(output);
            return;
        }
        self.accept_write(
            event,
            observed(
                tag.stamp,
                tag.ordinal,
                (tag.logical, tag.value),
                Target::Work {
                    bank: tag.bank,
                    address: tag.address,
                },
            ),
            cpu,
            output,
        );
    }
    pub(super) fn vdp_write(
        &mut self,
        event: &Event,
        receipt: vdp::Acceptance,
        cpu: &Cpu,
        output: &mut InstructionTraceStore,
    ) {
        let mapped = crate::hardware::vdp::owner::Companion::mapped_address(
            receipt.binding.address,
            receipt.binding.bank,
        );
        if mapped & !1 != receipt.bus_address
            || receipt.source > event.stamp
            || !receipt.lanes.into_iter().any(|lane| lane)
        {
            self.gap(output);
            return;
        }
        let mut write = observed(
            event.stamp,
            usize::from(receipt.primitive),
            (receipt.binding.address, receipt.word as u8),
            Target::Vdp {
                kind: receipt.kind,
                address: receipt.address,
            },
        );
        if receipt.kind == vdp::Kind::DisconnectedPsg {
            write.write.kind = TraceWriteKind::Bus;
        }
        self.accept_write(event, write, cpu, output);
    }
    pub(super) fn psg(
        &mut self,
        event: &Event,
        receipt: PsgWrite,
        cpu: &Cpu,
        output: &mut InstructionTraceStore,
    ) {
        let binding = receipt.binding;
        self.accept_write(
            event,
            observed(
                Stamp {
                    half: binding.half,
                    pass: binding.pass,
                },
                0,
                (binding.address, receipt.value),
                Target::Psg { bank: binding.bank },
            ),
            cpu,
            output,
        );
    }
}
fn observed(stamp: Stamp, ordinal: usize, transfer: (u16, u8), target: Target) -> ObservedWrite {
    ObservedWrite {
        stamp,
        ordinal,
        target,
        write: TraceWrite {
            address: u32::from(transfer.0),
            old_value: None,
            new_value: u32::from(transfer.1),
            width: TraceWriteWidth::Byte,
            kind: TraceWriteKind::Memory,
        },
    }
}
