use super::*;
use crate::emu_backend::loader::{DirectMdTasExecutionLoader, MdTasProfile, md_product};

#[test]
fn native_md_product_materialization_keeps_both_pads_and_rejects_hidden_controls() -> Result<()> {
    for pad in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        let directory = crate::test_support::test_directory("md-materialize-input")?;
        let source = directory.path().join("game.md");
        std::fs::write(&source, crate::netplay::md_fixture::rom())?;
        let mut project = DirectMdTasExecutionLoader::new(source, pad).create_project()?;
        let profile = md_product::md_execution_profile(pad);
        let mut input = crate::tas_project::TasInputFrame::default();
        input.players[0].buttons = 0x40;
        input.players[0].dpad = 1;
        input.players[1].buttons = 0x0a;
        input.players[1].dpad = 8;
        project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
        let actual = materialize_profile_input(project.branch("main").unwrap(), 0, profile)?;
        assert_eq!(
            (
                actual.p1_buttons,
                actual.p1_dpad,
                actual.p2_buttons,
                actual.p2_dpad
            ),
            (0x40, 1, 0x0a, 8)
        );
        input.players[2].buttons = 1;
        project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
        assert!(materialize_profile_input(project.branch("main").unwrap(), 0, profile).is_err());
        input.players[2].buttons = 0;
        input.players[0].buttons = 0x10;
        project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
        assert_eq!(
            materialize_profile_input(project.branch("main").unwrap(), 0, profile).is_ok(),
            pad == MdTasProfile::SixButton
        );
    }
    Ok(())
}
