use crate::{
    declarations::{DeclarationFileQueries, DeclarationSnapshotQueries},
    snapshot::SnapshotQueries,
};

use kagari_hir::{
    declarations::{Declaration, DeclarationId},
    imports::{NamespaceId, ResolvedTarget},
};
use kagari_syntax::ast::borrow::AstNode;

use {
    kagari_common::identity::ModuleIdentity,
    kagari_source::identity::{FileId, FileSpan},
};

#[cfg(test)]
mod tests;

/// Declaration documentation from exact source or retained structured metadata.
/// Checked call signatures are a separate semantic query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationDocumentation {
    /// Declaration in the owning snapshot's scoped identity table.
    pub declaration: Declaration,
    /// Attached documentation rendered as Markdown, without inventing source text.
    pub documentation: String,
    /// Authored signature spelling, absent for structured registrations.
    /// Checked semantic signatures remain available through callable queries.
    pub written_signature: Option<String>,
}

/// Module Markdown and its location in this immutable analysis snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDocumentation {
    /// Logical identity of the documented module.
    pub module: ModuleIdentity,
    /// Exact source range, absent for a structured registration.
    pub location: Option<FileSpan>,
    /// Module-level Markdown extracted from the syntax tree.
    pub documentation: String,
}

impl DeclarationSnapshotQueries<'_> {
    /// Finds module documentation in retained declarations; returns `None` for an unknown module.
    pub fn module_documentation(&self, module: &ModuleIdentity) -> Option<ModuleDocumentation> {
        let file = self
            .analysis
            .files()
            .find(|file| file.source().module_identity() == module)?;
        let source = file.source();
        if let Some(syntax) = file.syntax() {
            return Some(ModuleDocumentation {
                module: module.clone(),
                location: Some(source.span(syntax.content_range()?)?),
                documentation: syntax.module_documentation(),
            });
        }
        let provider = self
            .analysis
            .native_modules()
            .iter()
            .find(|provider| &provider.records().identity == module)?;
        Some(ModuleDocumentation {
            module: module.clone(),
            location: None,
            documentation: provider.records().module_documentation.clone(),
        })
    }

    /// Read documentation without resolving or checking any function body.
    /// Identities absent from this snapshot never fall back to a global catalog.
    pub fn documentation(&self, id: &DeclarationId) -> Option<DeclarationDocumentation> {
        let declaration = self.analysis.declaration(id)?;
        if declaration.source_location().is_some() {
            return self
                .analysis
                .files()
                .find_map(|file| DeclarationFileQueries::new(file).documentation(declaration));
        }
        let DeclarationId::Definition(id) = declaration.id else {
            return None;
        };
        let definitions = self.analysis.module_graph().catalog.definitions();
        let path = definitions.resolve(id).ok()?.to_path();
        let provider = self
            .analysis
            .native_modules()
            .iter()
            .find(|provider| provider.records().identity == path.module)?;
        let id = provider.definitions().lookup(&path)?;
        Some(DeclarationDocumentation {
            declaration: declaration.clone(),
            documentation: provider
                .records()
                .documentation
                .get(&id)
                .cloned()
                .unwrap_or_default(),
            written_signature: None,
        })
    }
}

impl SnapshotQueries<'_> {
    /// Resolves a source position to a module namespace and reads that module's documentation.
    pub fn module_documentation_at(
        &self,
        file: FileId,
        offset: usize,
    ) -> Option<ModuleDocumentation> {
        let target = self.source_target_at(file, offset)?;
        let ResolvedTarget::Namespace(NamespaceId::Module(unit)) = target.target else {
            return None;
        };
        DeclarationSnapshotQueries::new(self.analysis.declaration_snapshot()).module_documentation(
            self.analysis
                .module_graph()
                .catalog
                .definitions()
                .module_identity(unit.module)
                .ok()?,
        )
    }

    /// Read documentation for a resolved declaration, with source spelling only when available.
    pub fn documentation_at(
        &self,
        file: FileId,
        offset: usize,
    ) -> Option<DeclarationDocumentation> {
        let declaration = self.definition_at(file, offset)?;
        DeclarationSnapshotQueries::new(self.analysis.declaration_snapshot())
            .documentation(&declaration.id)
    }
}

impl DeclarationFileQueries<'_> {
    fn documentation(&self, declaration: &Declaration) -> Option<DeclarationDocumentation> {
        let declaration = self.analysis.declarations().get(&declaration.id)?;
        let source = self.analysis.source();
        let local_range = source.local_range(declaration.source_location()?)?;
        let syntax = self.analysis.syntax()?;
        let node = syntax.descendants().find(|node| {
            node.declaration_name()
                .is_some_and(|name| name.body_span() == local_range)
        })?;
        let documentation = node.documentation();
        let written_signature = node.text().to_owned();
        Some(DeclarationDocumentation {
            declaration: declaration.clone(),
            documentation,
            written_signature: Some(written_signature),
        })
    }
}
