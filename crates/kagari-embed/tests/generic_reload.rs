#![cfg(feature = "source")]
use kagari_bytecode::program::BytecodeProgram;
use kagari_embed::{BytecodeArtifact, engine::KagariEngine};
use kagari_runtime::{Runtime, RuntimeConfig, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::declaration::module::ModuleDecl;
use kagari_vm::vm::Vm;

fn prepare(engine: &KagariEngine, source: &str) -> BytecodeProgram {
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("generic-reload.kgr", source),
            Default::default(),
        )
        .unwrap();
    BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .program
}

#[test]
fn retained_generic_default_and_override_closures_pin_types_and_operations_after_reload() {
    let method =
        "fn keep<T: Ord>(self, a: T, b: T) -> fn() -> bool { || a.cmp(b) == Ordering::Less }";
    for implementation in [String::new(), method.to_owned()] {
        let source = format!(
            r#"use std::cmp::{{Ordering}};

trait Keep {{ {method} }}
impl Keep for i32 {{ {implementation} }}
struct Rank {{ val value: i32 }}
impl PartialEq for Rank {{ fn eq(self, other: Self) -> bool {{ self.value == other.value }} }}
impl Eq for Rank {{}}
impl PartialOrd for Rank {{ fn partial_cmp(self, other: Self) -> Option<Ordering> {{ self.value.partial_cmp(other.value) }} }}
impl Ord for Rank {{ fn cmp(self, other: Self) -> Ordering {{ self.value.cmp(other.value) }} }}
trait Run {{ fn run(self) -> bool; }}
struct Saved {{ val callback: fn() -> bool }}
impl Run for Saved {{ fn run(self) -> bool {{ (self.callback)() }} }}
fn make() -> Run {{
    val keeper: Keep = 0;
    Saved {{ callback: keeper.keep(Rank {{ value: 1 }}, Rank {{ value: 2 }}) }}
}}
"#
        );
        let engine = KagariEngine::default();
        let program = prepare(&engine, &source);
        let replacement = prepare(
            &engine,
            &source.replace("self.value.cmp(other.value)", "other.value.cmp(self.value)"),
        );
        let mut config = RuntimeConfig::default();
        config.gc.collection_threshold = Some(1);
        let mut runtime = Runtime::new(config);
        let old = runtime.load_program("generic-reload", program).unwrap();
        let contract = old
            .bytecode
            .trait_contracts
            .iter()
            .find(|contract| contract.abi.name == "Run")
            .unwrap();
        let run = ModuleDecl::method_id(
            &old.definitions()
                .resolve(contract.declaration)
                .unwrap()
                .to_path(),
            "run",
        );
        let old_key = old.key();
        let mut vm = Vm::new(runtime);
        let saved = vm.execute(&old, "make").unwrap().return_value;
        let root = vm.runtime().root_value(saved.clone()).unwrap();
        let current = vm
            .reload_program(&old, "generic-reload", replacement)
            .unwrap();
        drop(old);
        vm.runtime().collect_garbage().unwrap();
        assert!(
            vm.runtime()
                .modules()
                .retention_counts(old_key)
                .runtime_values
                > 0
        );
        let fresh = vm.execute(&current, "make").unwrap().return_value;
        let fresh_root = vm.runtime().root_value(fresh.clone()).unwrap();
        assert_eq!(
            vm.invoke_interface_method(&saved, &run, &[]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.invoke_interface_method(&fresh, &run, &[]).unwrap(),
            Value::Bool(false)
        );
        drop((root, fresh_root));
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
        assert_eq!(
            vm.runtime()
                .modules()
                .retention_counts(old_key)
                .runtime_values,
            0
        );
    }
}
