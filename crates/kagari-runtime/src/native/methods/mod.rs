//! Public inherent-member binding for script objects and native payloads.
mod application;
mod arguments;
pub mod asynchronous;
mod binding;
pub(crate) mod receiver;
mod resolution;

use crate::{
    error::RuntimeError,
    gc::roots::RootedValue,
    module::dispatch::CallRoute,
    native::{
        binding::NativeResult,
        conversion::{FromKagari, arguments::IntoKagariArguments},
        function_handle::Function,
        methods::{
            arguments::ReceiverArguments,
            receiver::{AppliedReceiver, RetainedReceiver},
        },
        typed::NativeContext,
    },
    source::FunctionSourceTarget,
};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_types::ty::{GenericParam, Ty};

/// Installed types expose public member evidence through the same resolver.
/// Members have private construction; implementing this trait cannot invent one.
pub trait InherentType {
    fn method(&self, name: &str) -> NativeResult<InherentMember>;
}

#[derive(Debug, Clone)]
pub struct InherentMember {
    pub(crate) applied: AppliedReceiver,
    pub(crate) declaration: DefinitionId,
    pub(crate) has_receiver: bool,
    receiver: Ty,
    parameters: Vec<GenericParam>,
    method_arity: usize,
}

impl InherentMember {
    /// Returns the member ID in the retained receiver program's definition scope.
    /// This identity does not replace the pinned generation or binding evidence.
    pub fn declaration(&self) -> DefinitionId {
        self.declaration
    }

    /// Borrows the retained scope for resolving or explicitly exporting the member ID.
    pub fn definitions(&self) -> &DefinitionTable {
        self.applied.owner().definitions()
    }
}

/// A cached checked method. The argument tuple excludes the receiver.
#[derive(Debug)]
pub struct Method<A, R> {
    receiver: AppliedReceiver,
    function: Function<ReceiverArguments<A>, R>,
}

impl<A, R> Clone for Method<A, R> {
    fn clone(&self) -> Self {
        Self {
            receiver: self.receiver.clone(),
            function: self.function.clone(),
        }
    }
}

impl<A, R> Method<A, R> {
    pub fn source_target(&self) -> FunctionSourceTarget {
        self.function.source_target()
    }

    /// Inspect the checked method route without selecting executable code.
    pub fn call_route(&self) -> Option<&CallRoute> {
        self.function.call_route()
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
                "method binding belongs to another applied type or installation",
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
