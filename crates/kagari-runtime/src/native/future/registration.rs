use crate::{
    closure::{CallableObservation, ObservedCallableTarget},
    error::RuntimeError,
    native::{
        adapter::{Async, NativeAdapter},
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        completion::Completion,
        conversion::{IntoKagari, arguments::FromKagariArguments, context::ConversionContext},
        future::{ColdFuture, FuturePayload, NativeFuture, NativeStart, Producer, TypedProducer},
        storage::value::StoredValue,
        types::TypeRef,
    },
    session::ExecutionPhase,
};
use kagari_types::declaration::{FnDecl, TypeDefKind, native::NativeStorageLayout};
use std::{marker::PhantomData, sync::Arc};

impl DeclarationCatalog {
    /// Resolve by a checked storage role, never by an application function name.
    pub fn future_type(&self) -> NativeResult<TypeRef> {
        let mut candidates = self
            .types
            .entries()
            .filter(|(_, ty)| ty.kind == TypeDefKind::NativeStorage(NativeStorageLayout::Future));
        let (id, declaration) = candidates
            .next()
            .ok_or_else(|| RuntimeError::metadata_conflict("missing Future storage declaration"))?;
        if candidates.next().is_some() {
            return Err(RuntimeError::metadata_conflict(
                "ambiguous Future storage declarations",
            ));
        }
        Ok(TypeRef {
            id,
            definitions: self.definitions(),
            declaration: Arc::new(declaration.clone()),
            parameter_names: (0..declaration.generic_params.len())
                .map(|index| format!("T{index}"))
                .collect(),
        })
    }
}

impl<F, A, R> NativeAdapter<Async<A>, R> for F
where
    A: FromKagariArguments + 'static,
    R: IntoKagari + Send + 'static,
    F: Fn(A, Completion<R>) -> NativeResult<NativeStart<R>> + Send + Sync + 'static,
{
    /// Construction only captures checked script arguments. Conversion/submission
    /// still happens on first await through the existing cold producer service.
    fn prepare(self, _: &FnDecl, catalog: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        let parameters = A::argument_types(catalog)?;
        let result = catalog.future_type()?.apply([R::kagari_type(catalog)?])?;
        let producer: Arc<dyn Producer> = Arc::new(TypedProducer {
            entry: self,
            marker: PhantomData::<fn(A) -> R>,
        });
        let binding = NativeBinding::new(
            parameters
                .iter()
                .map(|ty| Codec::Exact(ty.clone()))
                .collect::<Vec<_>>(),
            Codec::Exact(result.clone()),
            move |call| {
                call.runtime
                    .reject_static_initialization("cold native Future construction")?;
                if call.runtime.execution_options().phase != ExecutionPhase::Ordinary {
                    return Err(RuntimeError::execution_phase_violation(
                        "cold Future construction",
                    ));
                }
                let signature = call.function.type_signature()?;
                let output = signature.result.parameter(call.runtime, call.owner, 0)?;
                let cx = ConversionContext::in_native_call(call)?;
                cx.check_type::<R>(&output)?;
                A::check_types(&cx, &signature.params)?;
                let values = (0..call.arguments.len())
                    .map(|index| call.argument(index))
                    .collect::<NativeResult<Vec<_>>>()?;
                call.runtime.gc().validate_async_values(&values)?;
                call.allocate_result_payload(&[], |_, _| {
                    Ok(FuturePayload {
                        cold: Some(ColdFuture::Native(NativeFuture {
                            origin: Some(CallableObservation {
                                publication: call.owner.publication_token(),
                                module: call.owner.slot(),
                                target: ObservedCallableTarget::NativeBinding(
                                    call.function.declaration,
                                ),
                            }),
                            owner: call.owner.clone(),
                            parameters: signature.params.clone(),
                            output,
                            values: values.into_iter().map(StoredValue::new).collect(),
                            producer: producer.clone(),
                        })),
                    })
                })
            },
        );
        Ok(binding)
    }
}
