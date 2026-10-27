//! Element contracts retain one checked reified type, independently of collection length.
use crate::{
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    gc::GcHeap,
    module::ExecutableModule,
    value::Value,
    value_check::matches_view,
};
use kagari_types::ty::Ty;

#[derive(Debug)]
pub struct StorageType {
    pub(crate) owner: ExecutableModule,
    evidence: Evidence,
}

// Scoped contracts reuse the already shared TypeArgument rather than embedding
// a second, large Ty in every payload. Unscoped internal contracts own their type.
#[derive(Debug)]
enum Evidence {
    Scoped(TypeArgument),
    Unscoped(Box<Ty>),
}

impl StorageType {
    pub(crate) fn prepare(ty: Ty, owner: &ExecutableModule) -> Result<Self, RuntimeError> {
        Self::checked(Self {
            owner: owner.clone(),
            evidence: Evidence::Unscoped(Box::new(ty)),
        })
    }

    pub fn prepare_scoped(
        argument: TypeArgument,
        owner: &ExecutableModule,
    ) -> Result<Self, RuntimeError> {
        Self::checked(Self {
            owner: owner.clone(),
            evidence: Evidence::Scoped(argument),
        })
    }

    fn checked(contract: Self) -> Result<Self, RuntimeError> {
        // Establish storage evidence before exposing the prepared view.
        if !contract.ty().is_concrete() {
            return Err(RuntimeError::module_validation(
                "storage element requires a closed type",
            ));
        }
        let view = match &contract.evidence {
            Evidence::Scoped(scope) => scope.view(&contract.owner),
            Evidence::Unscoped(ty) => TypeView::new(ty, &contract.owner, None),
        };
        if !view.is_heap_type() {
            return Err(RuntimeError::module_validation(
                "storage element is not an available closed script-heap type",
            ));
        }
        Ok(contract)
    }

    pub(crate) fn ty(&self) -> &Ty {
        match &self.evidence {
            Evidence::Scoped(scope) => scope.ty(),
            Evidence::Unscoped(ty) => ty,
        }
    }

    pub(crate) fn argument(&self) -> Result<&TypeArgument, RuntimeError> {
        match &self.evidence {
            Evidence::Scoped(scope) => Ok(scope),
            Evidence::Unscoped(_) => Err(RuntimeError::module_validation(
                "retention requires a scoped storage contract",
            )),
        }
    }

    fn view(&self) -> TypeView<'_> {
        match &self.evidence {
            Evidence::Scoped(scope) => scope.view(&self.owner),
            Evidence::Unscoped(ty) => TypeView::prepared(ty, ty, &self.owner, None),
        }
    }

    pub(crate) fn matches_view(&self, expected: TypeView<'_>) -> bool {
        self.view().compatible(expected)
    }

    pub(crate) fn same_type(&self, other: &Self) -> bool {
        self.view().compatible(other.view())
    }

    pub(crate) fn accepts_value(&self, heap: &GcHeap, value: &Value) -> bool {
        match &self.evidence {
            Evidence::Scoped(scope) => scope.matches_heap(heap, value, &self.owner),
            Evidence::Unscoped(_) => matches_view(heap, value, self.view()),
        }
    }
}
