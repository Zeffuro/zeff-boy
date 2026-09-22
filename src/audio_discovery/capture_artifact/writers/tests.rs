use super::*;

fn event(cycle: u64, pc: u32, offset: u64) -> AudioTraceEvent {
    AudioTraceEvent {
        cycle,
        pc,
        instruction_source: AudioTraceSource::CartridgeRom {
            offset,
            bit_reversed: false,
        },
        write: AudioTraceWrite::Sn76489 {
            port: 0x7f,
            value: 0x90,
        },
    }
}

#[test]
fn ranking_separates_startup_window_banks_and_unknown_writers() -> Result<()> {
    let (mut trace, _) = crate::audio_discovery::trace_capture::tests::sega_trace("sms");
    trace.cycle_hz = 10;
    trace.cycle_hz_denominator = 3;
    trace.end_cycle = 10;
    trace.events = vec![event(0, 1, 10); 50];
    trace.events.extend([
        event(3, 2, 20),
        event(4, 2, 20),
        event(5, 2, 20),
        event(6, 2, 30),
    ]);
    let mut unknown = event(7, 2, 20);
    unknown.instruction_source = AudioTraceSource::Unknown;
    trace.events.push(unknown);
    let result = collect(&trace, sn, &AtomicBool::new(false))?;
    assert_eq!(result["first_second_boundary_cycle"], 4);
    assert_eq!(result["write_events"], 55);
    assert_eq!(result["unattributed_write_events"], 1);
    assert_eq!(result["instruction_write_events"], 54);
    assert_eq!(result["after_first_second_instruction_write_events"], 3);
    let rows = result["reported_sites"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["pc"], 2);
    assert_eq!(rows[0]["instruction_source"]["cartridge_rom"]["offset"], 20);
    assert_eq!(rows[0]["write_events"], 3);
    assert_eq!(rows[0]["after_first_second_write_events"], 2);
    assert_eq!(rows[1]["instruction_source"]["cartridge_rom"]["offset"], 30);
    assert_eq!(rows[2]["write_events"], 50);
    Ok(())
}

#[test]
fn bookkeeping_reads_fetches_and_blocked_wave_writes_are_excluded() {
    assert!(matches!(
        gb(&GameBoyTraceWrite::NativeBatch {
            cycles: 4,
            repetitions: 200
        }),
        Classification::Excluded("gb_native_batch")
    ));
    assert!(matches!(
        gb(&GameBoyTraceWrite::PcmDrain { frames: 900 }),
        Classification::Excluded("gb_pcm_drain")
    ));
    assert!(matches!(
        gb(&GameBoyTraceWrite::WaveRam {
            address: 0xff30,
            value: 10,
            applied_index: None,
            origin: GameBoyTraceOrigin::Cpu
        }),
        Classification::Excluded("gb_blocked_wave_ram")
    ));
    assert!(matches!(
        nes(&NesTraceWrite::DmcFetch {
            address: 0xc000,
            value: 1,
            source: AudioTraceSource::Unknown
        }),
        Classification::Excluded("nes_dmc_fetch")
    ));
    assert!(matches!(
        nes(&NesTraceWrite::StatusRead {
            value: 0,
            origin: NesTraceOrigin::Cpu
        }),
        Classification::Excluded("nes_status_read")
    ));
    for origin in [
        WonderSwanTraceOrigin::GeneralDma,
        WonderSwanTraceOrigin::SoundDma,
        WonderSwanTraceOrigin::CpuInterrupt,
    ] {
        assert!(matches!(
            ws(&WonderSwanTraceWrite::Register {
                port: 0x89,
                value: 1,
                origin
            }),
            Classification::Write(Write {
                origin: Origin::CpuInterrupt | Origin::GeneralDma | Origin::SoundDma,
                ..
            })
        ));
    }
    assert!(matches!(
        gb(&GameBoyTraceWrite::Register {
            address: 0xff12,
            value: 0xf0,
            origin: GameBoyTraceOrigin::CpuInterrupt
        }),
        Classification::Write(Write {
            origin: Origin::CpuInterrupt,
            ..
        })
    ));
}

