use super::super::{App, EmuBackend, Options, identity, input};
use super::ledger::Ledger;
use anyhow::{Context, Result, ensure};

pub(super) fn verify(
    app: &App,
    options: &Options,
    reference: &mut EmuBackend,
    config: [u8; 32],
    ledger: &Ledger,
) -> Result<()> {
    ledger.check_complete()?;
    for (frame, &raw) in ledger.inputs().iter().enumerate() {
        ensure!(
            raw == u16::from(input(frame as u64, options.role)),
            "cadence submitted input differs at {frame}"
        );
    }
    for (frame, record) in ledger.records().iter().enumerate() {
        let frame = frame as u64;
        let delay = options.input_delay.frames();
        let ports = if frame < delay {
            [0, 0]
        } else {
            [input(frame - delay, 0), input(frame - delay, 1)]
        };
        let EmuBackend::Nes(nes) = reference else {
            anyhow::bail!("lost cadence NES reference")
        };
        nes.emu.set_input_p1_raw(ports[0]);
        nes.emu.set_input_p2_raw(ports[1]);
        reference.step_frame();
        let mut audio = Vec::new();
        reference.drain_audio_samples_into(&mut audio);
        let expected = identity::checkpoint(reference, frame + 1, &audio, config)?;
        record.check(&expected, ports.map(u16::from), &audio)?;
    }
    ensure!(
        app.latest_frame
            .as_ref()
            .context("cadence final pixels missing")?
            .as_slice()
            == reference.framebuffer(),
        "cadence final pixels differ"
    );
    Ok(())
}
