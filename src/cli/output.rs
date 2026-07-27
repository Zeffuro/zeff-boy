pub(super) struct TraceContext<'a> {
    pub(super) pc: u16,
    pub(super) op: u8,
    pub(super) cb_prefix: bool,
    pub(super) step_cycles: u64,
    pub(super) total_t: u64,
    pub(super) ime: &'a str,
    pub(super) if_reg: u8,
    pub(super) ie: u8,
    pub(super) pending: u8,
    pub(super) div: u8,
    pub(super) tima: u8,
    pub(super) tac: u8,
    pub(super) ppu_cycles: u64,
    pub(super) ppu_lcdc: u8,
    pub(super) ppu_stat: u8,
    pub(super) ppu_ly: u8,
    pub(super) ppu_lyc: u8,
    pub(super) a: u8,
    pub(super) f: u8,
    pub(super) b: u8,
    pub(super) c: u8,
    pub(super) d: u8,
    pub(super) e: u8,
    pub(super) h: u8,
    pub(super) l: u8,
    pub(super) zf: u8,
    pub(super) nf: u8,
    pub(super) hf: u8,
    pub(super) cf: u8,
    pub(super) mode: &'a str,
    pub(super) op_extra: &'a str,
}

use std::fmt::Write;

pub(super) fn format_op_line(traced: u64, ctx: &TraceContext<'_>) -> String {
    let mut s = format!("[op] n={}", traced);
    write_op_fields(&mut s, ctx);
    s
}

pub(super) fn format_op_tail_line(ctx: &TraceContext<'_>) -> String {
    let mut s = String::from("[op-tail]");
    write_op_fields(&mut s, ctx);
    s
}

fn write_op_fields(out: &mut String, ctx: &TraceContext<'_>) {
    let _ = write!(
        out,
        " pc={:04X} op={:02X} cb={} step_t={} total_t={} ime={} if={:02X} ie={:02X} pend={:02X} div={:02X} tima={:02X} tac={:02X} ppu_dot={} lcdc={:02X} stat={:02X} ly={:02X} lyc={:02X} a={:02X} f={:02X} b={:02X} c={:02X} d={:02X} e={:02X} h={:02X} l={:02X} znhc={}{}{}{} mode={}{}",
        ctx.pc,
        ctx.op,
        if ctx.cb_prefix { 1 } else { 0 },
        ctx.step_cycles,
        ctx.total_t,
        ctx.ime,
        ctx.if_reg,
        ctx.ie,
        ctx.pending,
        ctx.div,
        ctx.tima,
        ctx.tac,
        ctx.ppu_cycles,
        ctx.ppu_lcdc,
        ctx.ppu_stat,
        ctx.ppu_ly,
        ctx.ppu_lyc,
        ctx.a,
        ctx.f,
        ctx.b,
        ctx.c,
        ctx.d,
        ctx.e,
        ctx.h,
        ctx.l,
        ctx.zf,
        ctx.nf,
        ctx.hf,
        ctx.cf,
        ctx.mode,
        ctx.op_extra
    );
}

pub(super) fn format_headless_summary(
    frames: u64,
    cycles: u64,
    pc: u16,
    serial_bytes: usize,
) -> String {
    format!(
        "[headless] frames={} cycles={} pc={:04X} serial_bytes={}",
        frames, cycles, pc, serial_bytes
    )
}

pub(super) fn format_headless_serial(serial_text: &str) -> String {
    format!("[headless] serial: {}", serial_text)
}

pub(super) fn format_headless_breakpoint(pc: u16, cycles: u64, a: u8, f: u8, sp: u16) -> String {
    format!(
        "[headless] breakpoint-hit pc={:04X} cycles={} a={:02X} f={:02X} sp={:04X}",
        pc, cycles, a, f, sp
    )
}
