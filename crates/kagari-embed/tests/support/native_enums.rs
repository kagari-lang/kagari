//! Source-independent native enum provider used by executable boundary tests.
use kagari_runtime::{
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::FunctionDecl,
        module::NativeModule,
        types::Type,
    },
    value::Value,
};

pub fn module() -> NativeResult<NativeModule> {
    let mut module = ModuleBuilder::new("external::enums", &DeclarationCatalog::default());
    let mut event = module.define_enum("Event");
    event.documentation("A native-authored managed enum.");
    let item = event.type_parameter("T")?;
    let data = event.variant("Data", [item.ty(), Type::i32()])?;
    let closed = event.variant("Closed", [])?;
    let event = event.finish()?;
    let mut other = module.define_enum("Other");
    let foreign = other.variant("Data", [Type::i32()])?;
    other.finish()?;
    module.define_enum("Empty").finish()?;

    for name in [
        "make",
        "wrong_payload",
        "wrong_count",
        "foreign_variant",
        "closed",
    ] {
        let function = module.define_function(FunctionDecl::new(name))?;
        module.function(&function, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("value", item.clone());
            function.parameter("sequence", Type::i32());
            function.returns(event.apply([item])?);
            Ok(())
        })?;
        let data = data.clone();
        let closed = closed.clone();
        let foreign = foreign.clone();
        module.bind_with(
            function,
            NativeBinding::new(
                [Codec::Value, Codec::Scalar(Type::i32().abi().clone())],
                event.codec(),
                move |call| {
                    let applied = call.result_type_argument()?;
                    let (variant, fields) = match name {
                        "wrong_payload" => (&data, vec![call.argument(0)?, Value::Bool(true)]),
                        "wrong_count" => (&data, vec![call.argument(0)?]),
                        "foreign_variant" => (&foreign, vec![call.argument(0)?]),
                        "closed" => (&closed, vec![]),
                        _ => (&data, vec![call.argument(0)?, call.argument(1)?]),
                    };
                    call.allocate_enum(&applied, variant, fields)
                },
            ),
        )?;
    }
    for name in [
        "sequence",
        "wrong_field",
        "wrong_variant",
        "is_data",
        "roundtrip",
    ] {
        let function = module.define_function(FunctionDecl::new(name))?;
        module.function(&function, |function| {
            let item = function.type_parameter("T")?.ty();
            let applied = event.apply([item])?;
            function.parameter("event", applied.clone());
            function.returns(if name == "roundtrip" {
                applied
            } else if name == "is_data" {
                Type::bool()
            } else {
                Type::i32()
            });
            Ok(())
        })?;
        let data = data.clone();
        let closed = closed.clone();
        module.bind_with(
            function,
            NativeBinding::new([event.codec()], Codec::Value, move |call| {
                call.collect_garbage()?;
                match name {
                    "is_data" => call.enum_argument_is(0, &data).map(Value::Bool),
                    "roundtrip" => Ok(call.argument(0)?),
                    "wrong_variant" => call.enum_argument_field(0, &closed, 0),
                    "wrong_field" => call.enum_argument_field(0, &data, 2),
                    _ => call.enum_argument_field(0, &data, 1),
                }
            }),
        )?;
    }
    module.finish()
}
