use super::*;

pub(super) fn after_play(
    app: &mut App,
    options: &Options,
    reference: &mut EmuBackend,
    config: [u8; 32],
) -> Result<()> {
    app.set_netplay_paused(options.role == 0);
    let lead = options.input_delay.pause_lead();
    for frame in options.frames..options.frames + lead {
        set_input(app, sample(frame, options.role));
        round(app)?;
        wait_confirmed_image(app, frame + 1)?;
        let delay = options.input_delay.frames();
        let ports = [sample(frame - delay, 0), sample(frame - delay, 1)];
        let EmuBackend::Nes(nes) = reference else {
            bail!("lost reference")
        };
        nes.emu.set_input_p1_raw(ports[0]);
        nes.emu.set_input_p2_raw(ports[1]);
        reference.step_frame();
        let mut audio = Vec::new();
        reference.drain_audio_samples_into(&mut audio);
        let expected = identity::checkpoint(reference, frame + 1, &audio, config)?;
        let (actual, used, pcm) = app
            .netplay
            .proof
            .as_ref()
            .unwrap()
            .last
            .as_ref()
            .context("missing pause approach frame")?;
        ensure!(
            *actual == expected && *used == ports,
            "pause approach checkpoint differs"
        );
        ensure!(
            pcm.iter()
                .map(|v| v.to_bits())
                .eq(audio.iter().map(|v| v.to_bits())),
            "pause approach PCM differs"
        );
    }
    wait(app, Duration::from_secs(5), |app| app.netplay.paused)?;
    let boundary = options.frames + lead;
    let image = app.latest_frame.clone();
    stage_barrier(app, options, 0)?;
    super::chat::exchange(app, options, "paused")?;
    for (index, flags) in [[true, false], [true, true], [false, true]]
        .into_iter()
        .enumerate()
    {
        app.set_netplay_paused(flags[options.role]);
        round(app)?;
        wait(app, Duration::from_secs(5), |app| {
            app.netplay.proof.as_ref().unwrap().last_pause
                == Some((boundary, flags[options.role], flags[1 - options.role]))
        })?;
        ensure!(
            app.netplay.paused && app.netplay.presented == boundary,
            "pause advanced presentation"
        );
        ensure!(
            app.netplay.confirmed == boundary,
            "pause advanced confirmed output"
        );
        ensure!(
            image
                .as_ref()
                .zip(app.latest_frame.as_ref())
                .is_some_and(|(a, b)| std::sync::Arc::ptr_eq(a, b)),
            "pause published an image"
        );
        stage_barrier(app, options, index + 1)?;
    }
    Ok(())
}

fn stage_barrier(app: &mut App, options: &Options, stage: usize) -> Result<()> {
    let name = format!("pause-stage-{stage}");
    write_new(
        &options.root.join(format!("{name}.json")),
        &serde_json::to_vec(&json!({
            "stage": stage,
            "frame": app.netplay.presented,
            "confirmed": app.netplay.confirmed,
            "pause": app.netplay.proof.as_ref().unwrap().last_pause,
        }))?,
    )?;
    // Proof controllers fence observed stages; real gameplay needs no such barrier.
    wait(app, Duration::from_secs(10), |_| {
        options.root.join(format!("{name}.continue")).exists()
    })
}
