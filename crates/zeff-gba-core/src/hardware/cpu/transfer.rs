use super::ops::{rotate_right, sign_extend};
use super::*;

struct SingleTransfer {
    operation: CpuBusOperation,
    address: u32,
    width: u8,
    value: u32,
    destination: usize,
    writeback: Option<(usize, u32)>,
}

impl Cpu {
    pub(super) fn begin_staged_transfer(&mut self, fetched: FetchedInstruction) -> bool {
        match fetched.decoded {
            DecodedInstruction::Arm {
                class: ArmInstructionClass::SingleDataTransfer,
                ..
            } => self.begin_arm_single_transfer(fetched.pc, fetched.raw),
            DecodedInstruction::Arm {
                class: ArmInstructionClass::BlockDataTransfer,
                ..
            } => {
                self.begin_arm_block_transfer(fetched.pc, fetched.raw);
                true
            }
            DecodedInstruction::Arm {
                class: ArmInstructionClass::SingleDataSwap,
                ..
            } => {
                self.begin_arm_swap(fetched.pc, fetched.raw);
                true
            }
            DecodedInstruction::Thumb { class } => match class {
                ThumbInstructionClass::PcRelativeLoad => {
                    self.begin_thumb_pc_relative_load(fetched.pc, fetched.raw as u16);
                    true
                }
                ThumbInstructionClass::LoadStore => {
                    self.begin_thumb_load_store(fetched.raw as u16);
                    true
                }
                ThumbInstructionClass::LoadStoreHalfword => {
                    self.begin_thumb_halfword_transfer(fetched.raw as u16);
                    true
                }
                ThumbInstructionClass::SpRelativeLoad => {
                    self.begin_thumb_sp_relative_transfer(fetched.raw as u16);
                    true
                }
                ThumbInstructionClass::PushPop => self.begin_thumb_push_pop(fetched.raw as u16),
                ThumbInstructionClass::MultipleLoadStore => {
                    self.begin_thumb_multiple_transfer(fetched.raw as u16);
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn begin_arm_single_transfer(&mut self, pc: u32, raw: u32) -> bool {
        if raw & 0x0E00_0090 == 0x0000_0090 && raw & 0x60 != 0 {
            return self.begin_arm_halfword_transfer(pc, raw);
        }

        let register_offset = raw & (1 << 25) != 0;
        let pre_index = raw & (1 << 24) != 0;
        let add = raw & (1 << 23) != 0;
        let byte = raw & (1 << 22) != 0;
        let writeback = raw & (1 << 21) != 0;
        let load = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let rd = ((raw >> 12) & 0xF) as usize;
        let base = self.reg_read_arm(rn, pc);
        let offset = if register_offset {
            self.arm_register_operand(raw, pc).0
        } else {
            raw & 0xFFF
        };
        let indexed = if add {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let address = if pre_index { indexed } else { base };
        let width = if byte { 1 } else { 4 };
        let value = if load {
            0
        } else {
            self.reg_read_arm(rd, pc)
                .wrapping_add(if rd == 15 { 4 } else { 0 })
        };
        self.prepare_single_transfer(SingleTransfer {
            operation: if load {
                CpuBusOperation::Read
            } else {
                CpuBusOperation::Write
            },
            address,
            width,
            value,
            destination: rd,
            writeback: ((!pre_index || writeback) && !(load && rn == rd)).then_some((rn, indexed)),
        });
        true
    }

    fn begin_arm_halfword_transfer(&mut self, pc: u32, raw: u32) -> bool {
        let pre_index = raw & (1 << 24) != 0;
        let add = raw & (1 << 23) != 0;
        let immediate = raw & (1 << 22) != 0;
        let writeback = raw & (1 << 21) != 0;
        let load = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let rd = ((raw >> 12) & 0xF) as usize;
        let mode = (raw >> 5) & 0x3;
        if !load && mode != 0b01 {
            return false;
        }
        let base = self.reg_read_arm(rn, pc);
        let offset = if immediate {
            ((raw >> 4) & 0xF0) | (raw & 0xF)
        } else {
            self.reg_read_arm((raw & 0xF) as usize, pc)
        };
        let indexed = if add {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let address = if pre_index { indexed } else { base };
        let width = match mode {
            0b01 => 2,
            0b10 => 1,
            0b11 if address & 1 != 0 => 1,
            0b11 => 2,
            _ => return false,
        };
        self.prepare_single_transfer(SingleTransfer {
            operation: if load {
                CpuBusOperation::Read
            } else {
                CpuBusOperation::Write
            },
            address,
            width,
            value: if load { 0 } else { self.reg_read_arm(rd, pc) },
            destination: rd,
            writeback: ((!pre_index || writeback) && !(load && rn == rd)).then_some((rn, indexed)),
        });
        true
    }

    fn begin_arm_swap(&mut self, pc: u32, raw: u32) {
        let byte = raw & (1 << 22) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let rm = (raw & 0xF) as usize;
        self.prepare_single_transfer(SingleTransfer {
            operation: CpuBusOperation::Read,
            address: self.reg_read_arm(rn, pc),
            width: if byte { 1 } else { 4 },
            value: self.reg_read_arm(rm, pc),
            destination: ((raw >> 12) & 0xF) as usize,
            writeback: None,
        });
    }

    fn prepare_single_transfer(&mut self, transfer: SingleTransfer) {
        self.execution_state.phase = CpuExecutionPhase::DataBus;
        self.execution_state.bus_operation = transfer.operation;
        self.execution_state.bus_address = transfer.address;
        self.execution_state.bus_width = transfer.width;
        self.execution_state.bus_sequential = false;
        self.execution_state.bus_value = transfer.value;
        self.execution_state.transfer_register_mask = 1 << transfer.destination;
        self.execution_state.transfer_next_register = transfer.destination as u8;
        self.execution_state.transfer_first_access = true;
        if let Some((register, value)) = transfer.writeback {
            self.execution_state.writeback_present = true;
            self.execution_state.writeback_register = register as u8;
            self.execution_state.writeback_value = value;
        }
    }

    fn begin_arm_block_transfer(&mut self, pc: u32, raw: u32) {
        let pre = raw & (1 << 24) != 0;
        let up = raw & (1 << 23) != 0;
        let force_user = raw & (1 << 22) != 0;
        let writeback = raw & (1 << 21) != 0;
        let load = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let list = (raw & 0xFFFF) as u16;
        let count = if list == 0 { 16 } else { list.count_ones() };
        let base = self.regs[rn];
        let address = match (up, pre) {
            (true, false) => base,
            (true, true) => base.wrapping_add(4),
            (false, false) => base.wrapping_sub(4 * (count - 1)),
            (false, true) => base.wrapping_sub(4 * count),
        };
        let writeback_value = if up {
            base.wrapping_add(4 * count)
        } else {
            base.wrapping_sub(4 * count)
        };
        let mask = if list == 0 { 1 << 15 } else { list };
        self.execution_state.phase = CpuExecutionPhase::DataBus;
        self.execution_state.bus_operation = if load {
            CpuBusOperation::Read
        } else {
            CpuBusOperation::Write
        };
        self.execution_state.bus_width = 4;
        self.execution_state.bus_address = address;
        self.execution_state.transfer_original_base = base;
        self.execution_state.transfer_current_address = address;
        self.execution_state.transfer_register_mask = mask;
        self.execution_state.transfer_next_register = mask.trailing_zeros() as u8;
        self.execution_state.transfer_first_access = true;
        self.execution_state.transfer_force_user =
            force_user && !(load && (list == 0 || list & (1 << 15) != 0));
        self.execution_state.transfer_exception_return =
            load && force_user && (list == 0 || list & (1 << 15) != 0);
        self.execution_state.transfer_writeback = writeback;
        if writeback && !(load && list & (1 << rn) != 0) {
            self.execution_state.writeback_present = true;
            self.execution_state.writeback_register = rn as u8;
            self.execution_state.writeback_value = writeback_value;
        }
        if !load {
            self.execution_state.bus_value = self.arm_block_store_value(pc, raw);
        }
    }

    fn arm_block_store_value(&self, pc: u32, raw: u32) -> u32 {
        let list = (raw & 0xFFFF) as u16;
        let reg = self.execution_state.transfer_next_register as usize;
        if list == 0 {
            return pc.wrapping_add(12);
        }
        let rn = ((raw >> 16) & 0xF) as usize;
        let first = list.trailing_zeros() as usize;
        let mut value =
            self.block_transfer_read_reg(reg, pc, self.execution_state.transfer_force_user);
        if self.execution_state.transfer_writeback && reg == rn && reg != first {
            value = self.execution_state.writeback_value;
        }
        if reg == 15 {
            value = value.wrapping_add(4);
        }
        value
    }

    fn begin_thumb_pc_relative_load(&mut self, pc: u32, raw: u16) {
        let rd = ((raw >> 8) & 0x7) as usize;
        let address = (pc.wrapping_add(4) & !3).wrapping_add(u32::from(raw & 0xFF) << 2);
        self.prepare_single_transfer(SingleTransfer {
            operation: CpuBusOperation::Read,
            address,
            width: 4,
            value: 0,
            destination: rd,
            writeback: None,
        });
    }

    fn begin_thumb_load_store(&mut self, raw: u16) {
        let rb = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        if raw & 0xF000 == 0x5000 {
            let ro = ((raw >> 6) & 0x7) as usize;
            let mode = (raw >> 9) & 0x7;
            let address = self.regs[rb].wrapping_add(self.regs[ro]);
            let operation = if mode <= 0b010 {
                CpuBusOperation::Write
            } else {
                CpuBusOperation::Read
            };
            let width = match mode {
                0b000 | 0b100 => 4,
                0b001 | 0b101 => 2,
                0b111 if address & 1 == 0 => 2,
                _ => 1,
            };
            self.prepare_single_transfer(SingleTransfer {
                operation,
                address,
                width,
                value: self.regs[rd],
                destination: rd,
                writeback: None,
            });
        } else {
            let byte = raw & (1 << 12) != 0;
            let load = raw & (1 << 11) != 0;
            let offset = u32::from((raw >> 6) & 0x1F) << if byte { 0 } else { 2 };
            let address = self.regs[rb].wrapping_add(offset);
            self.prepare_single_transfer(SingleTransfer {
                operation: if load {
                    CpuBusOperation::Read
                } else {
                    CpuBusOperation::Write
                },
                address,
                width: if byte { 1 } else { 4 },
                value: self.regs[rd],
                destination: rd,
                writeback: None,
            });
        }
    }

    fn begin_thumb_halfword_transfer(&mut self, raw: u16) {
        let rb = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        let load = raw & (1 << 11) != 0;
        let address = self.regs[rb].wrapping_add(u32::from((raw >> 6) & 0x1F) << 1);
        self.prepare_single_transfer(SingleTransfer {
            operation: if load {
                CpuBusOperation::Read
            } else {
                CpuBusOperation::Write
            },
            address,
            width: 2,
            value: self.regs[rd],
            destination: rd,
            writeback: None,
        });
    }

    fn begin_thumb_sp_relative_transfer(&mut self, raw: u16) {
        let load = raw & (1 << 11) != 0;
        let rd = ((raw >> 8) & 0x7) as usize;
        let address = self.regs[13].wrapping_add(u32::from(raw & 0xFF) << 2);
        self.prepare_single_transfer(SingleTransfer {
            operation: if load {
                CpuBusOperation::Read
            } else {
                CpuBusOperation::Write
            },
            address,
            width: 4,
            value: self.regs[rd],
            destination: rd,
            writeback: None,
        });
    }

    fn begin_thumb_push_pop(&mut self, raw: u16) -> bool {
        let pop = raw & (1 << 11) != 0;
        let extra = raw & (1 << 8) != 0;
        let list = raw & 0xFF;
        let mask = list
            | if extra {
                1 << if pop { 15 } else { 14 }
            } else {
                0
            };
        if mask == 0 {
            return false;
        }
        if !pop {
            self.regs[13] = self.regs[13].wrapping_sub(4 * mask.count_ones());
        }
        self.execution_state.phase = CpuExecutionPhase::DataBus;
        self.execution_state.bus_operation = if pop {
            CpuBusOperation::Read
        } else {
            CpuBusOperation::Write
        };
        self.execution_state.bus_width = 4;
        self.execution_state.bus_address = self.regs[13];
        self.execution_state.transfer_original_base = self.regs[13];
        self.execution_state.transfer_current_address = self.regs[13];
        self.execution_state.transfer_register_mask = mask;
        self.execution_state.transfer_next_register = mask.trailing_zeros() as u8;
        self.execution_state.transfer_first_access = true;
        if !pop {
            self.execution_state.bus_value = self.regs[usize::from(mask.trailing_zeros() as u8)];
        }
        true
    }

    fn begin_thumb_multiple_transfer(&mut self, raw: u16) {
        let load = raw & (1 << 11) != 0;
        let rb = ((raw >> 8) & 0x7) as usize;
        let list = raw & 0xFF;
        let mask = if list == 0 { 1 << 15 } else { list };
        let base = self.regs[rb];
        let final_address = base.wrapping_add(if list == 0 {
            0x40
        } else {
            4 * list.count_ones()
        });
        self.execution_state.phase = CpuExecutionPhase::DataBus;
        self.execution_state.bus_operation = if load {
            CpuBusOperation::Read
        } else {
            CpuBusOperation::Write
        };
        self.execution_state.bus_width = 4;
        self.execution_state.bus_address = base;
        self.execution_state.transfer_original_base = base;
        self.execution_state.transfer_current_address = base;
        self.execution_state.transfer_register_mask = mask;
        self.execution_state.transfer_next_register = mask.trailing_zeros() as u8;
        self.execution_state.transfer_first_access = true;
        if !load || list & (1 << rb) == 0 {
            self.execution_state.writeback_present = true;
            self.execution_state.writeback_register = rb as u8;
            self.execution_state.writeback_value = final_address;
        }
        if !load {
            self.execution_state.bus_value = self.thumb_multiple_store_value(raw);
        }
    }

    fn thumb_multiple_store_value(&self, raw: u16) -> u32 {
        let list = raw & 0xFF;
        let reg = self.execution_state.transfer_next_register as usize;
        if list == 0 {
            return self.regs[15].wrapping_add(4);
        }
        let rb = ((raw >> 8) & 0x7) as usize;
        let first = list.trailing_zeros() as usize;
        if reg == rb && reg != first {
            self.execution_state.writeback_value
        } else {
            self.regs[reg]
        }
    }

    pub(super) fn step_data_bus_phase(
        &mut self,
        bus: &mut Bus,
    ) -> Option<Option<FetchedInstruction>> {
        let operation = self.execution_state.bus_operation;
        let address = self.execution_state.bus_address;
        let width = self.execution_state.bus_width;
        let sequential = self.execution_state.bus_sequential;
        match operation {
            CpuBusOperation::Read => {
                self.execution_state.bus_read_latch = match (width, sequential) {
                    (1, false) => u32::from(self.cpu_read8(bus, address)),
                    (2, false) => u32::from(self.cpu_read16(bus, address)),
                    (4, false) => self.cpu_read32(bus, address),
                    (4, true) => self.cpu_read32_sequential(bus, address),
                    _ => unreachable!("invalid staged GBA CPU read"),
                };
                self.sync_staged_timing_state();
                self.execution_state.phase = CpuExecutionPhase::LoadInternal;
            }
            CpuBusOperation::Write => {
                let value = self.execution_state.bus_value;
                match (width, sequential) {
                    (1, false) => self.cpu_write8(bus, address, value as u8),
                    (2, false) => self.cpu_write16(bus, address, value as u16),
                    (4, false) => self.cpu_write32(bus, address, value),
                    (4, true) => self.cpu_write32_sequential(bus, address, value),
                    _ => unreachable!("invalid staged GBA CPU write"),
                }
                self.sync_staged_timing_state();
                self.complete_staged_store();
            }
            CpuBusOperation::None => unreachable!("missing staged GBA CPU bus operation"),
        }
        None
    }

    pub(super) fn step_load_internal_phase(
        &mut self,
        _bus: &mut Bus,
    ) -> Option<Option<FetchedInstruction>> {
        let fetched = self.active_fetched_instruction();
        match fetched.decoded {
            DecodedInstruction::Arm {
                class: ArmInstructionClass::SingleDataSwap,
                ..
            } => {
                if fetched.raw & (1 << 22) == 0 {
                    self.execution_state.bus_read_latch = rotate_right(
                        self.execution_state.bus_read_latch,
                        (self.execution_state.bus_address & 3) * 8,
                    );
                }
                self.execution_state.bus_operation = CpuBusOperation::Write;
                self.execution_state.bus_sequential = false;
                self.execution_state.phase = CpuExecutionPhase::DataBus;
            }
            DecodedInstruction::Arm {
                class: ArmInstructionClass::SingleDataTransfer,
                ..
            } => {
                let value = self.finish_arm_single_load(fetched.raw);
                self.write_reg(
                    self.execution_state.transfer_next_register as usize,
                    value,
                    false,
                );
                self.execution_state.transfer_register_mask = 0;
                self.execution_state.phase = CpuExecutionPhase::Writeback;
            }
            DecodedInstruction::Arm {
                class: ArmInstructionClass::BlockDataTransfer,
                ..
            } => self.complete_arm_block_load(fetched.raw),
            DecodedInstruction::Thumb { class } => match class {
                ThumbInstructionClass::PcRelativeLoad
                | ThumbInstructionClass::LoadStore
                | ThumbInstructionClass::LoadStoreHalfword
                | ThumbInstructionClass::SpRelativeLoad => {
                    let value = self.finish_thumb_single_load(fetched.raw as u16, class);
                    self.write_reg(
                        self.execution_state.transfer_next_register as usize,
                        value,
                        true,
                    );
                    self.execution_state.transfer_register_mask = 0;
                    self.execution_state.phase = CpuExecutionPhase::Writeback;
                }
                ThumbInstructionClass::PushPop | ThumbInstructionClass::MultipleLoadStore => {
                    self.complete_thumb_block_load(fetched.raw as u16, class)
                }
                _ => unreachable!("invalid staged Thumb load"),
            },
            _ => unreachable!("invalid staged load"),
        }
        None
    }

    pub(super) fn step_writeback_phase(
        &mut self,
        bus: &mut Bus,
    ) -> Option<Option<FetchedInstruction>> {
        let fetched = self.active_fetched_instruction();
        if matches!(
            fetched.decoded,
            DecodedInstruction::Arm {
                class: ArmInstructionClass::SingleDataSwap,
                ..
            }
        ) {
            self.write_reg(
                self.execution_state.transfer_next_register as usize,
                self.execution_state.bus_read_latch,
                false,
            );
        }
        if self.execution_state.writeback_present {
            self.regs[usize::from(self.execution_state.writeback_register)] =
                self.execution_state.writeback_value;
        }
        if self.execution_state.transfer_exception_return {
            self.return_from_exception(self.execution_state.bus_read_latch, false);
        }
        self.finish_staged_transfer(bus)
    }

    pub(super) fn sync_staged_timing_state(&mut self) {
        let (origin, elapsed, count) = self.data_access_cursor.state();
        debug_assert_eq!(origin, self.execution_state.active_fetch_cycles);
        self.execution_state.data_access_elapsed_cycles = elapsed;
        self.execution_state.data_access_count = count;
        self.execution_state.data_bus_phase_cycles = self.bus_phase_cycles;
    }

    fn finish_arm_single_load(&self, raw: u32) -> u32 {
        let address = self.execution_state.bus_address;
        let value = self.execution_state.bus_read_latch;
        if raw & 0x0E00_0090 == 0x0000_0090 && raw & 0x60 != 0 {
            return match (raw >> 5) & 0x3 {
                0b01 => rotate_right(value, (address & 1) * 8),
                0b10 => sign_extend(value, 8) as u32,
                0b11 if address & 1 != 0 => sign_extend(value, 8) as u32,
                0b11 => sign_extend(value, 16) as u32,
                _ => value,
            };
        }
        if raw & (1 << 22) == 0 {
            rotate_right(value, (address & 3) * 8)
        } else {
            value
        }
    }

    fn finish_thumb_single_load(&self, raw: u16, class: ThumbInstructionClass) -> u32 {
        let address = self.execution_state.bus_address;
        let value = self.execution_state.bus_read_latch;
        match class {
            ThumbInstructionClass::PcRelativeLoad => value,
            ThumbInstructionClass::LoadStore if raw & 0xF000 == 0x5000 => match (raw >> 9) & 0x7 {
                0b011 => sign_extend(value, 8) as u32,
                0b100 => rotate_right(value, (address & 3) * 8),
                0b101 => rotate_right(value, (address & 1) * 8),
                0b110 => value,
                0b111 if address & 1 != 0 => sign_extend(value, 8) as u32,
                0b111 => sign_extend(value, 16) as u32,
                _ => value,
            },
            ThumbInstructionClass::LoadStore => {
                if raw & (1 << 12) == 0 {
                    rotate_right(value, (address & 3) * 8)
                } else {
                    value
                }
            }
            ThumbInstructionClass::LoadStoreHalfword => rotate_right(value, (address & 1) * 8),
            ThumbInstructionClass::SpRelativeLoad => rotate_right(value, (address & 3) * 8),
            _ => value,
        }
    }

    fn complete_staged_store(&mut self) {
        let fetched = self.active_fetched_instruction();
        match fetched.decoded {
            DecodedInstruction::Arm {
                class: ArmInstructionClass::BlockDataTransfer,
                ..
            }
            | DecodedInstruction::Thumb {
                class: ThumbInstructionClass::PushPop | ThumbInstructionClass::MultipleLoadStore,
            } => self.advance_staged_block(fetched),
            _ => {
                self.execution_state.transfer_register_mask = 0;
                self.execution_state.phase = CpuExecutionPhase::Writeback;
            }
        }
    }

    fn complete_arm_block_load(&mut self, raw: u32) {
        let reg = self.execution_state.transfer_next_register as usize;
        let value = self.execution_state.bus_read_latch;
        if self.execution_state.transfer_exception_return && reg == 15 {
            self.execution_state.bus_read_latch = value;
        } else {
            self.block_transfer_write_reg(reg, value, self.execution_state.transfer_force_user);
        }
        self.advance_staged_block(self.active_fetched_instruction());
        if raw & 0xFFFF == 0 && self.execution_state.phase == CpuExecutionPhase::Writeback {
            self.execution_state.transfer_register_mask = 0;
        }
    }

    fn complete_thumb_block_load(&mut self, raw: u16, class: ThumbInstructionClass) {
        let reg = self.execution_state.transfer_next_register as usize;
        let value = self.execution_state.bus_read_latch;
        self.write_reg(reg, value, true);
        self.advance_staged_block(self.active_fetched_instruction());
        if class == ThumbInstructionClass::PushPop && raw & (1 << 11) != 0 {
            self.regs[13] = self.execution_state.transfer_current_address;
        }
    }

    fn advance_staged_block(&mut self, fetched: FetchedInstruction) {
        let completed = self.execution_state.transfer_next_register;
        self.execution_state.transfer_register_mask &= !(1 << completed);
        self.execution_state.transfer_current_address = self
            .execution_state
            .transfer_current_address
            .wrapping_add(4);
        self.execution_state.transfer_first_access = false;
        if self.execution_state.transfer_register_mask == 0 {
            self.execution_state.phase = CpuExecutionPhase::Writeback;
            return;
        }
        self.execution_state.transfer_next_register =
            self.execution_state.transfer_register_mask.trailing_zeros() as u8;
        self.execution_state.bus_address = self.execution_state.transfer_current_address;
        self.execution_state.bus_sequential = true;
        match fetched.decoded {
            DecodedInstruction::Arm {
                class: ArmInstructionClass::BlockDataTransfer,
                ..
            } if self.execution_state.bus_operation == CpuBusOperation::Write => {
                self.execution_state.bus_value =
                    self.arm_block_store_value(fetched.pc, fetched.raw);
            }
            DecodedInstruction::Thumb {
                class: ThumbInstructionClass::PushPop,
            } if self.execution_state.bus_operation == CpuBusOperation::Write => {
                self.execution_state.bus_value =
                    self.regs[usize::from(self.execution_state.transfer_next_register)];
            }
            DecodedInstruction::Thumb {
                class: ThumbInstructionClass::MultipleLoadStore,
            } if self.execution_state.bus_operation == CpuBusOperation::Write => {
                self.execution_state.bus_value =
                    self.thumb_multiple_store_value(fetched.raw as u16);
            }
            _ => {}
        }
        self.execution_state.phase = CpuExecutionPhase::DataBus;
    }

    fn finish_staged_transfer(&mut self, bus: &mut Bus) -> Option<Option<FetchedInstruction>> {
        let fetched = self.active_fetched_instruction();
        let total_cycles = fetched
            .fetch_cycles
            .saturating_add(instruction_base_cycles(fetched, true));
        self.finish_data_access_timing(bus, total_cycles);
        if instruction_has_load_final_internal_cycle(fetched, true) {
            self.pending_load_internal_cycle = true;
        }
        let active = self.execution_state;
        self.execution_state = CpuExecutionState {
            phase: CpuExecutionPhase::Execute,
            instruction_active: true,
            active_pc: active.active_pc,
            active_raw: active.active_raw,
            active_thumb: active.active_thumb,
            condition_passed: true,
            active_fetch_cycles: active.active_fetch_cycles,
            ..CpuExecutionState::default()
        };
        if self.state == CpuState::Running && self.pipeline.len() == 0 {
            self.begin_refill_phases();
            return None;
        }
        self.complete_active_instruction(bus)
    }

    pub(super) fn valid_staged_transfer_state(&self, state: CpuExecutionState) -> bool {
        if state.phase_cycles_remaining != 0
            || !state.instruction_active
            || !state.condition_passed
            || state.active_fetch_cycles == 0
            || state.active_pc & if state.active_thumb { 1 } else { 3 } != 0
            || !matches!(
                state.bus_operation,
                CpuBusOperation::Read | CpuBusOperation::Write
            )
            || !matches!(state.bus_width, 1 | 2 | 4)
            || state.bus_sequential && state.bus_width != 4
            || state.transfer_next_register > 15
            || state.writeback_register > 15
            || state.refill_target != 0
            || state.refill_thumb
            || state.refill_index != 0
            || state.data_access_elapsed_cycles > 1024
            || state.data_access_count > 16
            || state.data_bus_phase_cycles
                > state
                    .active_fetch_cycles
                    .saturating_add(state.data_access_elapsed_cycles)
        {
            return false;
        }
        let fetched = FetchedInstruction {
            pc: state.active_pc,
            raw: state.active_raw,
            instruction_set: if state.active_thumb {
                InstructionSet::Thumb
            } else {
                InstructionSet::Arm
            },
            width_bytes: if state.active_thumb { 2 } else { 4 },
            fetch_cycles: state.active_fetch_cycles,
            decoded: decode::decode_stub(
                state.active_raw,
                if state.active_thumb {
                    InstructionSet::Thumb
                } else {
                    InstructionSet::Arm
                },
            ),
        };
        let supported = matches!(
            fetched.decoded,
            DecodedInstruction::Arm {
                class: ArmInstructionClass::SingleDataTransfer
                    | ArmInstructionClass::BlockDataTransfer
                    | ArmInstructionClass::SingleDataSwap,
                ..
            } | DecodedInstruction::Thumb {
                class: ThumbInstructionClass::PcRelativeLoad
                    | ThumbInstructionClass::LoadStore
                    | ThumbInstructionClass::LoadStoreHalfword
                    | ThumbInstructionClass::SpRelativeLoad
                    | ThumbInstructionClass::PushPop
                    | ThumbInstructionClass::MultipleLoadStore,
            }
        );
        if !supported {
            return false;
        }
        match state.phase {
            CpuExecutionPhase::DataBus => state.transfer_register_mask != 0,
            CpuExecutionPhase::LoadInternal => {
                state.bus_operation == CpuBusOperation::Read
                    && state.transfer_register_mask != 0
                    && state.data_access_count != 0
            }
            CpuExecutionPhase::Writeback => {
                state.transfer_register_mask == 0 && state.data_access_count != 0
            }
            _ => false,
        }
    }
}
