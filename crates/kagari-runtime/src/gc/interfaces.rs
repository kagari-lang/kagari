//! Retained dynamic method and representation-adapter metadata.
use crate::{
    execution_metadata::{
        application_key::MethodIdentity, groups::OperationGroupId, interfaces::InterfaceSnapshotId,
        links::MetadataCache,
    },
    frame::types::{TypeEnvironment, bindings::TypeBindings, compatibility::TypeView},
    module::LoadedModule,
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{GenericParam, NominalTy, Ty};
use std::sync::{Arc, OnceLock};

#[derive(Debug)]
pub(crate) struct InterfaceMethodBinding {
    pub(crate) identity: OnceLock<MethodIdentity>,
    pub(crate) receiver_operations: MetadataCache<Option<OperationGroupId>>,
    pub(crate) parameters: Vec<GenericParam>,
    pub(crate) entry_parameters: Vec<GenericParam>,
    pub(crate) entry_arguments: Vec<Ty>,
    pub(crate) result_adapter: Option<InterfaceResultBinding>,
    pub(crate) method: DefinitionId,
    pub(crate) target: CallableTarget,
    pub(crate) parameter_types: Vec<Ty>,
    pub(crate) return_type: Ty,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceResultBinding {
    pub(crate) owner: LoadedModule,
    pub(crate) table: usize,
    pub(crate) arguments: Vec<Ty>,
    pub(crate) environment: Option<TypeEnvironment>,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceParentBinding {
    pub(crate) interface: NominalTy,
    pub(crate) binding: InterfaceResultBinding,
    pub(crate) view: bool,
}

#[derive(Debug)]
pub(crate) struct InterfaceDescription {
    pub(crate) receiver_operations: MetadataCache<OperationGroupId>,
    pub(crate) receiver_table: InterfaceResultBinding,
    pub(crate) parents: Vec<InterfaceParentBinding>,
    pub(crate) concrete_type: Ty,
    pub(crate) concrete_expression: Ty,
    pub(crate) interface_type: Ty,
    pub(crate) interface_expression: Ty,
    pub(crate) environment: Option<TypeEnvironment>,
    pub(crate) implementation: LoadedModule,
    pub(crate) methods: Vec<Option<InterfaceMethodBinding>>,
}

impl InterfaceDescription {
    pub(crate) fn interface_type(&self) -> &NominalTy {
        let Ty::Trait(interface) = &self.interface_type else {
            unreachable!("prepared interface type");
        };
        interface
    }

    /// Reuse the checked closed type while preserving its lexical provenance.
    pub(crate) fn interface_view(&self) -> TypeView<'_> {
        TypeView::prepared(
            &self.interface_expression,
            &self.interface_type,
            &self.implementation,
            self.environment
                .as_ref()
                .map(|environment| environment.types.as_ref()),
        )
    }
}

/// Receiver data and parent views are never shared with another receiver.
#[derive(Debug)]
pub(crate) struct InterfaceValueSnapshot {
    pub(crate) description: Arc<InterfaceDescription>,
    pub(crate) data: Value,
    pub(crate) parent_views: Box<[MetadataCache<InterfaceSnapshotId>]>,
}

impl InterfaceValueSnapshot {
    pub(crate) fn new(description: Arc<InterfaceDescription>, data: Value) -> Self {
        let parent_views = description
            .parents
            .iter()
            .map(|_| MetadataCache::new())
            .collect();
        Self {
            description,
            data,
            parent_views,
        }
    }

    pub(crate) fn matches_type(
        &self,
        ty: &Ty,
        owner: &LoadedModule,
        environment: Option<&TypeBindings>,
    ) -> bool {
        self.description
            .interface_view()
            .compatible(TypeView::new(ty, owner, environment))
    }
}
