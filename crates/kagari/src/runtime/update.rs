//! Exact-publication preparation associations and source update orchestration.

#[cfg(feature = "source")]
mod prepare;
#[cfg(all(test, feature = "source"))]
mod tests;

#[cfg(feature = "source")]
use crate::{ReloadResult, engine::source::compilation::Compilation, update::Update};
use crate::{
    RunResult,
    error::{EmbeddingError, RuntimeFailureKind},
    program::inputs::RetainedPreparation,
};
#[cfg(any(feature = "source", feature = "native"))]
use crate::{program::PreparedProgram, runtime::Runtime};
#[cfg(any(feature = "source", feature = "native"))]
use kagari_runtime::module::Program;
use kagari_runtime::module::dispatch::PublicationToken;
use std::{
    cell::{RefCell, RefMut},
    collections::HashMap,
};

#[derive(Debug, Default)]
pub(super) struct Publications {
    entries: RefCell<HashMap<PublicationToken, RetainedPreparation>>,
}

impl Publications {
    /// Reserve before inserting a staged publication's association. Both allocation
    /// and insertion precede the core visibility change, including initial load.
    pub(super) fn reserve(
        &self,
    ) -> RunResult<RefMut<'_, HashMap<PublicationToken, RetainedPreparation>>> {
        let mut entries = self.entries.try_borrow_mut().map_err(|_| {
            EmbeddingError::runtime(
                RuntimeFailureKind::ExecutionPhaseViolation,
                "preparation associations are already borrowed",
            )
        })?;
        entries.try_reserve(1).map_err(|error| {
            EmbeddingError::runtime(
                RuntimeFailureKind::ResourceLimitExceeded,
                format!("cannot reserve preparation association: {error}"),
            )
        })?;
        Ok(entries)
    }

    pub(super) fn current(&self, token: PublicationToken) -> RunResult<RetainedPreparation> {
        let mut entries = self.entries.try_borrow_mut().map_err(|_| {
            EmbeddingError::runtime(
                RuntimeFailureKind::ExecutionPhaseViolation,
                "preparation associations are already borrowed",
            )
        })?;
        // A low-level publication has no SDK capsule. Never fall back to the last
        // SDK baseline; release that stale capsule at the next SDK lookup instead.
        entries.retain(|key, _| key.installation_id() != token.installation_id() || *key == token);
        Ok(entries.get(&token).cloned().unwrap_or_default())
    }

    pub(super) fn stage(
        &self,
        token: PublicationToken,
        inputs: RetainedPreparation,
    ) -> RunResult<Association<'_>> {
        let mut entries = self.reserve()?;
        if entries.contains_key(&token) {
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::EngineInvariant,
                "duplicate preparation publication token",
            ));
        }
        entries.insert(token, inputs);
        Ok(Association {
            entries,
            token,
            published: false,
        })
    }
}

/// Own the precommit association through core commit. Failed publication or unwinding
/// removes it; successful publication prunes older capsules outside the core lock.
pub(super) struct Association<'a> {
    entries: RefMut<'a, HashMap<PublicationToken, RetainedPreparation>>,
    token: PublicationToken,
    published: bool,
}

impl Association<'_> {
    pub(super) fn published(mut self) {
        self.published = true;
        self.entries.retain(|key, _| {
            key.installation_id() != self.token.installation_id() || *key == self.token
        });
    }
}

impl Drop for Association<'_> {
    fn drop(&mut self) {
        if !self.published {
            self.entries.remove(&self.token);
        }
    }
}

#[cfg(feature = "source")]
impl Runtime {
    /// Check and publish one detached source batch at an idle host scheduling boundary.
    /// No engine source session, filesystem discovery, initializer or host callback is
    /// used. Any error leaves the executable and its authoritative source capsule intact.
    pub fn update(&self, program: &Program, update: Update) -> ReloadResult<()> {
        let guard = self.runtime().begin_publication(program)?;
        prepare::validate_batch(&update)?;
        let current = self.runtime().select_program(program)?;
        let retained = self.publications.current(current.publication_token())?;
        let inputs = retained.compilation.ok_or_else(|| EmbeddingError::Source {
            message: "MissingCompilationContext: current publication has no matching immutable compilation capsule".into(),
        })?;
        let checked =
            prepare::check_candidate(self.runtime(), program, &current, &inputs, &update)?;
        let options = inputs.preparation_options(update.cancel.clone());
        let demands = current.retained_demands();
        let compilation = Compilation::lower_with_demands(
            &checked,
            &options.lowering,
            demands.metadata(),
            demands.implementation_headers(),
        )?;
        let prepared = PreparedProgram::from_compilation(compilation, &options)?;
        update
            .cancel
            .check()
            .map_err(|_| EmbeddingError::Cancelled)?;
        let candidate = guard
            .stage_reload_verified_program(None, prepared.bytecode().clone())
            .map_err(EmbeddingError::reload_validation)?;
        let association = self
            .publications
            .stage(candidate.publication_token(), prepared.retained_inputs())?;
        update
            .cancel
            .check()
            .map_err(|_| EmbeddingError::Cancelled)?;
        guard
            .publish(candidate)
            .map_err(EmbeddingError::reload_validation)?;
        association.published();
        Ok(())
    }
}

#[cfg(feature = "native")]
impl Runtime {
    /// Create a fresh preparation cache over the current exact executable and optional
    /// MIR. The Runtime retains neither this Rc wrapper nor a codegen backend.
    pub fn prepared_program(&self, program: &Program) -> RunResult<PreparedProgram> {
        let current = self.runtime().select_program(program)?;
        let inputs = self.publications.current(current.publication_token())?;
        Ok(PreparedProgram::from_current(
            current.verified_program().clone(),
            inputs,
        ))
    }
}
