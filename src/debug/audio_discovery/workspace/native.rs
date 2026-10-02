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
        SongRef::GbCosmigo(song) => (
            &song.title,
            song.tracks.len(),
            format!("CGB double speed · bank {:02X}", song.bank),
            &song.warnings,
        ),
        SongRef::GbMplay(song) => (
            &song.title,
            song.tracks.len(),
            format!("CGB double speed · bank {:02X}", song.bank),
            &song.warnings,
        ),
        SongRef::GbImed(song) => (
            &song.title,
            song.tracks.len(),
            format!("bank {:02X}", song.bank),
            &song.warnings,
        ),
        SongRef::GbBlackBox(song) => (
            &song.title,
            4,
            format!("bank {:02X}", song.bank),
            &song.warnings,
        ),
        SongRef::GbResident(song) => (
            &song.title,
            4,
            format!(
                "DMG · bank {:02X} · {}",
                song.bank,
                if song.timer_modulo.is_some() {
                    "timer"
                } else {
                    "VBlank"
                }
            ),
            &song.warnings,
        ),
        SongRef::GbTimer(song) => (
            &song.title,
            4,
            "DMG · dynamic timer · 180 second limit".into(),
            &song.warnings,
        ),
        SongRef::GbCache(song) => (
            &song.title,
            4,
            "DMG · original startup then native frame · 180 second limit".into(),
            &song.warnings,
        ),
        SongRef::GbWave(song) => (
            &song.title,
            4,
            "CGB double · source timer or frame · 180 second limit".into(),
            &song.warnings,
        ),
        SongRef::GbChannel(song) => (
            &song.title,
            4,
            "DMG · timer with VBlank rearm".into(),
            &song.warnings,
        ),
        SongRef::GbPage(song) => (
            &song.title,
            4,
            format!(
                "CGB {} · VBlank",
                if song.double_speed {
                    "double speed"
                } else {
                    "normal speed"
                }
            ),
            &song.warnings,
        ),
        SongRef::GbTimed(song) => (&song.title, 4, "DMG · VBlank".into(), &song.warnings),
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
