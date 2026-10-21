//! One bounded traversal for mutually dependent method and construction evidence.
use crate::callable::{
    construction::evidence::{ConstructionSelection, ConstructionWitness},
    witness::OperationWitness,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::mapping::{DefinitionMappingError, check_cancel},
};

/// Borrow one node in the shared operation/construction proof tree.
/// File aggregate accounting uses the same child edges as codecs and admission.
#[derive(Clone, Copy)]
pub enum EvidenceRef<'a> {
    Operation(&'a OperationWitness),
    Construction(&'a ConstructionWitness),
}

impl<'a> EvidenceRef<'a> {
    pub fn children(self) -> (&'a [OperationWitness], &'a [ConstructionWitness]) {
        match self {
            Self::Operation(OperationWitness::SharedMethod(method)) => {
                (&method.operations, &method.constructions)
            }
            Self::Operation(OperationWitness::SharedAdapter(adapter)) => {
                (&adapter.call.operations, &adapter.call.constructions)
            }
            Self::Construction(ConstructionWitness {
                selection:
                    ConstructionSelection::Declared {
                        operations,
                        constructions,
                        ..
                    },
                ..
            }) => (operations, constructions),
            _ => (&[], &[]),
        }
    }

    pub(crate) fn check_shape(
        self,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        let mut pending = vec![(self, 0)];
        let mut count = 0;
        while let Some((node, depth)) = pending.pop() {
            check_cancel(cancel)?;
            count += 1;
            if count > 4096 || depth > 64 {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            let (operations, constructions) = node.children();
            if operations.len() > 4096 || constructions.len() > 4096 {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            pending.extend(
                constructions
                    .iter()
                    .rev()
                    .map(|child| (Self::Construction(child), depth + 1)),
            );
            pending.extend(
                operations
                    .iter()
                    .rev()
                    .map(|child| (Self::Operation(child), depth + 1)),
            );
        }
        Ok(())
    }
}
