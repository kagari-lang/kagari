//! Optional text output backed by an explicitly supplied host sink.
use kagari_runtime::{
    host::{HostCallContext, HostError},
    native::{
        binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
        declarations::FunctionDecl, host, module::NativeModule, types::Type,
    },
    value::view::ValueView,
};
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    scalar::BuiltinType,
};
use std::sync::Arc;

/// Build `std::io` independently of the standard bundle. The engine does not
/// expose its functions until this module is installed.
///
/// Each call invokes `write` once with a protected String and its suffix (`""`
/// for `print`, `"\n"` for `println`). Write the message followed by the suffix;
/// callback errors trap through the normal host-call boundary. The sink can use
/// `HostCallContext::read_string` without copying the complete message.
///
/// Returns an error if the module's declarations or bindings cannot be sealed
/// against `catalog`.
pub fn module(
    catalog: &DeclarationCatalog,
    write: impl for<'call> Fn(
        &mut HostCallContext<'call>,
        &ValueView<'call>,
        &'static str,
    ) -> Result<(), HostError>
    + Send
    + Sync
    + 'static,
) -> NativeResult<NativeModule> {
    let mut module = ModuleBuilder::new("std::io", catalog)?;
    let write = Arc::new(write);
    for (name, suffix, documentation) in [
        (
            "print",
            "",
            "Write text to the installed output sink without a newline.",
        ),
        (
            "println",
            "\n",
            "Write text followed by a newline to the installed output sink.",
        ),
    ] {
        let function_policy = HostCallPolicy {
            passing: vec![HostPassingStyle::SharedBorrow],
            effects: HostFunctionEffects {
                may_trap: true,
                may_call_host_services: true,
                may_mutate_host_state: true,
                ..Default::default()
            },
        };
        let function = module.define_function(
            FunctionDecl::new(name)
                .parameter("message", Type::scalar(BuiltinType::String))
                .documentation(documentation)
                .host_policy(function_policy.clone()),
        )?;
        let write = write.clone();
        module.bind(
            function,
            host::callback(function_policy, move |context, arguments| {
                let [message] = arguments else {
                    return Err(HostError::new("std::io output expects one String"));
                };
                write(context, message, suffix)?;
                Ok(ValueView::unit())
            }),
        )?;
    }
    module.finish()
}
