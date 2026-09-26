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
    val map: MutableMap<(Tag, i32), i32> = MutableMap::new();
    map.insert((Tag::Name("a"), 2), 20);
    val key = Key { value: 1 };
    val identities: MutableMap<Key, i32> = MutableMap::new();
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
    val set: MutableSet<Option<(i32, String)>> = MutableSet::new();
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
        "enum Key { Good(i32), Bad(f64) } fn main() { val map: MutableMap<Key, i32> = MutableMap::new(); }",
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
        "trait Hash {} fn f<T: Eq + Hash>(x:T)->MutableSet<T> { MutableSet::new() } fn main() {}",
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
    val unit: MutableMap<(), i32> = MutableMap::new(); unit.insert((),42);
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

#[test]
fn identity_operators_use_object_handles_across_backends() {
    execute(
        r#"
struct Item { var value: i32 }
fn main()->i32 {
    val a = Item { value: 1 }; val alias = a;
    val b = Item { value: 1 };
    val values = [1]; val copy = [1];
    val map: MutableMap<i32, i32> = MutableMap::new();
    val set: MutableSet<i32> = MutableSet::new();
    a.value = 2;
    if a === alias && a !== b && values === values && values !== copy && map === map && set === set { 42 } else { 0 }
}
"#,
    );
}

#[test]
fn identity_operators_reject_value_types_and_mismatched_objects() {
    for expression in [
        "1 === 1",
        "true !== false",
        "\"a\" === \"a\"",
        "(1,2) === (1,2)",
        "Some(1) === Some(1)",
        "[1] === [true]",
    ] {
        let source = format!("fn main()->bool {{ {expression} }}");
        assert!(matches!(
            KagariEngine::default().compile_to_artifact(
                SourceFile::new("bad-identity.kgr", source),
                Default::default(),
                Default::default()
            ),
            Err(kagari_embed::EmbeddingError::Diagnostics { .. })
        ));
    }
}

#[test]
fn custom_equality_composes_and_enum_overrides_variant_checks() {
    execute(
        r#"
struct Point { val x:i32 }
impl PartialEq for Point { fn eq(self, other:Self)->bool { self.x == other.x } }
impl Eq for Point {}
enum Message { Missing, Here(Point) }
enum Id { Local(i32), Remote(i32) }
fn number(v:Id)->i32 { match v { Id::Local(x)=>x, Id::Remote(x)=>x } }
impl PartialEq for Id { fn eq(self, other:Self)->bool { number(self)==number(other) } }
impl Eq for Id {}
impl Hash for Id { fn hash(self)->i64 { number(self).hash() } }
fn same<T:Eq>(a:T,b:T)->bool { a==b && a.eq(b) }
fn main()->i32 {
 val a=Point{x:1}; val b=Point{x:1};
 if same(a,b) && a !== b && same((a,7),(b,7)) && same(Message::Here(a),Message::Here(b)) && same(Id::Local(42),Id::Remote(42)) && Id::Local(42).hash()==Id::Remote(42).hash() {42} else {0}
}
"#,
    );
}

#[test]
fn custom_keys_use_hash_buckets_and_script_equality() {
    execute(
        r#"
struct Key { val id:i32, var ignored:i32 }
impl PartialEq for Key { fn eq(self, other:Self)->bool { self.id == other.id } }
impl Eq for Key {}
impl Hash for Key { fn hash(self)->i64 { 0.hash() } }
enum Tag { Value(Key), Empty }
fn store<T:Eq+Hash>(map:MutableMap<T,i32>,key:T,value:i32) { map.insert(key,value); }
fn main()->i32 {
 val map:MutableMap<(Tag,i32),i32> = MutableMap::new();
 val a=Key{id:1,ignored:0}; val b=Key{id:2,ignored:0};
 store(map,(Tag::Value(a),7),20); store(map,(Tag::Value(b),7),21);
 a.ignored=99;
 store(map,(Tag::Value(Key{id:1,ignored:3}),7),22);
 val set:MutableSet<Key> = MutableSet::new();
 set.insert(a); set.insert(b); set.insert(Key{id:1,ignored:4});
 std::debug::assert(set.len()==[1,2].len(),"dedup");
 std::debug::assert(set.contains(Key{id:2,ignored:5}),"collision lookup");
 std::debug::assert(set.remove(Key{id:2,ignored:5}),"remove");
 std::debug::assert(!set.contains(b),"removed");
 std::debug::assert(map.len()==[1,2].len(),"update");
 std::debug::assert(map.contains_key((Tag::Value(Key{id:2,ignored:0}),7)),"contains");
 val result=map.get((Tag::Value(Key{id:1,ignored:0}),7)).unwrap_or(0);
 std::debug::assert(map.remove((Tag::Value(b),7)).unwrap_or(0)==21,"map remove");
 result+20
}
"#,
    );
}

