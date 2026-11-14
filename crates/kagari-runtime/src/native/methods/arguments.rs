//! Prefix the independently retained receiver to the ordinary typed argument pack.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            KagariType,
            arguments::{ArgumentValues, IntoKagariArguments, KagariArguments, PreparedArguments},
            context::ConversionContext,
        },
        methods::receiver::RetainedReceiver,
        types::Type,
    },
};

#[derive(Debug)]
pub(super) struct ReceiverArguments<A> {
    pub(super) receiver: RetainedReceiver,
    pub(super) arguments: A,
}

impl<A: KagariArguments> KagariArguments for ReceiverArguments<A> {
    fn argument_types(_: &DeclarationCatalog) -> NativeResult<Vec<Type>> {
        Err(RuntimeError::module_validation(
            "method arguments require an installed receiver scope",
        ))
    }

    fn check_types(cx: &ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<()> {
        let (receiver, arguments) = expected
            .split_first()
            .ok_or_else(|| RuntimeError::module_validation("missing method receiver"))?;
        RetainedReceiver::check_type(cx, receiver)?;
        A::check_types(cx, arguments)
    }
}

impl<A: IntoKagariArguments> IntoKagariArguments for ReceiverArguments<A> {
    fn encode_arguments<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        prepared: &PreparedArguments<'_, Self>,
    ) -> NativeResult<ArgumentValues<'call>> {
        let (receiver_type, arguments_type) = prepared
            .types()
            .split_first()
            .ok_or_else(|| RuntimeError::module_validation("missing method receiver"))?;
        let receiver = cx.encode_prepared(receiver_type, self.receiver)?;
        let arguments =
            PreparedArguments::<A>::from_binding(cx, arguments_type).encode(cx, self.arguments)?;
        cx.prefix_arguments(receiver, arguments)
    }
}
