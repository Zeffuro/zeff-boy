use super::device::Device;
use super::model::Failure;
use super::{
    CpuClocks, EntryFault, InstructionFault, PrefetchCpu, TimingFault, check, classifier, oracle,
    seed,
};

pub(super) fn run(mark: fn(bool)) -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    let mut totals = [0u32; 3];
    for word in 0..=u16::MAX {
        let mut c = seed::make(0x4200, false, 0);
        c.next = word;
        c.retained = word;
        let mut good = check::run(c, |b| b.fault(0, Failure::Boundary, 8), mark);
        totals[0] += u32::from(classifier::known(word));
        for bank in [false, true] {
            let mut c = seed::make(0x4200, bank, 0);
            c.word = word;
            c.retained = 0x4e71;
            c.start = u64::MAX;
            let mut b = Device::seed(c);
            let mut e = oracle::initial(c, &b);
            let selected = classifier::selected(word, bank);
            totals[1 + usize::from(bank)] += u32::from(selected);
            let error = if selected {
                e.overflow();
                InstructionFault::Entry(EntryFault::ClockOverflow)
            } else {
                InstructionFault::Execution(TimingFault::UnsupportedInstruction {
                    pc: c.r.pc,
                    opcode: word,
                })
            };
            mark(true);
            let mut cpu =
                PrefetchCpu::from_prefetched(c.r, [word, 0x4e71], CpuClocks(c.start)).unwrap();
            let actual = check::step(&mut cpu, &mut b);
            mark(false);
            good &= actual == Err(error.clone()) && check::verify(&cpu, &b, &e);
            good &= if selected {
                check::guards(&mut cpu, &mut b, TimingFault::Frozen, mark)
            } else {
                super::preflight::attempt(cpu, b, error, mark)
            };
        }
        count += 1;
        bad += u32::from(!good);
    }
    bad += u32::from(totals != [26599, 19634, 19687]);
    (count, bad)
}
