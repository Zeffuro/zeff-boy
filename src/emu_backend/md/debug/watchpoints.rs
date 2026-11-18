use super::*;
use crate::debug::{CpuDebugSnapshot, DebugSection, WatchHitDisplay, WatchpointDisplay};
use zeff_emu_common::debug::WatchType;
use zeff_m68k::bus::Direction;

impl PcBreakpoints {
    pub(super) fn observe(&mut self, access: MemoryAccess) {
        if self.watch_hit.is_some() {
            return;
        }
        let matches = self.controller.watchpoints.iter().any(|watch| {
            (watch.address..=watch.end_address).contains(&access.address)
                && matches!(
                    (watch.watch_type, access.direction),
                    (WatchType::ReadWrite, _)
                        | (WatchType::Read, Direction::Read)
                        | (WatchType::Write, Direction::Write)
                )
        });
        if matches {
            self.watch_hit = Some(access);
        }
    }

    pub(super) fn overlay_watchpoints(&self, cpu: &mut CpuDebugSnapshot) {
        cpu.watchpoints = self
            .controller
            .watchpoints
            .iter()
            .map(|watch| WatchpointDisplay {
                address: watch.address,
                end_address: watch.end_address,
                watch_type: watch.watch_type,
            })
            .collect();
        cpu.hit_watchpoint = self.watch_hit.map(|access| WatchHitDisplay {
            address: access.address,
            old_value: access.old_value,
            new_value: access.value,
            watch_type: match access.direction {
                Direction::Read => WatchType::Read,
                Direction::Write => WatchType::Write,
            },
        });
        cpu.sections.push(DebugSection {
            heading: "Memory watchpoints",
            lines: vec![
                "M68000 operand/stack accesses; stops after the CPU boundary".into(),
                "Fetches, vectors, DMA, Z80 and device-internal accesses excluded".into(),
                "Write hits report the driven byte; prior work-RAM bytes when available".into(),
            ],
        });
    }
}

impl MdBackend {
    pub(in crate::emu_backend::md) fn apply_watchpoint_actions(
        &mut self,
        actions: &DebugUiActions,
    ) {
        if self.rollback_owned() {
            return;
        }
        let valid = |start, end| start <= 0xff_ffff && end <= 0xff_ffff;
        let controller = &mut self.debugger.controller;
        if let Some((start, end, watch_type)) = actions.add_watchpoint
            && valid(start, end)
        {
            controller.add_watchpoint_range(start, end, watch_type);
        }
        for &(start, end, watch_type) in &actions.remove_watchpoints {
            if valid(start, end) {
                controller.remove_watchpoint(start, end, watch_type);
            }
        }
    }
}
