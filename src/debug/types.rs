use std::collections::VecDeque;

pub(crate) struct DebugInfo {
    pub(crate) pc: u16,
    pub(crate) sp: u16,
    pub(crate) a: u8,
    pub(crate) f: u8,
    pub(crate) b: u8,
    pub(crate) c: u8,
    pub(crate) d: u8,
    pub(crate) e: u8,
    pub(crate) h: u8,
    pub(crate) l: u8,

    pub(crate) cycles: u64,
    pub(crate) ime: &'static str,
    pub(crate) cpu_state: &'static str,
    pub(crate) last_opcode: u8,
    pub(crate) last_opcode_pc: u16,

    pub(crate) fps: f64,

    pub(crate) ly: u8,
    pub(crate) lcdc: u8,
    pub(crate) stat: u8,

    pub(crate) div: u8,
    pub(crate) tima: u8,
    pub(crate) tma: u8,
    pub(crate) tac: u8,

    pub(crate) if_reg: u8,
    pub(crate) ie: u8,

    pub(crate) mem_around_pc: Vec<(u16, u8)>,

    pub(crate) recent_ops: Vec<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct PpuSnapshot {
    pub(crate) lcdc: u8,
    pub(crate) stat: u8,
    pub(crate) scy: u8,
    pub(crate) scx: u8,
    pub(crate) ly: u8,
    pub(crate) lyc: u8,
    pub(crate) wy: u8,
    pub(crate) wx: u8,
    pub(crate) bgp: u8,
    pub(crate) obp0: u8,
    pub(crate) obp1: u8,
}

#[derive(Default)]
pub(crate) struct DebugWindowState {
    pub(crate) show_tile_viewer: bool,
    pub(crate) show_tilemap_viewer: bool,
    pub(crate) show_oam_viewer: bool,
    pub(crate) show_palette_viewer: bool,
}

pub(crate) struct DebugViewerData {
    pub(crate) vram: Vec<u8>,
    pub(crate) oam: Vec<u8>,
    pub(crate) ppu: PpuSnapshot,
}

pub(crate) struct OpcodeLog {
    entries: VecDeque<String>,
    capacity: usize,
}

impl OpcodeLog {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub(crate) fn push(&mut self, pc: u16, opcode: u8, is_cb: bool) {
        let label = if is_cb {
            format!("{:04X}: CB {:02X}", pc, opcode)
        } else {
            format!("{:04X}: {:02X}", pc, opcode)
        };
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(label);
    }

    pub(crate) fn recent(&self, n: usize) -> Vec<String> {
        self.entries.iter().rev().take(n).cloned().collect()
    }
}

