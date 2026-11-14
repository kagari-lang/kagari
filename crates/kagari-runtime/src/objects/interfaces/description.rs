//! Immutable checked table facts contain no receiver or managed-state ownership.
use crate::{
    error::RuntimeError,
    execution_metadata::{MetadataEdge, application_key::MethodIdentity},
    frame::types::{TypeEnvironment, compatibility::TypeView},
    module::ExecutableModule,
    resource::ResourceState,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{GenericParam, NominalTy, Ty};
use std::sync::OnceLock;

#[derive(Debug)]
pub(crate) struct InterfaceMethodBinding {
    pub(crate) identity: OnceLock<MethodIdentity>,
    pub(crate) requires_receiver_operations: bool,
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

#[derive(Debug, Clone)]
pub(crate) struct InterfaceParentBinding {
    pub(crate) interface: NominalTy,
    pub(crate) binding: InterfaceResultBinding,
    pub(crate) view: bool,
}

#[derive(Debug)]
pub(crate) struct InterfaceDescription {
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
    /// Enumerate fixed executable relationships. Receiver dictionaries belong to
    /// actual interface providers; shared descriptions never retain managed state.
    pub(crate) fn trace<'a, E, F>(
        &'a self,
        resources: &ResourceState,
        visit: &mut F,
    ) -> Result<(), E>
    where
        E: From<RuntimeError>,
        F: FnMut(MetadataEdge<'a>) -> Result<(), E> + ?Sized,
    {
        visit(MetadataEdge::Program(&self.implementation))?;
        visit(MetadataEdge::Binding(&self.receiver_table))?;
        if let Some(environment) = &self.environment {
            visit(MetadataEdge::EnvironmentFacts(environment))?;
        }
        for (index, parent) in self.parents.iter().enumerate() {
            if index.is_multiple_of(1024) {
                resources.poll_closed()?;
            }
            visit(MetadataEdge::Binding(&parent.binding))?;
        }
        for (index, method) in self.methods.iter().enumerate() {
            if index.is_multiple_of(1024) {
                resources.poll_closed()?;
            }
            if let Some(adapter) = method
                .as_ref()
                .and_then(|method| method.result_adapter.as_ref())
            {
                visit(MetadataEdge::Binding(adapter))?;
            }
        }
        Ok(())
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
