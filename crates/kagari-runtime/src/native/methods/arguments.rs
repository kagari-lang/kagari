//! Prefix the independently retained receiver to the ordinary typed argument pack.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootSet,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            KagariType,
            arguments::{IntoKagariArguments, KagariArguments},
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
    fn into_arguments(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &[TypeArgument],
    ) -> NativeResult<RootSet> {
        Self::check_types(cx, expected)?;
        cx.argument_scope(|cx| {
            let receiver = cx.encode_prepared(&expected[0], self.receiver)?;
            let arguments = self.arguments.into_arguments(cx, &expected[1..])?;
            let mut values = Vec::new();
            values
                .try_reserve_exact(expected.len())
                .map_err(|_| RuntimeError::resource_limit("method arguments"))?;
            values.push(receiver);
            for slot in 0..expected.len() - 1 {
                values.push(
                    arguments
                        .get(cx.runtime().gc(), slot)
                        .ok_or_else(|| RuntimeError::module_validation("method argument root"))?,
                );
            }
            cx.runtime()
                .gc()
                .root_execution_values(values)
                .ok_or_else(|| RuntimeError::module_validation("method argument retention"))
        })
    }
}
