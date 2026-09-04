use super::{TasEditorExecutionAvailability, TasEditorWindowState};
use crate::tas_project::TasEditorExecutionAttachment;

impl TasEditorWindowState {
    pub(super) fn detach_incompatible_execution(&mut self) -> Option<anyhow::Error> {
        let error = match (self.execution_engine.as_ref(), self.session.as_ref()) {
            (Some(engine), Some(session)) => engine.validate_editor_session(session).err(),
            (Some(_), None) => Some(anyhow::anyhow!(
                "private TAS execution has no open editor session"
            )),
            (None, _) => None,
        };
        if error.is_some() {
            let _ = self.execution_engine.take();
            self.execution_availability = TasEditorExecutionAvailability::Unavailable(
                "The edited project no longer matches the loaded game".to_owned(),
            );
        }
        error
    }

    pub(crate) fn attach_execution(&mut self, attachment: TasEditorExecutionAttachment) {
        let _ = self.execution_engine.take();
        self.execution_preview.clear();
        let Some(session) = self.session.as_ref() else {
            self.execution_availability = match attachment {
                TasEditorExecutionAttachment::Available(_) => {
                    TasEditorExecutionAvailability::GameReady
                }
                TasEditorExecutionAttachment::Unavailable(reason) => {
                    TasEditorExecutionAvailability::Unavailable(reason.to_string())
                }
            };
            return;
        };
        let provider = match attachment {
            TasEditorExecutionAttachment::Available(provider) => provider,
            TasEditorExecutionAttachment::Unavailable(reason) => {
                self.execution_availability =
                    TasEditorExecutionAvailability::Unavailable(reason.to_string());
                self.message = Some((
                    false,
                    format!("Private TAS execution unavailable: {reason}"),
                ));
                return;
            }
        };
        match provider.load_editor_engine(session.project()) {
            Ok(engine) => {
                self.execution_engine = Some(engine);
                self.execution_availability = TasEditorExecutionAvailability::Ready;
                self.message = Some((false, "Offline playback ready".to_owned()));
            }
            Err(error) => {
                self.execution_availability =
                    TasEditorExecutionAvailability::Unavailable(format!("{error:#}"));
                self.message = Some((
                    true,
                    format!("Private TAS execution was not attached: {error:#}"),
                ));
            }
        }
    }
}