#[test]
fn default_protocols_follow_custom_members_and_recursive_enums() {
    execute(
        r#"
struct Key<T> { val value:T }
impl<T:PartialEq> PartialEq for Key<T> { fn eq(self, other:Self)->bool { self.value == other.value } }
impl<T:Eq> Eq for Key<T> {}
impl<T:Eq+Hash> Hash for Key<T> { fn hash(self)->i64 { self.value.hash() } }
enum Chain { End, Next(Key<i32>, Chain) }
fn main()->i32 {
 val a=Chain::Next(Key{value:42},Chain::End);
 val b=Chain::Next(Key{value:42},Chain::End);
 val set:MutableSet<Chain> = MutableSet::new(); set.insert(a);set.insert(b);
 std::debug::assert_eq(a,b,"composed comparison");
 if set.len()==[1].len() && set.contains(b) {42} else {0}
}
"#,
    );
}

#[test]
fn custom_enum_keys_ignore_variants_and_uncompared_payload_capabilities() {
    execute(
        r#"
enum Id { Local(i32,f32), Remote(i32) }
fn number(v:Id)->i32 { match v { Id::Local(id,_)=>id, Id::Remote(id)=>id } }
impl PartialEq for Id { fn eq(self,other:Self)->bool {number(self)==number(other)} }
impl Eq for Id {}
impl Hash for Id {fn hash(self)->i64 {number(self).hash()}}
fn main()->i32 {
 val a:MutableSet<Id> = MutableSet::new(); val b:MutableSet<Id> = MutableSet::new();
 a.insert(Id::Local(42,1.5));b.insert(Id::Remote(42));b.insert(Id::Remote(7));
 val union=a.union(b);val intersection=a.intersection(b);val difference=b.difference(a);
 std::debug::assert(union.len()==[1,2].len(),"union");
 std::debug::assert(intersection.contains(Id::Remote(42)),"intersection");
 std::debug::assert(!difference.contains(Id::Remote(42)) && difference.contains(Id::Remote(7)),"difference");
 val map:MutableMap<Option<Id>,i32> = MutableMap::new();
 map.insert(Some(Id::Local(42,2.5)),42);
 map.get(Some(Id::Remote(42))).unwrap_or(0)
}
"#,
    );
}

#[test]
fn custom_comparisons_short_circuit_without_identity_shortcuts() {
    execute(
        r#"
struct Counter {var count:i32}
struct Key {val id:i32,val counter:Counter}
impl PartialEq for Key {fn eq(self,other:Self)->bool {self.counter.count+=1;self.id==other.id}}
enum Pair {Values(Key,Key), Empty}
fn main()->i32 {
 val c=Counter{count:0};val a=Key{id:1,counter:c};val b=Key{id:2,counter:c};
 std::debug::assert(!(a,a).eq((b,a)),"tuple mismatch");
 std::debug::assert(c.count==1,"tuple short circuit");
 std::debug::assert(Pair::Values(a,a)!=Pair::Empty,"variant mismatch");
 std::debug::assert(c.count==1,"variant skips members");
 std::debug::assert(Pair::Values(a,a)!=Pair::Values(b,a),"enum mismatch");
 std::debug::assert(c.count==2,"enum short circuit");
 std::debug::assert(a==a,"alias comparison");
 std::debug::assert(c.count==3,"custom implementation still runs for alias");
 42
}
"#,
    );
}

