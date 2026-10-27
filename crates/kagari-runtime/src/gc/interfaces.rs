//! Retained dynamic method and representation-adapter metadata.
use crate::{
    execution_metadata::{
        MetadataEdge, application_key::MethodIdentity, groups::OperationGroupId,
        interfaces::InterfaceSnapshotId, links::MetadataCache,
        scan_records::InterfaceDescriptionScanId,
    },
    frame::types::{TypeEnvironment, bindings::TypeBindings, compatibility::TypeView},
    module::ExecutableModule,
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
    pub(crate) owner: ExecutableModule,
    pub(crate) table: usize,
    pub(crate) arguments: Vec<Ty>,
    pub(crate) environment: Option<TypeEnvironment>,
}

impl InterfaceResultBinding {
    pub(crate) fn scan_field(&self, field: usize) -> Option<MetadataEdge<'_>> {
        match field {
            0 => Some(MetadataEdge::Program(&self.owner)),
            1 => self
                .environment
                .as_ref()
                .map(|environment| MetadataEdge::Environment(environment.id)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceParentBinding {
    pub(crate) interface: NominalTy,
    pub(crate) binding: InterfaceResultBinding,
    pub(crate) view: bool,
}

#[derive(Debug)]
pub(crate) struct InterfaceDescription {
    pub(crate) scan_id: InterfaceDescriptionScanId,
    pub(crate) receiver_operations: MetadataCache<OperationGroupId>,
    pub(crate) receiver_table: InterfaceResultBinding,
    pub(crate) parents: Vec<InterfaceParentBinding>,
    pub(crate) concrete_type: Ty,
    pub(crate) concrete_expression: Ty,
    pub(crate) interface_type: Ty,
    pub(crate) interface_expression: Ty,
    pub(crate) environment: Option<TypeEnvironment>,
    pub(crate) implementation: ExecutableModule,
    pub(crate) methods: Vec<Option<InterfaceMethodBinding>>,
}

impl InterfaceDescription {
    pub(crate) fn scan_len(&self) -> usize {
        5 + self.parents.len() * 2 + self.methods.len() * 3
    }

    pub(crate) fn scan_field(&self, index: usize) -> Option<MetadataEdge<'_>> {
        match index {
            0 => return Some(MetadataEdge::Program(&self.implementation)),
            1 | 2 => return self.receiver_table.scan_field(index - 1),
            3 => {
                return self
                    .environment
                    .as_ref()
                    .map(|environment| MetadataEdge::Environment(environment.id));
            }
            4 => {
                return self
                    .receiver_operations
                    .get()
                    .copied()
                    .map(MetadataEdge::Group);
            }
            _ => {}
        }
        let index = index - 5;
        if index < self.parents.len() * 2 {
            return self.parents[index / 2].binding.scan_field(index % 2);
        }
        let index = index - self.parents.len() * 2;
        let method = self.methods.get(index / 3)?.as_ref()?;
        match index % 3 {
            0 => method
                .receiver_operations
                .get()
                .copied()
                .flatten()
                .map(MetadataEdge::Group),
            field => method.result_adapter.as_ref()?.scan_field(field - 1),
        }
    }

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
        owner: &ExecutableModule,
        environment: Option<&TypeBindings>,
    ) -> bool {
        self.description
            .interface_view()
            .compatible(TypeView::new(ty, owner, environment))
    }
}
