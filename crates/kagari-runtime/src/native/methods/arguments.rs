//! Prefix the independently retained receiver to the ordinary typed argument pack.
use crate::{
    error::RuntimeError,
    frame::{call_slots::ArgumentWriter, types::arguments::TypeArgument},
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            KagariType,
            arguments::{IntoKagariArguments, KagariArguments, PreparedArguments},
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
    fn write_prepared_arguments<'runtime>(
        self,
        cx: &mut ConversionContext<'runtime>,
        prepared: &PreparedArguments<'_, Self>,
        writer: &mut ArgumentWriter<'_, '_, 'runtime>,
    ) -> NativeResult<()> {
        let expected = prepared.types();
        cx.argument_scope(|cx| {
            writer.encode_prepared(cx, 0, self.receiver)?;
            let suffix = PreparedArguments::from_checked(&expected[1..]);
            self.arguments
                .write_prepared_arguments(cx, &suffix, &mut writer.suffix(1)?)
        })
    }
}
