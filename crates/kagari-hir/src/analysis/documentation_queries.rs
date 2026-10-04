//! Source presentation is resolved through declarations owned by the snapshot.

use crate::analysis::ownership;
use crate::{
    analysis::{
        AnalysisSnapshot, FileAnalysis,
        declaration_queries::{DeclarationSnapshot, FileDeclarations},
    },
    declarations::{Declaration, DeclarationId},
    host::origin::HostDeclarationOrigin,
};
use kagari_syntax::ast::{
    item::{Item, MethodDef},
    misc::{Field, Name, Variant},
    traits::AstNode,
};
use {
    kagari_common::{
        identity::{ModuleIdentity, reference::DefinitionReference, table::DefinitionId},
        span::Span,
    },
    kagari_source::identity::{FileId, FileSpan},
};

#[cfg(test)]
mod tests;

/// Written source metadata. Checked call signatures are a separate semantic query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationDocumentation {
    pub declaration: Declaration<DefinitionId>,
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
        self.result.records().facts().names.hosts.origin(id)
    }
}

/// Module Markdown and its location in this immutable analysis snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDocumentation {
    pub module: ModuleIdentity,
    pub location: FileSpan,
    pub documentation: String,
}

impl DeclarationSnapshot {
    pub fn module_documentation(&self, module: &ModuleIdentity) -> Option<ModuleDocumentation> {
        let file = self
            .files
            .values()
            .find(|file| file.source().module_identity() == module)?;
        let source = file.source();
        Some(ModuleDocumentation {
            module: module.clone(),
            location: source.span(Span::new(0, source.text().len()))?,
            documentation: file.syntax().module_documentation(),
        })
    }

    /// Read documentation without resolving or checking any function body.
    /// Identities absent from this snapshot never fall back to a global catalog.
    pub fn documentation<I: DefinitionReference>(
        &self,
        id: &DeclarationId<I>,
    ) -> Option<DeclarationDocumentation> {
        self.files.values().find_map(|file| file.documentation(id))
    }
}

impl AnalysisSnapshot {
    pub fn module_documentation_at(
        &self,
        file: FileId,
        offset: usize,
    ) -> Option<ModuleDocumentation> {
        let target = self.source_import_at(file, offset)?;
        if target.item.is_some() {
            return None;
        }
        self.declaration_snapshot()
            .module_documentation(&target.module)
    }

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
    fn documentation<I: DefinitionReference>(
        &self,
        id: &DeclarationId<I>,
    ) -> Option<DeclarationDocumentation> {
        let id = ownership::locate(id, self.declarations().definitions())?;
        let declaration = self.declarations().get(&id)?;
        let source = self.source();
        let local_range = source.local_range(declaration.location)?;
        let (documentation, written_signature) = {
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
                            == local_range
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