#[test]
fn comparison_only_types_do_not_inherit_identity_hashing() {
    for tail in [
        "fn main(){val set:MutableSet<Key> = MutableSet::new();}",
        "fn main(){val set:MutableSet<(Key,i32)> = MutableSet::new();}",
        "enum E {Value(Key)} fn main(){val set:MutableSet<E> = MutableSet::new();}",
        "fn needs<T:Eq+Hash>(v:T){} fn main(){needs(Key{});}",
        "fn main(){Key{}.hash();}",
    ] {
        let source = format!(
            "struct Key {{}} impl PartialEq for Key {{fn eq(self,other:Self)->bool {{true}}}} impl Eq for Key {{}} {tail}"
        );
        assert!(
            matches!(
                KagariEngine::default().compile_to_artifact(
                    SourceFile::new("bad-key.kgr", source),
                    Default::default(),
                    Default::default()
                ),
                Err(kagari_embed::EmbeddingError::Diagnostics { .. })
            ),
            "{tail}"
        );
    }
}

#[test]
fn key_callback_traps_and_reentry_release_guards_without_partial_insertion() {
    let source = r#"
struct State {var mode:i32,var calls:i32}
struct Key {val id:i32,val owner:MutableSet<Key>,val state:State}
impl PartialEq for Key {fn eq(self,other:Self)->bool {
 self.state.calls+=1;
 if self.state.mode==1 {self.owner.clear();}
 if self.state.mode==2 {std::debug::panic("comparison failure");}
 if self.state.mode==5 {
  self.state.mode=0;
  std::debug::assert(!self.owner.contains(self),"nested read of absent query");
  self.state.mode=5;
 }
 self.id==other.id
}}
impl Eq for Key {}
impl Hash for Key {fn hash(self)->i64 {
 if self.state.mode==3 {self.owner.clear();}
 if self.state.mode==4 {std::debug::panic("hash failure");}
 0.hash()
}}
trait Test {fn mode(self,value:i32);fn attempt(self);fn calls(self)->i32;fn clear(self);}
struct Tester {val set:MutableSet<Key>,val key:Key,val state:State}
impl Test for Tester {
 fn mode(self,value:i32){self.state.mode=value;}
 fn attempt(self){self.set.insert(self.key);}
 fn calls(self)->i32 {self.state.calls}
 fn clear(self){self.set.clear();}
}
fn make()->(Test,MutableSet<Key>) {
 val set:MutableSet<Key> = MutableSet::new();val state=State{mode:0,calls:0};
 set.insert(Key{id:1,owner:set,state:state});
 val tester:Test=Tester{set:set,key:Key{id:2,owner:set,state:state},state:state};
 (tester,set)
}
"#;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("callback-cleanup.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let root_module = &artifact.program.modules[artifact.program.root.index()];
    let declaration = root_module
        .trait_contracts
        .iter()
        .find(|c| c.abi.name == "Test")
        .unwrap()
        .declaration
        .clone();
    let method = |name: &str| {
        let mut id = declaration.clone();
        id.path
            .push(kagari_common::identity::DefinitionPathSegment {
                kind: kagari_common::identity::DefinitionKind::Method,
                name: name.into(),
                occurrence: 0,
            });
        id
    };
    let mut config = kagari_runtime::RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = kagari_runtime::Runtime::new(config);
    let loaded = runtime
        .load_program("callback-cleanup", artifact.program)
        .unwrap();
    let mut vm = kagari_vm::Vm::new(runtime);
    for mode in 1..=4 {
        let value = vm.execute(&loaded, "make").unwrap().return_value;
        let root = vm.runtime().root_value(value.clone()).unwrap();
        let Value::Tuple(values) = value else {
            panic!("tuple")
        };
        vm.invoke_interface_method(&values[0], &method("mode"), &[Value::I32(mode)])
            .unwrap();
        let error = vm
            .invoke_interface_method(&values[0], &method("attempt"), &[])
            .unwrap_err();
        let message = format!("{error:?}");
        assert!(
            message.contains(if mode == 1 || mode == 3 {
                "container mutation during"
            } else if mode == 2 {
                "comparison failure"
            } else {
                "hash failure"
            }),
            "{message}"
        );
        let Value::Set(id) = values[1] else {
            panic!("set")
        };
        assert_eq!(vm.runtime().gc().set_len(id), Some(1));
        assert_eq!(vm.runtime().gc().active_roots(), 1);
        if mode <= 2 {
            assert_eq!(
                vm.invoke_interface_method(&values[0], &method("calls"), &[])
                    .unwrap(),
                Value::I32(1)
            );
        }
        vm.invoke_interface_method(&values[0], &method("clear"), &[])
            .unwrap();
        assert_eq!(vm.runtime().gc().set_len(id), Some(0));
        assert!(!vm.runtime().is_quarantined());
        drop(root);
    }

    // Nested reads use a second guard; dropping it must preserve the outer guard.
    let value = vm.execute(&loaded, "make").unwrap().return_value;
    let root = vm.runtime().root_value(value.clone()).unwrap();
    let Value::Tuple(values) = value else {
        panic!("tuple")
    };
    vm.invoke_interface_method(&values[0], &method("mode"), &[Value::I32(5)])
        .unwrap();
    vm.invoke_interface_method(&values[0], &method("attempt"), &[])
        .unwrap();
    let Value::Set(id) = values[1] else {
        panic!("set")
    };
    assert_eq!(vm.runtime().gc().set_len(id), Some(2));
    assert_eq!(vm.runtime().gc().active_roots(), 1);
    // Native helpers cannot silently run identity lookup on stored custom keys.
    assert!(
        vm.runtime()
            .invoke_standard_builtin(
                kagari_hir::builtin::surface::StandardIntrinsic::SetContains,
                &[values[1].clone(), values[0].clone()],
            )
            .is_err()
    );
    drop(root);

    for limit in [2, 12, 24, 40] {
        let value = vm.execute(&loaded, "make").unwrap().return_value;
        let root = vm.runtime().root_value(value.clone()).unwrap();
        let Value::Tuple(values) = value else {
            panic!("tuple")
        };
        let Value::Set(id) = values[1] else {
            panic!("set")
        };
        let mut options = vm.runtime().execution_options();
        options.resources.max_instruction_steps = Some(limit);
        let session = vm.runtime().begin_execution(&loaded, options).unwrap();
        let result = vm.invoke_interface_method(&values[0], &method("attempt"), &[]);
        drop(session);
        if result.is_err() {
            assert_eq!(vm.runtime().gc().set_len(id), Some(1));
        }
        assert_eq!(vm.runtime().gc().active_roots(), 1);
        vm.invoke_interface_method(&values[0], &method("clear"), &[])
            .unwrap();
        assert_eq!(vm.runtime().gc().set_len(id), Some(0));
        drop(root);
    }
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn imported_equality_and_hash_use_the_defining_modules_implementations() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
    for downstream_override in [false, true] {
        let engine = KagariEngine::default();
        let model = r#"
pub struct Key {pub val id:i32}
impl PartialEq for Key {fn eq(self,other:Self)->bool {self.id==other.id}}
impl Eq for Key {}
impl Hash for Key {fn hash(self)->i64 {self.id.hash()}}
pub fn equal(a:Key,b:Key)->bool {a==b}
pub fn make()->MutableMap<Key,i32> {val m:MutableMap<Key,i32> = MutableMap::new();m.insert(Key{id:1},42);m}
"#;
        let root_source = if downstream_override {
            "use pkg::model::Key; impl PartialEq for Key {fn eq(self,other:Self)->bool {false}} fn main()->i32 {42}"
        } else {
            "use pkg::model::{Key,equal,make}; fn same<T:Eq>(a:T,b:T)->bool {a==b} fn main()->i32 {val a=Key{id:1};val b=Key{id:1};if equal(a,b) && same(a,b) && a !== b {make().get(b).unwrap_or(0)} else {0}}"
        };
        let mut root = None;
        for (name, source) in [("model", model), ("root", root_source)] {
            let path = format!("mem://{name}");
            engine
                .bind_module(
                    &path,
                    ModuleIdentity {
                        package: PackageId("pkg".into()),
                        path: vec![name.into()],
                    },
                )
                .unwrap();
            let id = engine
                .set_source(&path, source.into(), SourceLayer::Base)
                .unwrap();
            if name == "root" {
                root = Some(id);
            }
        }
        let checked = engine.compile_snapshot(
            engine.source_snapshot(),
            root.unwrap(),
            Default::default(),
            &Default::default(),
        );
        if downstream_override {
            assert!(checked.is_err());
            continue;
        }
        let artifact = engine
            .emit_bytecode(&checked.unwrap(), Default::default())
            .unwrap();
        let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}

#[test]
fn portable_hash_implementations_require_explicit_comparison_contracts() {
    use kagari_hir::builtin::traits::StandardTrait;
    use kagari_ir::module::{PublicAbiItem, abi::AbiType};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "key-wire.kgr",
                r#"
struct Key {val id:i32}
impl PartialEq for Key {fn eq(self,other:Self)->bool {self.id==other.id}}
impl Eq for Key {}
impl Hash for Key {fn hash(self)->i64 {self.id.hash()}}
fn main()->i64 {Key{id:1}.hash()}
"#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for missing in [StandardTrait::Eq, StandardTrait::PartialEq] {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        module.public_items.retain(|item| !matches!(item,PublicAbiItem::InterfaceTable(table) if matches!(&table.trait_type, AbiType::Trait(t) if t.declaration==missing.contract().id)));
        assert!(
            kagari_ir::bytecode::verify_program(&program).is_err(),
            "missing {missing:?}"
        );
    }
}

