//! Retained dynamic method and representation-adapter metadata.
use crate::{
    execution_metadata::{
        applications::ApplicationId, groups::OperationGroupId, interfaces::InterfaceSnapshotId,
        links::MetadataCache,
    },
    frame::types::{TypeEnvironment, bindings::TypeBindings, compatibility::TypeView},
    module::LoadedModule,
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{GenericParam, NominalTy, Ty};

#[derive(Debug, Clone)]
pub(crate) struct InterfaceMethodBinding {
    pub(crate) application: MetadataCache<ApplicationId>,
    pub(crate) receiver_operations: MetadataCache<Option<OperationGroupId>>,
    pub(crate) parameters: Vec<GenericParam<DefinitionId>>,
    pub(crate) entry_parameters: Vec<GenericParam<DefinitionId>>,
    pub(crate) entry_arguments: Vec<Ty<DefinitionId>>,
    pub(crate) result_adapter: Option<InterfaceResultBinding>,
    pub(crate) method: DefinitionId,
    pub(crate) target: CallableTarget,
    pub(crate) parameter_types: Vec<Ty<DefinitionId>>,
    pub(crate) return_type: Ty<DefinitionId>,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceResultBinding {
    pub(crate) owner: LoadedModule,
    pub(crate) table: usize,
    pub(crate) arguments: Vec<Ty<DefinitionId>>,
    pub(crate) environment: Option<TypeEnvironment>,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceParentBinding {
    pub(crate) prepared: MetadataCache<InterfaceSnapshotId>,
    pub(crate) interface: NominalTy<DefinitionId>,
    pub(crate) binding: InterfaceResultBinding,
    pub(crate) view: bool,
}

#[derive(Debug)]
pub(crate) struct InterfaceValueSnapshot {
    pub(crate) receiver_operations: MetadataCache<OperationGroupId>,
    pub(crate) receiver_table: InterfaceResultBinding,
    pub(crate) parents: Vec<InterfaceParentBinding>,
    pub(crate) data: Value,
    pub(crate) concrete_type: Ty<DefinitionId>,
    pub(crate) concrete_expression: Ty<DefinitionId>,
    pub(crate) interface_type: NominalTy<DefinitionId>,
    pub(crate) interface_expression: NominalTy<DefinitionId>,
    pub(crate) environment: Option<TypeEnvironment>,
    pub(crate) implementation: LoadedModule,
    pub(crate) methods: Vec<Option<InterfaceMethodBinding>>,
}

impl InterfaceValueSnapshot {
    pub(crate) fn matches_type(
        &self,
        ty: &Ty<DefinitionId>,
        owner: &LoadedModule,
        environment: Option<&TypeBindings>,
    ) -> bool {
        TypeView::new(
            &Ty::Trait(self.interface_expression.clone()),
            &self.implementation,
            self.environment
                .as_ref()
                .map(|environment| environment.types.as_ref()),
        )
        .compatible(TypeView::new(ty, owner, environment))
    }
}
