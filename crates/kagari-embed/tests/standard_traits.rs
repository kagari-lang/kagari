use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("standard-traits.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let mut context = ExecutionContext::default();
        context.capabilities.jit = jit;
        context.language_profile.allow_jit = jit;
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn standard_trait_methods_and_generic_bounds_execute() {
    execute(
        r#"
fn same<T: Eq>(a:T, b:T)->bool { a.eq(b) && a == b }
fn hash<T: Eq + Hash>(a:T)->i64 { a.hash() }
fn format<T: Debug>(a:T)->String { a.debug() }
fn show<T: Display>(a:T)->String { a.display() }
fn main()->i32 {
    val x = (20, "key");
    if same(x, (20, "key")) && hash(x) == hash((20, "key")) && format(x) == "(20, \"key\")" && show(42) == "42" { 42 } else { 0 }
}
"#,
    );
}
#[test]
fn explicit_formatting_uses_normal_static_dispatch() {
    execute(
        r#"
struct Item { val value: i32 }
impl Debug for Item { fn debug(self)->String { "custom debug" } }
impl Display for Item { fn display(self)->String { self.value.display() } }
fn format<T: Debug>(a:T)->String { a.debug() }
fn show<T: Display>(a:T)->String { a.display() }
fn main()->i32 {
    val item = Item { value: 42 };
    if format(item) == "custom debug" && show(item) == "42" { 42 } else { 0 }
}
"#,
    );
}
#[test]
fn structural_and_identity_keys_execute_through_artifacts() {
    execute(
        r#"
struct Key { var value: i32 }
enum Tag { Name(String), Number(i32) }
fn main()->i32 {
    val map: Map<(Tag, i32), i32> = std::map::new();
    map.insert((Tag::Name("a"), 2), 20);
    val key = Key { value: 1 };
    val identities: Map<Key, i32> = std::map::new();
    identities.insert(key, 22);
    key.value = 9;
    val different = Key { value: 9 };
    if identities.contains_key(different) { 0 } else {
        map.get((Tag::Name("a"), 2)).unwrap_or(0) + identities.get(key).unwrap_or(0)
    }
}
"#,
    );
}

#[test]
fn imports_supertraits_and_nested_structural_keys() {
    execute(
        r#"
use std::cmp::Eq as Equal;
use std::hash::*;
use std::fmt as formatting;
trait Named: Equal { fn name(self)->String; }
struct Item { val value: i32 }
impl Named for Item { fn name(self)->String { "item" } }
fn same<T: Named>(a:T,b:T)->bool { a.eq(b) }
fn output<T: formatting::Display>(x:T)->String { x.display() }
fn hashed<T: Equal + Hash>(x:T)->i64 { x.hash() }
fn main()->i32 {
    val item = Item { value:42 };
    val set: Set<Option<(i32, String)>> = std::set::new();
    set.insert(Some((42,"ok")));
    if same(item,item) && set.contains(Some((42,"ok"))) && output(42) == "42" && hashed(item) == hashed(item) { item.value } else { 0 }
}
"#,
    );
}

#[test]
fn invalid_standard_trait_uses_report_semantic_diagnostics() {
    for source in [
        "fn needs<T: Eq>(x:T) {} fn main() { needs(1.5); }",
        "fn needs<T: Hash>(x:T) {} fn main() { needs(1.5); }",
        "enum Key { Good(i32), Bad(f64) } fn main() { val map: Map<Key, i32> = std::map::new(); }",
        "enum Key { Good(i32), Bad(f64) } fn needs<T: Eq + Hash>(x:T) {} fn main() { needs(Key::Good(1)); }",
        "fn needs<T: Eq<i32>>(x:T) {} fn main() {}",
        "trait Named: Debug {} fn f(x:Named) {} fn main() {}",
        "struct Item {} impl Eq for Item {} fn main() {}",
        "struct Item {} impl Hash for Item { fn hash(self)->i64 { 1 } } fn main() {}",
        "impl Debug for i32 { fn debug(self)->String { \"x\" } } fn main() {}",
        "struct Item {} impl Debug for Item { fn debug(self)->i32 { 1 } } fn main() {}",
        "struct Item {} impl Display for Item {} fn main() {}",
        "fn f(x: Debug) {} fn main() {}",
        "trait Eq {} fn f<T: Eq>(x:T)->bool { x == x } fn main() {}",
        "trait Hash {} fn f<T: Eq + Hash>(x:T)->Set<T> { std::set::new() } fn main() {}",
        "fn f<T: HashKey>(x:T) {} fn main() {}",
    ] {
        let error = KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new("bad-traits.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap_err();
        assert!(
            matches!(error, kagari_embed::EmbeddingError::Diagnostics { .. }),
            "{source}: {error:?}"
        );
    }
}

#[test]
fn standard_impls_are_available_to_trait_inputs_and_associated_outputs() {
    execute(
        r#"
trait Read<T: Eq + Hash> { type Item: Eq + Hash; fn get(self, value:T)->Self::Item; }
struct Reader {}
impl Read<i32> for Reader { type Item = (i32, String); fn get(self,value:i32)->(i32,String) { (value,"ok") } }
fn same<T: PartialEq>(a:T,b:T)->bool { a.eq(b) }
fn main()->i32 {
    val unit: Map<(), i32> = std::map::new(); unit.insert((),42);
    val value = Reader {}.get(42);
    if same(1.5,1.5) && same(value,(42,"ok")) && ().debug() == "()" { unit.get(()).unwrap_or(0) } else { 0 }
}
"#,
    );
}

#[test]
fn malformed_standard_implementations_and_reserved_modules_are_rejected() {
    use kagari_ir::module::{
        PublicAbiItem,
        abi::{AbiType, BuiltinType},
    };
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("format-wire.kgr", "struct Item {} impl Debug for Item { fn debug(self)->String { \"ok\" } } fn main()->i32 { val item=Item {}; item.debug(); 42 }"),Default::default(),Default::default()).unwrap();
    for mutation in 0..5 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation == 0 {
            module.identity.package.0 = "kagari-std".into();
        } else {
            let table = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    if let PublicAbiItem::InterfaceTable(table) = item {
                        Some(table)
                    } else {
                        None
                    }
                })
                .unwrap();
            match mutation {
                1 => table.methods[0].return_type = AbiType::Builtin(BuiltinType::I32),
                2 => {
                    let AbiType::Trait(trait_type) = &mut table.trait_type else {
                        panic!("trait");
                    };
                    trait_type.declaration = kagari_hir::builtin::traits::StandardTrait::Hash
                        .contract()
                        .id
                        .clone();
                }
                3 => table.methods.clear(),
                _ => {
                    let AbiType::Trait(trait_type) = &mut table.trait_type else {
                        panic!("trait");
                    };
                    trait_type
                        .arguments
                        .push(AbiType::Builtin(BuiltinType::I32));
                }
            }
        }
        assert!(
            kagari_ir::bytecode::verify_program(&program).is_err(),
            "mutation {mutation}"
        );
        assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
    }
}

#[test]
fn automatic_debug_of_nested_values_never_invokes_custom_member_code() {
    execute(
        r#"
enum Callback { Run(fn()->i32) }
impl Debug for Callback { fn debug(self)->String { "custom" } }
fn format<T: Debug>(x:T)->String { (x,).debug() }
fn main()->i32 {
    val c = Callback::Run(||42);
    if c.debug() == "custom" && format(c) == "(Callback::Run(<function>),)" { 42 } else { 0 }
}
"#,
    );
}
