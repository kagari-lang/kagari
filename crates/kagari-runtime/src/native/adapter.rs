//! One declaration-aware adapter selection for raw, scalar and owned callbacks.
use crate::{
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, arguments::FromKagariArguments},
        functions::NativeFunction,
        scalar::NativeScalar,
        typed::NativeContext,
    },
};
use kagari_types::declaration::FnDecl;
use std::marker::PhantomData;

/// Inference marker for existing flat call-context scalar and scoped-view entries.
#[doc(hidden)]
pub struct Flat<A>(PhantomData<A>);

/// Inference marker for an outer tuple of owned conversions or retained handles.
#[doc(hidden)]
pub struct Owned<A>(PhantomData<A>);

/// Inference marker for an owning receiver followed by an outer argument tuple.
#[doc(hidden)]
pub struct OwnedMethod<S, A>(PhantomData<(S, A)>);

/// Inference marker for an explicitly prepared callback or runtime primitive.
#[doc(hidden)]
pub struct Explicit;

/// Inference marker for checked external-ID callbacks using existing host scopes.
#[doc(hidden)]
pub struct Host;

/// Inference marker for an existing cold producer with owned arguments/results.
#[doc(hidden)]
pub struct Async<A>(PhantomData<A>);

/// Select a local implementation against an already authored Kagari signature.
/// `bind` checks the produced binding through its ordinary declaration validator;
/// concrete contextual mappings are checked before conversion/callback effects.
/// Callback context types must be annotated so Rust can select the adapter.
pub trait NativeAdapter<Mode, R> {
    fn prepare(
        self,
        declaration: &FnDecl,
        catalog: &DeclarationCatalog,
    ) -> NativeResult<NativeBinding>;
}

impl<F, A, R> NativeAdapter<Flat<A>, R> for F
where
    F: NativeFunction<A, R>,
    R: NativeScalar,
{
    fn prepare(self, _: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        // SAFETY: Sealed scalar outputs cannot publish an unrooted managed result.
        Ok(unsafe { self.binding_transport() })
    }
}

impl NativeAdapter<Explicit, ()> for NativeBinding {
    fn prepare(self, _: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        Ok(self)
    }
}

impl<F, A, R> NativeAdapter<Owned<A>, R> for F
where
    A: FromKagariArguments,
    R: IntoKagari,
    F: for<'call> Fn(&mut NativeContext<'call>, A) -> NativeResult<R> + Send + Sync + 'static,
{
    fn prepare(
        self,
        declaration: &FnDecl,
        _catalog: &DeclarationCatalog,
    ) -> NativeResult<NativeBinding> {
        Ok(NativeBinding::contextual(declaration.params.len(), self))
    }
}

impl<F, S, A, R> NativeAdapter<OwnedMethod<S, A>, R> for F
where
    S: FromKagari,
    A: FromKagariArguments,
    R: IntoKagari,
    F: for<'call> Fn(&mut NativeContext<'call>, S, A) -> NativeResult<R> + Send + Sync + 'static,
{
    fn prepare(
        self,
        declaration: &FnDecl,
        _catalog: &DeclarationCatalog,
    ) -> NativeResult<NativeBinding> {
        if !declaration
            .params
            .first()
            .is_some_and(|parameter| parameter.name == "self")
        {
            return Err(RuntimeError::metadata_conflict(
                "method callback requires a receiver",
            ));
        }
        Ok(NativeBinding::contextual_method(
            declaration.params.len(),
            self,
        ))
    }
}
