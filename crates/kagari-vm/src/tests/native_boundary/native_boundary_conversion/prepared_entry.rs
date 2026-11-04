//! Real bind/link/application owners prepare Rust mappings before native bodies.
use super::source_free_vm;
use crate::tests::native_boundary::{compile_program, support::round_trip_program};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        declarations::FunctionDecl,
        module::NativeModule,
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

static CLOSED_MAPPINGS: AtomicUsize = AtomicUsize::new(0);
static GENERIC_MAPPINGS: AtomicUsize = AtomicUsize::new(0);

struct Counted(i32);

impl KagariType for Counted {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        CLOSED_MAPPINGS.fetch_add(1, Ordering::SeqCst);
        cx.check_type::<i32>(expected)
    }
}

impl FromKagari for Counted {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        i32::from_kagari(cx, expected, value).map(Self)
    }
}

impl IntoKagari for Counted {
    fn into_kagari(self, _: &mut ConversionContext<'_>, _: &TypeArgument) -> NativeResult<Value> {
        Ok(if self.0 < 0 {
            Value::Bool(true)
        } else {
            Value::I32(self.0)
        })
    }
}

#[test]
fn closed_native_mappings_prepare_once_and_actual_results_remain_checked() {
    let mut builder = ModuleBuilder::new(
        "example::prepared",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let declaration = builder
        .define_function(
            FunctionDecl::new("echo")
                .parameter("value", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    builder
        .bind(
            declaration,
            move |_: &mut NativeContext<'_>, (value,): (Counted,)| -> NativeResult<Counted> {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(value)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, program) = source_free_vm(
        "pub fn run(value: i32) -> i32 { example::prepared::echo(value) }",
        &module,
    );
    let owner = vm.runtime().select_program(&program).unwrap();
    assert_eq!(
        CLOSED_MAPPINGS.load(Ordering::SeqCst),
        2,
        "input and output admitted at link"
    );
    for value in [41, 42] {
        assert_eq!(
            vm.execute_typed::<_, i32>(&owner, "run", (value,)).unwrap(),
            value
        );
    }
    assert!(vm.execute_typed::<_, i32>(&owner, "run", (-1,)).is_err());
    assert_eq!(
        effects.load(Ordering::SeqCst),
        3,
        "failed return retains completed effects"
    );
    assert_eq!(
        vm.execute_typed::<_, i32>(&owner, "run", (43,)).unwrap(),
        43
    );
    assert_eq!(CLOSED_MAPPINGS.load(Ordering::SeqCst), 2);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

struct CountedValue(ScriptValue);

impl KagariType for CountedValue {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        ScriptValue::kagari_type(catalog)
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        GENERIC_MAPPINGS.fetch_add(1, Ordering::SeqCst);
        ScriptValue::check_type(cx, expected)
    }
}

impl FromKagari for CountedValue {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        cx.runtime().collect_garbage()?;
        ScriptValue::from_kagari(cx, expected, value).map(Self)
    }
}

impl IntoKagari for CountedValue {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        self.0.into_kagari(cx, expected)
    }
}

#[test]
fn applied_native_mappings_reuse_real_scalar_and_nominal_applications_through_gc() {
    let mut builder = ModuleBuilder::new(
        "example::prepared",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let declaration = builder
        .define_function(FunctionDecl::new("identity"))
        .unwrap();
    builder
        .function(&declaration, |function| {
            let ty = function.type_parameter("T")?.ty();
            function.parameter("value", ty.clone())?;
            function.returns(ty)
        })
        .unwrap();
    builder
        .bind(
            declaration,
            |cx: &mut NativeContext<'_>, (value,): (CountedValue,)| -> NativeResult<CountedValue> {
                cx.collect_garbage()?;
                Ok(value)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, program) = source_free_vm(
        r#"
        use example::prepared::identity;
        struct Item { val value: i32 }
        pub fn scalar() -> i32 { identity(42) }
        pub fn object() -> i32 { identity(Item { value: 43 }).value }
    "#,
        &module,
    );
    let owner = vm.runtime().select_program(&program).unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(&owner, "scalar", ()).unwrap(),
        42
    );
    assert_eq!(
        vm.execute_typed::<_, i32>(&owner, "object", ()).unwrap(),
        43
    );
    let checked = GENERIC_MAPPINGS.load(Ordering::SeqCst);
    assert_eq!(checked, 4, "two mappings per concrete application");
    for _ in 0..2 {
        assert_eq!(
            vm.execute_typed::<_, i32>(&owner, "scalar", ()).unwrap(),
            42
        );
        assert_eq!(
            vm.execute_typed::<_, i32>(&owner, "object", ()).unwrap(),
            43
        );
    }
    assert_eq!(GENERIC_MAPPINGS.load(Ordering::SeqCst), checked);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn wrong_native_rust_mapping_rejects_source_free_publication_before_body_effects() {
    let mut builder = ModuleBuilder::new(
        "example::prepared",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let declaration = builder
        .define_function(FunctionDecl::new("wrong").returns(Type::i32()))
        .unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    builder
        .bind(
            declaration,
            move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<bool> {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(true)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let program = round_trip_program(compile_program(
        "pub fn main() -> i32 { example::prepared::wrong() }",
        Some(&module),
    ));
    let mut runtime = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
    let error = runtime.load_program("wrong-native", program).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert!(!runtime.is_quarantined());
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}
