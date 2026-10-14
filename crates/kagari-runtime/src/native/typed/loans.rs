//! Lifetime-aware callbacks use the ordinary declaration and conversion boundary.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        adapter::NativeAdapter,
        binding::{NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari},
        storage::NativePayload,
        typed::{
            NativeContext,
            loan_arguments::{LoanArguments, LoanReceiver},
        },
    },
    value::Value,
};
use kagari_types::{declaration::FnDecl, reference::ReferenceAccess, ty::Ty};
use std::marker::PhantomData;

/// Services available while Rust payload references are alive. A callback may
/// cooperate with cancellation but cannot allocate in the script heap or execute
/// scripts through this context. Captured runtime handles retain their ordinary
/// conflict checks. Results must be owned independently of the payload loan.
///
/// A borrowed callback cannot return its receiver:
/// ```compile_fail
/// use kagari_runtime::native::{builder::inherent::InherentMethodsBuilder,
///     typed::loans::LoanContext, types::FunctionRef};
/// fn register(methods: &mut InherentMethodsBuilder, method: FunctionRef) {
///     methods.bind(method, |_: &mut LoanContext<'_>, (value,): (&i32,)| Ok(value));
/// }
/// ```
///
/// Nor can it retain the receiver in captured Rust state:
/// ```compile_fail
/// use std::sync::{Arc, Mutex};
/// use kagari_runtime::native::{builder::inherent::InherentMethodsBuilder,
///     typed::loans::LoanContext, types::FunctionRef};
/// fn register(methods: &mut InherentMethodsBuilder, method: FunctionRef,
///             saved: Arc<Mutex<Option<&'static i32>>>) {
///     methods.bind(method, move |_: &mut LoanContext<'_>, (value,): (&i32,)| {
///         *saved.lock().unwrap() = Some(value);
///         Ok(())
///     });
/// }
/// ```
pub struct LoanContext<'call> {
    runtime: &'call Runtime,
}

impl LoanContext<'_> {
    /// Cooperatively check cancellation during a long host operation.
    pub fn poll(&self) -> NativeResult<()> {
        self.runtime.resources().poll_execution()
    }
}

/// Inference marker for one shared receiver followed by an owned argument pack.
#[doc(hidden)]
pub struct Shared<T, A>(PhantomData<(T, A)>);

/// Inference marker for one exclusive receiver followed by an owned argument pack.
#[doc(hidden)]
pub struct Mutable<T, A>(PhantomData<(T, A)>);

fn check_receiver(declaration: &FnDecl, access: ReferenceAccess, arity: usize) -> NativeResult<()> {
    if declaration.params.len() != arity
        || !declaration.params.first().is_some_and(|p| p.name == "self")
        || declaration.method_policy.receiver_access != Some(access)
    {
        return Err(RuntimeError::metadata_conflict(
            "borrowed callback differs from declared receiver access or argument count",
        ));
    }
    Ok(())
}

pub(super) fn check_receiver_storage(
    cx: &NativeContext<'_>,
    access: Option<ReferenceAccess>,
    types: &[TypeArgument],
    values: &[Value],
) -> NativeResult<()> {
    if access == Some(ReferenceAccess::Mutable) {
        if values
            .first()
            .is_some_and(|value| matches!(value, Value::Ephemeral(_)))
        {
            // External storage is owned by the host, so the runtime-owned data
            // edit flag does not govern it. Its installed Rust mapping, target,
            // scope and mutable capability are checked before converters below.
            let view = cx.runtime().scoped_view(&values[0])?;
            if !cx.runtime().host_borrows.scoped_matches(
                cx.runtime(),
                view,
                &types[0],
                ReferenceAccess::Mutable,
                cx.conversion.owner(),
            ) {
                return Err(RuntimeError::module_validation(
                    "mutable external receiver capability",
                ));
            }
            return Ok(());
        }
        let Some(Ty::NativeObject(nominal)) = types.first().map(TypeArgument::ty) else {
            return Err(RuntimeError::module_validation(
                "unsupported mutable receiver storage",
            ));
        };
        if cx
            .runtime()
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .is_none_or(|storage| !storage.is_editable_data())
        {
            return Err(RuntimeError::module_validation(
                "storage does not permit direct data editing",
            ));
        }
    }
    Ok(())
}

