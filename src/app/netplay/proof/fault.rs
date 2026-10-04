use super::*;

pub(super) fn run(app: &mut App, options: &Options, frame: u64) -> Result<()> {
    ensure!(
        !app.netplay.in_flight
            && app.netplay.presented == frame
            && app.netplay.confirmed == frame
            && app.netplay.published_confirmed == frame,
        "fault baseline is incomplete"
    );
    write_new(
        &options.root.join("fault-ready.json"),
        &serde_json::to_vec(&json!({
            "player": options.role + 1,
            "frame": app.netplay.presented,
            "confirmed": app.netplay.confirmed,
            "published_confirmed": app.netplay.published_confirmed,
        }))?,
    )?;
    wait(app, Duration::from_secs(15), |_| {
        options.root.join("fault.continue").exists()
    })?;
    let fault = options.fault.as_deref().context("missing fault mode")?;
    if options.role == options.fault_role && fault == "disconnect" {
        app.request_netplay_stop();
    } else if fault != "disconnect" && app.netplay.running() {
        set_input(app, sample(frame, options.role));
        app.netplay.next_frame = None;
        app.pump_netplay();
        ensure!(app.netplay.in_flight, "fault exchange was not submitted");
        hold(app);
    }
    wait(app, Duration::from_secs(10), |app| {
        app.netplay.phase == Phase::Idle
    })?;
    let observation = app.netplay.proof.as_ref().context("fault observer lost")?;
    ensure!(
        observation.frames <= frame + 1,
        "fault exceeded already scheduled confirmed input"
    );
    ensure!(observation.last.as_ref().is_some_and(|(checkpoint, _, _)| matches!(checkpoint, Message::Checkpoint { frame: confirmed, .. } if (*confirmed >= frame && *confirmed <= frame + 1))), "fault confirmed beyond known inputs");
    if options.role != options.fault_role && fault == "stall" {
        ensure!(
            app.debug_windows
                .netplay
                .status
                .contains("I/O deadline expired")
                || app.debug_windows.netplay.status.contains("netplay stalled"),
            "expected peer-silence timeout, received {}",
            app.debug_windows.netplay.status
        );
    }
    if fault != "disconnect" || options.role != options.fault_role {
        ensure!(
            app.debug_windows.netplay.status != "local stop",
            "peer fault did not propagate"
        );
    }
    Ok(())
}
