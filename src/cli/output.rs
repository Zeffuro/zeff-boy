#[allow(clippy::too_many_arguments)]
pub(super) fn format_op_line(
    traced: u64,
    pc: u16,
    op: u8,
    cb_prefix: bool,
    step_cycles: u64,
    total_t: u64,
    ime: &str,
    if_reg: u8,
    ie: u8,
    pending: u8,
    div: u8,
    tima: u8,
    tac: u8,
    a: u8,
    f: u8,
    zf: u8,
    nf: u8,
    hf: u8,
    cf: u8,
    mode: &str,
    op_extra: &str,
) -> String {
    format!(
        "[op] n={} pc={:04X} op={:02X} cb={} step_t={} total_t={} ime={} if={:02X} ie={:02X} pend={:02X} div={:02X} tima={:02X} tac={:02X} a={:02X} f={:02X} znhc={}{}{}{} mode={}{}",
        traced,
        pc,
        op,
        if cb_prefix { 1 } else { 0 },
        step_cycles,
        total_t,
        ime,
        if_reg,
        ie,
        pending,
        div,
        tima,
        tac,
        a,
        f,
        zf,
        nf,
        hf,
        cf,
        mode,
        op_extra
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn format_op_tail_line(
    pc: u16,
    op: u8,
    cb_prefix: bool,
    step_cycles: u64,
    total_t: u64,
    ime: &str,
    if_reg: u8,
    ie: u8,
    pending: u8,
    div: u8,
    tima: u8,
    tac: u8,
    a: u8,
    f: u8,
    zf: u8,
    nf: u8,
    hf: u8,
    cf: u8,
    mode: &str,
    op_extra: &str,
) -> String {
    format!(
        "[op-tail] pc={:04X} op={:02X} cb={} step_t={} total_t={} ime={} if={:02X} ie={:02X} pend={:02X} div={:02X} tima={:02X} tac={:02X} a={:02X} f={:02X} znhc={}{}{}{} mode={}{}",
        pc,
        op,
        if cb_prefix { 1 } else { 0 },
        step_cycles,
        total_t,
        ime,
        if_reg,
        ie,
        pending,
        div,
        tima,
        tac,
        a,
        f,
        zf,
        nf,
        hf,
        cf,
        mode,
        op_extra
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
