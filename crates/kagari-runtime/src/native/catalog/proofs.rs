//! Explicit adoption boundary for the remaining path-based linked proof owner.
use crate::{error::RuntimeError, native::catalog::DeclarationCatalog};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::types::proofs::{ProofCatalog, implementation::Implementation};
use kagari_types::ty::{Constraint, Ty};

impl DeclarationCatalog {
    pub(crate) fn validate_callable_contracts(&self) -> Result<(), RuntimeError> {
        // The linked proof owner is still path-based until the remainder of
        // SI02 migrates it. Project only its explicit inputs at this boundary.
        let implementations = self
            .implementations
            .iter()
            .map(|(id, record)| Ok((id, self.paths(record)?)))
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let traits = self
            .traits
            .iter()
            .map(|(id, record)| Ok((id, self.paths(record)?)))
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let declarations = self
            .declarations
            .values()
            .map(|record| self.paths(record))
            .collect::<Result<Vec<_>, _>>()?;
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
            traits.iter().map(|(id, contract)| (id.clone(), contract)),
            declarations.iter(),
            &CancellationToken::default(),
        )
        .map_err(|_| {
            RuntimeError::metadata_conflict(
                "native template defaults or selected calls differ from their declared contracts",
            )
        })?;
        for declaration in declarations.iter() {
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
