use crate::tests::runtime_boundary::support::compile_program;
use crate::tests::support::bytecode::load_bytecode;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::{RuntimeError, RuntimeErrorKind},
    execution::memory::GcHeapConfig,
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, IntoKagari, KagariType,
            arguments::{FromKagariArguments, IntoKagariArguments, PreparedArguments},
            context::{ConversionContext, ConversionLimits},
        },
        module::NativeModule,
        types::Type,
        value_handle::ScriptValue,
    },
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    cell::Cell,
    fmt::Debug,
    panic::{AssertUnwindSafe, catch_unwind},
};

fn fixture() -> (Runtime, ExecutableModule) {
    fixture_with(
        None,
        include_str!("../../../tests/fixtures/runtime_boundary/conversion-signature.kgr"),
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
    runtime.collect_garbage().unwrap();
    (runtime, loaded)
}

#[test]
fn nested_owned_values_round_trip_under_collection_and_release_temporary_roots() {
    type Entry = Option<Result<(String, (i32, i32)), String>>;
    type NestedValues = (Entry, Entry, Entry);
    let (mut runtime, owner) = fixture();
    let baseline = runtime.gc_stats();
    let input = (
        Some(Ok(("火🔥".to_owned(), (20, 22)))),
        None,
        Some(Err("business error".to_owned())),
    );
    let root = {
        let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
        let root = cx.encode(input.clone()).unwrap();
        cx.runtime_mut().collect_garbage().unwrap();
        assert_eq!(
            cx.runtime().gc_stats().root_registrations,
            baseline.root_registrations + 1
        );
        let decoded: NestedValues = cx.decode(&root).unwrap();
        assert_eq!(decoded, input);
        cx.runtime_mut().collect_garbage().unwrap();
        assert_eq!(
            cx.runtime().gc_stats().root_registrations,
            baseline.root_registrations + 1
        );
        root
    };
    assert!(runtime.gc_stats().completed_cycles > baseline.completed_cycles);
    drop(root);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn scalar_widths_tuple_arity_and_foreign_roots_are_checked() {
    let (mut runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
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
    assert!(cx.import(&unit).unwrap().is_unit());
    let tuple = cx.encode((true,)).unwrap();
    assert_eq!(cx.decode::<(bool,)>(&tuple).unwrap(), (true,));
    let invalid = RootedValue::from(256i64);
    assert!(cx.decode::<u8>(&invalid).is_err());
    let short = cx.encode((42i32,)).unwrap();
    assert!(cx.decode::<(i32, i32)>(&short).is_err());
    let array = cx
        .runtime_mut()
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(42i32)],
        )
        .unwrap();
    assert!(cx.decode::<(i32, i32)>(&array).is_err());
    let (mut foreign, foreign_owner) = fixture();
    let foreign_root = ConversionContext::new(&mut foreign, &foreign_owner)
        .unwrap()
        .encode(("foreign".to_owned(),))
        .unwrap();
    assert_eq!(
        cx.decode::<(String,)>(&foreign_root).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
}

#[test]
fn supported_tuple_arities_each_round_trip_as_one_value() {
    let (mut runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
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
    let arguments = PreparedArguments::<A>::new(cx, &expected)
        .unwrap()
        .encode(cx, value.clone())
        .unwrap();
    assert_eq!(arguments.len(), count);
    assert_eq!(
        arguments.get(cx, count).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        PreparedArguments::<A>::new(cx, &expected)
            .unwrap()
            .decode(cx, arguments)
            .unwrap(),
        value
    );
}

#[test]
fn outer_argument_tuples_cover_zero_through_twelve_and_preserve_nested_tuple_values() {
    let (mut runtime, owner) = fixture();
    let baseline = runtime.gc_stats();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
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
    let arguments = PreparedArguments::new(&cx, &[tuple])
        .unwrap()
        .encode(&mut cx, ((1, 2),))
        .unwrap();
    let tuple = arguments.get(&mut cx, 0).unwrap();
    assert_eq!(cx.tuple_len(&tuple).unwrap(), 2);
    assert_eq!(cx.tuple_element(&tuple, 0).unwrap().as_i32(), Some(1));
    assert_eq!(cx.tuple_element(&tuple, 1).unwrap().as_i32(), Some(2));
    assert_eq!(
        arguments.get(&mut cx, 1).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    // Views do not own leases; the enclosing context owns all encoded prefixes.
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
    drop(cx);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
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
        _: &ValueView<'_>,
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
    fn into_kagari<'call>(
        self,
        _: &mut ConversionContext<'call>,
        _: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        self.0.set(self.0.get() + 1);
        Ok(ValueView::from(7i32))
    }
}

#[test]
fn argument_signature_errors_precede_any_user_conversion_effects() {
    let (mut runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    let effects = Cell::new(0);
    let integer = cx.type_for::<i32>().unwrap();
    let boolean = cx.type_for::<bool>().unwrap();
    assert!(
        PreparedArguments::new(&cx, &[integer.clone(), integer.clone()])
            .and_then(|prepared| prepared.encode(&mut cx, (Observed(&effects), false)))
            .is_err()
    );
    assert!(
        PreparedArguments::new(&cx, &[])
            .and_then(|prepared| prepared.encode(&mut cx, (Observed(&effects), false)))
            .is_err()
    );
    assert_eq!(effects.get(), 0);
    let arguments = PreparedArguments::new(&cx, &[integer.clone(), boolean.clone()])
        .unwrap()
        .encode(&mut cx, (Observed(&effects), false))
        .unwrap();
    assert_eq!(effects.get(), 1);
    assert_eq!(arguments.get(&mut cx, 0).unwrap().as_i32(), Some(7));
    DECODE_EFFECTS.set(0);
    let signature = [integer, boolean];
    let prepared = PreparedArguments::<(Decoded, bool)>::new(&cx, &signature).unwrap();
    let invalid = cx.allocate_string("wrong".into()).unwrap();
    let invalid = cx
        .argument_values(&[ValueView::from(7i32), invalid])
        .unwrap();
    assert!(prepared.decode(&mut cx, invalid).is_err());
    assert_eq!(DECODE_EFFECTS.get(), 0);
    let (mut foreign, foreign_owner) = fixture();
    let mut foreign_cx = ConversionContext::new(&mut foreign, &foreign_owner).unwrap();
    let foreign_arguments = foreign_cx
        .argument_values(&[ValueView::from(7i32), ValueView::from(false)])
        .unwrap();
    let error = prepared
        .decode(&mut foreign_cx, foreign_arguments)
        .err()
        .unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(DECODE_EFFECTS.get(), 0);
    let encoded = PreparedArguments::new(&cx, &signature).unwrap();
    let error = encoded
        .encode(&mut foreign_cx, (Observed(&effects), false))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(effects.get(), 1);
    let sibling = owner
        .members()
        .find(|member| member.key() != owner.key())
        .unwrap();
    let mut sibling_cx = ConversionContext::new(cx.runtime_mut(), &sibling).unwrap();
    let error = encoded
        .encode(&mut sibling_cx, (Observed(&effects), false))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(effects.get(), 1);
    drop(sibling_cx);
    prepared.decode(&mut cx, arguments).unwrap();
    assert_eq!(DECODE_EFFECTS.get(), 1);
}

#[test]
fn missing_nominal_providers_and_bounded_conversion_fail_without_retention() {
    assert!(Option::<i32>::kagari_type(&DeclarationCatalog::default()).is_err());
    assert!(Result::<i32, String>::kagari_type(&DeclarationCatalog::default()).is_err());
    let (mut runtime, owner) = fixture();
    let baseline = runtime.gc_stats();
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
        let mut cx = ConversionContext::with_limits(&mut runtime, &owner, limits).unwrap();
        let error = match run {
            0 => cx.encode(((1,),)).unwrap_err(),
            1 => cx.encode((1, 2)).unwrap_err(),
            _ => cx.encode("火".to_owned()).unwrap_err(),
        };
        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        cx.runtime_mut().collect_garbage().unwrap();
        assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
        assert_eq!(
            cx.runtime().gc_stats().root_registrations,
            baseline.root_registrations
        );
    }
}

#[test]
fn conversion_observes_root_cancellation_without_publishing_partial_values() {
    let (mut runtime, owner) = fixture();
    let baseline = runtime.gc_stats();
    let options = runtime.execution_options();
    let cancellation = options.cancellation.clone();
    let mut session = runtime.begin_execution(&owner, options).unwrap();
    let mut cx = ConversionContext::new(session.runtime_mut(), &owner).unwrap();
    cancellation.cancel();
    assert_eq!(
        cx.encode(((1, 2),)).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
    drop(cx);
    drop(session);
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
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
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        let value = cx.encode_value(expected, ("retained until failure".to_owned(),))?;
        cx.runtime_mut().collect_garbage()?;
        assert_eq!(cx.tuple_len(&value)?, 1);
        let field = cx.tuple_element(&value, 0)?;
        assert_eq!(
            cx.read_string(&field, str::to_owned)?,
            "retained until failure"
        );
        assert!(!self.panic, "conversion unwind fixture");
        Err(RuntimeError::module_validation("conversion rejected"))
    }
}

#[test]
fn conversion_errors_and_panics_release_scratch_roots_and_restore_scope_depth() {
    let (mut runtime, owner) = fixture();
    let baseline = runtime.gc_stats();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    assert!(cx.encode(Failing { panic: false }).is_err());
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(catch_unwind(AssertUnwindSafe(|| cx.encode(Failing { panic: true }))).is_err());
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
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
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        cx.check_elements(self.0.len())?;
        let item = cx.parameter(expected, 0)?;
        let values = self
            .0
            .into_iter()
            .map(|value| cx.encode_value(&item, value))
            .collect::<NativeResult<Vec<_>>>()?;
        cx.allocate_array(&item, &values)
    }
}

impl<T: FromKagari> FromKagari for Children<T> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        let count = cx.array_len(value)?;
        cx.check_elements(count)?;
        // Snapshot every immediate child into source locations before any custom
        // child converter may collect, reenter or mutate an alias of the array.
        let values = (0..count)
            .map(|index| {
                cx.array_element(value, index)?
                    .ok_or_else(|| RuntimeError::module_validation("children array index"))
            })
            .collect::<NativeResult<Vec<_>>>()?;
        let item = cx.parameter(expected, 0)?;
        let mut children = Vec::new();
        for value in values {
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
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        cx.retain(value).map(Self)
    }
}

// Rust-only indirection must preserve the retained adapter's cycle policy.
#[allow(
    clippy::vec_box,
    reason = "Box conversion must preserve retained child identity through a collection."
)]
struct NodeView(Vec<Box<RetainedNode>>);

