//! Serde visitors preserve immediate-child snapshots through reentry and unwind.
use super::{module, program_for_source};
use crate::bytecode_fixture::load_bytecode;
use kagari_derive::KagariSchema;
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    frame::types::arguments::TypeArgument,
    gc::{HeapObjectId, roots::RootedValue},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, KagariType, context::ConversionContext},
        module::NativeModule,
        schema::TypeRegistration,
        serde::{KagariSchema as Schema, Serde},
        types::Type,
    },
    value::Value,
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

enum Hook {
    Rewrite(HeapObjectId, RootedValue),
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
        value: &Value,
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
        value: &Value,
    ) -> NativeResult<Self> {
        let value = cx.decode_value(expected, value)?;
        let hook = HOOK.with(|hook| hook.borrow_mut().take());
        match hook {
            Some(Hook::Rewrite(parent, next)) => {
                let runtime = cx.runtime_mut();
                let layout = runtime.gc().struct_layout(parent).unwrap();
                runtime
                    .gc()
                    .struct_set_slot(parent, &layout, 1, next.value(runtime.gc()).unwrap())
                    .unwrap();
                runtime.collect_garbage()?;
            }
            Some(Hook::Panic) => panic!("owned child converter unwind"),
            None => {}
        }
        Ok(Self(value))
    }
}

fn fixture() -> (Runtime, ExecutableModule) {
    let module = module();
    let program = program_for_source(
        &module,
        "use game::serde::Graph; pub fn inspect(input: Graph) -> i32 { input.value }",
    );
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
    let Value::Struct(parent) = root.value(cx.runtime().gc()).unwrap() else {
        panic!("Graph");
    };
    let empty = cx.encode(Serde(None::<Box<Graph>>)).unwrap();
    HOOK.with(|hook| *hook.borrow_mut() = Some(Hook::Rewrite(parent, empty)));
    let decoded: Graph = cx.decode(&root).unwrap();
    assert_eq!(decoded.children.unwrap().value, Reentrant(2));
    assert!(
        cx.decode::<Serde<Graph>>(&root)
            .unwrap()
            .0
            .children
            .is_none()
    );
    let roots = cx.runtime().gc().active_roots();
    HOOK.with(|hook| *hook.borrow_mut() = Some(Hook::Panic));
    assert!(catch_unwind(AssertUnwindSafe(|| cx.decode::<Serde<Graph>>(&root))).is_err());
    assert_eq!(cx.runtime().gc().active_roots(), roots);
    assert_eq!(
        cx.decode::<Serde<Graph>>(&root).unwrap().0.value,
        Reentrant(1)
    );
    drop(root);
    assert_eq!(cx.runtime_mut().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn serde_owned_graph_rejects_cycles_through_recursive_enums() {
    let (mut runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    let root = cx
        .encode(Serde(Graph {
            value: Reentrant(1),
            children: None,
        }))
        .unwrap();
    let value = root.value(cx.runtime().gc()).unwrap();
    let Value::Struct(parent) = value else {
        panic!("Graph");
    };
    let layout = cx.runtime().gc().struct_layout(parent).unwrap();
    let child = cx
        .encode(Serde(Some(Box::new(Graph {
            value: Reentrant(2),
            children: None,
        }))))
        .unwrap();
    let Value::Enum(child_id) = child.value(cx.runtime().gc()).unwrap() else {
        panic!("Option");
    };
    let tag = cx.runtime().gc().enum_snapshot(child_id).unwrap().tag;
    let cycle = Value::Enum(cx.runtime().alloc_enum(tag, vec![value]).unwrap());
    cx.runtime()
        .gc()
        .struct_set_slot(parent, &layout, 1, cycle)
        .unwrap();
    drop(child);
    let error = cx.decode::<Serde<Graph>>(&root).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.to_string().contains("cycle"));
    assert!(error.to_string().contains("field children"));
    assert_eq!(cx.runtime().gc().active_roots(), 1);
    drop(root);
    assert_eq!(cx.runtime_mut().collect_garbage().unwrap().live_objects, 0);
}
