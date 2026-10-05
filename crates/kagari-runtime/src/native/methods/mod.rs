//! Public inherent-member binding for script objects and native payloads.
mod application;
mod arguments;
mod binding;
pub(crate) mod receiver;
mod resolution;

use crate::{
    error::RuntimeError,
    gc::roots::RootedValue,
    native::{
        binding::NativeResult,
        conversion::{FromKagari, arguments::IntoKagariArguments},
        function_handle::PinnedFunction,
        methods::{
            arguments::ReceiverArguments,
            receiver::{AppliedReceiver, RetainedReceiver},
        },
        typed::NativeContext,
    },
};
use kagari_common::identity::{DefinitionPath, table::DefinitionId};
use kagari_types::ty::{GenericParam, Ty};

/// Installed types expose public member evidence through the same resolver.
/// Members have private construction; implementing this trait cannot invent one.
pub trait InherentType {
    fn method(&self, name: &str) -> NativeResult<InherentMember>;
}

#[derive(Debug, Clone)]
pub struct InherentMember {
    pub(crate) applied: AppliedReceiver,
    pub(crate) declaration: DefinitionPath,
    pub(crate) has_receiver: bool,
    receiver: Ty<DefinitionId>,
    parameters: Vec<GenericParam<DefinitionId>>,
    method_arity: usize,
}

impl InherentMember {
    pub fn declaration(&self) -> &DefinitionPath {
        &self.declaration
    }
}

/// A cached checked method. The argument tuple excludes the receiver.
#[derive(Debug)]
pub struct Method<A, R> {
    receiver: AppliedReceiver,
    function: PinnedFunction<ReceiverArguments<A>, R>,
}

impl<A, R> Clone for Method<A, R> {
    fn clone(&self) -> Self {
        Self {
            receiver: self.receiver.clone(),
            function: self.function.clone(),
        }
    }
}

impl<A: IntoKagariArguments, R: FromKagari> Method<A, R> {
    pub(crate) fn call_on(
        &self,
        cx: &mut NativeContext<'_>,
        receiver: AppliedReceiver,
        root: RootedValue,
        arguments: A,
    ) -> NativeResult<R> {
        self.receiver.validate(cx.runtime())?;
        receiver.validate(cx.runtime())?;
        if !self.receiver.matches(&receiver) {
            return Err(RuntimeError::module_validation(
                "method binding belongs to another applied type or generation",
            ));
        }
        self.function.call(
            cx,
            ReceiverArguments {
                receiver: RetainedReceiver {
                    root,
                    applied: receiver,
                },
                arguments,
            },
        )
    }
}
