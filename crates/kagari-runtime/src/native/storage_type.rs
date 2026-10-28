//! Element contracts retain one checked reified type, independently of collection length.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    gc::{GcHeap, publication::CheckedTarget},
    module::ExecutableModule,
    value::Value,
    value_check::matches_view,
};
use kagari_types::ty::Ty;

/// An immutable checked storage contract with its original lexical type scope.
/// Cloning a scoped contract shares metadata without allocating or retaining an
/// executable lease or GC root. Internal unscoped contracts clone their owned type.
#[derive(Debug, Clone)]
pub struct StorageType {
    pub(crate) owner: ExecutableModule,
    evidence: Evidence,
}

// Scoped contracts reuse the already shared TypeArgument rather than embedding
// a second, large Ty in every payload. Unscoped internal contracts own their type.
#[derive(Debug, Clone)]
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

    /// Shared admission for storage publication, retention and immediate access.
    /// Recovery policy belongs to the enclosing loan, not the immutable contract.
    pub(in crate::native) fn checked_target<'runtime>(
        &self,
        runtime: &'runtime Runtime,
        value: &Value,
    ) -> Result<CheckedTarget<'runtime>, RuntimeError> {
        runtime.resources().ensure_execution_allowed()?;
        self.argument()?.validate(runtime)?;
        let heap = runtime.gc();
        let invalid = || {
            RuntimeError::module_validation("native storage value differs from its closed contract")
        };
        if !value.is_default_heap_payload(heap) {
            return Err(invalid());
        }
        let target = heap.checked_target(*value).ok_or_else(invalid)?;
        // Nominal/layout admission stays inside the checked directory borrow.
        if !self.accepts_value(heap, value) {
            return Err(invalid());
        }
        Ok(target)
    }

    pub(crate) fn accepts_value(&self, heap: &GcHeap, value: &Value) -> bool {
        match &self.evidence {
            Evidence::Scoped(scope) => scope.matches_heap(heap, value, &self.owner),
            Evidence::Unscoped(_) => matches_view(heap, value, self.view()),
        }
    }
}
