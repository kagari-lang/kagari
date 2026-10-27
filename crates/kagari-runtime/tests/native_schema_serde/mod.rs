//! Serde executes its own representation against static, source-free DTO facts.
use kagari_stdlib::serde::Serde;
mod formatting;
mod snapshot;
use super::program_for_source;
use kagari_derive::KagariSchema;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    gc::GcHeapConfig,
    native::{
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, IntoKagari, KagariType,
            context::{ConversionContext, ConversionLimits},
        },
        module::NativeModule,
    },
};
use kagari_stdlib::declarations::StandardDeclarations;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};

#[derive(Default, Debug, PartialEq)]
struct LocalState;

#[derive(Debug, PartialEq, Serialize, Deserialize, KagariSchema)]
#[serde(rename = "Request", rename_all = "camelCase")]
struct Input {
    target_id: i64,
    #[serde(rename = "power")]
    damage: i32,
    nested: Vec<Choice>,
    maybe: Option<String>,
    result: Result<(bool, usize, isize), String>,
    #[serde(skip)]
    local: LocalState,
    #[serde(default)]
    label: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, KagariSchema)]
#[serde(rename_all = "snake_case")]
enum Choice {
    NoData,
    OneValue(i16),
    PairValues(String, u32),
    SkippedPayload(#[serde(skip)] LocalState),
    PartialPayload(i8, #[serde(skip)] LocalState, f32),
}

#[derive(Deserialize, KagariSchema)]
struct Transformed {
    value: i32,
}

impl Serialize for Transformed {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("Transformed", 1)?;
        record.serialize_field("value", &(self.value * 2))?;
        record.end()
    }
}

fn module() -> NativeModule {
    let providers = StandardDeclarations::default().catalog().unwrap();
    let mut builder = ModuleBuilder::new("game::serde", &providers).unwrap();
    builder.register_type::<Serde<Input>>().unwrap();
    builder.register_type::<Serde<Transformed>>().unwrap();
    builder.register_type::<Serde<snapshot::Graph>>().unwrap();
    assert!(Serde::<Input>::kagari_type(&DeclarationCatalog::default()).is_err());
    builder.finish().unwrap()
}

#[test]
fn serde_schema_roundtrips_renames_skips_composites_and_all_variant_shapes() {
    let module = module();
    let declaration = module.declaration();
    assert!(declaration.functions.is_empty());
    assert!(declaration.implementations.is_empty());
    let request = declaration
        .types
        .iter()
        .find(|item| item.name == "Request")
        .unwrap();
    assert_eq!(
        request
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["targetId", "power", "nested", "maybe", "result", "label"]
    );
    let program = program_for_source(
        &module,
        "use game::serde::Request; pub fn inspect(input: Request) -> i32 { input.power }",
    );
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let owner_program = runtime.load_program("serde", program).unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    for (maybe, result, nested) in [
        (
            Some("target".into()),
            Ok((true, usize::MAX, isize::MIN)),
            vec![
                Choice::NoData,
                Choice::OneValue(-9),
                Choice::PairValues("pair".into(), u32::MAX),
                Choice::SkippedPayload(LocalState),
                Choice::PartialPayload(7, LocalState, 3.5),
            ],
        ),
        (None, Err("business failure".into()), vec![]),
    ] {
        let root = cx
            .encode(Serde(Input {
                target_id: 8,
                damage: 20,
                nested,
                maybe,
                result,
                local: LocalState,
                label: "label".into(),
            }))
            .unwrap();
        runtime.gc_safepoint().unwrap();
        let Serde(decoded): Serde<Input> = cx.decode(&root).unwrap();
        assert_eq!(decoded.target_id, 8);
        assert_eq!(decoded.damage, 20);
        assert_eq!(decoded.local, LocalState);
        let copy = cx.encode(Serde(decoded)).unwrap();
        let original: Serde<Input> = cx.decode(&root).unwrap();
        let copied: Serde<Input> = cx.decode(&copy).unwrap();
        assert_eq!(original, copied);
    }
    // The callback's serialization, rather than the direct Rust field, defines
    // the wire value. KagariSchema alone never substitutes a converter.
    let root = cx.encode(Serde(Transformed { value: 7 })).unwrap();
    let Serde(decoded): Serde<Transformed> = cx.decode(&root).unwrap();
    assert_eq!(decoded.value, 14);

    // Serde's fixed-width visits also serve Rust machine-sized conveniences,
    // without narrowing the stored script value on 32-bit hosts.
    for value in [u32::MAX as u64 + 1, u64::MAX] {
        let root = cx.encode(Serde(value)).unwrap();
        assert_eq!(cx.decode::<Serde<u64>>(&root).unwrap().0, value);
        let host = cx.decode::<Serde<usize>>(&root);
        match usize::try_from(value) {
            Ok(expected) => assert_eq!(host.unwrap().0, expected),
            Err(_) => assert!(host.is_err()),
        }
    }
    for value in [i64::MIN, i64::MAX] {
        let root = cx.encode(Serde(value)).unwrap();
        assert_eq!(cx.decode::<Serde<i64>>(&root).unwrap().0, value);
        let host = cx.decode::<Serde<isize>>(&root);
        match isize::try_from(value) {
            Ok(expected) => assert_eq!(host.unwrap().0, expected),
            Err(_) => assert!(host.is_err()),
        }
    }
}

#[test]
fn serde_char_roundtrips_scalars_and_containers_without_integer_coercion() {
    let (runtime, owner) = super::conversion_fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    for ch in ['\0', '\'', 'é', '🙂', char::MAX] {
        let root = cx.encode(Serde(ch)).unwrap();
        assert_eq!(cx.decode::<Serde<char>>(&root).unwrap().0, ch);
        assert!(cx.decode::<Serde<u32>>(&root).is_err());
        assert!(cx.decode::<Serde<String>>(&root).is_err());
        let integer = cx.encode(Serde(u32::from(ch))).unwrap();
        assert!(cx.decode::<Serde<char>>(&integer).is_err());
    }
    let expected = (vec!['\0', 'é', '🙂'], Some('a'));
    let root = cx.encode(Serde(expected.clone())).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        cx.decode::<Serde<(Vec<char>, Option<char>)>>(&root)
            .unwrap()
            .0,
        expected
    );
}

