//! An application provider implements propagation using public registration APIs.
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        module::NativeModule,
    },
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::language::Protocol;

pub fn module() -> NativeResult<NativeModule> {
    let standard = StandardDeclarations::default();
    let mut module = ModuleBuilder::new("external::try_carrier", &standard.catalog()?)?;
    let option = StandardDeclarations::enumeration("Option")?;
    let none = option.variant("None")?;
    let residual = option.apply([StandardDeclarations::enumeration("Infallible")?.apply([])?])?;
    let flow = StandardDeclarations::enumeration("ControlFlow")?;
    let continued = flow.variant("Continue")?;
    let stopped = flow.variant("Break")?;
    let mut carrier = module.define_enum("Carrier")?;
    carrier.documentation("An application-owned propagation carrier with traced payloads.");
    let item = carrier.type_parameter("T")?;
    let data = carrier.variant("Data", [item.ty()])?;
    let stop = carrier.variant("Stop", [])?;
    carrier.variant_documentation(&data, "Continue with one application item.")?;
    carrier.variant_documentation(&stop, "Return without an item.")?;
    let carrier = carrier.finish()?;
    module.implement(carrier.clone(), |implementation| {
        let item = implementation.parameter("T")?.ty();
        implementation.trait_impl(
            standard
                .protocol(Protocol::FromResidual)
                .apply([residual.clone()])?,
            |methods| {
                let stop = stop.clone();
                let none = none.clone();
                methods.bind("from_residual", unsafe {
                    NativeBinding::new_transport([option.codec()], carrier.codec(), move |cx| {
                        if !cx.enum_argument_is(0, &none)? {
                            return Err(RuntimeError::module_validation("uninhabited residual"));
                        }
                        // SAFETY: The argument pack and residual root protect payloads; each result
                        // is rooted or returned before any safepoint.
                        cx.allocate_enum_transport(&cx.result_type_argument()?, &stop, vec![])
                    })
                })
            },
        )?;
        implementation.trait_impl(standard.protocol(Protocol::Try).apply([])?, |methods| {
            methods.associated_type("Output", item)?;
            methods.associated_type("Residual", residual.clone())?;
            let from_data = data.clone();
            methods.bind("from_output", unsafe {
                NativeBinding::new_transport([Codec::Value], carrier.codec(), move |cx| {
                    // SAFETY: The frame retains the payload through terminal enum publication.
                    cx.allocate_enum_transport(
                        &cx.result_type_argument()?,
                        &from_data,
                        vec![cx.argument_transport(0)?],
                    )
                })
            })?;
            methods.documentation(
                "branch",
                "Inspect the installed carrier once; preserve its traced item.",
            )?;
            methods.bind("branch", unsafe {
                NativeBinding::new_transport([carrier.codec()], flow.codec(), move |cx| {
                    let result = cx.result_type_argument()?;
                    cx.collect_garbage()?;
                    if cx.enum_argument_is(0, &data)? {
                        // SAFETY: The call retains the source enum through destination publication.
                        let value = cx.enum_argument_field_transport(0, &data, 0)?;
                        // SAFETY: The argument pack and residual root protect payloads; each result
                        // is rooted or returned before any safepoint.
                        return cx.allocate_enum_transport(&result, &continued, vec![value]);
                    }
                    if !cx.enum_argument_is(0, &stop)? {
                        return Err(RuntimeError::module_validation("carrier member"));
                    }
                    let applied_residual = cx.type_parameter(&result, 0)?;
                    let residual =
                        // SAFETY: The argument pack and residual root protect payloads; each result
                        // is rooted or returned before any safepoint.
                        cx.allocate_enum_transport(&applied_residual, &none, vec![])?;
                    let _root = cx
                        .heap()
                        .root_value_transport(residual)
                        .ok_or_else(|| RuntimeError::module_validation("residual root"))?;
                    // SAFETY: The argument pack and residual root protect payloads; each result is
                    // rooted or returned before any safepoint.
                    cx.allocate_enum_transport(&result, &stopped, vec![residual])
                })
            })
        })
    })?;
    module.finish()
}
