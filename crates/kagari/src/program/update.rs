//! Detached source edits and authored locators owned by an immutable Program base.
mod prepare;

#[cfg(test)]
mod tests;

use crate::{
    CompileResult, RunResult,
    engine::source::compilation::Compilation,
    error::EmbeddingError,
    program::{Program, ProgramState},
    update::Update,
};
use kagari_common::identity::{DefinitionPath, table::DefinitionId};
use kagari_contract::source::{AuthoredDeclaration, AuthoredKind};
use std::sync::{Arc, Weak};

/// Exact authored edit locator for one immutable base. It retains diagnostic
/// identity only, not code or installation authority. A different base rejects it;
/// acquire a new target after preparing another revision. Private declarations can
/// be edited without granting permission to invoke them.
#[derive(Debug, Clone)]
pub struct SourceEditTarget {
    base: Weak<ProgramState>,
    declaration: DefinitionId,
    path: DefinitionPath,
    kind: AuthoredKind,
}

impl SourceEditTarget {
    pub fn name(&self) -> &str {
        self.path.path.last().map_or("", |part| part.name.as_str())
    }

    pub fn path(&self) -> &DefinitionPath {
        &self.path
    }

    pub fn kind(&self) -> AuthoredKind {
        self.kind
    }
}

impl Program {
    /// Copy checked authored locators, including private functions and named locals.
    /// Diagnostic source attachment is sufficient for discovery; preparing edits
    /// additionally requires this product's immutable compilation inputs.
    pub fn source_declarations(&self) -> RunResult<Vec<SourceEditTarget>> {
        let package = self
            .bytecode()
            .source_package()
            .ok_or_else(|| source_error("source declaration metadata is unavailable"))?;
        package
            .records()
            .declarations
            .iter()
            .map(|declaration| {
                let path = package
                    .definitions()
                    .resolve(declaration.declaration)
                    .map_err(|error| source_error(error.to_string()))?
                    .to_path();
                Ok(SourceEditTarget {
                    base: Arc::downgrade(&self.state),
                    declaration: declaration.declaration,
                    path,
                    kind: declaration.kind,
                })
            })
            .collect()
    }

    /// Check, lower and verify one complete edited candidate without a Runtime or
    /// filesystem lookup. The result expects this exact base and carries cumulative
    /// historical demands. Cancellation/errors preserve the immutable base; no
    /// installation, initializer, callback or publication is touched.
    pub fn prepare_update(&self, update: Update) -> CompileResult<Self> {
        prepare::validate_batch(&update)?;
        let inputs = self.state.compilation.as_ref().ok_or_else(|| {
            source_error(
                "MissingCompilationContext: Program has no matching immutable compilation capsule",
            )
        })?;
        let checked = prepare::check_candidate(self, inputs, &update)?;
        let options = inputs.preparation_options(update.cancel.clone());
        let compilation = Compilation::lower_with_demands(
            &checked,
            &options.lowering,
            self.state.history.metadata(),
            self.state.history.implementation_headers(),
        )?;
        let candidate = Self::from_compilation(compilation, &options, Some(self))?;
        update
            .cancel
            .check()
            .map_err(|_| EmbeddingError::Cancelled)?;
        Ok(candidate)
    }

    fn resolve_edit_target(&self, target: &SourceEditTarget) -> CompileResult<AuthoredDeclaration> {
        if !Weak::ptr_eq(&target.base, &Arc::downgrade(&self.state)) {
            return Err(source_error(
                "source edit target belongs to a different Program base",
            ));
        }
        self.bytecode()
            .source_package()
            .and_then(|package| {
                package
                    .records()
                    .declarations
                    .iter()
                    .find(|record| record.declaration == target.declaration)
            })
            .cloned()
            .ok_or_else(|| source_error("source edit target is absent from its base"))
    }
}

fn source_error(message: impl Into<String>) -> EmbeddingError {
    EmbeddingError::Source {
        message: message.into(),
    }
}
