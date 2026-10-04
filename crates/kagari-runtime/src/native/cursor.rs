//! A traced foundation cursor for native iterator composition. Its position,
//! generation and source guards remain owned by the checked heap cursor.
use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, context::CallContext, storage::NativePayload},
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{operations::IterOp, types::Ty};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct NativeCursor(Rc<Cursor>);

#[derive(Debug)]
struct Cursor {
    value: Value,
    ty: Ty<DefinitionId>,
}

impl NativePayload for NativeCursor {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        visit(&self.0.value);
    }

    fn units(&self) -> usize {
        1
    }
}

impl NativeCursor {
    pub fn next(&self, cx: &CallContext<'_>) -> NativeResult<Option<Value>> {
        cx.heap().next_iter_item(&self.0.value, &self.0.ty)
    }

    pub fn close(&self, cx: &CallContext<'_>) -> NativeResult<()> {
        cx.heap()
            .advance_iter(&self.0.value, &self.0.ty, IterOp::Close)
            .map(|_| ())
    }
}

impl CallContext<'_> {
    /// Create a shared cursor over a declared Vec argument. Store it in a
    /// native payload and visit it in iteration_sources for for-scope cleanup.
    pub fn sequence_cursor(&self, index: usize) -> NativeResult<NativeCursor> {
        let ty = self.argument_type(index)?;
        let Ty::Array(item, _) = ty else {
            return Err(RuntimeError::module_validation(
                "sequence cursor requires Vec",
            ));
        };
        let value = self.iter_operation(index, IterOp::New)?;
        Ok(NativeCursor(Rc::new(Cursor {
            value,
            ty: Ty::Iter(item.clone()),
        })))
    }
}
