use super::*;

pub(super) fn expected_schedule(handler: u32, elapsed: &[u32], origin: u64) -> Vec<Event> {
    let mut events = vec![Event::Idle(origin, 14)];
    let mut clock = origin + 14;
    for (index, address) in [0, 2, 4, 6, handler, handler.wrapping_add(2)]
        .into_iter()
        .enumerate()
    {
        if index == 5 {
            events.push(Event::Idle(clock, 2));
            clock += 2;
        }
        events.push(Event::Read(
            access(
                address,
                FunctionCode::SupervisorProgram,
                if index < 4 {
                    AccessKind::ResetVector
                } else {
                    AccessKind::Prefetch
                },
            ),
            clock,
        ));
        clock += u64::from(elapsed[index]);
    }
    events
}

#[test]
fn warm_reset_uses_release_boundary_fresh_words_and_preserves_warm_registers() {
    for sr in [0x001f, TRACE | SUPERVISOR | 0x0415] {
        for sp in [0xdead_7000, 0xdead_7001] {
            for waits in [false, true] {
                let mut initial = registers();
                initial.set_status_register(sr);
                let mut cpu =
                    PrefetchCpu::from_prefetched(initial, [0x4e70, 0x4e71], CpuClocks(23)).unwrap();
                let mut bus = Device::new(121);
                let handler = 0xfffe_6000;
                bus.reset_vectors(sp, handler);
                if waits {
                    bus.elapsed = vec![5, 7, 11, 13, 17, 19];
                }
                let result = cpu
                    .external_warm_reset(&mut bus, reset_boundary(121))
                    .unwrap();
                let total = 16 + bus.elapsed[..6].iter().map(|&n| u64::from(n)).sum::<u64>();
                assert_eq!(result.clocks, total);
                assert_eq!(result.kind, EntryKind::ExternalWarmReset);
                assert_eq!(bus.events, expected_schedule(handler, &bus.elapsed, 121));
                assert_eq!(cpu.clock().0, 121 + total);
                assert_eq!(cpu.clock().0, bus.clock);
                let mut final_registers = initial;
                final_registers.ssp = sp;
                final_registers.pc = handler;
                final_registers.set_status_register(0x2700 | (sr & 31));
                assert_eq!(cpu.registers, final_registers);
                assert_eq!(cpu.queued_words(), Some([0x70a5, 0x4e71]));
                assert!(!cpu.stopped());
                assert!(!cpu.trace_pending());
                assert_eq!(bus.ownership_edges, 0);
                assert_eq!(cpu.registers.d[0], initial.d[0]);
                let step = cpu.step_instruction(&mut bus).unwrap();
                assert_eq!(step.opcode, 0x70a5);
                assert_eq!(cpu.registers.d[0], 0xffff_ffa5);
            }
        }
    }
}

#[test]
fn reset_clears_stop_and_pending_trace_without_inventing_level7_edge() {
    for stopped in [false, true] {
        let mut initial = registers();
        initial.set_status_register(SUPERVISOR | if stopped { 0 } else { TRACE });
        let words = if stopped {
            [0x4e72, 0x2700]
        } else {
            [0x4e71, 0x4e71]
        };
        let mut cpu = PrefetchCpu::from_prefetched(initial, words, CpuClocks(0)).unwrap();
        let mut bus = Device::new(0);
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(cpu.stopped(), stopped);
        assert_eq!(cpu.trace_pending(), !stopped);
        cpu.interrupt_input().set_level(7);
        let release = bus.clock + 31;
        bus.clock = release;
        bus.events.clear();
        bus.transfers = 0;
        bus.reset_vectors(0x9000, 0x8000);
        cpu.external_warm_reset(&mut bus, reset_boundary(release))
            .unwrap();
        assert_eq!(cpu.interrupt_input().level(), 7);
        assert!(!cpu.stopped());
        assert!(!cpu.trace_pending());
        assert_eq!(cpu.registers.status_register() & 0xff00, 0x2700);
        assert!(matches!(
            cpu.step_boundary(&mut bus),
            Ok(zeff_m68k::timing::TimedOutcome::Instruction(_))
        ));
        assert_eq!(cpu.interrupt_input().level(), 7);
    }
}

#[test]
fn only_qualified_external_reset_recovers_doublehalt_and_never_host_frozen() {
    for host in [false, true] {
        let initial = registers();
        let mut cpu = imported(initial, capture(), 0);
        let mut bus = Device::new(0);
        bus.inject = Some((
            0,
            if host {
                Injection::Host
            } else {
                Injection::Released
            },
        ));
        cpu.enter_imported_group0(&mut bus).unwrap_err();
        let before = bus.events.clone();
        let clock = cpu.clock();
        assert!(
            cpu.external_warm_reset(
                &mut bus,
                ExternalResetBoundary {
                    sampled_release: clock,
                    qualification: ResetQualification::Unqualified
                }
            )
            .is_err()
        );
        assert_eq!(bus.events, before);
        assert_eq!(cpu.clock(), clock);
        bus.inject = None;
        bus.transfers = 0;
        bus.reset_vectors(0x3001, 0x2000);
        let result = cpu.external_warm_reset(&mut bus, reset_boundary(clock.0));
        if host {
            assert_eq!(result, Err(EntryFault::HostFrozen));
            assert_eq!(bus.events, before);
            assert_eq!(cpu.registers, initial);
        } else {
            assert_eq!(result.unwrap().clocks, 40);
            assert_eq!(cpu.registers.ssp, 0x3001);
            assert_eq!(cpu.registers.pc, 0x2000);
            assert_eq!(
                cpu.physical_inspection(),
                PhysicalInspection::DiagnosticCompleteValues
            );
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::Critical(EntryKind::ExternalWarmReset)
            );
            cpu.step_instruction(&mut bus).unwrap();
            assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
        }
    }
}

#[test]
fn invalid_reset_boundaries_are_side_effect_free_and_leave_old_queue_intact() {
    let initial = registers();
    let mut cpu = PrefetchCpu::from_prefetched(initial, [0x4e71, 0x7201], CpuClocks(200)).unwrap();
    let mut bus = Device::new(200);
    for (boundary, expected) in [
        (
            ExternalResetBoundary {
                sampled_release: CpuClocks(200),
                qualification: ResetQualification::Unqualified,
            },
            EntryInputError::UnqualifiedReset,
        ),
        (reset_boundary(199), EntryInputError::NonmonotonicRelease),
    ] {
        assert_eq!(
            cpu.external_warm_reset(&mut bus, boundary),
            Err(EntryFault::Input(expected))
        );
        assert_eq!(cpu.registers, initial);
        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
        assert_eq!(cpu.clock(), CpuClocks(200));
        assert!(bus.events.is_empty());
    }
}
