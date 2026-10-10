//! Checked Rust conversion views prepared once for a concrete executable import.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{TypeEnvironment, arguments::ScopedSignature},
    module::LoadedModule,
    native::{
        catalog::DeclarationCatalog,
        context::{CallContext, LinkedCallable, LinkedOperation},
        declarations::SelectedCall,
        result::LinkedResultAdapter,
    },
    value::Value,
};
use kagari_common::identity::{DefinitionPath, table::DefinitionId};
use kagari_types::{
    callable::Signature,
    collection::CollectionAccess,
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::Ty,
};
use std::{
    fmt, slice,
    sync::{Arc, OnceLock},
};

pub type NativeResult<T> = Result<T, RuntimeError>;

pub type NativeEntry =
    dyn for<'call> Fn(&mut CallContext<'call>) -> NativeResult<Value> + Send + Sync;

/// Scalars have an exact semantic type; generic views inherit their declared slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Codec {
    Scalar(Ty),
    Value,
    Object(DefinitionPath),
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

    pub(crate) fn accepts(&self, ty: &Ty, catalog: &DeclarationCatalog) -> bool {
        let layout = match ty {
            Ty::NativeObject(nominal) => {
                catalog
                    .types
                    .get(&nominal.declaration)
                    .and_then(|ty| match ty.kind {
                        TypeDefKind::NativeStorage(layout) => Some(layout),
                        _ => None,
                    })
            }
            _ => None,
        };
        match self {
            Self::Scalar(expected) => expected == ty,
            Self::Value => true,
            Self::Object(id) => {
                matches!(ty, Ty::NativeObject(nominal) if nominal.declaration == *id)
            }
            Self::Sequence => {
                matches!(ty, Ty::Array(_, _))
                    || matches!(layout, Some(NativeStorageLayout::Sequence { .. }))
            }
            Self::MutableSequence => {
                matches!(ty, Ty::Array(_, CollectionAccess::Mutable))
                    || matches!(layout, Some(NativeStorageLayout::Sequence { .. }))
            }
            Self::Map => {
                matches!(ty, Ty::Map { .. })
                    || matches!(layout, Some(NativeStorageLayout::Map { .. }))
            }
            Self::Set => {
                matches!(ty, Ty::Set(_, _))
                    || matches!(layout, Some(NativeStorageLayout::Set { .. }))
            }
            Self::Callable => matches!(ty, Ty::Function { .. }),
            Self::Iterator => {
                matches!(ty, Ty::Iter(_))
                    || matches!(layout, Some(NativeStorageLayout::Iterator { .. }))
            }
        }
    }
}

/// Shared native registration. Captured services must support shared use across
/// independent runtimes; mutable Send-only state belongs in a runtime payload.
///
/// ```compile_fail
/// use std::rc::Rc;
/// use kagari_runtime::{native::binding::{Codec, NativeBinding}, value::Value};
/// let local = Rc::new(1);
/// let binding = NativeBinding::new(Vec::<Codec>::new(), Codec::Value, move |_| {
///     Ok(Value::I32(*local))
/// });
/// ```
#[derive(Clone)]
pub struct NativeBinding {
    pub(crate) arguments: Box<[Codec]>,
    pub(crate) result: Codec,
    pub(crate) entry: Arc<NativeEntry>,
    pub(crate) converted_result: bool,
    pub(crate) requirement_owner: Option<DefinitionPath>,
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
        entry: impl for<'call> Fn(&mut CallContext<'call>) -> NativeResult<Value>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            arguments: arguments.into(),
            result,
            entry: Arc::new(entry),
            converted_result: false,
            requirement_owner: None,
        }
    }

    pub(crate) fn check(
        &self,
        signature: &Signature,
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
    pub(crate) declaration: DefinitionId,
    pub(crate) binding: NativeBinding,
    pub(crate) signature: Signature<DefinitionId>,
    pub(crate) scoped_signature: Option<Arc<ScopedSignature>>,
    pub(crate) prepared_signature: OnceLock<NativeResult<ScopedSignature>>,
    pub(crate) selected: Box<[LinkedOperation]>,
    pub(crate) result_adapter: Option<LinkedResultAdapter>,
}

impl LinkedNativeFunction {
    /// Runtime-local bindings own this preparation; immutable type provenance
    /// does not create an executable retention lease or a program ownership cycle.
    pub(crate) fn type_signature<'a>(
        &'a self,
        runtime: &Runtime,
        owner: &LoadedModule,
    ) -> NativeResult<&'a ScopedSignature> {
        if let Some(signature) = &self.scoped_signature {
            return Ok(signature);
        }
        self.prepared_signature
            .get_or_init(|| {
                let params = runtime.type_arguments(owner, None, &self.signature.params)?;
                let result = runtime
                    .type_arguments(owner, None, slice::from_ref(&self.signature.result))?
                    .pop()
                    .ok_or_else(|| RuntimeError::module_validation("native result type scope"))?;
                Ok(ScopedSignature { params, result })
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub(crate) fn check_requirement(
        &self,
        owner: &LoadedModule,
        key: &SelectedCall,
    ) -> NativeResult<()> {
        let declaration = match &self.binding.requirement_owner {
            Some(declaration) => declaration.clone(),
            None => owner.definition(self.declaration)?.to_path(),
        };
        if declaration != key.declaration {
            return Err(RuntimeError::module_validation(
                "foreign native callable requirement",
            ));
        }
        Ok(())
    }

    pub(crate) fn apply(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        environment: TypeEnvironment,
    ) -> NativeResult<Self> {
        let params = runtime.type_arguments(
            owner,
            Some(environment.types.clone()),
            &self.signature.params,
        )?;
        let result = runtime
            .type_arguments(
                owner,
                Some(environment.types.clone()),
                slice::from_ref(&self.signature.result),
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("native return type"))?;
        let signature = Signature {
            params: params
                .iter()
                .map(|argument| argument.ty().clone())
                .collect(),
            result: result.ty().clone(),
        };
        let scoped_signature = Some(Arc::new(ScopedSignature { params, result }));
        Ok(Self {
            declaration: self.declaration,
            binding: self.binding.clone(),
            signature,
            scoped_signature,
            prepared_signature: OnceLock::new(),
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
                        let resolved = environment.types.resolve_requirement(required)?;
                        let operation =
                            environment
                                .operation(&runtime.gc, &resolved)
                                .ok_or_else(|| {
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
            && !match self.scoped_signature.as_deref().or_else(|| {
                self.prepared_signature
                    .get()
                    .and_then(|signature| signature.as_ref().ok())
            }) {
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
