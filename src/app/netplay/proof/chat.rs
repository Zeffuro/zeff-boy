use super::*;
use crate::debug::MenuAction;

pub(super) fn exchange(app: &mut App, options: &Options, stage: &str) -> Result<()> {
    let own = format!("{stage}: player {} 🎮", options.role + 1);
    let peer = format!("{stage}: player {} 🎮", 2 - options.role);
    let before = (app.netplay.presented, app.netplay.confirmed);
    app.handle_netplay_action(&MenuAction::SendNesNetplayChat(own.clone()));
    wait(app, Duration::from_secs(5), |app| {
        app.debug_windows
            .netplay
            .chat
            .messages()
            .any(|message| message.local && message.text == own)
            && app
                .debug_windows
                .netplay
                .chat
                .messages()
                .any(|message| !message.local && message.text == peer)
    })?;
    ensure!(
        app.debug_windows
            .netplay
            .chat
            .messages()
            .any(|message| message.local && message.text == own)
            && app
                .debug_windows
                .netplay
                .chat
                .messages()
                .any(|message| !message.local && message.text == peer),
        "chat transcript differs"
    );
    ensure!(
        (app.netplay.presented, app.netplay.confirmed) == before,
        "chat advanced gameplay"
    );
    ensure!(
        app.debug_windows.netplay.chat.error.is_empty(),
        "chat rejected"
    );
    Ok(())
}
