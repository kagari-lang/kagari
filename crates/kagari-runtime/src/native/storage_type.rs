//! Element contracts retain one checked reified type, independently of collection length.
use crate::{
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    gc::GcHeap,
    module::LoadedModule,
    value::Value,
    value_check::matches_view,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::Ty;

#[derive(Debug)]
pub(crate) struct StorageType {
    pub(crate) ty: Ty<DefinitionId>,
    pub(crate) owner: LoadedModule,
    scope: Option<TypeArgument>,
}

impl StorageType {
    pub(crate) fn prepare(
        ty: Ty<DefinitionId>,
        owner: &LoadedModule,
    ) -> Result<Self, RuntimeError> {
        Self::checked(Self {
            ty,
            owner: owner.clone(),
            scope: None,
        })
    }

    pub(crate) fn prepare_scoped(
        argument: TypeArgument,
        owner: &LoadedModule,
    ) -> Result<Self, RuntimeError> {
        Self::checked(Self {
            ty: argument.ty().clone(),
            owner: owner.clone(),
            scope: argument.has_origin().then_some(argument),
        })
    }

    fn checked(contract: Self) -> Result<Self, RuntimeError> {
        // Establish storage evidence before exposing the prepared view.
        let view = match &contract.scope {
            Some(scope) => scope.view(&contract.owner),
            None => TypeView::new(&contract.ty, &contract.owner, None),
        };
        if !view.is_heap_type() {
            return Err(RuntimeError::module_validation(
                "storage element is not an available closed script-heap type",
            ));
        }
        Ok(contract)
    }

    fn view(&self) -> TypeView<'_> {
        match &self.scope {
            Some(scope) => scope.view(&self.owner),
            None => TypeView::prepared(&self.ty, &self.ty, &self.owner, None),
        }
    }

    pub(crate) fn matches_view(&self, expected: TypeView<'_>) -> bool {
        self.view().compatible(expected)
    }

    pub(crate) fn same_type(&self, other: &Self) -> bool {
        self.view().compatible(other.view())
    }

    pub(crate) fn accepts_value(&self, heap: &GcHeap, value: &Value) -> bool {
        match &self.scope {
            Some(scope) => scope.matches_heap(heap, value, &self.owner),
            None => matches_view(heap, value, self.view()),
        }
    }
}