macro_rules! receiver_adapters {
    ($count:expr; $($argument:ident:$value:ident),*) => {
        impl<F, T, R, $($argument,)*> NativeAdapter<Shared<T, ($($argument,)*)>, R> for F
        where
            T: NativePayload,
            R: IntoKagari + 'static,
            $($argument: FromKagari + 'static,)*
            F: for<'loan> Fn(&mut LoanContext<'_>, (&'loan T, $($argument,)*)) -> NativeResult<R>
                + Send + Sync + 'static,
        {
            fn prepare(self, declaration: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
                check_receiver(declaration, ReferenceAccess::Shared, 1 + $count)?;
                let mut binding = NativeBinding::contextual(
                    declaration.params.len(),
                    move |cx: &mut NativeContext<'_>, LoanArguments(receiver, arguments): LoanArguments<T, ($($argument,)*), false>| {
                        let ($($value,)*) = arguments;
                        let mut loan = LoanContext { runtime: cx.runtime() };
                        match receiver {
                            LoanReceiver::Opaque(receiver) => receiver.read(cx, |payload| self(&mut loan, (payload, $($value,)*))),
                            LoanReceiver::External(view) => cx.runtime().host_borrows.read_scoped(cx.runtime(), view,
                                |payload| self(&mut loan, (payload, $($value,)*))),
                        }
                    },
                );
                binding.receiver_access = Some(ReferenceAccess::Shared);
                Ok(binding)
            }
        }

        impl<F, T, R, $($argument,)*> NativeAdapter<Mutable<T, ($($argument,)*)>, R> for F
        where
            T: NativePayload,
            R: IntoKagari + 'static,
            $($argument: FromKagari + 'static,)*
            F: for<'loan> Fn(&mut LoanContext<'_>, (&'loan mut T, $($argument,)*)) -> NativeResult<R>
                + Send + Sync + 'static,
        {
            fn prepare(self, declaration: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
                check_receiver(declaration, ReferenceAccess::Mutable, 1 + $count)?;
                let mut binding = NativeBinding::contextual(
                    declaration.params.len(),
                    move |cx: &mut NativeContext<'_>, LoanArguments(receiver, arguments): LoanArguments<T, ($($argument,)*), true>| {
                        let ($($value,)*) = arguments;
                        let mut loan = LoanContext { runtime: cx.runtime() };
                        match receiver {
                            LoanReceiver::Opaque(receiver) => receiver.edit_checked(cx, |payload| self(&mut loan, (payload, $($value,)*))),
                            LoanReceiver::External(view) => cx.runtime().host_borrows.edit_scoped(cx.runtime(), view,
                                |payload| self(&mut loan, (payload, $($value,)*))),
                        }
                    },
                );
                binding.receiver_access = Some(ReferenceAccess::Mutable);
                Ok(binding)
            }
        }
    };
}

receiver_adapters!(0;);
receiver_adapters!(1; A:a);
receiver_adapters!(2; A:a, B:b);
receiver_adapters!(3; A:a, B:b, C:c);
receiver_adapters!(4; A:a, B:b, C:c, D:d);
receiver_adapters!(5; A:a, B:b, C:c, D:d, E:e);
receiver_adapters!(6; A:a, B:b, C:c, D:d, E:e, G:g);
receiver_adapters!(7; A:a, B:b, C:c, D:d, E:e, G:g, H:h);
receiver_adapters!(8; A:a, B:b, C:c, D:d, E:e, G:g, H:h, I:i);
receiver_adapters!(9; A:a, B:b, C:c, D:d, E:e, G:g, H:h, I:i, J:j);
receiver_adapters!(10; A:a, B:b, C:c, D:d, E:e, G:g, H:h, I:i, J:j, K:k);
receiver_adapters!(11; A:a, B:b, C:c, D:d, E:e, G:g, H:h, I:i, J:j, K:k, L:l);
