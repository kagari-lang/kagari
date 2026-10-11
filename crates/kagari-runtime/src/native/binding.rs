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
        primitive::NativePrimitive,
        result::LinkedResultAdapter,
        types::{Type, same_definition},
    },
    value::Value,
};
use kagari_common::identity::{
    mapping::DefinitionRecord,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    callable::Signature,
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

/// Exact conversions retain scoped semantic types; generic views inherit their declared slot.
#[derive(Debug, Clone)]
pub enum Codec {
    Exact(Type),
    Value,
    Object {
        declaration: DefinitionId,
        definitions: DefinitionTable,
    },
    Sequence,
    MutableSequence,
    Map,
    Set,
    Iterator,
    Callable,
}

impl Codec {
    /// Both sequence codecs require a nominal storage declaration. Interface
    /// values and builtin arrays cannot acquire a concrete sequence converter.
    pub(crate) fn receiver_shape_matches(&self, method: &Self) -> bool {
        match (self, method) {
            (Self::Exact(left), Self::Exact(right)) => left.same_type(right),
            (
                Self::Object {
                    declaration: left,
                    definitions: left_scope,
                },
                Self::Object {
                    declaration: right,
                    definitions: right_scope,
                },
            ) => same_definition(*left, left_scope, *right, right_scope),
            (Self::Sequence | Self::MutableSequence, Self::Sequence | Self::MutableSequence)
            | (Self::Value, Self::Value)
            | (Self::Map, Self::Map)
            | (Self::Set, Self::Set)
            | (Self::Iterator, Self::Iterator)
            | (Self::Callable, Self::Callable) => true,
            _ => false,
        }
    }

    pub(crate) fn accepts(
        &self,
        ty: &Ty<DefinitionId>,
        definitions: &DefinitionTable,
        catalog: &DeclarationCatalog,
    ) -> bool {
        let layout = match ty {
            Ty::NativeObject(nominal) => {
                catalog
                    .types
                    .get_id(nominal.declaration)
                    .and_then(|ty| match ty.kind {
                        TypeDefKind::NativeStorage(layout) => Some(layout),
                        _ => None,
                    })
            }
            _ => None,
        };
        match self {
            Self::Exact(expected) => Type::from_semantic(ty.clone(), definitions)
                .is_ok_and(|actual| expected.same_type(&actual)),
            Self::Value => true,
            Self::Object {
                declaration,
                definitions: source,
            } => {
                matches!(ty, Ty::NativeObject(nominal) if same_definition(*declaration, source, nominal.declaration, definitions))
            }
            Self::Sequence | Self::MutableSequence => {
                matches!(ty, Ty::NativeObject(_))
                    && matches!(layout, Some(NativeStorageLayout::Sequence { .. }))
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
    pub(crate) entry: BindingEntry,
    pub(crate) converted_result: bool,
    pub(crate) requirement_owner: Option<(DefinitionId, DefinitionTable)>,
}

/// Primitive authority owns the implementation; a callback cannot claim its effects.
#[derive(Clone)]
pub(crate) enum BindingEntry {
    Callback(Arc<NativeEntry>),
    Primitive(NativePrimitive),
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
            entry: BindingEntry::Callback(Arc::new(entry)),
            converted_result: false,
            requirement_owner: None,
        }
    }

    pub(crate) fn check(
        &self,
        signature: &Signature<DefinitionId>,
        definitions: &DefinitionTable,
        catalog: &DeclarationCatalog,
    ) -> NativeResult<()> {
        signature
            .visit_definitions(
                &mut |id| Ok(definitions.resolve(*id).map(|_| ())?),
                &Default::default(),
            )
            .map_err(|cause| RuntimeError::metadata_conflict(cause.to_string()))?;
        if self.arguments.len() != signature.params.len()
            || self
                .arguments
                .iter()
                .zip(&signature.params)
                .any(|(codec, ty)| !codec.accepts(ty, definitions, catalog))
            || !self.result.accepts(&signature.result, definitions, catalog)
        {
            return Err(RuntimeError::metadata_conflict(
                "native codec does not match the Kagari declaration",
            ));
        }
        if let BindingEntry::Primitive(primitive) = self.entry
            && !primitive.accepts_signature(signature)
        {
            return Err(RuntimeError::metadata_conflict(
                "native primitive signature relationship",
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
    pub(crate) prepared_signature: OnceLock<ScopedSignature>,
    pub(crate) selected: Box<[LinkedOperation]>,
    pub(crate) result_adapter: Option<LinkedResultAdapter>,
}

impl LinkedNativeFunction {
    /// Prepare closed facts before candidate publication. Their immutable provenance
    /// creates neither an executable retention lease nor a program ownership cycle.
    pub(crate) fn prepare_signature(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
    ) -> NativeResult<()> {
        let params = runtime.type_arguments(owner, None, &self.signature.params)?;
        let result = runtime
            .type_arguments(owner, None, slice::from_ref(&self.signature.result))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("native result type scope"))?;
        self.prepared_signature
            .set(ScopedSignature { params, result })
            .map_err(|_| RuntimeError::module_validation("duplicate native signature preparation"))
    }

    pub(crate) fn type_signature(&self) -> NativeResult<&ScopedSignature> {
        self.prepared_signature
            .get()
            .ok_or_else(|| RuntimeError::module_validation("native signature is not prepared"))
    }

    pub(crate) fn check_requirement(
        &self,
        owner: &LoadedModule,
        key: &SelectedCall,
    ) -> NativeResult<()> {
        let (declaration, definitions) = match &self.binding.requirement_owner {
            Some((declaration, definitions)) => (*declaration, definitions),
            None => (self.declaration, owner.definitions()),
        };
        if !same_definition(declaration, definitions, key.declaration, &key.definitions) {
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
        Ok(Self {
            declaration: self.declaration,
            binding: self.binding.clone(),
            signature,
            prepared_signature: OnceLock::from(ScopedSignature { params, result }),
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
        let signature = self.type_signature()?;
        let result = match &self.binding.entry {
            BindingEntry::Callback(entry) => entry(context),
            BindingEntry::Primitive(primitive) => primitive
                .execute(context.runtime, context.owner, signature, |index| {
                    context.argument(index)
                })
                .map_err(|error| *error),
        };
        context.poll()?;
        let value = result?;
        if (!self.binding.converted_result || self.result_adapter.is_some())
            && !signature
                .result
                .matches(context.runtime, &value, context.owner)
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
