//! Serde visitors preserve immediate-child snapshots through reentry and unwind.
use super::{module, program_for_source};
use kagari_derive::KagariSchema;
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    gc::{HeapObjectId, roots::RootedValue},
    module::LoadedModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{KagariType, context::ConversionContext},
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
    rc::Rc,
};

#[derive(Debug, PartialEq, Serialize, Deserialize, KagariSchema)]
pub(super) struct Graph {
    value: Reentrant,
    children: Vec<Graph>,
}

#[derive(Debug, PartialEq)]
struct Reentrant(i32);

enum Hook {
    Rewrite(Rc<Runtime>, HeapObjectId, RootedValue),
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
            Some(Hook::Rewrite(runtime, parent, next)) => {
                let layout = runtime.gc().struct_layout(parent).unwrap();
                runtime
                    .gc()
                    .struct_set_slot(parent, &layout, 1, next.value(runtime.gc()).unwrap())
                    .unwrap();
                runtime.collect_garbage().unwrap();
            }
            Some(Hook::Panic) => panic!("Serde child visitor unwind"),
            None => {}
        }
        Ok(Self(value))
    }
}

fn fixture() -> (Rc<Runtime>, LoadedModule) {
    let module = module();
    let program = program_for_source(
        &module,
        "use game::serde::Graph; pub fn inspect(input: Graph) -> i32 { input.value }",
    );
    let mut runtime = Runtime::default();
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let owner = runtime.load_program("serde", program).unwrap();
    (Rc::new(runtime), owner)
}

#[test]
fn serde_snapshot_survives_alias_mutation_gc_and_visitor_unwind() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let root = cx
        .encode(Serde(Graph {
            value: Reentrant(1),
            children: vec![Graph {
                value: Reentrant(2),
                children: vec![],
            }],
        }))
        .unwrap();
    let Value::Struct(parent) = root.value(runtime.gc()).unwrap() else {
        panic!("Graph");
    };
    let empty = cx.encode(Serde(Vec::<Graph>::new())).unwrap();
    HOOK.with(|hook| *hook.borrow_mut() = Some(Hook::Rewrite(Rc::clone(&runtime), parent, empty)));
    let decoded: Serde<Graph> = cx.decode(&root).unwrap();
    assert_eq!(decoded.0.children[0].value, Reentrant(2));
    assert!(
        cx.decode::<Serde<Graph>>(&root)
            .unwrap()
            .0
            .children
            .is_empty()
    );
    let roots = runtime.gc().active_roots();
    HOOK.with(|hook| *hook.borrow_mut() = Some(Hook::Panic));
    assert!(catch_unwind(AssertUnwindSafe(|| cx.decode::<Serde<Graph>>(&root))).is_err());
    assert_eq!(runtime.gc().active_roots(), roots);
    assert_eq!(
        cx.decode::<Serde<Graph>>(&root).unwrap().0.value,
        Reentrant(1)
    );
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn serde_owned_graph_rejects_cycles_through_nominal_sequences() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let root = cx
        .encode(Serde(Graph {
            value: Reentrant(1),
            children: vec![],
        }))
        .unwrap();
    let value = root.value(runtime.gc()).unwrap();
    let Value::Struct(parent) = value else {
        panic!("Graph");
    };
    let layout = runtime.gc().struct_layout(parent).unwrap();
    let Value::GcHandle(children) = runtime.gc().struct_get_slot(parent, &layout, 1).unwrap()
    else {
        panic!("Vec");
    };
    runtime.gc().sequence_push(children, value).unwrap();
    let error = cx.decode::<Serde<Graph>>(&root).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.message().contains("cycle"));
    assert!(error.message().contains("field children"));
    assert_eq!(runtime.gc().active_roots(), 1);
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}
