//! Mixed two-loan packs reuse contextual binding and the existing loan owners.
//! Neither input can escape through a callback result:
//! ```compile_fail
//! use kagari_runtime::native::{builder::ModuleBuilder, types::FunctionRef,
//!     typed::loans::LoanContext};
//! struct Stats { hp: i32 }
//! fn bind(module: &mut ModuleBuilder, function: FunctionRef) {
//!     module.bind(function, |_: &mut LoanContext<'_>, (_, second): (&mut Stats, &mut Stats)| Ok(second));
//! }
//! ```
use crate::{
    error::RuntimeError,
    native::{
        adapter::NativeAdapter,
        binding::{NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari},
        typed::{
            NativeContext,
            loan_arguments::{LoanReceiver, PairArguments},
            loans::{LoanContext, check_receiver},
        },
    },
};
use kagari_types::{declaration::FnDecl, reference::ReferenceAccess, ty::Ty};
use std::{any::TypeId, marker::PhantomData};

#[doc(hidden)]
pub struct SharedShared<T, U, A>(PhantomData<(T, U, A)>);

#[doc(hidden)]
pub struct SharedMutable<T, U, A>(PhantomData<(T, U, A)>);

#[doc(hidden)]
pub struct MutableShared<T, U, A>(PhantomData<(T, U, A)>);

#[doc(hidden)]
pub struct MutableMutable<T, U, A>(PhantomData<(T, U, A)>);

macro_rules! pair_adapter {
    ($mode:ident, $m1:expr, $m2:expr, $a1:ident, $a2:ident, $native:ident, $external:ident;
        [$($mut1:ident)?], [$($mut2:ident)?]; $count:expr; $($argument:ident:$value:ident),*) => {
        impl<F, T, U, R, $($argument,)*> NativeAdapter<$mode<T, U, ($($argument,)*)>, R> for F
        where
            T: Send + 'static,
            U: Send + 'static,
            R: IntoKagari + 'static,
            $($argument: FromKagari + 'static,)*
            F: for<'loan> Fn(&mut LoanContext<'_>, (&'loan $($mut1)? T, &'loan $($mut2)? U, $($argument,)*)) -> NativeResult<R>
                + Send + Sync + 'static,
        {
            fn prepare(self, declaration: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
                let receiver_access = check_receiver(declaration, ReferenceAccess::$a1, 2 + $count)?;
                if !matches!(&declaration.params[1].ty, Ty::Reference { access, .. } if *access == ReferenceAccess::$a2) {
                    return Err(RuntimeError::metadata_conflict("second borrowed callback input differs from its declared reference access"));
                }
                let mut binding = NativeBinding::contextual(declaration.params.len(),
                    move |cx: &mut NativeContext<'_>, PairArguments(first, second, owned): PairArguments<T, U, ($($argument,)*), $m1, $m2>| {
                        let ($($value,)*) = owned;
                        let LoanReceiver::External(second) = second else {
                            return Err(RuntimeError::module_validation("reference argument requires an external location"));
                        };
                        match first {
                            LoanReceiver::External(first) => {
                                let runtime = cx.runtime();
                                let mut loan = LoanContext { resources: runtime.resources() };
                                runtime.host_borrows.$external(runtime, first, second,
                                    |first: &$($mut1)? T, second: &$($mut2)? U| self(&mut loan, (first, second, $($value,)*)))
                            }
                            LoanReceiver::Native(first) => cx.conversion.borrow_native_argument(
                                first.request(ReferenceAccess::$a1),
                                [(second, TypeId::of::<U>(), ReferenceAccess::$a2)],
                                |resources, first, [second]| {
                                    let first = first.$native()?;
                                    // SAFETY: the external reservation checked the
                                    // exact Rust target/access before projection.
                                    // The separately owned GC payload loan is live;
                                    // both refs end before result conversion or GC.
                                    unsafe { self(&mut LoanContext { resources }, (first, &$($mut2)? *second.cast::<U>(), $($value,)*)) }
                                }),
                        }
                    });
                binding.receiver_access = receiver_access;
                Ok(binding)
            }
        }
    };
}

macro_rules! pairs_for_arity {
    ($count:expr; $($argument:ident:$value:ident),*) => {
        pair_adapter!(SharedShared, false, false, Shared, Shared, shared, read_pair; [], []; $count; $($argument:$value),*);
        pair_adapter!(SharedMutable, false, true, Shared, Mutable, shared, read_edit_pair; [], [mut]; $count; $($argument:$value),*);
        pair_adapter!(MutableShared, true, false, Mutable, Shared, mutable, edit_read_pair; [mut], []; $count; $($argument:$value),*);
        pair_adapter!(MutableMutable, true, true, Mutable, Mutable, mutable, edit_pair; [mut], [mut]; $count; $($argument:$value),*);
    };
}

pairs_for_arity!(0;);
pairs_for_arity!(1; A:a);
pairs_for_arity!(2; A:a, B:b);
pairs_for_arity!(3; A:a, B:b, C:c);
pairs_for_arity!(4; A:a, B:b, C:c, D:d);
pairs_for_arity!(5; A:a, B:b, C:c, D:d, E:e);
pairs_for_arity!(6; A:a, B:b, C:c, D:d, E:e, G:g);
pairs_for_arity!(7; A:a, B:b, C:c, D:d, E:e, G:g, H:h);
pairs_for_arity!(8; A:a, B:b, C:c, D:d, E:e, G:g, H:h, I:i);
pairs_for_arity!(9; A:a, B:b, C:c, D:d, E:e, G:g, H:h, I:i, J:j);
pairs_for_arity!(10; A:a, B:b, C:c, D:d, E:e, G:g, H:h, I:i, J:j, K:k);
