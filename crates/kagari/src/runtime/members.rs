//! Retained object/member access under the same execution context as ordinary calls.
use crate::{RunResult, context::ExecutionContext, error::EmbeddingError, runtime::Runtime};
use kagari_runtime::native::{
    conversion::{FromKagari, IntoKagari, arguments::IntoKagariArguments},
    interfaces::{Interface, binding::InterfaceMethod},
    methods::Method,
    objects::{Object, fields::Field},
};

impl Runtime {
    pub fn get<T: FromKagari>(
        &mut self,
        object: &Object<impl Sized>,
        field: &Field<T>,
    ) -> RunResult<T> {
        self.get_with_context(object, field, &self.default_context.clone())
    }

    pub fn get_with_context<T: FromKagari>(
        &mut self,
        object: &Object<impl Sized>,
        field: &Field<T>,
        context: &ExecutionContext,
    ) -> RunResult<T> {
        self.with_context(object.object_type().owner(), context, |cx| {
            object.get(cx, field)
        })
    }

    pub fn set<T: IntoKagari>(
        &mut self,
        object: &Object<impl Sized>,
        field: &Field<T>,
        value: T,
    ) -> RunResult<()> {
        self.set_with_context(object, field, value, &self.default_context.clone())
    }

    pub fn set_with_context<T: IntoKagari>(
        &mut self,
        object: &Object<impl Sized>,
        field: &Field<T>,
        value: T,
        context: &ExecutionContext,
    ) -> RunResult<()> {
        self.with_context(object.object_type().owner(), context, |cx| {
            object.set(cx, field, value)
        })
    }

    pub fn call_method<A: IntoKagariArguments, R: FromKagari>(
        &mut self,
        object: &Object<impl Sized>,
        method: &Method<A, R>,
        arguments: A,
    ) -> RunResult<R> {
        self.call_method_with_context(object, method, arguments, &self.default_context.clone())
    }

    pub fn call_method_with_context<A: IntoKagariArguments, R: FromKagari>(
        &mut self,
        object: &Object<impl Sized>,
        method: &Method<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<R> {
        self.with_context(object.object_type().owner(), context, |cx| {
            object.call(cx, method, arguments)
        })
    }

    pub fn call_interface_method<A: IntoKagariArguments, R: FromKagari>(
        &mut self,
        object: &Interface<impl Sized>,
        method: &InterfaceMethod<A, R>,
        arguments: A,
    ) -> RunResult<R> {
        self.call_interface_method_with_context(
            object,
            method,
            arguments,
            &self.default_context.clone(),
        )
    }

    pub fn call_interface_method_with_context<A: IntoKagariArguments, R: FromKagari>(
        &mut self,
        object: &Interface<impl Sized>,
        method: &InterfaceMethod<A, R>,
        arguments: A,
        context: &ExecutionContext,
    ) -> RunResult<R> {
        context.validate_for_execute("interface method")?;
        let mut cx = self
            .vm
            .context(object.owner())
            .map_err(EmbeddingError::vm)?;
        object
            .call_with_options(&mut cx, method, arguments, context.runtime_options())
            .map_err(Into::into)
    }
}
