use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Cell {
    pub address: u32,
    pub word: u16,
    pub upper: bool,
    pub lower: bool,
    pub code: u8,
}

impl Cell {
    fn zero() -> Self {
        Self {
            address: 0,
            word: 0,
            upper: false,
            lower: false,
            code: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Fifo {
    pub producer: [u8; 2],
    pub consumer: [u8; 2],
    pub cells: [Cell; 4],
    pub bus_word: u16,
    pub previous_address: u32,
    pub code_bits: u8,
    pub write: bool,
    pub low_delay: [u8; 2],
}

pub(super) struct FifoEvaluation {
    pub bus_load: Option<(u16, [bool; 2])>,
    pub drives: MemoryDrives,
    pub capture: Option<u8>,
    pub read_address: u32,
    pub dma_activity: bool,
}

pub(super) struct Transfer {
    pub capture: bool,
    pub produced: bool,
    pub external_capture: bool,
    pub new_write: bool,
    pub upper: bool,
    pub lower: bool,
    pub read_cram: bool,
    pub read_vsram: bool,
    pub read_kind_delay: bool,
}

impl Fifo {
    pub fn software_origin() -> Self {
        Self {
            producer: [0; 2],
            consumer: [0; 2],
            cells: [Cell::zero(); 4],
            bus_word: 0,
            previous_address: 0,
            code_bits: 0,
            write: false,
            low_delay: [0; 2],
        }
    }

    pub fn equal(self) -> bool {
        self.producer[1] == self.consumer[1] >> 1
    }

    pub fn evaluate(&mut self, context: Context, dma: &Dma, transfer: Transfer) -> FifoEvaluation {
        let Context {
            input,
            control,
            code,
            dma: view,
            old_selected,
            old_read,
            ..
        } = context;
        let Transfer {
            capture,
            produced,
            external_capture,
            new_write,
            upper,
            lower,
            read_cram,
            read_vsram,
            read_kind_delay,
        } = transfer;
        let phases = input.phases;
        let old_producer_master = self.producer[0];
        let old_consumer = self.consumer[1];
        let old_cell = self.cells[usize::from(old_consumer >> 1)];
        let byte_split = old_cell.upper ^ old_cell.lower;
        let old_registers = control.registers.before().logical();
        let mode128 = old_registers[1] & 0x84 == 0x84;
        // This selector precedes the current cell's combinational code replacement.
        let ordinary = !mode128 && self.code_bits & 6 == 0;
        let second = ordinary && old_selected && old_consumer & 1 != 0;
        let copy_read = old_read && view.copy;
        let read = old_read && !view.copy;
        let old_write = self.write && self.code_bits & 14 == 0;
        let address_before = input.memory.address;
        let mut address = address_before;
        if view.copy && new_write {
            let source = control.registers.before().dma();
            address = u32::from(source.source_low()) | (u32::from(source.source_high() & 1) << 16);
        }
        if control.address.memory_cycle {
            address = control_address(control);
        }
        let read_address = address;
        let driven_upper = view.active || upper;
        let driven_lower = view.active || lower;
        let fill = dma.fill_selected[1];
        let next_code = if fill { code & 14 } else { old_cell.code & 14 };
        let next_upper = if fill { driven_upper } else { old_cell.upper };
        let next_lower = if fill { driven_lower } else { old_cell.lower };
        let upper_byte = copy_read && (mode128 || address & 1 != 0) || old_write && next_upper;
        let lower_byte = copy_read && (mode128 || address & 1 == 0) || old_write && next_lower;
        let lower_write = copy_read || old_write && (next_lower || !mode128);
        let upper_write = copy_read || old_write && mode128 && next_upper;
        if phases.data_master {
            self.producer[0] = (self.producer[1] + u8::from(external_capture || produced)) & 3;
            if input.reset_comb {
                self.producer[0] = 0;
            }
        }
        if phases.data_slave {
            self.producer[1] = self.producer[0];
        }
        let capture_slot = self.producer[1];
        if phases.horizontal_master {
            let step = if ordinary && !byte_split { 1 } else { 2 };
            self.consumer[0] = (old_consumer + if dma.selected[1] { step } else { 0 }) & 7;
            if input.reset_comb {
                self.consumer[0] = 0;
            }
        }
        if phases.horizontal_slave {
            self.consumer[1] = self.consumer[0];
        }
        if capture || external_capture && phases.data_master {
            let cell = &mut self.cells[usize::from(capture_slot)];
            cell.upper = driven_upper;
            cell.lower = driven_lower;
            cell.code = code & 15;
        }
        let capture = capture || external_capture && phases.data_master;
        let first_byte = old_selected && old_consumer & 1 == 0;
        if phases.horizontal_master
            && (control.address.memory_cycle
                || dma.selected[1] && self.consumer[1] & 1 == 0 && !read)
        {
            self.previous_address = address;
        }
        if read || second {
            address = self.previous_address ^ 1;
        }
        if capture {
            self.cells[usize::from(capture_slot)].address = control_address(control);
        }
        if first_byte {
            address = self.cells[usize::from(old_consumer >> 1)].address;
        }
        let mut bus_load = None;
        if external_capture || control.intake.data_write {
            self.bus_word = input.main.data_bus;
            bus_load = Some((self.bus_word, [control.intake.data_write, external_capture]));
        }
        if capture {
            self.cells[usize::from(capture_slot)].word = self.bus_word;
        }
        let data_window = input.memory.data_window && !(view.copy && dma.empty[1]);
        let select_upper = if ordinary && byte_split {
            old_cell.upper
        } else {
            self.consumer[1] & 1 != 0
        };
        let mut data = input.memory.data;
        let mut low = 0;
        if data_window {
            let word = self.cells[usize::from(self.consumer[1] >> 1)].word;
            let byte = if select_upper { word >> 8 } else { word & 255 };
            data = (word & 0xff00) | byte;
            low = word as u8;
        }
        if phases.horizontal_master {
            self.low_delay[0] = low;
        }
        if phases.horizontal_slave {
            self.low_delay[1] = self.low_delay[0];
        }
        let write_code = if fill {
            code & 1 != 0
        } else {
            old_cell.code & 1 != 0 || control.registers.after().logical()[1] & 4 == 0
        };
        self.write = write_code && (fill || dma.selected[1]);
        self.code_bits = next_code;
        FifoEvaluation {
            bus_load,
            drives: MemoryDrives {
                address,
                data,
                upper_byte,
                lower_byte,
                upper_write,
                lower_write,
                cram_upper: self.write && next_code == 2 && next_upper,
                cram_lower: self.write && next_code == 2 && next_lower,
                vsram_upper: self.write && next_code == 4 && next_upper,
                vsram_lower: self.write && next_code == 4 && next_lower,
                read_cram,
                read_vsram,
                read_kind_delay,
                fifo_write: self.write,
                code_bits: next_code,
                fill: view.fill && dma.empty[1],
                copy: view.copy && dma.empty[1],
                data_low_delay: self.low_delay[1],
            },
            capture: capture.then_some(capture_slot),
            read_address,
            dma_activity: external_capture
                && phases.data_master
                && self.producer[0] != old_producer_master,
        }
    }
}

fn control_address(control: ControlEvaluation) -> u32 {
    control.register_load.source_address
}
