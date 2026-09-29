//! Source presentation is resolved through declarations owned by the snapshot.

use crate::{
    analysis::{AnalysisSnapshot, DeclarationSnapshot, FileDeclarations},
    declarations::{Declaration, DeclarationId},
};
use kagari_common::{Span, identity::FileId};
use kagari_syntax::ast::{AstNode, Field, Item, MethodDef, Name, Variant};

#[cfg(test)]
mod tests;

/// Written source metadata. Checked call signatures are a separate semantic query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationDocumentation {
    pub declaration: Declaration,
    pub documentation: String,
    pub written_signature: String,
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
                        let range = name.syntax().text_range();
                        Span::new(usize::from(range.start()), usize::from(range.end()))
                            == declaration.location.range
                    })
                })?;
                let documentation = if let Some(item) = Item::cast(node.clone()) {
                    item.documentation(source.text())
                } else if let Some(method) = MethodDef::cast(node.clone()) {
                    method.documentation(source.text())
                } else if let Some(variant) = Variant::cast(node.clone()) {
                    variant.documentation(source.text())
                } else if let Some(field) = Field::cast(node.clone()) {
                    field.documentation(source.text())
                } else {
                    return None;
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
