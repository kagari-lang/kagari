use crate::{
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        completion::Completion,
        conversion::{IntoKagari, arguments::FromKagariArguments, context::ConversionContext},
        future::{ColdFuture, FuturePayload, NativeStart, Producer, TypedProducer},
        registration::FunctionSpec,
        types::{FunctionRef, TypeRef},
    },
    session::ExecutionPhase,
};
use kagari_types::declaration::{TypeDefKind, native::NativeStorageLayout};
use std::{marker::PhantomData, sync::Arc};

impl DeclarationCatalog {
    /// Resolve by a checked storage role, never by an application function name.
    pub fn future_type(&self) -> NativeResult<TypeRef> {
        let mut candidates = self
            .types
            .iter()
            .filter(|(_, ty)| ty.kind == TypeDefKind::NativeStorage(NativeStorageLayout::Future));
        let (id, _) = candidates
            .next()
            .ok_or_else(|| RuntimeError::metadata_conflict("missing Future storage declaration"))?;
        if candidates.next().is_some() {
            return Err(RuntimeError::metadata_conflict(
                "ambiguous Future storage declarations",
            ));
        }
        self.type_reference(&id)
    }
}

impl ModuleBuilder {
    /// Calling the registered function only captures checked script arguments.
    /// Conversion and submission happen on first await. The producer receives
    /// owned arguments and an endpoint, with no runtime borrow that could escape.
    pub fn add_async_function<A, R>(
        &mut self,
        spec: FunctionSpec,
        entry: impl Fn(A, Completion<R>) -> NativeResult<NativeStart<R>> + Send + Sync + 'static,
    ) -> NativeResult<FunctionRef>
    where
        A: FromKagariArguments + 'static,
        R: IntoKagari + Send + 'static,
    {
        let mut catalog = self.providers.clone();
        catalog.merge(&DeclarationCatalog::declared([&self.declaration])?)?;
        let parameters = A::argument_types(&catalog)?;
        let result = catalog.future_type()?.apply([R::kagari_type(&catalog)?])?;
        let producer: Arc<dyn Producer> = Arc::new(TypedProducer {
            entry,
            marker: PhantomData::<fn(A) -> R>,
        });
        let binding = NativeBinding::new(
            parameters
                .iter()
                .map(|ty| Codec::Scalar(ty.0.clone()))
                .collect::<Vec<_>>(),
            Codec::Scalar(result.0.clone()),
            move |call| {
                if call.runtime.execution_options().phase != ExecutionPhase::Ordinary {
                    return Err(RuntimeError::execution_phase_violation(
                        "cold Future construction",
                    ));
                }
                let signature = call.function.type_signature(call.runtime, call.owner)?;
                let output = signature.result.parameter(call.runtime, call.owner, 0)?;
                let cx = ConversionContext::in_native_call(call)?;
                cx.check_type::<R>(&output)?;
                A::check_types(&cx, &signature.params)?;
                let values = (0..call.arguments.len())
                    .map(|index| call.argument(index))
                    .collect::<NativeResult<Vec<_>>>()?;
                call.allocate_result_payload(FuturePayload {
                    cold: Some(ColdFuture {
                        owner: call.owner.clone(),
                        parameters: signature.params.clone(),
                        output,
                        values,
                        producer: producer.clone(),
                    }),
                })
            },
        );
        self.add_prepared_function(spec, parameters, result, binding, &catalog)
    }
}
