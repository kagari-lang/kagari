//! Hover query policy and protocol presentation.
use crate::analysis::{AnalysisFailure, AnalysisState, features::DocumentQueryInput};
use kagari_common::cancellation::CancellationToken;
use lsp_types::{Hover, HoverContents, LanguageString, MarkedString};

impl AnalysisState {
    pub fn hover(
        &mut self,
        input: &DocumentQueryInput,
        cancel: &CancellationToken,
    ) -> Result<Option<Hover>, AnalysisFailure> {
        let retained = self.editor_for_input(input, cancel)?;
        let editor = &retained.editor;
        let mut contents = Vec::new();
        let documentation = editor.documentation_at(input.offset);
        if let Some(documentation) = &documentation {
            if documentation
                .written_signature
                .len()
                .saturating_add(documentation.documentation.len())
                > input.limit / 4
            {
                return Err(AnalysisFailure::Capacity(
                    "hover documentation exceeds its byte limit".into(),
                ));
            }
            if !documentation.written_signature.is_empty() {
                contents.push(MarkedString::LanguageString(LanguageString {
                    language: "kagari".into(),
                    value: documentation.written_signature.clone(),
                }));
            }
            if !documentation.documentation.is_empty() {
                contents.push(MarkedString::String(documentation.documentation.clone()));
            }
        }
        if let Some(ty) = editor
            .type_at(input.offset)
            .filter(|ty| !ty.is_unresolved())
        {
            let spelling = ty.display_name(editor.definitions());
            let name = editor
                .definition_at(input.offset)
                .map(|declaration| declaration.name.as_str());
            let text = name.map_or_else(|| spelling.clone(), |name| format!("{name}: {spelling}"));
            contents.push(MarkedString::LanguageString(LanguageString {
                language: "kagari".into(),
                value: text,
            }));
        }
        cancel.check()?;
        let value = if contents.is_empty() {
            None
        } else {
            Some(Hover {
                contents: HoverContents::Array(contents),
                range: None,
            })
        };
        Ok(value)
    }
}
