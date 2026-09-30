use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::standard::traits::StandardTrait;
use kagari_common::identity::associated_type_id;
use kagari_hir::{
    builtin::traits::{self, StandardTraitSemantics},
    types::TypeId,
};

impl FunctionLowerer<'_, '_> {
    pub(super) fn iteration_output(
        &self,
        protocol: StandardTrait,
        receiver: &TypeId,
        name: &str,
    ) -> Result<TypeId, MirLoweringError> {
        let interface = protocol.nominal();
        let member = associated_type_id(&interface.declaration, name);
        if let Some(output) = traits::iteration_outputs(
            protocol,
            receiver,
            Some(self.planner.catalog),
            &Default::default(),
        )
        .and_then(|outputs| outputs.get(&member).cloned())
        {
            return Ok(output);
        }
        let output = self.planner.catalog.normalize_type(&TypeId::Projection {
            receiver: Box::new(receiver.clone()),
            interface: Box::new(interface),
            member,
            arguments: vec![],
        });
        if !output.is_concrete() {
            return Err(MirLoweringError::MissingBinding(
                "concrete iteration output",
            ));
        }
        Ok(output)
    }

    pub(super) fn iterator_item(&self, receiver: &TypeId) -> Result<TypeId, MirLoweringError> {
        self.iteration_output(StandardTrait::Iterator, receiver, "Item")
    }
}
