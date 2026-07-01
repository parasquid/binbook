use binbook_compiler::{CompileEvent, CompileObserver, CompilePhase, CompileWarning};

#[derive(Default)]
pub struct Events {
    pub phases: Vec<CompilePhase>,
    pub warnings: Vec<CompileWarning>,
}

impl CompileObserver for Events {
    fn on_event(&mut self, event: CompileEvent<'_>) {
        match event {
            CompileEvent::Progress { phase, .. } => self.phases.push(phase),
            CompileEvent::Warning(warning) => self.warnings.push(warning.clone()),
        }
    }
}

#[allow(dead_code)]
pub const IMAGE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="black"/></svg>"#;
