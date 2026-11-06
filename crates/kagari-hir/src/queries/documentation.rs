//! Source presentation is resolved through declarations owned by the snapshot.

use crate::{
    analysis::{
        AnalysisSnapshot,
        declarations::{DeclarationSnapshot, FileDeclarations},
    },
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

/// Written source metadata. Checked call signatures are a separate semantic query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationDocumentation {
    /// Declaration in the owning snapshot's scoped identity table.
    pub declaration: Declaration,
    /// Attached source documentation rendered as Markdown text.
    pub documentation: String,
    /// Signature spelling recovered from source, separate from checked semantic types.
    pub written_signature: String,
}

/// Module Markdown and its location in this immutable analysis snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDocumentation {
    /// Logical identity of the documented module.
    pub module: ModuleIdentity,
    /// Source range covering the module's retained source.
    pub location: FileSpan,
    /// Module-level Markdown extracted from the syntax tree.
    pub documentation: String,
}

impl DeclarationSnapshot {
    /// Finds module documentation in retained declarations; returns `None` for an unknown module.
    pub fn module_documentation(&self, module: &ModuleIdentity) -> Option<ModuleDocumentation> {
        let file = self
            .files()
            .find(|file| file.source().module_identity() == module)?;
        let source = file.source();
        Some(ModuleDocumentation {
            module: module.clone(),
            location: source.span(file.syntax().content_range()?)?,
            documentation: file.syntax().module_documentation(),
        })
    }

    /// Read documentation without resolving or checking any function body.
    /// Identities absent from this snapshot never fall back to a global catalog.
    pub fn documentation(&self, id: &DeclarationId) -> Option<DeclarationDocumentation> {
        let declaration = self.declaration(id)?;
        self.files()
            .find_map(|file| file.documentation(declaration))
    }
}

impl AnalysisSnapshot {
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
        self.declaration_snapshot().module_documentation(
            self.module_graph()
                .catalog
                .definitions
                .module_identity(unit.module)
                .ok()?,
        )
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
    fn documentation(&self, declaration: &Declaration) -> Option<DeclarationDocumentation> {
        let declaration = self.declarations().get(&declaration.id)?;
        let source = self.source();
        let local_range = source.local_range(declaration.location)?;
        let syntax = self.syntax();
        let node = syntax.descendants().find(|node| {
            node.declaration_name()
                .is_some_and(|name| name.body_span() == local_range)
        })?;
        let documentation = node.documentation();
        let written_signature = node.text().to_owned();
        Some(DeclarationDocumentation {
            declaration: declaration.clone(),
            documentation,
            written_signature,
        })
    }
}
