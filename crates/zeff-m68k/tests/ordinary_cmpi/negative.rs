use super::*;
use model::{Spec, bank, encoded, pairs};
use oracle::Count;

fn paths(total: &mut Count, s: Spec) {
    let (s, bus) = s.seed();
    let p = plan::validate(s, &bus);
    for path in 0..3 {
        let mut b = bus.clone();
        if path == 1 {
            b.fail = Some(p.final_read);
        } else if path == 2 {
            capacity::blocked(&p, &mut b, p.final_read);
        }
        total.case(s, b);
    }
}
pub(super) fn outside() -> (u32, u32) {
    let mut total = Count::default();
    for supervisor in [false, true] {
        for form in encoded().into_iter().filter(|s| s.size == 2) {
            for lane in 0..2 {
                let mut s = bank(form, supervisor, 31);
                s.size = 1;
                (s.operand, s.immediate) = pairs(1)[3];
                let target = model::address(s, s.extension) & !1 | lane;
                s.target(target);
                paths(&mut total, s);
            }
        }
        for size in [1, 2, 4] {
            for reg in 0..8 {
                let mut s = Spec::new(size, 0, reg, supervisor);
                (s.operand, s.immediate) = pairs(size)[4];
                paths(&mut total, s);
            }
        }
    }
    total.result()
}
pub(super) fn families() -> (u32, u32) {
    let mut total = Count::default();
    for family in [0x0000, 0x0200, 0x0400, 0x0600, 0x0a00] {
        for supervisor in [false, true] {
            for size in [1, 2, 4] {
                for reg in 0..8 {
                    let mut s = Spec::new(size, 0, reg, supervisor);
                    s.family = family;
                    (s.operand, s.immediate) = pairs(size)[3];
                    paths(&mut total, s);
                }
                for form in encoded().into_iter().filter(|s| s.size == 2) {
                    for lane in 0..if size == 1 { 2 } else { 1 } {
                        let mut s = bank(form, supervisor, 31);
                        s.family = family;
                        s.size = size;
                        (s.operand, s.immediate) = pairs(size)[3];
                        if size == 1 {
                            let target = model::address(s, s.extension) & !1 | lane;
                            s.target(target);
                        }
                        paths(&mut total, s);
                    }
                }
            }
        }
    }
    total.result()
}
pub(super) fn illegal() -> (u32, u32) {
    let mut total = Count::default();
    for opcode in 0x0c00..=0x0cff {
        let mode = (opcode >> 3) & 7;
        let reg = opcode & 7;
        let width = (opcode >> 6) & 3;
        if width != 3 && mode != 1 && (mode != 7 || reg <= 1) {
            continue;
        }
        for supervisor in [false, true] {
            let r = fixture::registers(supervisor, 0x3000);
            let queue = [opcode, 0x8000];
            let mut cpu = PrefetchCpu::from_prefetched(r, queue, CpuClocks(7)).unwrap();
            let saved = cpu.clone();
            let mut bus = device::Device::new(7);
            let original = bus.clone();
            let result = cpu.step_instruction(&mut bus);
            let good = result == Err(TimingFault::UnsupportedInstruction { pc: r.pc, opcode })
                && cpu == saved
                && bus == original;
            if !good && total.bad < 3 {
                #[cfg(not(target_arch = "wasm32"))]
                eprintln!(
                    "ordinary illegal CMPI {opcode:04x} result={result:?} cpu={cpu:?}/{saved:?} device={bus:?}/{original:?}"
                );
            }
            total.count += 1;
            total.bad += u32::from(!good);
        }
    }
    total.result()
}
