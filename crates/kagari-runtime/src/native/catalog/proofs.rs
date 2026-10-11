//! Linked proofs borrow scoped native declarations directly from their catalog.
use crate::{error::RuntimeError, native::catalog::DeclarationCatalog};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::types::proofs::{ProofCatalog, implementation::Implementation};
use kagari_types::ty::{Constraint, Ty};

impl DeclarationCatalog {
    pub(crate) fn validate_callable_contracts(&self) -> Result<(), RuntimeError> {
        let implementations = self.implementations.entries().collect::<Vec<_>>();
        let catalog = ProofCatalog::new(
            implementations
                .iter()
                .map(|(declaration, implementation)| Implementation::Native {
                    declaration,
                    implementation,
                })
                .collect(),
            vec![],
            [],
            self.traits.entries(),
            self.declarations.values(),
            &self.definitions(),
            &CancellationToken::default(),
        )
        .map_err(|_| {
            RuntimeError::metadata_conflict(
                "native template defaults or selected calls differ from their declared contracts",
            )
        })?;
        for declaration in self.declarations.values() {
            if let Some(receiver) = &declaration.concrete_result {
                let Ty::Trait(interface) = &declaration.function.return_type else {
                    return Err(RuntimeError::metadata_conflict(
                        "native concrete result requires an interface return",
                    ));
                };
                if !catalog
                    .constraints_hold(
                        receiver,
                        &[Constraint::Trait(interface.clone())],
                        &declaration.function.bounds,
                        &CancellationToken::default(),
                    )
                    .map_err(|_| RuntimeError::metadata_conflict("invalid native result proof"))?
                {
                    return Err(RuntimeError::metadata_conflict(
                        "native concrete result does not implement its return interface",
                    ));
                }
            }
        }
        Ok(())
    }
}
