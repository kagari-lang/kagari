//! Checked Rust conversion views prepared once for a concrete executable import.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{
        TypeEnvironment, arguments::ScopedSignature, compatibility::TypeView,
        operations::OperationBindings,
    },
    module::ExecutableModule,
    native::{
        catalog::DeclarationCatalog,
        context::{CallContext, LinkedCallable, LinkedOperation},
        declarations::SelectedCall,
        primitive::NativePrimitive,
        result::LinkedResultAdapter,
        types::{Type, TypeRef, same_definition},
    },
    value::Value,
};
use kagari_bytecode::instruction::NativeImportId;
use kagari_common::identity::{
    scoped::DefinitionReferences,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    callable::Signature,
    declaration::{FnDecl, TypeDefKind, host::HostCallPolicy, native::NativeStorageLayout},
    reference::ReferenceAccess,
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
    /// Prove the outer enum declaration and arity without fixing its applied children.
    ///
    /// Registration and executable linking require the declaration in the available
    /// catalog and compare its scoped identity with the actual signature. Generic
    /// application retains the signature's concrete children. This grants no Rust
    /// child-conversion proof and never bypasses dynamic return-value validation.
    NominalEnum {
        declaration: TypeRef,
        arity: usize,
    },
    Object {
        declaration: DefinitionId,
        definitions: DefinitionTable,
    },
    Iterator,
    Callable,
}

impl Codec {
    /// Receiver codecs must agree on their representation and nominal declaration.
    pub(crate) fn receiver_shape_matches(&self, method: &Self) -> bool {
        match (self, method) {
            (Self::Exact(left), Self::Exact(right)) => left.same_type(right),
            (
                Self::NominalEnum {
                    declaration: left,
                    arity: left_arity,
                },
                Self::NominalEnum {
                    declaration: right,
                    arity: right_arity,
                },
            ) => {
                left_arity == right_arity
                    && same_definition(
                        left.id(),
                        left.definitions(),
                        right.id(),
                        right.definitions(),
                    )
            }
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
            (Self::Value, Self::Value)
            | (Self::Iterator, Self::Iterator)
            | (Self::Callable, Self::Callable) => true,
            _ => false,
        }
    }

    pub(crate) fn accepts(
        &self,
        ty: &Ty,
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
            Self::NominalEnum { declaration, arity } => {
                let Ty::Enum(nominal) = ty else {
                    return false;
                };
                if declaration.declaration.kind != TypeDefKind::Enum
                    || declaration.declaration.generic_params.len() != *arity
                    || nominal.arguments.len() != *arity
                    || !same_definition(
                        declaration.id(),
                        declaration.definitions(),
                        nominal.declaration,
                        definitions,
                    )
                {
                    return false;
                }
                let Ok(identity) = declaration.definitions().resolve(declaration.id()) else {
                    return false;
                };
                // A scoped identity alone is not a provider. Resolve through the
                // catalog at this registration/link boundary, never during a call.
                catalog
                    .types
                    .get(&identity.to_path())
                    .is_some_and(|installed| {
                        installed.kind == TypeDefKind::Enum
                            && installed.generic_params.len() == *arity
                    })
            }
            Self::Object {
                declaration,
                definitions: source,
            } => {
                matches!(ty, Ty::NativeObject(nominal) if same_definition(*declaration, source, nominal.declaration, definitions))
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
/// let binding = unsafe { NativeBinding::new_transport(Vec::<Codec>::new(), Codec::Value, move |_| {
///     Ok(Value::I32(*local))
/// }) };
/// ```
#[derive(Clone)]
pub struct NativeBinding {
    pub(crate) host_policy: Option<HostCallPolicy>,
    pub(crate) arguments: Box<[Codec]>,
    pub(crate) result: Codec,
    pub(crate) entry: BindingEntry,
    pub(crate) converted_result: bool,
    pub(crate) requirement_owner: Option<(DefinitionId, DefinitionTable)>,
    pub(crate) receiver_access: Option<ReferenceAccess>,
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
    /// Register a low-level provider with an explicit transport contract.
    ///
    /// # Safety
    /// The provider must protect managed arguments, results and captured edges through
    /// callbacks, collection and publication. A raw captured Value is not a root.
    /// Return results without an intervening collection before caller publication;
    /// release storage loans before reentry and preserve scoped host lifetimes.
    /// Signature codecs and dynamic checks do not establish pointer lifetime.
    #[doc(hidden)]
    pub unsafe fn new_transport(
        arguments: impl Into<Box<[Codec]>>,
        result: Codec,
        entry: impl for<'call> Fn(&mut CallContext<'call>) -> NativeResult<Value>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::new(arguments, result, entry)
    }

    pub(crate) fn new(
        arguments: impl Into<Box<[Codec]>>,
        result: Codec,
        entry: impl for<'call> Fn(&mut CallContext<'call>) -> NativeResult<Value>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            host_policy: None,
            arguments: arguments.into(),
            result,
            entry: BindingEntry::Callback(Arc::new(entry)),
            converted_result: false,
            requirement_owner: None,
            receiver_access: None,
        }
    }

    pub(crate) fn check_policies(&self, declaration: &FnDecl) -> NativeResult<()> {
        if self.receiver_access != declaration.method_policy.receiver_access
            || self.host_policy != declaration.host_policy
        {
            return Err(RuntimeError::metadata_conflict(
                "binding differs from declared native access policy",
            ));
        }
        Ok(())
    }

    pub(crate) fn check(
        &self,
        signature: &Signature,
        definitions: &DefinitionTable,
        catalog: &DeclarationCatalog,
    ) -> NativeResult<()> {
        signature
            .visit_definition_ids(
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
    /// Exact slot in the invocation owner's immutable native import table.
    pub(crate) import: NativeImportId,
    /// Only checked shared-call application installs a symbolic import environment.
    pub(crate) environment: Option<TypeEnvironment>,
    pub(crate) declaration: DefinitionId,
    pub(crate) binding: NativeBinding,
    pub(crate) signature: Signature,
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
        owner: &ExecutableModule,
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
        owner: &ExecutableModule,
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
        owner: &ExecutableModule,
        environment: TypeEnvironment,
        operations: &OperationBindings,
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
            import: self.import,
            environment: Some(environment.clone()),
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
                        let receiver = TypeView::new(
                            &required.receiver,
                            owner,
                            Some(environment.types.as_ref()),
                        )
                        .identity()
                        .ok_or_else(|| {
                            RuntimeError::module_validation("native operation receiver identity")
                        })?;
                        let resolved = environment.types.resolve_requirement(required)?;
                        let operation = operations
                            .operation(&runtime.gc, &resolved, &receiver)
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
        let result = context
            .narrow_arguments()
            .and_then(|mut narrowed| self.invoke_narrowed(&mut narrowed));
        result.map_err(|error| context.runtime.resources().admit_execution_error(error))
    }

    fn invoke_narrowed(&self, context: &mut CallContext<'_>) -> NativeResult<Value> {
        let signature = self.type_signature()?;
        let result = match &self.binding.entry {
            BindingEntry::Callback(entry) => entry(context),
            BindingEntry::Primitive(primitive) => primitive
                .execute(context.runtime, context.owner, signature, |index| {
                    context.argument(index)
                })
                .map_err(|error| *error),
        }
        .map_err(|error| context.runtime.resources().admit_execution_error(error));
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
