//! Root the evaluated source tuple, then reuse the generation-pinned lazy constructor.
use crate::{LoadedModule, Runtime, RuntimeError, gc::RootSet, native::NativeAction, value::Value};
use kagari_abi::{
    operations::{IterOp, StringIterKind},
    standard::StandardIntrinsic,
};

pub(super) const SCRATCH_ROOTS: usize = 1;

pub(super) struct StringIterator {
    kind: StringIterKind,
    source: usize,
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native string iterator contract mismatch")
}

impl StringIterator {
    pub(super) fn start(
        operation: StandardIntrinsic,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let kind = match operation {
            StandardIntrinsic::StringBytes => StringIterKind::Bytes,
            StandardIntrinsic::StringCharIndices => StringIterKind::CharIndices,
            StandardIntrinsic::StringSplit => StringIterKind::Split,
            StandardIntrinsic::StringSplitN => StringIterKind::SplitN,
            StandardIntrinsic::StringSplitWhitespace => StringIterKind::Whitespace,
            StandardIntrinsic::StringLines => StringIterKind::Lines,
            _ => return Err(invalid()),
        };
        Ok(Self {
            kind,
            source: arguments.len(),
        })
    }

    /// Tuple creation is the original charged entry, without stepping the iterator.
    pub(super) fn initialize(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        let values = (0..self.source)
            .map(|slot| roots.get(slot).ok_or_else(invalid))
            .collect::<Result<_, _>>()?;
        roots
            .set(runtime.gc(), self.source, Value::Tuple(values))
            .ok_or_else(invalid)?;
        Ok(NativeAction::Continue)
    }

    pub(super) fn advance(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        runtime
            .iter_operation(
                owner,
                &roots.get(self.source).ok_or_else(invalid)?,
                &self.kind.source_type(),
                IterOp::String(self.kind),
            )
            .map(NativeAction::Complete)
    }
}