#[test]
fn serde_uses_conversion_limits_and_reports_nested_error_paths() {
    let module = module();
    let program = program_for_source(
        &module,
        "use game::serde::Request; pub fn inspect(input: Request) -> i32 { input.power }",
    );
    let mut runtime = Runtime::default();
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let owner_program = runtime.load_program("serde", program).unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    let mut cx = ConversionContext::with_limits(
        &runtime,
        &owner,
        ConversionLimits {
            max_string_bytes: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let error = cx
        .encode(Serde(Input {
            target_id: 0,
            damage: 0,
            nested: vec![Choice::PairValues("oversized".into(), 1)],
            maybe: None,
            result: Ok((false, 0, 0)),
            local: LocalState,
            label: String::new(),
        }))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
    assert!(error.to_string().contains("field nested"));
    assert!(error.to_string().contains("element 0"));
    runtime.gc_safepoint().unwrap();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let root = cx.encode(Serde(Transformed { value: 2 })).unwrap();
    assert_eq!(cx.decode::<Serde<Transformed>>(&root).unwrap().0.value, 4);
}

#[derive(Serialize, KagariSchema)]
struct OutputOnly {
    value: i32,
}

#[derive(Deserialize, KagariSchema)]
struct InputOnly {
    value: i32,
}

#[test]
fn serde_capabilities_are_directional() {
    fn outbound<T: IntoKagari>() {}

    fn inbound<T: FromKagari>() {}
    outbound::<Serde<OutputOnly>>();
    inbound::<Serde<InputOnly>>();
    let value = InputOnly { value: 4 };
    assert_eq!(value.value, 4);
}
