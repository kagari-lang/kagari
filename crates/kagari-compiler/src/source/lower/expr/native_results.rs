//! Carry the checked interface table used to construct a native List result.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    native_import::{NativeWitness, NativeWitnessImplementation},
    standard::traits::StandardTrait,
    types::ConcreteFunctionIdentity,
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionKind, DefinitionPathSegment},
};
use kagari_hir::types::{
    TypeId,
    abi::{lower_nominal_type, lower_type},
};

impl FunctionLowerer<'_, '_> {
    pub(super) fn native_list_result(
        &mut self,
        result: &TypeId,
    ) -> Result<NativeWitness, MirLoweringError> {
        let invalid =
            || MirLoweringError::MissingBinding("checked native List result representation");
        let TypeId::Trait(interface) = result else {
            return Err(invalid());
        };
        if StandardTrait::from_id(&interface.declaration) != Some(StandardTrait::List) {
            return Err(invalid());
        }
        let [item] = interface.arguments.as_slice() else {
            return Err(invalid());
        };
        let storage = TypeId::Array(Box::new(item.clone()), CollectionAccess::Mutable);
        let span = self.function.debug.source_span;
        self.planner
            .require_parent_interfaces(&storage, interface, span)?;
        let declaration = self.planner.native_interface(&storage, interface, span)?;
        let owner = self
            .planner
            .catalog
            .trait_(&interface.declaration)
            .ok_or_else(invalid)?;
        let methods = owner
            .methods
            .iter()
            .filter(|method| method.default.is_none())
            .map(|method| {
                let mut target = declaration.clone();
                target.path.push(DefinitionPathSegment {
                    kind: DefinitionKind::Method,
                    name: method.name.clone(),
                    occurrence: 0,
                });
                ConcreteFunctionIdentity {
                    declaration: target,
                    arguments: vec![],
                }
            })
            .collect();
        Ok(NativeWitness {
            receiver: lower_type(&storage),
            interface: lower_nominal_type(interface),
            implementation: NativeWitnessImplementation::Table(ConcreteFunctionIdentity {
                declaration,
                arguments: vec![],
            }),
            methods,
        })
    }
}
