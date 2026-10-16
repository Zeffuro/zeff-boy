use super::*;
pub(super) fn check(case: Case, detector: Option<(Failure, u32)>) -> bool {
    let (mut cpu, mut bus) = case.seed();
    bus.detection = detector;
    let initial = bus.words.clone();
    let plan = oracle::plan(case, &initial);
    let at = case.form.extensions();
    let t = plan[at];
    let prefix = oracle::clocks(case, &bus.elapsed, at, true);
    let clocks = prefix + u64::from(detector.map_or(0, |(_, e)| e));
    let mut events = oracle::events(case, &plan[..at], &bus.elapsed, true);
    if detector.is_some() {
        events.push(Event::Detect(
            t.access,
            Direction::Read,
            case.start + prefix,
        ));
    }
    let result = if detector.is_none() {
        cpu.step_physical_instruction(&mut DefaultDetection(&mut bus))
    } else {
        cpu.step_physical_instruction(&mut bus)
    };
    let mut good = t.access.logical_address & 1 != 0
        && t.registers.status_register() == case.r.status_register()
        && t.registers.d == case.r.d
        && t.ird == case.opcode()
        && bus.count == at;
    let mut end = case.start + clocks;
    let mut memory = initial;
    if detector.is_some_and(|(f, _)| f == Failure::Boundary) {
        let p = entry::predict(case, t, 3, end, memory, (None, None));
        end = p.end;
        memory = p.words;
        events.extend(p.events);
        good &= result == p.result
            && cpu.registers == p.registers
            && cpu.queued_words() == p.queue
            && bus.entry_count == p.count
            && cpu.physical_entry_state() == p.state
            && cpu.physical_inspection() == p.inspection;
        if p.queue.is_none() {
            good &= run::frozen(&mut cpu, &mut bus, TimingFault::DoubleFaultHalted);
        }
    } else {
        let cause = CriticalFault::AddressError {
            access: t.access,
            direction: Direction::Read,
        };
        let host = detector.is_some_and(|(f, _)| f == Failure::Host);
        let fault = if host {
            InstructionFault::Entry(EntryFault::HostFailure {
                access: t.access,
                direction: Direction::Read,
                source: "host",
            })
        } else {
            InstructionFault::TransitionTimingUnavailable {
                phase: t.phase,
                cause,
            }
        };
        good &= result == Err(fault)
            && cpu.registers == t.registers
            && cpu.queued_words().is_none()
            && bus.entry_count == 0;
        if !host {
            good &= cpu.physical_entry_state() == PhysicalEntryState::FaultUnqualified { cause }
                && cpu.physical_inspection() == PhysicalInspection::Unavailable;
        }
        good &= run::frozen(
            &mut cpu,
            &mut bus,
            if host {
                TimingFault::Frozen
            } else {
                TimingFault::PhysicalFaultUnqualified
            },
        );
    }
    good && bus.words == memory && bus.events == events && cpu.clock().0 == end && bus.clock == end
}
