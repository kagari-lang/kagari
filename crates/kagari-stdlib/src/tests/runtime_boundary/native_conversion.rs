use crate::tests::bytecode::load_bytecode;
use crate::tests::runtime_boundary::support::compile_program;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::arguments::TypeArgument,
    gc::{GcHeapConfig, roots::RootedValue},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, IntoKagari, KagariType,
            arguments::{FromKagariArguments, IntoKagariArguments},
            context::{ConversionContext, ConversionLimits},
        },
        module::NativeModule,
        types::Type,
    },
    value::Value,
};
use kagari_types::ty::Ty;
use std::{
    cell::Cell,
    fmt::Debug,
    panic::{AssertUnwindSafe, catch_unwind},
};

fn fixture() -> (Runtime, ExecutableModule) {
    fixture_with(
        None,
        "fn signature(value: Option<Result<(String, (i32, i32)), String>>) {}",
    )
}

fn fixture_with(extra: Option<NativeModule>, source: &str) -> (Runtime, ExecutableModule) {
    let mut modules = crate::modules().unwrap();
    modules.extend(extra);
    let program = compile_program(&modules, "conversion.kgr", source);
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let loaded = load_bytecode(&mut runtime, "conversion", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    (runtime, loaded)
}

#[test]
fn nested_owned_values_round_trip_under_collection_and_release_temporary_roots() {
    type Entry = Option<Result<(String, (i32, i32)), String>>;
    type NestedValues = (Entry, Entry, Entry);
    let (runtime, owner) = fixture();
    let input = (
        Some(Ok(("火🔥".to_owned(), (20, 22)))),
        None,
        Some(Err("business error".to_owned())),
    );
    let root = {
        let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
        let root = cx.encode(input.clone()).unwrap();
        assert_eq!(runtime.gc().active_roots(), 1);
        runtime.collect_garbage().unwrap();
        let decoded: NestedValues = cx.decode(&root).unwrap();
        assert_eq!(decoded, input);
        assert_eq!(runtime.gc().active_roots(), 1);
        root
    };
    assert!(runtime.gc().stats().collections > 0);
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn scalar_widths_tuple_arity_and_foreign_roots_are_checked() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let values = (
        i8::MIN,
        i16::MAX,
        i32::MIN,
        i64::MAX,
        isize::MIN,
        u8::MAX,
        u16::MAX,
        u32::MAX,
        u64::MAX,
        usize::MAX,
        1.25f32,
        2.5f64,
    );
    let root = cx.encode(values).unwrap();
    assert_eq!(
        cx.decode::<(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64)>(&root)
            .unwrap(),
        values
    );
    let unsigned = (1u64 << 32, u64::MAX);
    let root = cx.encode(unsigned).unwrap();
    assert_eq!(cx.decode::<(u64, u64)>(&root).unwrap(), unsigned);
    let signed = (i64::MIN, i64::MAX);
    let root = cx.encode(signed).unwrap();
    assert_eq!(cx.decode::<(i64, i64)>(&root).unwrap(), signed);
    let unit = cx.encode(()).unwrap();
    assert_eq!(unit.value(runtime.gc()), Some(Value::Unit));
    let tuple = cx.encode((true,)).unwrap();
    assert_eq!(cx.decode::<(bool,)>(&tuple).unwrap(), (true,));
    let invalid = runtime.root_value(Value::I32(256)).unwrap();
    assert!(cx.decode::<u8>(&invalid).is_err());
    for values in [vec![], vec![Value::I32(42)]] {
        let array = runtime
            .alloc_array(
                &owner,
                Ty::Builtin(kagari_types::scalar::BuiltinType::I32),
                values,
            )
            .unwrap();
        let root = runtime.root_value(Value::Array(array)).unwrap();
        assert!(cx.decode::<(i32, i32)>(&root).is_err());
    }
    let foreign = Runtime::default().root_value(Value::I32(1)).unwrap();
    assert!(cx.decode::<i32>(&foreign).is_err());
}

#[test]
fn supported_tuple_arities_each_round_trip_as_one_value() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    macro_rules! round_trip {
        ($($ty:ty => $value:expr),*) => {
            let value = ($($value,)*);
            let root = cx.encode(value).unwrap();
            assert_eq!(cx.decode::<($($ty,)*)>(&root).unwrap(), value);
        };
    }
    round_trip!();
    round_trip!(i32 => 1);
    round_trip!(i32 => 1, i32 => 2);
    round_trip!(i32 => 1, i32 => 2, i32 => 3);
    round_trip!(i32 => 1, i32 => 2, i32 => 3, i32 => 4);
    round_trip!(i32 => 1, i32 => 2, i32 => 3, i32 => 4, i32 => 5);
    round_trip!(i32 => 1, i32 => 2, i32 => 3, i32 => 4, i32 => 5, i32 => 6);
    round_trip!(i32 => 1, i32 => 2, i32 => 3, i32 => 4, i32 => 5, i32 => 6, i32 => 7);
    round_trip!(i32 => 1, i32 => 2, i32 => 3, i32 => 4, i32 => 5, i32 => 6, i32 => 7, i32 => 8);
    round_trip!(i32 => 1, i32 => 2, i32 => 3, i32 => 4, i32 => 5, i32 => 6, i32 => 7, i32 => 8, i32 => 9);
    round_trip!(i32 => 1, i32 => 2, i32 => 3, i32 => 4, i32 => 5, i32 => 6, i32 => 7, i32 => 8, i32 => 9, i32 => 10);
    round_trip!(i32 => 1, i32 => 2, i32 => 3, i32 => 4, i32 => 5, i32 => 6, i32 => 7, i32 => 8, i32 => 9, i32 => 10, i32 => 11);
    round_trip!(i32 => 1, i32 => 2, i32 => 3, i32 => 4, i32 => 5, i32 => 6, i32 => 7, i32 => 8, i32 => 9, i32 => 10, i32 => 11, i32 => 12);
}