#[test]
fn builtin_keys_keep_native_lookup_and_custom_keys_emit_guarded_calls() {
    use kagari_ir::bytecode::{BytecodeInstruction, CallTarget, StandardIntrinsic};
    for custom in [false, true] {
        let implementation = if custom {
            "impl PartialEq for Key {fn eq(self,other:Self)->bool {self.id==other.id}} impl Eq for Key {} impl Hash for Key {fn hash(self)->i64 {self.id.hash()}}"
        } else {
            ""
        };
        let source = format!(
            "struct Key {{val id:i32}} {implementation} fn main()->i32 {{val m:MutableMap<Key,i32> = MutableMap::new();val k=Key{{id:1}};m.insert(k,42);m.get(k).unwrap_or(0)}}"
        );
        let artifact = KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new("fast-key.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let calls: Vec<_> = artifact
            .program
            .modules
            .iter()
            .flat_map(|m| &m.functions)
            .flat_map(|f| &f.instructions)
            .filter_map(|op| match op {
                BytecodeInstruction::Call {
                    callee: CallTarget::StandardIntrinsic(op),
                    ..
                } => Some(*op),
                _ => None,
            })
            .collect();
        assert_eq!(calls.contains(&StandardIntrinsic::KeyLookupBegin), custom);
        assert_eq!(calls.contains(&StandardIntrinsic::MapInsert), !custom);
    }
}

#[test]
fn composed_enum_hash_uses_variant_identity_instead_of_version_local_slots() {
    let mut hashes = Vec::new();
    for variants in ["Empty, Present(Key)", "Present(Key), Empty"] {
        let source = format!(
            "struct Key {{val id:i32}} impl PartialEq for Key {{fn eq(self,other:Self)->bool {{self.id==other.id}}}} impl Eq for Key {{}} impl Hash for Key {{fn hash(self)->i64 {{self.id.hash()}}}} enum Envelope {{{variants}}} fn main()->i64 {{Envelope::Present(Key{{id:1}}).hash()}}"
        );
        let engine = KagariEngine::default();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("stable-variant.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        hashes.push(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
        );
    }
    assert_eq!(hashes[0], hashes[1]);
}
