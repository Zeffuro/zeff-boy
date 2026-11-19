use crate::hardware::sound::observer::{self, Metric, Report, Session};
use crate::{ControllerKind, ControllerPort, Emulator, LoadConfig, Region};
use anyhow::{Context, Result, anyhow, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::time::MachineTiming;

pub const METRIC_NAMES: [&str; 24] = [
    "OwnersCreated",
    "OwnersClosed",
    "GenerationsCreated",
    "Snapshots",
    "DeepCopies",
    "MutBorrows",
    "SharedDetaches",
    "WeakDissociations",
    "CopyBytes",
    "ImagesCreated",
    "ImagesClosed",
    "ImageClones",
    "Attempts",
    "Successes",
    "Failures",
    "Publications",
    "RamReads",
    "RamWrites",
    "OtherReads",
    "OtherWrites",
    "ZeroWrites",
    "OneWrite",
    "TwoWrites",
    "ManyWrites",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Census {
    pub scope: &'static str,
    pub full_coverage: bool,
    pub flags: u32,
    pub closed: bool,
    pub final_live_owners: usize,
    pub final_live_images: usize,
    pub fields: u64,
    pub region: Region,
    pub controller: ControllerKind,
    pub ticks: u64,
    pub presented_frames: u64,
    pub pc: u32,
    pub dimensions: (usize, usize),
    pub samples: usize,
    pub nonzero_samples: usize,
    pub sampled_terminal_owners: usize,
    pub sampled_terminal_images: usize,
    pub counters: [u64; 24],
    pub raw_sha256: [u8; 32],
    pub frontend_sha256: [u8; 32],
    pub rgba_sha256: [u8; 32],
    pub pcm_sha256: [u8; 32],
}

fn execute(
    rom: Vec<u8>,
    restore: Option<&[u8]>,
    region: Region,
    controller: ControllerKind,
    fields: u64,
) -> Result<(Emulator, Vec<u32>)> {
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .map_err(|error| anyhow!("load: {error:?}"))?;
    for port in [ControllerPort::One, ControllerPort::Two] {
        machine.input_mut().set_controller_kind(port, controller);
    }
    if let Some(state) = restore {
        machine.decode_state(state).context("restore raw state")?;
    }
    ensure!(machine.sample_rate() == 48_000, "expected 48 kHz audio");
    let mut pcm = Vec::new();
    let mut field_pcm = Vec::new();
    for field in 1..=fields {
        for port in [ControllerPort::One, ControllerPort::Two] {
            machine.input_mut().set_controller_raw(port, 0xff);
            machine.input_mut().set_controller_extra_raw(port, 0x0f);
        }
        machine
            .step_field()
            .map_err(|error| anyhow!("field {field}: {error:?}"))?;
        ensure!(
            machine.video_error().is_none(),
            "field {field}: video error"
        );
        field_pcm.clear();
        machine.drain_audio_samples_into(&mut field_pcm);
        pcm.extend(field_pcm.iter().copied().map(f32::to_bits));
    }
    Ok((machine, pcm))
}

struct Observation {
    result: Result<Vec<u32>>,
    report: Report,
    terminal: Option<Report>,
}

fn observe(
    control: &Emulator,
    expected_pcm: &[u32],
    execute: impl FnOnce() -> Result<(Emulator, Vec<u32>)>,
) -> Observation {
    let session = Session::begin();
    let mut terminal = None;
    let result = (|| {
        let initial = session.report();
        ensure!(
            initial.closed() && initial.counters == [0; 24],
            "observer unavailable: {initial:?}"
        );
        let (measured, actual_pcm) = execute()?;
        ensure!(
            measured == *control,
            "observer changed terminal machine state"
        );
        ensure!(actual_pcm == expected_pcm, "observer changed PCM bits");
        terminal = Some(session.report());
        drop(measured);
        Ok(actual_pcm)
    })();
    Observation {
        result,
        report: session.finish(),
        terminal,
    }
}

pub fn run(
    rom: Vec<u8>,
    restore: Option<&[u8]>,
    region: Region,
    controller: ControllerKind,
    fields: u64,
) -> Result<Census> {
    ensure!((1..=120).contains(&fields), "fields must be in 1..=120");
    ensure!(observer::idle(), "observer already active or unavailable");
    let (control, expected_pcm) =
        execute(rom.clone(), restore, region, controller, fields).context("unarmed reference")?;
    let Observation {
        result,
        report,
        terminal,
    } = observe(&control, &expected_pcm, || {
        execute(rom, restore, region, controller, fields)
    });
    let actual_pcm = result.with_context(|| format!("measured execution: {report:?}"))?;
    let sampled = terminal.expect("successful observation captured terminal owners");
    ensure!(report.closed(), "incomplete RAM observation: {report:?}");
    ensure!(
        sampled.live_owners > 0 && report.count(Metric::OwnersCreated) > 0,
        "unobserved root RAM"
    );
    ensure!(
        report.count(Metric::OwnersCreated) == report.count(Metric::OwnersClosed)
            && report.count(Metric::ImagesCreated) == report.count(Metric::ImagesClosed)
            && report.count(Metric::Attempts)
                == report.count(Metric::Successes) + report.count(Metric::Failures)
            && report.count(Metric::ZeroWrites)
                + report.count(Metric::OneWrite)
                + report.count(Metric::TwoWrites)
                + report.count(Metric::ManyWrites)
                == report.count(Metric::Successes),
        "unbalanced RAM observation: {report:?}"
    );
    let mut pcm_hash = Sha256::new();
    for value in &actual_pcm {
        pcm_hash.update(value.to_le_bytes());
    }
    Ok(Census {
        scope: report.scope,
        full_coverage: report.full_coverage,
        flags: report.flags,
        closed: report.closed(),
        final_live_owners: report.live_owners,
        final_live_images: report.live_images,
        fields,
        region,
        controller,
        ticks: control.timing_snapshot().now().get(),
        presented_frames: control.frame_count(),
        pc: control.registers().pc,
        dimensions: control.framebuffer_dimensions(),
        samples: actual_pcm.len(),
        nonzero_samples: actual_pcm
            .iter()
            .filter(|value| *value & 0x7fff_ffff != 0)
            .count(),
        sampled_terminal_owners: sampled.live_owners,
        sampled_terminal_images: sampled.live_images,
        counters: report.counters,
        raw_sha256: Sha256::digest(control.encode_state()?).into(),
        frontend_sha256: Sha256::digest(control.encode_frontend_state()?).into(),
        rgba_sha256: Sha256::digest(control.framebuffer()).into(),
        pcm_sha256: pcm_hash.finalize().into(),
    })
}

#[cfg(test)]
mod tests;