fn round_trip_arguments<A>(cx: &mut ConversionContext<'_>, value: A, count: usize)
where
    A: IntoKagariArguments + FromKagariArguments + KagariType + Clone + PartialEq + Debug,
{
    let tuple = cx.type_for::<A>().unwrap();
    let expected = (0..count)
        .map(|index| cx.parameter(&tuple, index).unwrap())
        .collect::<Vec<_>>();
    let arguments = value.clone().into_arguments(cx, &expected).unwrap();
    let slots = (0..count)
        .map(|index| arguments.get(cx.runtime().gc(), index).unwrap())
        .collect::<Vec<_>>();
    assert!(arguments.get(cx.runtime().gc(), count).is_none());
    assert_eq!(A::from_arguments(cx, &expected, &slots).unwrap(), value);
}

#[test]
fn outer_argument_tuples_cover_zero_through_twelve_and_preserve_nested_tuple_values() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    round_trip_arguments(&mut cx, (), 0);
    round_trip_arguments(&mut cx, (1,), 1);
    round_trip_arguments(&mut cx, (1, 2), 2);
    round_trip_arguments(&mut cx, (1, 2, 3), 3);
    round_trip_arguments(&mut cx, (1, 2, 3, 4), 4);
    round_trip_arguments(&mut cx, (1, 2, 3, 4, 5), 5);
    round_trip_arguments(&mut cx, (1, 2, 3, 4, 5, 6), 6);
    round_trip_arguments(&mut cx, (1, 2, 3, 4, 5, 6, 7), 7);
    round_trip_arguments(&mut cx, (1, 2, 3, 4, 5, 6, 7, 8), 8);
    round_trip_arguments(&mut cx, (1, 2, 3, 4, 5, 6, 7, 8, 9), 9);
    round_trip_arguments(&mut cx, (1, 2, 3, 4, 5, 6, 7, 8, 9, 10), 10);
    round_trip_arguments(&mut cx, (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11), 11);
    round_trip_arguments(&mut cx, (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12), 12);
    round_trip_arguments(
        &mut cx,
        ("first".to_owned(), ("second".to_owned(),), (3, true)),
        3,
    );
    let tuple = cx.type_for::<(i32, i32)>().unwrap();
    let arguments = ((1, 2),).into_arguments(&mut cx, &[tuple]).unwrap();
    let Some(Value::Tuple(id)) = arguments.get(runtime.gc(), 0) else {
        panic!("tuple argument")
    };
    assert_eq!(
        &*runtime.gc().tuple(id).unwrap(),
        &[Value::I32(1), Value::I32(2)]
    );
    assert_eq!(arguments.get(runtime.gc(), 1), None);
    drop(arguments);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

struct Observed<'a>(&'a Cell<usize>);

thread_local! { static DECODE_EFFECTS: Cell<usize> = const { Cell::new(0) }; }
struct Decoded;

