//! Script structs expose the common checked inherent-member path.
use crate::{
    Runtime,
    error::RuntimeError,
    native::{
        binding::NativeResult,
        conversion::{FromKagari, arguments::IntoKagariArguments},
        methods::{
            InherentMember, InherentType, Method, asynchronous::AsyncMethod,
            receiver::AppliedReceiver,
        },
        objects::{Object, ObjectType},
        typed::NativeContext,
    },
    scheduler::result::ScheduledResult,
    session::ExecutionOptions,
};
use kagari_common::identity::DefinitionPath;

impl ObjectType {
    fn method_receiver(&self) -> NativeResult<AppliedReceiver> {
        if !self.0.public {
            return Err(RuntimeError::module_validation("object type is not public"));
        }
        Ok(AppliedReceiver::new(
            self.owner().clone(),
            self.type_argument().clone(),
            self.0._program.clone(),
        ))
    }

    pub fn method(&self, name: &str) -> NativeResult<InherentMember> {
        self.method_receiver()?.method(name)
    }

    pub fn method_declaration(&self, declaration: &DefinitionPath) -> NativeResult<InherentMember> {
        self.method_receiver()?.method_declaration(declaration)
    }
}

impl InherentType for ObjectType {
    fn method(&self, name: &str) -> NativeResult<InherentMember> {
        self.method(name)
    }
}

impl<S> Object<S> {
    pub fn schedule_async<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &self,
        runtime: &Runtime,
        method: &AsyncMethod<A, R>,
        arguments: A,
        options: ExecutionOptions,
    ) -> NativeResult<ScheduledResult<R>> {
        method.schedule_on(
            runtime,
            self.object_type.method_receiver()?,
            self.root.clone(),
            arguments,
            options,
        )
    }

    pub fn call<A: IntoKagariArguments, R: FromKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        method: &Method<A, R>,
        arguments: A,
    ) -> NativeResult<R> {
        self.id(cx)?;
        method.call_on(
            cx,
            self.object_type.method_receiver()?,
            self.root.clone(),
            arguments,
        )
    }
}