#[test]
fn non_instruction_origins_and_unknown_sources_remain_distinct() -> Result<()> {
    let source = AudioTraceSource::CartridgeRom {
        offset: 10,
        bit_reversed: false,
    };
    let events = [
        (WonderSwanTraceOrigin::Cpu, source),
        (
            WonderSwanTraceOrigin::CpuInterrupt,
            AudioTraceSource::Unknown,
        ),
        (WonderSwanTraceOrigin::GeneralDma, source),
        (WonderSwanTraceOrigin::SoundDma, AudioTraceSource::Unknown),
        (WonderSwanTraceOrigin::Cpu, AudioTraceSource::Unknown),
        (WonderSwanTraceOrigin::Cpu, AudioTraceSource::Unmapped),
    ]
    .into_iter()
    .map(|(origin, instruction_source)| AudioTraceEvent {
        cycle: 20,
        pc: 10,
        instruction_source,
        write: WonderSwanTraceWrite::Register {
            port: 0x89,
            value: 1,
            origin,
        },
    })
    .collect();
    let trace = ChipAudioTrace {
        generation: 0,
        cycle_hz: 10,
        cycle_hz_denominator: 1,
        chip: (),
        timing: AudioTraceTiming::BusServiceBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: 30,
        events,
        dropped_events: 0,
        invalidated: None,
    };
    let result = collect(&trace, ws, &AtomicBool::new(false))?;
    assert_eq!(result["instruction_write_events"], 1);
    assert_eq!(result["unattributed_write_events"], 5);
    let unranked = result["unranked_attribution"].as_array().unwrap();
    assert_eq!(unranked.len(), 5);
    assert!(
        unranked
            .iter()
            .all(|row| row["write_events"] == 1 && row["after_first_second_write_events"] == 1)
    );
    assert!(
        unranked
            .iter()
            .any(|row| row["origin"] == "general_dma" && row["source_kind"] == "cartridge_rom")
    );
    assert!(
        unranked
            .iter()
            .any(|row| row["origin"] == "instruction" && row["source_kind"] == "unmapped")
    );
    assert!(
        unranked
            .iter()
            .any(|row| row["origin"] == "instruction" && row["source_kind"] == "unknown")
    );
    Ok(())
}

#[test]
fn site_and_destination_limits_retain_explicit_omitted_counts() -> Result<()> {
    let (mut trace, _) = crate::audio_discovery::trace_capture::tests::sega_trace("sms");
    trace.events = (0..MAX_SITES + 1)
        .map(|index| event(index as u64, index as u32, index as u64))
        .collect();
    for port in 0x40..=0x7f {
        let mut next = event(2000, 0, 0);
        next.write = AudioTraceWrite::Sn76489 { port, value: 0x90 };
        trace.events.push(next);
    }
    let result = collect(&trace, sn, &AtomicBool::new(false))?;
    assert_eq!(result["site_inventory_truncated"], true);
    assert_eq!(result["tracked_sites"], MAX_SITES);
    assert_eq!(result["untracked_site_write_events"], 1);
    assert_eq!(
        result["reported_sites"].as_array().unwrap().len(),
        MAX_REPORTED_SITES
    );
    assert_eq!(
        result["reported_sites"][0]["destinations"]
            .as_array()
            .unwrap()
            .len(),
        MAX_DESTINATIONS
    );
    assert_eq!(result["reported_sites"][0]["destinations_truncated"], true);
    assert_eq!(
        result["reported_write_events"].as_u64().unwrap()
            + result["unreported_tracked_site_write_events"]
                .as_u64()
                .unwrap()
            + result["untracked_site_write_events"].as_u64().unwrap(),
        trace.events.len() as u64
    );
    assert!(collect(&trace, sn, &AtomicBool::new(true)).is_err());
    Ok(())
}

#[test]
fn artifact_api_admits_native_contract_and_binds_hashes() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let (trace, context) = crate::audio_discovery::trace_capture::tests::sega_trace("gg");
    let path = directory.path().join("capture.zip");
    crate::audio_discovery::trace_capture::write_new(
        &path,
        &trace,
        context,
        &AtomicBool::new(false),
    )?;
    let artifact = super::super::CaptureArtifact::load(&path)?;
    let evidence = artifact.writer_evidence(Default::default(), &AtomicBool::new(false))?;
    assert_eq!(evidence["archive_sha256"], artifact.archive_sha256);
    assert_eq!(evidence["trace_sha256"], artifact.trace_sha256);
    assert_eq!(evidence["total_events"], trace.events.len());
    assert_eq!(evidence["write_events"], trace.events.len());
    assert!(
        artifact
            .writer_evidence(Default::default(), &AtomicBool::new(true))
            .is_err()
    );
    Ok(())
}