impl KagariType for Decoded {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl FromKagari for Decoded {
    fn from_kagari(
        _: &mut ConversionContext<'_>,
        _: &TypeArgument,
        _: &Value,
    ) -> NativeResult<Self> {
        DECODE_EFFECTS.set(DECODE_EFFECTS.get() + 1);
        Ok(Self)
    }
}

impl KagariType for Observed<'_> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl IntoKagari for Observed<'_> {
    fn into_kagari(self, _: &mut ConversionContext<'_>, _: &TypeArgument) -> NativeResult<Value> {
        self.0.set(self.0.get() + 1);
        Ok(Value::I32(7))
    }
}

#[test]
fn argument_signature_errors_precede_any_user_conversion_effects() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let effects = Cell::new(0);
    let integer = cx.type_for::<i32>().unwrap();
    let boolean = cx.type_for::<bool>().unwrap();
    assert!(
        (Observed(&effects), false)
            .into_arguments(&mut cx, &[integer.clone(), integer.clone()])
            .is_err()
    );
    assert!(
        (Observed(&effects), false)
            .into_arguments(&mut cx, &[])
            .is_err()
    );
    assert_eq!(effects.get(), 0);
    let arguments = (Observed(&effects), false)
        .into_arguments(&mut cx, &[integer.clone(), boolean.clone()])
        .unwrap();
    assert_eq!(effects.get(), 1);
    assert_eq!(arguments.get(runtime.gc(), 0), Some(Value::I32(7)));
    DECODE_EFFECTS.set(0);
    let signature = [integer, boolean];
    assert!(
        <(Decoded, bool)>::from_arguments(
            &mut cx,
            &signature,
            &[
                Value::I32(7),
                runtime.gc().alloc_string("wrong".into()).unwrap()
            ]
        )
        .is_err()
    );
    assert_eq!(DECODE_EFFECTS.get(), 0);
    <(Decoded, bool)>::from_arguments(&mut cx, &signature, &[Value::I32(7), Value::Bool(false)])
        .unwrap();
    assert_eq!(DECODE_EFFECTS.get(), 1);
}

#[test]
fn missing_nominal_providers_and_bounded_conversion_fail_without_retention() {
    assert!(Option::<i32>::kagari_type(&DeclarationCatalog::default()).is_err());
    assert!(Result::<i32, String>::kagari_type(&DeclarationCatalog::default()).is_err());
    let (runtime, owner) = fixture();
    for (limits, run) in [
        (
            ConversionLimits {
                max_depth: 1,
                ..Default::default()
            },
            0,
        ),
        (
            ConversionLimits {
                max_nodes: 1,
                ..Default::default()
            },
            1,
        ),
        (
            ConversionLimits {
                max_string_bytes: 1,
                ..Default::default()
            },
            2,
        ),
    ] {
        let mut cx = ConversionContext::with_limits(&runtime, &owner, limits).unwrap();
        let error = match run {
            0 => cx.encode(((1,),)).unwrap_err(),
            1 => cx.encode((1, 2)).unwrap_err(),
            _ => cx.encode("火".to_owned()).unwrap_err(),
        };
        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        assert_eq!(runtime.gc().active_roots(), 0);
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    }
}

#[test]
fn conversion_observes_root_cancellation_without_publishing_partial_values() {
    let (runtime, owner) = fixture();
    let options = runtime.execution_options();
    let cancellation = options.cancellation.clone();
    let session = runtime.begin_execution(&owner, options).unwrap();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    cancellation.cancel();
    assert_eq!(
        cx.encode(((1, 2),)).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    drop(cx);
    drop(session);
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let root = cx.encode((1, 2)).unwrap();
    assert_eq!(cx.decode::<(i32, i32)>(&root).unwrap(), (1, 2));
}

struct Failing {
    panic: bool,
}

impl KagariType for Failing {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        <(String,)>::kagari_type(catalog)
    }
}

impl IntoKagari for Failing {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        let value = cx.encode_value(expected, ("retained until failure".to_owned(),))?;
        cx.runtime().collect_garbage()?;
        assert!(cx.runtime().gc().validate_value(&value));
        assert!(!self.panic, "conversion unwind fixture");
        Err(RuntimeError::module_validation("conversion rejected"))
    }
}

#[test]
fn conversion_errors_and_panics_release_scratch_roots_and_restore_scope_depth() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    assert!(cx.encode(Failing { panic: false }).is_err());
    assert_eq!(runtime.gc().active_roots(), 0);
    assert!(catch_unwind(AssertUnwindSafe(|| cx.encode(Failing { panic: true }))).is_err());
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    let root = cx.encode(("still usable".to_owned(),)).unwrap();
    assert_eq!(
        cx.decode::<(String,)>(&root).unwrap(),
        ("still usable".to_owned(),)
    );
}

// This explicit test provider maps its recursive graph to builtin arrays. It
// does not install a Vec identity or implement automatic Rust collection conversion.
struct Children<T>(Vec<T>);

impl<T: KagariType> KagariType for Children<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Type::array(T::kagari_type(catalog)?)
    }
}

