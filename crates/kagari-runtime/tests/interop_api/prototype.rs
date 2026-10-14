//! HR01/RF01 compile-only contract prototype; not an executable host API.
//! Remove this prototype when RF05 migrates the cases to production entrypoints.
use std::marker::PhantomData;

pub struct NativeContext;

/// Borrowed callbacks receive no script execution or heap allocation service.
pub struct LoanContext;

pub trait OwnedValue: 'static {}

impl OwnedValue for () {}

impl OwnedValue for i32 {}

impl<A: OwnedValue> OwnedValue for (A,) {}

impl<A: OwnedValue, B: OwnedValue> OwnedValue for (A, B) {}

#[derive(Debug)]
pub struct Player {
    pub hp: i32,
}

impl OwnedValue for Player {}

pub struct Owned<A>(PhantomData<A>);

pub struct Shared<T, A>(PhantomData<(T, A)>);

pub struct Mutable<T, A>(PhantomData<(T, A)>);

pub struct SharedMutable<T, U, A>(PhantomData<(T, U, A)>);

pub struct MutableMutable<T, U>(PhantomData<(T, U)>);

pub struct Explicit;

pub enum NativeBinding {
    Raw,
    Primitive,
}

pub trait Adapter<Mode, R> {}

impl<F, A: OwnedValue, R: OwnedValue> Adapter<Owned<A>, R> for F where
    F: Fn(&mut NativeContext, A) -> Result<R, ()> + Send + Sync + 'static
{
}

impl<F, T: 'static, A: OwnedValue, R: OwnedValue> Adapter<Shared<T, A>, R> for F where
    F: for<'loan> Fn(&mut LoanContext, (&'loan T, A)) -> Result<R, ()> + Send + Sync + 'static
{
}

impl<F, T: 'static, A: OwnedValue, R: OwnedValue> Adapter<Mutable<T, A>, R> for F where
    F: for<'loan> Fn(&mut LoanContext, (&'loan mut T, A)) -> Result<R, ()> + Send + Sync + 'static
{
}

impl<F, T: 'static, U: 'static, A: OwnedValue, R: OwnedValue> Adapter<SharedMutable<T, U, A>, R>
    for F
where
    F: for<'loan> Fn(&mut LoanContext, (&'loan T, &'loan mut U, A)) -> Result<R, ()>
        + Send
        + Sync
        + 'static,
{
}

impl<F, T: 'static, U: 'static, R: OwnedValue> Adapter<MutableMutable<T, U>, R> for F where
    F: for<'loan> Fn(&mut LoanContext, (&'loan mut T, &'loan mut U)) -> Result<R, ()>
        + Send
        + Sync
        + 'static
{
}

impl Adapter<Explicit, ()> for NativeBinding {}

pub fn bind<Mode, R>(_: impl Adapter<Mode, R>) {}

pub struct Runtime;

/// The environment outlives the fresh invariant scope brand.
pub struct Scope<'scope, 'host: 'scope> {
    _runtime: &'host mut Runtime,
    _brand: PhantomData<&'scope mut &'scope ()>,
}

pub struct Ref<'scope, T: 'static>(PhantomData<(&'scope T, &'scope mut &'scope ())>);

pub struct RefMut<'scope, T: 'static>(PhantomData<(&'scope mut T, &'scope mut &'scope ())>);

impl Runtime {
    pub fn with_host_scope<'host, R>(
        &'host mut self,
        f: impl for<'scope> FnOnce(&mut Scope<'scope, 'host>) -> R,
    ) -> R {
        f(&mut Scope {
            _runtime: self,
            _brand: PhantomData,
        })
    }
}

impl<'scope, 'host> Scope<'scope, 'host> {
    pub fn borrow<T: 'static>(&self, _: &'scope T) -> Ref<'scope, T> {
        Ref(PhantomData)
    }

    pub fn borrow_mut<T: 'static>(&self, _: &'scope mut T) -> RefMut<'scope, T> {
        RefMut(PhantomData)
    }

    pub fn execute_shared<T: 'static>(&mut self, _: Ref<'scope, T>) -> i32 {
        1
    }

    pub fn execute_mut<T: 'static>(&mut self, _: RefMut<'scope, T>) -> i32 {
        1
    }
}