impl KagariType for NodeView {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Node::kagari_type(catalog)
    }
}

impl FromKagari for NodeView {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
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
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        cx.encode_variant(expected, "Children", (Children(self.0),))
    }
}

impl FromKagari for Node {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
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
    let (mut runtime, owner) = fixture_with(
        Some(builder.finish().unwrap()),
        include_str!("../../../tests/fixtures/runtime_boundary/conversion-node.kgr"),
    );
    let baseline = runtime.gc_stats();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    let root = cx
        .encode(Node(vec![Node(vec![]), Node(vec![]), Node(vec![])]))
        .unwrap();
    let node_type = cx.type_for::<Node>().unwrap();
    let node = cx.import(&root).unwrap();
    let (children,): (ScriptValue,) = cx.decode_variant(&node_type, &node, "Children").unwrap();
    let array_type = children.type_argument().clone();
    let children = children.into_kagari(&mut cx, &array_type).unwrap();
    let child = cx.array_element(&children, 0).unwrap().unwrap();
    cx.array_set(&children, 1, &child).unwrap();
    assert_eq!(
        cx.decode::<Node>(&root).unwrap(),
        Node(vec![Node(vec![]), Node(vec![]), Node(vec![])])
    );
    cx.array_set(&children, 2, &node).unwrap();
    let error = cx.decode::<Node>(&root).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(
        error
            .to_string()
            .contains("cycle in owned value conversion")
    );
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations + 1
    );
    let view = cx.decode::<NodeView>(&root).unwrap();
    cx.array_set(&children, 2, &child).unwrap();
    assert_eq!(
        cx.decode::<Node>(&view.0[2].0).unwrap(),
        Node(vec![Node(vec![]), Node(vec![]), Node(vec![])])
    );
    drop(view);
    cx.array_set(&children, 2, &node).unwrap();
    drop(cx);
    drop(root);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}
