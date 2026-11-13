//! Serde visitors preserve immediate-child snapshots through reentry and unwind.
use super::{module, program_for_source};
use crate::bytecode_fixture::load_bytecode;
use kagari_derive::KagariSchema;
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, KagariType, context::ConversionContext},
        module::NativeModule,
        objects::{Object, fields::Field},
        schema::TypeRegistration,
        serde::{KagariSchema as Schema, Serde},
        typed::NativeContext,
        types::Type,
    },
    value::view::ValueView,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
};

#[derive(Debug, PartialEq, Serialize, Deserialize, KagariSchema)]
pub(super) struct Graph {
    value: Reentrant,
    children: Option<Box<Graph>>,
}

#[derive(Debug, PartialEq)]
struct Reentrant(i32);

type GraphObject = Object<Serde<Graph>>;
type ChildrenField = Field<Option<GraphObject>>;

enum Hook {
    Rewrite(GraphObject, ChildrenField),
    Panic,
}

thread_local! { static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) }; }

impl Schema for Reentrant {
    fn schema_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }

    fn register_schema(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        i32::register_type(registration)
    }
}

impl Serialize for Reentrant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i32(self.0)
    }
}

impl<'de> Deserialize<'de> for Reentrant {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = i32::deserialize(deserializer)?;
        let hook = HOOK.with(|hook| hook.borrow_mut().take());
        match hook {
            Some(Hook::Panic) => panic!("Serde child visitor unwind"),
            Some(Hook::Rewrite(..)) => panic!("rewrite requires explicit conversion services"),
            None => {}
        }
        Ok(Self(value))
    }
}

// The struct snapshot owner also backs Serde's field visitor. A custom owned
// child converter receives the exclusive services needed for actual reentry.
impl KagariType for Graph {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Serde::<Self>::kagari_type(catalog)
    }
}

impl FromKagari for Graph {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        let mut fields = cx.read_struct(expected, value)?;
        Ok(Self {
            value: fields.field("value")?,
            children: fields.field("children")?,
        })
    }
}

impl KagariType for Reentrant {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl FromKagari for Reentrant {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        let value = cx.decode_value(expected, value)?;
        let hook = HOOK.with(|hook| hook.borrow_mut().take());
        match hook {
            Some(Hook::Rewrite(parent, children)) => {
                let owner = cx.owner().clone();
                let mut native = NativeContext::new(cx.runtime_mut(), &owner)?;
                // Use declared mutable field access, including actual conversion
                // and publication, rather than reconstructing a bare heap ID.
                parent.set(&mut native, &children, None)?;
                native.collect_garbage()?;
            }
            Some(Hook::Panic) => panic!("owned child converter unwind"),
            None => {}
        }
        Ok(Self(value))
    }
}

fn fixture() -> (Runtime, ExecutableModule) {
    let module = module();
    let program = program_for_source(&module, include_str!("../fixtures/native/serde_graph.kgr"));
    let mut runtime = Runtime::default();
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let owner_program = load_bytecode(&mut runtime, "serde", program).unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    (runtime, owner)
}

#[test]
fn serde_snapshot_survives_alias_mutation_gc_and_visitor_unwind() {
    let (mut runtime, owner) = fixture();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    let root = cx
        .encode(Serde(Graph {
            value: Reentrant(1),
            children: Some(Box::new(Graph {
                value: Reentrant(2),
                children: None,
            })),
        }))
        .unwrap();
    let parent: GraphObject = cx.decode(&root).unwrap();
    let children = cx
        .runtime_mut()
        .bind_field(parent.object_type(), "children")
        .unwrap();
    HOOK.with(|hook| *hook.borrow_mut() = Some(Hook::Rewrite(parent, children)));
    let decoded: Graph = cx.decode(&root).unwrap();
    assert_eq!(decoded.children.unwrap().value, Reentrant(2));
    assert!(
        cx.decode::<Serde<Graph>>(&root)
            .unwrap()
            .0
            .children
            .is_none()
    );
    cx.runtime_mut().collect_garbage().unwrap();
    let roots = cx.runtime().gc_stats().root_registrations;
    HOOK.with(|hook| *hook.borrow_mut() = Some(Hook::Panic));
    assert!(catch_unwind(AssertUnwindSafe(|| cx.decode::<Serde<Graph>>(&root))).is_err());
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().root_registrations, roots);
    assert_eq!(
        cx.decode::<Serde<Graph>>(&root).unwrap().0.value,
        Reentrant(1)
    );
    drop(root);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
}

#[test]
fn serde_owned_graph_rejects_cycles_through_recursive_enums() {
    let (mut runtime, owner) = fixture();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    let root = cx
        .encode(Serde(Graph {
            value: Reentrant(1),
            children: None,
        }))
        .unwrap();
    let parent: GraphObject = cx.decode(&root).unwrap();
    let children: ChildrenField = cx
        .runtime_mut()
        .bind_field(parent.object_type(), "children")
        .unwrap();
    let mut native = NativeContext::new(cx.runtime_mut(), &owner).unwrap();
    parent
        .set(&mut native, &children, Some(parent.clone()))
        .unwrap();
    drop(native);
    drop(children);
    drop(parent);
    let error = cx.decode::<Serde<Graph>>(&root).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.to_string().contains("cycle"));
    assert!(error.to_string().contains("field children"));
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations + 1
    );
    drop(root);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
}
