//! Checked Rust conversion views prepared once for a concrete executable import.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{
        TypeEnvironment,
        arguments::{ScopedSignature, TypeArgument},
    },
    module::LoadedModule,
    native::{
        catalog::DeclarationCatalog,
        context::{CallContext, LinkedCallable, LinkedOperation},
        result::LinkedResultAdapter,
    },
    value::Value,
};
use kagari_abi::{
    native_import::NativeSignature,
    types::{AbiType, TypeAbiKind, native::NativeStorageLayout},
};
use kagari_common::{collection::CollectionAccess, identity::DefinitionId};
use std::{fmt, rc::Rc, slice};

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
    pub(crate) scoped_signature: Option<Rc<ScopedSignature>>,
    pub(crate) selected: Box<[LinkedOperation]>,
    pub(crate) result_adapter: Option<LinkedResultAdapter>,
}
impl LinkedNativeFunction {
    pub(crate) fn apply(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        environment: Rc<TypeEnvironment>,
    ) -> NativeResult<Self> {
        let params =
            runtime.type_arguments(owner, Some(environment.clone()), &self.signature.params)?;
        let result = runtime
            .type_arguments(
                owner,
                Some(environment.clone()),
                slice::from_ref(&self.signature.result),
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("native return type"))?;
        let signature = NativeSignature {
            params: params
                .iter()
                .map(|argument| argument.ty().clone())
                .collect(),
            result: result.ty().clone(),
        };
        let scoped_signature = (result.has_origin() || params.iter().any(TypeArgument::has_origin))
            .then(|| Rc::new(ScopedSignature { params, result }));
        Ok(Self {
            binding: self.binding.clone(),
            signature,
            scoped_signature,
            result_adapter: self
                .result_adapter
                .as_ref()
                .map(|adapter| adapter.apply(runtime, owner, environment.clone()))
                .transpose()?,
            selected: self
                .selected
                .iter()
                .map(|operation| match operation {
                    LinkedOperation::Ready(callable) => {
                        Ok(LinkedOperation::Ready(callable.clone()))
                    }
                    LinkedOperation::Forward(required) => {
                        let resolved = environment.resolve_requirement(required)?;
                        let operation = environment.operation(&resolved).ok_or_else(|| {
                            RuntimeError::module_validation(
                                "native constraint operation environment",
                            )
                        })?;
                        Ok(LinkedOperation::Ready(LinkedCallable::prepare(
                            runtime,
                            owner,
                            &environment,
                            required,
                            operation,
                        )?))
                    }
                })
                .collect::<NativeResult<Vec<_>>>()?
                .into_boxed_slice(),
        })
    }
    pub(crate) fn invoke(&self, context: &mut CallContext<'_>) -> NativeResult<Value> {
        if context.arguments.len() != self.signature.params.len() {
            return Err(RuntimeError::module_validation(
                "native invocation argument count",
            ));
        }
        context.poll()?;
        let result = (self.binding.entry)(context);
        context.poll()?;
        let value = result?;
        if (!self.binding.converted_result || self.result_adapter.is_some())
            && !match &self.scoped_signature {
                Some(signature) => signature
                    .result
                    .matches(context.runtime, &value, context.owner),
                None => context.runtime.matches_interface_method_abi(
                    &value,
                    &self.signature.result,
                    context.owner,
                ),
            }
        {
            return Err(RuntimeError::module_validation(
                "native result differs from its Kagari declaration",
            ));
        }
        match &self.result_adapter {
            Some(adapter) => adapter.convert(context.runtime, context.owner, value),
            None => Ok(value),
        }
    }
}
