//! Recursive conversion state is created only when an interaction needs it.
use crate::{
    Runtime,
    module::ExecutableModule,
    native::{binding::NativeResult, context::CallContext, conversion::context::ConversionContext},
};
use std::{
    cell::OnceCell,
    ops::{Deref, DerefMut},
};

pub(crate) struct NativeConversion<'call> {
    runtime: &'call Runtime,
    owner: &'call ExecutableModule,
    context: OnceCell<ConversionContext<'call>>,
}

impl<'call> NativeConversion<'call> {
    pub(super) fn owned(context: ConversionContext<'call>) -> Self {
        Self {
            runtime: context.runtime(),
            owner: context.owner(),
            context: OnceCell::from(context),
        }
    }

    pub(super) fn in_call(call: &'call CallContext<'_>) -> Self {
        // Only an admitted Native frame constructs CallContext. That frame owns
        // the exact executable and argument roots through conversion/reentry;
        // publication cannot occur during this synchronous activation.
        Self {
            runtime: call.runtime,
            owner: call.owner,
            context: OnceCell::new(),
        }
    }

    pub(crate) fn runtime(&self) -> &'call Runtime {
        self.runtime
    }

    pub(crate) fn owner(&self) -> &'call ExecutableModule {
        self.owner
    }

    pub(crate) fn poll(&self) -> NativeResult<()> {
        self.runtime.resources().poll_execution()
    }
}

impl<'call> Deref for NativeConversion<'call> {
    type Target = ConversionContext<'call>;

    fn deref(&self) -> &Self::Target {
        self.context
            .get_or_init(|| ConversionContext::admitted(self.runtime, self.owner))
    }
}

impl DerefMut for NativeConversion<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.context
            .get_or_init(|| ConversionContext::admitted(self.runtime, self.owner));
        self.context.get_mut().expect("initialized conversion")
    }
}
