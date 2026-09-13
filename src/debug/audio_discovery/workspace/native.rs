use super::SongRef;

pub(super) fn draw(ui: &mut egui::Ui, song: SongRef<'_>) {
    let (title, tracks, timing, warnings) = match song {
        SongRef::GbTose(song) => (
            &song.title,
            song.tracks.len(),
            format!("DMG · bank {:02X}", song.bank),
            &song.warnings,
        ),
        SongRef::GbQuickThunder(song) => (
            &song.title,
            song.tracks.len(),
            format!("{:?} · bank {:02X}", song.hardware, song.bank),
            &song.warnings,
        ),
        SongRef::GbGhx(song) => (
            &song.title,
            song.tracks.len(),
            format!(
                "{:?} · bank {:02X} · module {} / subsong {}",
                song.hardware, song.bank, song.module, song.subsong
            ),
            &song.warnings,
        ),
        SongRef::GbSoundSystem(song) => (
            &song.title,
            song.tracks.len(),
            format!("{:?}", song.hardware),
            &song.warnings,
        ),
        SongRef::GbCarillon(song) => (
            &song.title,
            song.tracks.len(),
            format!(
                "CGB double speed · bank {:02X} · {} alias IDs",
                song.bank,
                song.aliases.len()
            ),
            &song.warnings,
        ),
        SongRef::WsTose(song) => (
            &song.title,
            song.tracks.len(),
            format!("WonderSwan {:?}", song.hardware),
            &song.warnings,
        ),
        SongRef::NesTose(song) => (
            &song.title,
            song.tracks.len(),
            "NES NTSC".into(),
            &song.warnings,
        ),
        SongRef::GbMusyx(song) => (
            &song.title,
            song.tracks.len(),
            "CGB normal speed".into(),
            &song.warnings,
        ),
        _ => unreachable!("native summary dispatch"),
    };
    ui.label(format!("{title} · {tracks} tracks"));
    ui.small(timing);
    for warning in warnings {
        ui.small(warning);
    }
}
