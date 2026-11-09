//! Shared interface conversion operands; installation keys are linked separately.
use crate::module::execution::layout::Location;
use kagari_types::ty::Ty;

#[derive(Debug)]
pub(crate) struct PreparedUpcast {
    pub(crate) dst: Location,
    pub(crate) value: Location,
    pub(crate) source: Ty,
    pub(crate) target: Ty,
}

impl PreparedUpcast {
    pub(crate) fn is_closed(&self) -> bool {
        self.source.is_concrete() && self.target.is_concrete()
    }
}
