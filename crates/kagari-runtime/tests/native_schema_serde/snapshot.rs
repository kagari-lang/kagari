//! Serde visitors preserve immediate-child snapshots through reentry and unwind.
use super::{module, program_for_source};
use kagari_derive::KagariSchema;
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    gc::{reference::GcRef, roots::RootedValue},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{KagariType, context::ConversionContext},
        module::NativeModule,
        schema::TypeRegistration,
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    value::Value,
};
use kagari_stdlib::collections::vector::ScriptVec;
use kagari_stdlib::serde::{KagariSchema as Schema, Serde};
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
    Rewrite(Rc<Runtime>, GcRef, RootedValue),
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
                let layout = unsafe { runtime.gc().struct_layout_transport(parent) }.unwrap();
                unsafe {
                    runtime.gc().struct_set_slot_transport(
                        parent,
                        &layout,
                        1,
                        next.value_transport(runtime.gc()).unwrap(),
                    )
                }
                .unwrap();
                runtime.collect_garbage().unwrap();
            }
            Some(Hook::Panic) => panic!("Serde child visitor unwind"),
            None => {}
        }
        Ok(Self(value))
    }
}

fn fixture() -> (Rc<Runtime>, ExecutableModule) {
    let module = module();
    let program = program_for_source(
        &module,
        "use game::serde::Graph; pub fn inspect(input: Graph) -> i32 { input.value }",
    );
    let mut runtime = Runtime::default();
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let owner_program = runtime.load_program("serde", program).unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
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
    let Value::Struct(parent) = unsafe { root.value_transport(runtime.gc()) }.unwrap() else {
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
    let value = unsafe { root.value_transport(runtime.gc()) }.unwrap();
    let Value::Struct(_) = value else {
        panic!("Graph");
    };
    let ty = cx.type_for::<Serde<Graph>>().unwrap();
    let child = cx.retain(&ty, root.clone()).unwrap();
    let mut fields = cx.read_struct(&ty, &child).unwrap();
    let children: ScriptVec<ScriptValue> = fields.field("children").unwrap();
    drop(fields);
    children
        .push(&mut NativeContext::new(&runtime, &owner).unwrap(), child)
        .unwrap();
    drop(children);
    let error = cx.decode::<Serde<Graph>>(&root).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.message().contains("cycle"));
    assert!(error.message().contains("field children"));
    assert_eq!(runtime.gc().active_roots(), 1);
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}