impl<T: IntoKagari> IntoKagari for Children<T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.check_elements(self.0.len())?;
        let item = cx.parameter(expected, 0)?;
        let values = self
            .0
            .into_iter()
            .map(|value| cx.encode_value(&item, value))
            .collect::<NativeResult<Vec<_>>>()?;
        cx.runtime()
            .alloc_array(cx.owner(), item.ty().clone(), values)
            .map(Value::Array)
    }
}

impl<T: FromKagari> FromKagari for Children<T> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let Value::Array(id) = value else {
            return Err(RuntimeError::module_validation("children array"));
        };
        let snapshot = cx
            .runtime()
            .gc()
            .array_snapshot(*id)
            .ok_or_else(|| RuntimeError::module_validation("children array"))?;
        cx.check_elements(snapshot.len())?;
        let count = snapshot.len();
        let roots = cx
            .runtime()
            .gc()
            .root_execution_values(snapshot)
            .ok_or_else(|| RuntimeError::module_validation("children roots"))?;
        let item = cx.parameter(expected, 0)?;
        let mut children = Vec::new();
        for index in 0..count {
            let value = roots.get(cx.runtime().gc(), index).unwrap();
            children.push(cx.decode_value(&item, &value)?);
        }
        Ok(Self(children))
    }
}

#[derive(Debug, PartialEq)]
struct Node(Vec<Node>);

struct RetainedNode(RootedValue);

impl KagariType for RetainedNode {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Node::kagari_type(catalog)
    }
}

impl FromKagari for RetainedNode {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        _: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        cx.runtime()
            .root_value(*value)
            .map(Self)
            .ok_or_else(|| RuntimeError::module_validation("retained Node"))
    }
}

struct NodeView(Vec<RetainedNode>);

impl KagariType for NodeView {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Node::kagari_type(catalog)
    }
}

impl FromKagari for NodeView {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let (children,): (Children<_>,) = cx.decode_variant(expected, value, "Children")?;
        Ok(Self(children.0))
    }
}

fn node_identity() -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity {
            package: PackageId("fixture".into()),
            path: vec!["conversion".into()],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Enum,
            name: "Node".into(),
            occurrence: 0,
        }],
    }
}

impl KagariType for Node {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.type_reference(&node_identity())?.apply([])
    }
}

impl IntoKagari for Node {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.encode_variant(expected, "Children", (Children(self.0),))
    }
}

impl FromKagari for Node {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let (children,): (Children<_>,) = cx.decode_variant(expected, value, "Children")?;
        Ok(Self(children.0))
    }
}

#[test]
fn owned_conversion_rejects_a_cycle_but_accepts_shared_noncyclic_children() {
    let catalog =
        DeclarationCatalog::from_modules(&crate::modules().unwrap().iter().collect::<Vec<_>>())
            .unwrap();
    let mut builder = ModuleBuilder::new("fixture::conversion", &catalog).unwrap();
    let mut node = builder.define_enum("Node").unwrap();
    let children = Type::array(node.self_type().unwrap()).unwrap();
    node.variant("Children", [children]).unwrap();
    node.finish().unwrap();
    let (runtime, owner) = fixture_with(
        Some(builder.finish().unwrap()),
        "fn signature(value: fixture::conversion::Node) {}",
    );
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let root = cx
        .encode(Node(vec![Node(vec![]), Node(vec![]), Node(vec![])]))
        .unwrap();
    let Value::Enum(node) = root.value(runtime.gc()).unwrap() else {
        panic!("Node");
    };
    let Value::Array(children) = runtime.gc().enum_snapshot(node).unwrap().fields[0] else {
        panic!("children");
    };
    let child = runtime.gc().array_get(children, 0).unwrap();
    runtime.gc().array_set(children, 1, child).unwrap();
    assert_eq!(
        cx.decode::<Node>(&root).unwrap(),
        Node(vec![Node(vec![]), Node(vec![]), Node(vec![])])
    );
    runtime
        .gc()
        .array_set(children, 2, Value::Enum(node))
        .unwrap();
    let error = cx.decode::<Node>(&root).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(
        error
            .to_string()
            .contains("cycle in owned value conversion")
    );
    assert_eq!(runtime.gc().active_roots(), 1);
    let view = cx.decode::<NodeView>(&root).unwrap();
    assert_eq!(view.0[2].0.value(runtime.gc()), Some(Value::Enum(node)));
    drop(view);
    runtime.gc().array_set(children, 2, child).unwrap();
    assert_eq!(
        cx.decode::<Node>(&root).unwrap(),
        Node(vec![Node(vec![]), Node(vec![]), Node(vec![])])
    );
    runtime
        .gc()
        .array_set(children, 2, Value::Enum(node))
        .unwrap();
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}
