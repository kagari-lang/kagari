//! Checked Rust conversion views prepared once for a concrete executable import.
use crate::{
    error::RuntimeError,
    native::{
        catalog::DeclarationCatalog,
        context::{CallContext, LinkedCallable},
    },
    value::Value,
};
use kagari_abi::{
    native_import::NativeSignature,
    types::{AbiType, TypeAbiKind, native::NativeStorageLayout},
};
use kagari_common::{collection::CollectionAccess, identity::DefinitionId};
use std::{fmt, rc::Rc};

pub type NativeResult<T> = Result<T, RuntimeError>;
pub type NativeEntry = dyn for<'call> Fn(&mut CallContext<'call>) -> NativeResult<Value>;

/// Scalars have an exact semantic type; generic views inherit their declared slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Codec {
    Scalar(AbiType),
    Value,
    Object(DefinitionId),
    Sequence,
    MutableSequence,
    Map,
    Set,
    Iterator,
    Callable,
}
impl Codec {
    /// A group selects the sequence storage family. Individual methods select
    /// read or write access; check() still checks that access against the actual
    /// declared receiver, so a readonly array cannot acquire a mutable converter.
    pub(crate) fn receiver_shape_matches(&self, method: &Self) -> bool {
        self == method
            || matches!(
                (self, method),
                (
                    Self::Sequence | Self::MutableSequence,
                    Self::Sequence | Self::MutableSequence
                )
            )
    }
    pub(crate) fn accepts(&self, ty: &AbiType, catalog: &DeclarationCatalog) -> bool {
        let layout = match ty {
            AbiType::NativeObject(nominal) => {
                catalog
                    .types
                    .get(&nominal.declaration)
                    .and_then(|ty| match ty.kind {
                        TypeAbiKind::NativeStorage(layout) => Some(layout),
                        _ => None,
                    })
            }
            _ => None,
        };
        match self {
            Self::Scalar(expected) => expected == ty,
            Self::Value => true,
            Self::Object(id) => {
                matches!(ty, AbiType::NativeObject(nominal) if nominal.declaration == *id)
            }
            Self::Sequence => {
                matches!(ty, AbiType::Array(_, _))
                    || matches!(layout, Some(NativeStorageLayout::Sequence { .. }))
            }
            Self::MutableSequence => {
                matches!(ty, AbiType::Array(_, CollectionAccess::Mutable))
                    || matches!(layout, Some(NativeStorageLayout::Sequence { .. }))
            }
            Self::Map => {
                matches!(ty, AbiType::Map { .. })
                    || matches!(layout, Some(NativeStorageLayout::Map { .. }))
            }
            Self::Set => {
                matches!(ty, AbiType::Set(_, _))
                    || matches!(layout, Some(NativeStorageLayout::Set { .. }))
            }
            Self::Callable => matches!(ty, AbiType::Function { .. }),
            Self::Iterator => {
                matches!(ty, AbiType::Iter(_))
                    || matches!(layout, Some(NativeStorageLayout::Iterator { .. }))
            }
        }
    }
}

#[derive(Clone)]
pub struct NativeBinding {
    pub(crate) arguments: Box<[Codec]>,
    pub(crate) result: Codec,
    pub(crate) entry: Rc<NativeEntry>,
    pub(crate) converted_result: bool,
}
impl fmt::Debug for NativeBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeBinding")
            .field("arguments", &self.arguments)
            .field("result", &self.result)
            .finish_non_exhaustive()
    }
}
impl NativeBinding {
    /// Explicit low-level conversion contract. The body remains trusted Rust;
    /// dynamic values are checked at the return boundary.
    pub fn new(
        arguments: impl Into<Box<[Codec]>>,
        result: Codec,
        entry: impl for<'call> Fn(&mut CallContext<'call>) -> NativeResult<Value> + 'static,
    ) -> Self {
        Self {
            arguments: arguments.into(),
            result,
            entry: Rc::new(entry),
            converted_result: false,
        }
    }
    pub(crate) fn check(
        &self,
        signature: &NativeSignature,
        catalog: &DeclarationCatalog,
    ) -> NativeResult<()> {
        if self.arguments.len() != signature.params.len()
            || self
                .arguments
                .iter()
                .zip(&signature.params)
                .any(|(codec, ty)| !codec.accepts(ty, catalog))
            || !self.result.accepts(&signature.result, catalog)
        {
            return Err(RuntimeError::metadata_conflict(
                "native codec does not match the Kagari declaration",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct LinkedNativeFunction {
    pub(crate) binding: NativeBinding,
    pub(crate) signature: NativeSignature,
    pub(crate) selected: Box<[LinkedCallable]>,
}
impl LinkedNativeFunction {
    pub(crate) fn invoke(&self, context: &mut CallContext<'_>) -> NativeResult<Value> {
        if context.arguments.len() != self.signature.params.len() {
            return Err(RuntimeError::module_validation(
                "native invocation argument count",
            ));
        }
        let value = (self.binding.entry)(context)?;
        if !self.binding.converted_result
            && !context.runtime.matches_interface_method_abi(
                &value,
                &self.signature.result,
                context.owner,
            )
        {
            return Err(RuntimeError::module_validation(
                "native result differs from its Kagari declaration",
            ));
        }
        Ok(value)
    }
}
