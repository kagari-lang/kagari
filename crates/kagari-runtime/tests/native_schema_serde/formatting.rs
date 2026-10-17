//! Custom serializers preserve work limits and validation-before-child effects.
use super::{Transformed, module, program_for_source};
use kagari_runtime::{
    Runtime,
    module::LoadedModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            KagariType,
            context::{ConversionContext, ConversionLimits},
        },
        module::NativeModule,
        schema::TypeRegistration,
        serde::{KagariSchema, Serde},
        types::Type,
    },
};
use serde::{Serialize, Serializer, ser::SerializeStruct};
use std::{
    cell::Cell,
    fmt::{self, Display},
    rc::Rc,
};

struct Formatted(Rc<Cell<usize>>);

impl Display for Formatted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for _ in 0..100 {
            self.0.set(self.0.get() + 1);
            formatter.write_str("ab")?;
        }
        Ok(())
    }
}

impl Serialize for Formatted {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl KagariSchema for Formatted {
    fn schema_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        String::kagari_type(catalog)
    }

    fn register_schema(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        String::register_type(registration)
    }
}

struct Duplicate(Rc<Cell<usize>>);

impl Serialize for Duplicate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("Transformed", 2)?;
        record.serialize_field("value", &1i32)?;
        record.serialize_field("value", &Formatted(Rc::clone(&self.0)))?;
        record.end()
    }
}

impl KagariSchema for Duplicate {
    fn schema_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Serde::<Transformed>::kagari_type(catalog)
    }

    fn register_schema(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        registration.dependency::<Serde<Transformed>>()
    }
}

fn fixture() -> (Runtime, LoadedModule) {
    let module = module();
    let program = program_for_source(
        &module,
        "use game::serde::Transformed; pub fn inspect(input: Transformed) -> i32 { input.value }",
    );
    let mut runtime = Runtime::default();
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let owner = runtime.load_program("serde", program).unwrap();
    (runtime, owner)
}

#[test]
fn serde_formatted_output_is_bounded_and_duplicate_fields_skip_child_effects() {
    let (runtime, owner) = fixture();
    let calls = Rc::new(Cell::new(0));
    let mut cx = ConversionContext::with_limits(
        &runtime,
        &owner,
        ConversionLimits {
            max_string_bytes: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let error = cx.encode(Serde(Formatted(Rc::clone(&calls)))).unwrap_err();
    assert!(error.message().contains("string byte limit"));
    assert_eq!(calls.get(), 2);
    assert_eq!(runtime.gc().active_roots(), 0);
    calls.set(0);
    let error = cx.encode(Serde(Duplicate(Rc::clone(&calls)))).unwrap_err();
    assert!(error.message().contains("duplicate object initializer"));
    assert_eq!(calls.get(), 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}
