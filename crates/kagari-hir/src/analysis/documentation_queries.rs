//! Source presentation is resolved through declarations owned by the snapshot.

use crate::{
    analysis::{
        AnalysisSnapshot, FileAnalysis,
        declaration_queries::{DeclarationSnapshot, FileDeclarations},
    },
    declarations::{Declaration, DeclarationId},
    host::origin::HostDeclarationOrigin,
};
use kagari_common::{identity::FileId, span::Span};
use kagari_syntax::ast::{
    item::{Item, MethodDef},
    misc::{Field, Name, Variant},
    traits::AstNode,
};

#[cfg(test)]
mod tests;

/// Written source metadata. Checked call signatures are a separate semantic query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationDocumentation {
    pub declaration: Declaration,
    pub documentation: String,
    pub written_signature: String,
}

impl FileAnalysis {
    /// Supplied host navigation metadata, resolved by the checked declaration
    /// identity. Missing metadata never creates a synthetic source declaration.
    pub fn host_origin_at(&self, offset: usize) -> Option<&HostDeclarationOrigin> {
        let id = self
            .host_function_at(offset)
            .map(|declaration| &declaration.id)
            .or_else(|| {
                self.host_field_at(offset)
                    .map(|declaration| &declaration.id)
            })
            .or_else(|| self.host_type_at(offset).map(|declaration| &declaration.id))?;
        self.result.facts().names.hosts.origin(id)
    }
}

impl DeclarationSnapshot {
    /// Read documentation without resolving or checking any function body.
    /// Identities absent from this snapshot never fall back to a global catalog.
    pub fn documentation(&self, id: &DeclarationId) -> Option<DeclarationDocumentation> {
        self.files.values().find_map(|file| file.documentation(id))
    }
}

impl AnalysisSnapshot {
    /// Read source metadata for the resolved declaration at a use or declaration site.
    pub fn documentation_at(
        &self,
        file: FileId,
        offset: usize,
    ) -> Option<DeclarationDocumentation> {
        let declaration = self.definition_at(file, offset)?;
        self.declaration_snapshot().documentation(&declaration.id)
    }
}

impl FileDeclarations {
    fn documentation(&self, id: &DeclarationId) -> Option<DeclarationDocumentation> {
        let declaration = self.declarations().get(id)?;
        let source = self.source();
        if source.span(declaration.location.range)? != declaration.location {
            return None;
        }
        let (documentation, written_signature) =
            if let Some(package) = &self.declared.lowered.installed_stdlib {
                let file = package
                    .files()
                    .iter()
                    .find(|file| file.source().id() == source.id())?;
                let site = file
                    .declarations()
                    .iter()
                    .find(|site| site.name_span == Some(declaration.location.range))?;
                (site.documentation.clone(), site.written_signature.clone())
            } else {
                let node = self.parsed.syntax().syntax().descendants().find(|node| {
                    node.children().filter_map(Name::cast).any(|name| {
                        let range = name
                            .syntax()
                            .descendants_with_tokens()
                            .filter_map(|element| element.into_token())
                            .find(|token| !token.kind().is_trivia())
                            .map(|token| token.text_range());
                        range.is_some_and(|range| {
                            Span::new(usize::from(range.start()), usize::from(range.end()))
                                == declaration.location.range
                        })
                    })
                })?;
                let documentation = if let Some(item) = Item::cast(node.clone()) {
                    item.documentation(source.text())
                } else if let Some(method) = MethodDef::cast(node.clone()) {
                    method.documentation(source.text())
                } else if let Some(variant) = Variant::cast(node.clone()) {
                    variant.documentation(source.text())
                } else {
                    Field::cast(node.clone())?.documentation(source.text())
                };
                (documentation, node.text().to_string())
            };
        Some(DeclarationDocumentation {
            declaration: declaration.clone(),
            documentation,
            written_signature,
        })
    }
}
