//! Host constant reads borrow an explicit actual revision and retain escaping values.
use crate::{
    Runtime,
    error::RuntimeError,
    module::{ExecutableModule, managed::revision::RevisionState, retention::RetentionOrigin},
    value::carriers::RootedValue,
};
use kagari_bytecode::instruction::ConstantId;

impl Runtime {
    /// Materialize a portable constant once in its actual revision's managed pool.
    ///
    /// Current selects the matching publication; Admitted and Retained borrow an
    /// exact protected body. A descriptor alone cannot reopen a retired revision.
    /// Managed results acquire an independent Host lease. Foreign, mismatched or
    /// expired origins and out-of-bounds constants fail before materialization.
    pub fn read_constant(
        &mut self,
        module: &ExecutableModule,
        constant: ConstantId,
        origin: RetentionOrigin<'_>,
    ) -> Result<RootedValue, RuntimeError> {
        let origin = origin.resolve(self, module)?;
        self.with_revision_origin(module, origin, |slots, revision| {
            let value = RevisionState::constant(slots.scope(), revision, module.slot(), constant)?;
            value.retain(slots.scope())
        })
    }
}
