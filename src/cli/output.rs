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
    pub(super) a: u8,
    pub(super) f: u8,
    pub(super) zf: u8,
    pub(super) nf: u8,
    pub(super) hf: u8,
    pub(super) cf: u8,
    pub(super) mode: &'a str,
    pub(super) op_extra: &'a str,
}

pub(super) fn format_op_line(traced: u64, ctx: &TraceContext<'_>) -> String {
    format!(
        "[op] n={} pc={:04X} op={:02X} cb={} step_t={} total_t={} ime={} if={:02X} ie={:02X} pend={:02X} div={:02X} tima={:02X} tac={:02X} a={:02X} f={:02X} znhc={}{}{}{} mode={}{}",
        traced,
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
        ctx.a,
        ctx.f,
        ctx.zf,
        ctx.nf,
        ctx.hf,
        ctx.cf,
        ctx.mode,
        ctx.op_extra
    )
}

pub(super) fn format_op_tail_line(ctx: &TraceContext<'_>) -> String {
    format!(
        "[op-tail] pc={:04X} op={:02X} cb={} step_t={} total_t={} ime={} if={:02X} ie={:02X} pend={:02X} div={:02X} tima={:02X} tac={:02X} a={:02X} f={:02X} znhc={}{}{}{} mode={}{}",
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
        ctx.a,
        ctx.f,
        ctx.zf,
        ctx.nf,
        ctx.hf,
        ctx.cf,
        ctx.mode,
        ctx.op_extra
    )
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
