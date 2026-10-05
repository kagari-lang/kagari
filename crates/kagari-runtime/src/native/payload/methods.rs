//! Registered payloads share inherent member lookup and prepared calls with structs.
use crate::native::{
    binding::NativeResult,
    conversion::{FromKagari, arguments::IntoKagariArguments},
    methods::{InherentMember, InherentType, Method, receiver::AppliedReceiver},
    payload::{NativeObject, NativeType},
    storage::NativePayload,
    typed::NativeContext,
};
use kagari_common::identity::DefinitionPath;

impl<T: NativePayload> NativeType<T> {
    fn method_receiver(&self) -> AppliedReceiver {
        AppliedReceiver::new(
            self.owner().clone(),
            self.type_argument().clone(),
            self.record._program.clone(),
        )
    }

    pub fn method(&self, name: &str) -> NativeResult<InherentMember> {
        self.method_receiver().method(name)
    }

    pub fn method_declaration(&self, declaration: &DefinitionPath) -> NativeResult<InherentMember> {
        self.method_receiver().method_declaration(declaration)
    }
}

impl<T: NativePayload> InherentType for NativeType<T> {
    fn method(&self, name: &str) -> NativeResult<InherentMember> {
        self.method(name)
    }
}

impl<T: NativePayload> NativeObject<T> {
    pub fn call<A: IntoKagariArguments, R: FromKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        method: &Method<A, R>,
        arguments: A,
    ) -> NativeResult<R> {
        cx.runtime().gc().ensure_no_native_borrow()?;
        method.call_on(
            cx,
            self.native_type.method_receiver(),
            self.root.clone(),
            arguments,
        )
    }
}
