use crate::test_support::execution;
use execution::Route;
use kagari_runtime::module::entry::ProgramEntry;

use crate::test_support::cancellation as support;
use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_bytecode::program::{identity, verify_program};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::types::PublicItem;
use kagari_runtime::{Runtime, RuntimeConfig, native::module::NativeModule, value::Value};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_types::{language as standard_traits, language::Protocol, scalar::BuiltinType, ty::Ty};
use kagari_vm::vm::Vm;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let artifact = engine
        .compile_source(SourceFile::new("standard-traits.kgr", source))
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[Route::Source, Route::Artifact, Route::ArtifactJit],
        42,
    );
}

#[test]
fn standard_trait_methods_and_generic_bounds_execute() {
    execute(include_str!(
        "../../../../tests/fixtures/test-fixtures/language/standard-traits.kgr"
    ));
}

#[test]
fn char_standard_traits_preserve_scalar_semantics() {
    execute(
        r#"use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::collections::HashSet;
use core::cmp::Ordering;

fn inspect<T: Eq + Ord + Hash + Display + Debug>(a:T, b:T)->bool {
    a.eq(b) && a == b && a.cmp(b) == Ordering::Equal
        && a.partial_cmp(b) == Some(Ordering::Equal)
        && a.hash() == b.hash() && a.display() == b.display() && a.debug() == b.debug()
}
fn make<T: Default>()->T { T::default() }
fn main()->i32 {
    val nul: char = make();
    val original = '🙂'; val copied = original;
    if nul != '\0' || !inspect(original, copied) { return 1; }
    if !('\0' < 'a' && 'a' < 'é' && 'é' < '🙂' && '🙂' < '\u{10ffff}') { return 2; }
    if original.display() != "🙂" || original.debug() != "'🙂'" { return 3; }
    if nul.display() != "\0" || nul.debug() != "'\\0'" { return 4; }
    if '\n'.debug() != "'\\n'" || '\''.debug() != "'\\''" || '\\'.debug() != "'\\\\'" { return 5; }
    val keys: HashSet<char> = HashSet::new();
    keys.insert(original); keys.insert(copied); keys.insert(nul);
    if keys.len() != 2u64 || !keys.contains('🙂') || keys.contains('a') { return 6; }
    42
}
"#,
    );
}

#[test]
fn char_conversions_validate_scalars_and_use_reverse_adapters() {
    execute(
        r#"use core::char::CharTryFromError;
use std::char::CharTryFromError as FacadeError;

fn convert<T, U: From<T>>(value:T)->U { U::from(value) }
fn reverse<T: Into<U>, U>(value:T)->U { value.into() }
fn checked<T: TryInto<char, Error = CharTryFromError>>(value:T)->Result<char, CharTryFromError> {
    value.try_into()
}
fn main()->i32 {
    for scalar in [0u32, 0x7fu32, 0x80u32, 0xd7ffu32, 0xe000u32, 0xffffu32, 0x10000u32, 0x1f642u32, 0x10ffffu32] {
        val value = match char::try_from(scalar) { Ok(value) => value, Err(_) => { return 1; } };
        val code: u32 = convert(value);
        val reversed: u32 = reverse(value);
        if code != scalar || reversed != scalar { return 2; }
        match checked(scalar) { Ok(other) => { if other != value { return 3; } }, Err(_) => { return 4; } }
    }
    for scalar in [0xd800u32, 0xdbffu32, 0xdc00u32, 0xdfffu32, 0x110000u32, 0xffffffffu32] {
        match char::try_from(scalar) { Err(CharTryFromError::InvalidScalar) => {}, Ok(_) => { return 5; } };
        match checked(scalar) { Err(FacadeError::InvalidScalar) => {}, Ok(_) => { return 6; } }
    }
    val text: String = convert('🙂');
    val reversed: String = reverse('é');
    if text != "🙂" || reversed != "é" || String::from('\0') != "\0" { return 7; }
    42
}
"#,
    );
}

#[test]
fn char_does_not_acquire_numeric_or_traversal_capabilities() {
    let engine = KagariEngine::default();
    for source in [
        "fn main()->char { 'a' + 'b' }",
        "fn main()->char { 'a' & 'b' }",
        "fn main()->u32 { 'a' as u32 }",
        "fn main()->char { 65u32 as char }",
        "fn main()->char { char::from(65u32) }",
        "fn main()->char { val value: char = 65u32; value }",
        "fn main()->bool { 'a' == 97u32 }",
        "use core::iter::Sum; fn needs<T: Sum<T>>(value:T) {} fn main() { needs('a'); }",
        "use core::iter::Product; fn needs<T: Product<T>>(value:T) {} fn main() { needs('a'); }",
        "use core::str::FromStr; fn needs<T: FromStr>(value:T) {} fn main() { needs('a'); }",
        "fn main() { for value in 'a'..'z' {} }",
        "fn main() { for value in 'a'..='z' {} }",
        "fn main() { for value in 'a'.. {} }",
    ] {
        let error = engine
            .compile_to_artifact(
                SourceFile::new("char-capabilities.kgr", source),
                Default::default(),
            )
            .unwrap_err();
        assert!(
            matches!(error, EmbeddingError::Diagnostics { .. }),
            "{source}: {error:?}"
        );
    }
}

#[test]
fn explicit_formatting_uses_normal_static_dispatch() {
    execute(
        r#"use std::fmt::{Debug, Display};

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
fn explicit_identity_keys_and_standard_member_keys_execute_through_artifacts() {
    execute(
        r#"use std::collections::{HashMap};

struct Key { var value: i32 }
impl PartialEq for Key { fn eq(self, other: Self) -> bool { self === other } }
impl Eq for Key {}
// A constant hash is correct for explicit identity equality, with collisions.
impl std::hash::Hash for Key { fn hash(self) -> i64 { 0i64 } }
fn main()->i32 {
    val map: HashMap<(Option<String>, i32), i32> = HashMap::new();
    map.insert((Some("a"), 2), 20);
    val key = Key { value: 1 };
    val identities: HashMap<Key, i32> = HashMap::new();
    identities.insert(key, 22);
    key.value = 9;
    val different = Key { value: 9 };
    if identities.contains_key(different) { 0 } else {
        (match map.get((Some("a"), 2)) { Some(value) => value, None => 0 }) + (match identities.get(key) { Some(value) => value, None => 0 })
    }
}
"#,
    );
}

#[test]
fn imports_supertraits_and_nested_structural_keys() {
    execute(
        r#"use std::collections::{HashSet};
use std::fmt::{Display};

use core::cmp::Eq as Equal;
use core::hash::Hash;
use core::fmt as formatting;
trait Named: Equal { fn name(self)->String; }
struct Item { val value: i32 }
impl PartialEq for Item { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl Equal for Item {}
impl Hash for Item { fn hash(self) -> i64 { self.value.hash() } }
impl Named for Item { fn name(self)->String { "item" } }
fn same<T: Named>(a:T,b:T)->bool { a.eq(b) }
fn output<T: formatting::Display>(x:T)->String { x.display() }
fn hashed<T: Equal + Hash>(x:T)->i64 { x.hash() }
fn main()->i32 {
    val item = Item { value:42 };
    val set: HashSet<Option<(i32, String)>> = HashSet::new();
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
        "use std::hash::{Hash};\nfn needs<T: Hash>(x:T) {} fn main() { needs(1.5); }",
        "use std::collections::{HashMap};\nenum Key { Good(i32), Bad(f64) } fn main() { val map: HashMap<Key, i32> = HashMap::new(); }",
        "use std::hash::{Hash};\nenum Key { Good(i32), Bad(f64) } fn needs<T: Eq + Hash>(x:T) {} fn main() { needs(Key::Good(1)); }",
        "fn needs<T: Eq<i32>>(x:T) {} fn main() {}",
        "struct Item {} impl Eq for Item {} fn main() {}",
        "use std::hash::{Hash};\nstruct Item {} impl Hash for Item { fn hash(self)->i64 { 1 } } fn main() {}",
        "use std::fmt::{Debug};\nimpl Debug for i32 { fn debug(self)->String { \"x\" } } fn main() {}",
        "use std::fmt::{Debug};\nstruct Item {} impl Debug for Item { fn debug(self)->i32 { 1 } } fn main() {}",
        "use std::fmt::{Display};\nstruct Item {} impl Display for Item {} fn main() {}",
        "trait Eq {} fn f<T: Eq>(x:T)->bool { x == x } fn main() {}",
        "use std::collections::{HashSet};\ntrait Hash {} fn f<T: Eq + Hash>(x:T)->HashSet<T> { HashSet::new() } fn main() {}",
        "fn f<T: HashKey>(x:T) {} fn main() {}",
    ] {
        let error = KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new("bad-traits.kgr", source),
                Default::default(),
            )
            .unwrap_err();
        assert!(
            matches!(error, EmbeddingError::Diagnostics { .. }),
            "{source}: {error:?}"
        );
    }
}

#[test]
fn standard_impls_are_available_to_trait_inputs_and_associated_outputs() {
    execute(
        r#"use std::collections::{HashMap};
use std::hash::{Hash};

trait Read<T: Eq + Hash> { type Item: Eq + Hash; fn get(self, value:T)->Self::Item; }
struct Reader {}
impl Read<i32> for Reader { type Item = (i32, String); fn get(self,value:i32)->(i32,String) { (value,"ok") } }
fn same<T: PartialEq>(a:T,b:T)->bool { a.eq(b) }
fn main()->i32 {
    val unit: HashMap<(), i32> = HashMap::new(); unit.insert((),42);
    val value = Reader {}.get(42);
    if same(1.5,1.5) && same(value,(42,"ok")) && ().debug() == "()" { (match unit.get(()) { Some(value) => value, None => 0 }) } else { 0 }
}
"#,
    );
}

#[test]
fn malformed_standard_implementations_and_reserved_modules_are_rejected() {
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("format-wire.kgr", "use std::fmt::{Debug};\nstruct Item {} impl Debug for Item { fn debug(self)->String { \"ok\" } } fn main()->i32 { val item=Item {}; item.debug(); 42 }"),Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact.import_into(&context, &Default::default()).unwrap();
    let table = identity::collect(
        &artifact
            .records()
            .program
            .modules
            .iter()
            .collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.records().program, &table, &Default::default()).unwrap();
    for mutation in 0..5 {
        let mut program = artifact.records().program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation == 0 {
            module.identity.package.0 = "kagari-core".into();
        } else {
            let table = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    if let PublicItem::InterfaceTable(table) = item {
                        Some(table)
                    } else {
                        None
                    }
                })
                .unwrap();
            match mutation {
                1 => table.methods[0].return_type = Ty::Builtin(BuiltinType::I32),
                2 => {
                    let Ty::Trait(trait_type) = &mut table.trait_type else {
                        panic!("trait");
                    };
                    trait_type.declaration = context
                        .intern(&standard_traits::identity(Protocol::Hash))
                        .unwrap();
                }
                3 => table.methods.clear(),
                _ => {
                    let Ty::Trait(trait_type) = &mut table.trait_type else {
                        panic!("trait");
                    };
                    trait_type.arguments.push(Ty::Builtin(BuiltinType::I32));
                }
            }
        }
        assert!(
            verify_program(&program, &context.snapshot(), &Default::default()).is_err(),
            "mutation {mutation}"
        );
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err()
        );
    }
}

#[test]
fn automatic_debug_of_nested_values_never_invokes_custom_member_code() {
    execute(
        r#"use std::fmt::{Debug};

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
        r#"use std::collections::{HashMap, HashSet};

struct Item { var value: i32 }
fn main()->i32 {
    val a = Item { value: 1 }; val alias = a;
    val b = Item { value: 1 };
    val values = [1]; val copy = [1];
    val map: HashMap<i32, i32> = HashMap::new();
    val set: HashSet<i32> = HashSet::new();
    a.value = 2;
    if a === alias && a !== b && values === values && values !== copy && values !== [true] && map === map && set === set { 42 } else { 0 }
}
"#,
    );
}

#[test]
fn identity_operators_reject_value_types() {
    for expression in [
        "1 === 1",
        "true !== false",
        "\"a\" === \"a\"",
        "(1,2) === (1,2)",
        "Some(1) === Some(1)",
    ] {
        let source = format!("fn main()->bool {{ {expression} }}");
        assert!(matches!(
            KagariEngine::default().compile_to_artifact(
                SourceFile::new("bad-identity.kgr", source),
                Default::default()
            ),
            Err(EmbeddingError::Diagnostics { .. })
        ));
    }
}

#[test]
fn custom_equality_composes_and_enum_overrides_variant_checks() {
    execute(
        r#"use std::hash::{Hash};

struct Point { val x:i32 }
impl PartialEq for Point { fn eq(self, other:Self)->bool { self.x == other.x } }
impl Eq for Point {}
enum Id { Local(i32), Remote(i32) }
fn number(v:Id)->i32 { match v { Id::Local(x)=>x, Id::Remote(x)=>x } }
impl PartialEq for Id { fn eq(self, other:Self)->bool { number(self)==number(other) } }
impl Eq for Id {}
impl Hash for Id { fn hash(self)->i64 { number(self).hash() } }
fn same<T:Eq>(a:T,b:T)->bool { a==b && a.eq(b) }
fn main()->i32 {
 val a=Point{x:1}; val b=Point{x:1};
 if same(a,b) && a !== b && same((a,7),(b,7)) && same(Some(a),Some(b)) && same(Id::Local(42),Id::Remote(42)) && Id::Local(42).hash()==Id::Remote(42).hash() {42} else {0}
}
"#,
    );
}

#[test]
fn custom_keys_use_hash_buckets_and_script_equality() {
    execute(
        r#"use std::collections::{HashMap, HashSet};
use std::hash::{Hash};

struct Key { val id:i32, var ignored:i32 }
impl PartialEq for Key { fn eq(self, other:Self)->bool { self.id == other.id } }
impl Eq for Key {}
impl Hash for Key { fn hash(self)->i64 { 0.hash() } }
fn store<T:Eq+Hash>(map:HashMap<T,i32>,key:T,value:i32) { map.insert(key,value); }
fn main()->i32 {
 val map:HashMap<(Option<Key>,i32),i32> = HashMap::new();
 val a=Key{id:1,ignored:0}; val b=Key{id:2,ignored:0};
 store(map,(Some(a),7),20); store(map,(Some(b),7),21);
 a.ignored=99;
 store(map,(Some(Key{id:1,ignored:3}),7),22);
 val set:HashSet<Key> = HashSet::new();
 set.insert(a); set.insert(b); set.insert(Key{id:1,ignored:4});
 { val passed = set.len()==Vec::from([1,2]).len(); if !passed { val zero = 0; 1 / zero; } };
 { val passed = set.contains(Key{id:2,ignored:5}); if !passed { val zero = 0; 1 / zero; } };
 { val passed = set.remove(Key{id:2,ignored:5}); if !passed { val zero = 0; 1 / zero; } };
 { val passed = !set.contains(b); if !passed { val zero = 0; 1 / zero; } };
 { val passed = map.len()==Vec::from([1,2]).len(); if !passed { val zero = 0; 1 / zero; } };
 { val passed = map.contains_key((Some(Key{id:2,ignored:0}),7)); if !passed { val zero = 0; 1 / zero; } };
 val query = (Some(Key{id:1,ignored:0}),7);
 val found = map.get(query);
 val result = match found { Some(value) => value, None => 0 };
 { val passed = (match map.remove((Some(b),7)) { Some(value) => value, None => 0 })==21; if !passed { val zero = 0; 1 / zero; } };
 result+20
}
"#,
    );
}

#[test]
fn explicit_recursive_enum_protocols_compose_with_member_operations() {
    execute(
        r#"use std::collections::{HashSet};
use std::hash::{Hash};

struct Key<T> { val value:T }
impl<T:PartialEq> PartialEq for Key<T> { fn eq(self, other:Self)->bool { self.value == other.value } }
impl<T:Eq> Eq for Key<T> {}
impl<T:Eq+Hash> Hash for Key<T> { fn hash(self)->i64 { self.value.hash() } }
enum Chain { End, Next(Key<i32>, Chain) }
impl PartialEq for Chain {
 fn eq(self, other: Self) -> bool { match (self, other) {
  (Chain::End, Chain::End) => true,
  (Chain::Next(a, tail), Chain::Next(b, rest)) => a == b && tail == rest,
  _ => false
 } }
}
impl Eq for Chain {}
impl Hash for Chain { fn hash(self) -> i64 { match self { Chain::End => 0i64, Chain::Next(key, tail) => key.hash() + tail.hash() } } }
fn main()->i32 {
 val a=Chain::Next(Key{value:42},Chain::End);
 val b=Chain::Next(Key{value:42},Chain::End);
 val set:HashSet<Chain> = HashSet::new(); set.insert(a);set.insert(b);
 if a != b { return 0; }
 if set.len()==Vec::from([1]).len() && set.contains(b) {42} else {0}
}
"#,
    );
}

#[test]
fn custom_enum_keys_ignore_variants_and_uncompared_payload_capabilities() {
    execute(
        r#"use std::collections::{HashMap, HashSet};
use std::hash::{Hash};

enum Id { Local(i32,f32), Remote(i32) }
fn number(v:Id)->i32 { match v { Id::Local(id,_)=>id, Id::Remote(id)=>id } }
impl PartialEq for Id { fn eq(self,other:Self)->bool {number(self)==number(other)} }
impl Eq for Id {}
impl Hash for Id {fn hash(self)->i64 {number(self).hash()}}
fn main()->i32 {
 val a:HashSet<Id> = HashSet::new(); val b:HashSet<Id> = HashSet::new();
 a.insert(Id::Local(42,1.5));b.insert(Id::Remote(42));b.insert(Id::Remote(7));
 val equivalent = Id::Remote(42);
 if !a.contains(equivalent) || !b.contains(equivalent) { return 0; }
 a.insert(Id::Remote(42));
 if a.len() != 1u64 || b.len() != 2u64 { return 0; }
 val map:HashMap<Option<Id>,i32> = HashMap::new();
 map.insert(Some(Id::Local(42,2.5)),42);
 (match map.get(Some(Id::Remote(42))) { Some(value) => value, None => 0 })
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
fn main()->i32 {
 val c=Counter{count:0};val a=Key{id:1,counter:c};val b=Key{id:2,counter:c};
 { val passed = !(a,a).eq((b,a)); if !passed { val zero = 0; 1 / zero; } };
 { val passed = c.count==1; if !passed { val zero = 0; 1 / zero; } };
 { val passed = Some((a,a))!=None; if !passed { val zero = 0; 1 / zero; } };
 { val passed = c.count==1; if !passed { val zero = 0; 1 / zero; } };
 { val passed = Some((a,a))!=Some((b,a)); if !passed { val zero = 0; 1 / zero; } };
 { val passed = c.count==2; if !passed { val zero = 0; 1 / zero; } };
 { val passed = a==a; if !passed { val zero = 0; 1 / zero; } };
 { val passed = c.count==3; if !passed { val zero = 0; 1 / zero; } };
 42
}
"#,
    );
}

#[test]
fn comparison_only_types_do_not_inherit_identity_hashing() {
    for tail in [
        "use std::collections::{HashSet};\nfn main(){val set:HashSet<Key> = HashSet::new();}",
        "use std::collections::{HashSet};\nfn main(){val set:HashSet<(Key,i32)> = HashSet::new();}",
        "use std::collections::{HashSet};\nenum E {Value(Key)} fn main(){val set:HashSet<E> = HashSet::new();}",
        "use std::hash::{Hash};\nfn needs<T:Eq+Hash>(v:T){} fn main(){needs(Key{});}",
        "fn main(){Key{}.hash();}",
    ] {
        let source = format!(
            "struct Key {{}} impl PartialEq for Key {{fn eq(self,other:Self)->bool {{true}}}} impl Eq for Key {{}} {tail}"
        );
        assert!(
            matches!(
                KagariEngine::default().compile_to_artifact(
                    SourceFile::new("bad-key.kgr", source),
                    Default::default()
                ),
                Err(EmbeddingError::Diagnostics { .. })
            ),
            "{tail}"
        );
    }
}

#[test]
fn key_callback_traps_and_reentry_release_guards_without_partial_insertion() {
    let source = r#"use std::collections::{HashSet};
use std::hash::{Hash};

struct State {var mode:i32,var calls:i32}
struct Key {val id:i32,val owner:HashSet<Key>,val state:State}
impl PartialEq for Key {fn eq(self,other:Self)->bool {
 self.state.calls+=1;
 if self.state.mode==1 {self.owner.clear();}
 if self.state.mode==2 {val limit = 2147483647; limit + 1;}
 if self.state.mode==5 {
  self.state.mode=0;
  val absent = Key { id:99, owner:self.owner, state:self.state };
  { val passed = !self.owner.contains(absent); if !passed { val zero = 0; 1 / zero; } };
  self.state.mode=5;
 }
 self.id==other.id
}}
impl Eq for Key {}
impl Hash for Key {fn hash(self)->i64 {
 if self.state.mode==3 {self.owner.clear();}
 if self.state.mode==4 {val zero = 0; 1 / zero;}
 0.hash()
}}
trait Test {fn mode(self,value:i32);fn attempt(self);fn calls(self)->i32;fn clear(self);}
struct Tester {val set:HashSet<Key>,val key:Key,val state:State}
impl Test for Tester {
 fn mode(self,value:i32){self.state.mode=value;}
 fn attempt(self){self.set.insert(self.key);}
 fn calls(self)->i32 {self.state.calls}
 fn clear(self){self.set.clear();}
}
pub fn make()->(Test,HashSet<Key>) {
 val set:HashSet<Key> = HashSet::new();val state=State{mode:0,calls:0};
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
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let root_module = &artifact.records().program.modules[artifact.records().program.root.index()];
    let declaration = root_module
        .trait_contracts
        .iter()
        .find(|c| c.abi.name == "Test")
        .unwrap()
        .declaration;
    let query = artifact
        .definitions()
        .resolve(declaration)
        .unwrap()
        .to_path();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = {
        let mut registered = Runtime::new(config);
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    };
    let (definitions, artifact) = artifact.into_parts();
    let program =
        DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap();
    let loaded = runtime.load_program("callback-cleanup", program).unwrap();
    let declaration = loaded.definitions().lookup(&query).unwrap();
    let method = |name: &str| {
        loaded
            .definitions()
            .lookup_child(declaration, DefinitionKind::Method, name, 0)
            .unwrap()
    };
    let vm = Vm::new(runtime);
    for mode in 1..=4 {
        let value = vm
            .execute(&loaded, "make")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result");
        let root = vm.runtime().root_value(value).unwrap();
        let Value::Tuple(values) = value else {
            panic!("tuple")
        };
        let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
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
                "integer overflow"
            } else {
                "division by zero"
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
    let value = vm
        .execute(&loaded, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
    let Value::Tuple(values) = value else {
        panic!("tuple")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    vm.invoke_interface_method(&values[0], &method("mode"), &[Value::I32(5)])
        .unwrap();
    vm.invoke_interface_method(&values[0], &method("attempt"), &[])
        .unwrap();
    let Value::Set(id) = values[1] else {
        panic!("set")
    };
    assert_eq!(vm.runtime().gc().set_len(id), Some(2));
    assert_eq!(vm.runtime().gc().active_roots(), 1);
    drop(root);

    for limit in [2, 12, 24, 40] {
        let value = vm
            .execute(&loaded, "make")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result");
        let root = vm.runtime().root_value(value).unwrap();
        let Value::Tuple(values) = value else {
            panic!("tuple")
        };
        let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
        let Value::Set(id) = values[1] else {
            panic!("set")
        };
        let _options = vm.runtime().execution_options();

        let session = support::cancel_after(vm.runtime(), &loaded, limit);
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
    for downstream_override in [false, true] {
        let engine = KagariEngine::default();
        let model = r#"use std::collections::{HashMap};
use std::hash::{Hash};

pub struct Key {pub val id:i32}
impl PartialEq for Key {fn eq(self,other:Self)->bool {self.id==other.id}}
impl Eq for Key {}
impl Hash for Key {fn hash(self)->i64 {self.id.hash()}}
pub fn equal(a:Key,b:Key)->bool {a==b}
pub fn make()->HashMap<Key,i32> {val m:HashMap<Key,i32> = HashMap::new();m.insert(Key{id:1},42);m}
"#;
        let root_source = if downstream_override {
            "use pkg::model::Key; impl PartialEq for Key {fn eq(self,other:Self)->bool {false}} fn main()->i32 {42}"
        } else {
            "use pkg::model::{Key,equal,make}; fn same<T:Eq>(a:T,b:T)->bool {a==b} fn main()->i32 {val a=Key{id:1};val b=Key{id:1};if equal(a,b) && same(a,b) && a !== b {(match make().get(b) { Some(value) => value, None => 0 })} else {0}}"
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
        let checked =
            engine.compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default());
        if downstream_override {
            assert!(checked.is_err());
            continue;
        }
        let artifact = engine
            .emit_artifact(&checked.unwrap(), Default::default())
            .unwrap();
        let artifact = BytecodeArtifact::from_bytes(
            &artifact.records().to_bytes(artifact.definitions()).unwrap(),
        )
        .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[test]
fn portable_hash_implementations_require_explicit_comparison_contracts() {
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "key-wire.kgr",
                r#"use std::hash::{Hash};

struct Key {val id:i32}
impl PartialEq for Key {fn eq(self,other:Self)->bool {self.id==other.id}}
impl Eq for Key {}
impl Hash for Key {fn hash(self)->i64 {self.id.hash()}}
fn main()->i64 {Key{id:1}.hash()}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    for missing in [Protocol::Eq, Protocol::PartialEq] {
        let mut program = artifact.records().program.clone();
        let module = &mut program.modules[program.root.index()];
        module.public_items.retain(|item| !matches!(item,PublicItem::InterfaceTable(table) if matches!(&table.trait_type, Ty::Trait(t) if Some(t.declaration)==missing.definition(artifact.definitions()))));
        assert!(
            verify_program(&program, artifact.definitions(), &Default::default()).is_err(),
            "missing {missing:?}"
        );
    }
}

#[test]
fn explicit_enum_hash_is_stable_across_variant_reordering() {
    let mut hashes = Vec::new();
    for variants in ["Empty, Present(Key)", "Present(Key), Empty"] {
        let source = format!(
            "use std::hash::{{Hash}};\nstruct Key {{val id:i32}} impl PartialEq for Key {{fn eq(self,other:Self)->bool {{self.id==other.id}}}} impl Eq for Key {{}} impl Hash for Key {{fn hash(self)->i64 {{self.id.hash()}}}} enum Envelope {{{variants}}} impl PartialEq for Envelope {{fn eq(self,other:Self)->bool {{as_option(self)==as_option(other)}}}} impl Eq for Envelope {{}} impl Hash for Envelope {{fn hash(self)->i64 {{as_option(self).hash()}}}} fn as_option(value:Envelope)->Option<Key> {{match value {{Envelope::Empty=>None,Envelope::Present(key)=>Some(key)}}}} fn main()->i64 {{Envelope::Present(Key{{id:1}}).hash()}}"
        );
        let engine = KagariEngine::default();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("stable-variant.kgr", source),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        hashes.push(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
        );
    }
    assert_eq!(hashes[0], hashes[1]);
}

#[test]
fn structurally_dynamic_standard_traits_dispatch_through_parents() {
    execute(
        r#"
use std::fmt::{Debug, Display};
use std::hash::Hash;
trait Inspect: Debug + Display + Hash { fn identity(self) -> i32 where Self: Inspect { 42 } }
struct Item { val value: i32 }
impl Debug for Item { fn debug(self) -> String { "item" } }
impl Display for Item { fn display(self) -> String { val value: Self = self; value.value.display() } }
impl Hash for Item { fn hash(self) -> i64 { self.value.hash() } }
impl PartialEq for Item { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl Eq for Item {}
impl Inspect for Item {}
fn main() -> i32 {
    val view: Inspect = Item { value: 42 };
    if view.identity() != 42 { return 0; }
    val debug: Debug = view;
    val display: Display = view;
    val hash: Hash = view;
    if debug.debug() != "item" || display.display() != "42" { return 0; }
    if hash.hash() != 42.hash() { return 0; }
    42
}
"#,
    );
}

#[test]
fn equality_requires_an_explicit_or_conditional_static_implementation() {
    let engine = KagariEngine::default();
    for source in [
        "struct Item {} fn main() -> bool { val a = Item {}; a == a }",
        "enum Item { One } fn main() -> bool { Item::One == Item::One }",
        "struct Item {} fn main() -> bool { val a = Vec::from([Item {}]); a == a }",
        "struct Item {} fn main() -> bool { val a = [Item {}]; a == a }",
        "struct Item {} fn main() -> bool { val a = Some(Item {}); a == a }",
        "struct Item {} fn main() -> bool { val a = (Item {},); a == a }",
        "fn same<T>(a: T, b: T) -> bool { a == b }",
        "trait Read {} struct Item {} impl Read for Item {} impl PartialEq for Item { fn eq(self, other: Self) -> bool { true } } fn main() -> bool { val a: Read = Item {}; a == a }",
        "struct Item {} fn main() -> bool { Item {}.eq(Item {}) }",
        "fn main() -> i64 { [1].hash() }",
    ] {
        assert!(
            matches!(
                engine.compile_to_artifact(
                    SourceFile::new("comparison-required.kgr", source),
                    Default::default()
                ),
                Err(EmbeddingError::Diagnostics { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn arrays_compare_contents_through_selected_element_operations() {
    execute(
        r#"
struct Counter { var calls: i32 }
struct Item { val value: i32, val counter: Counter }
impl PartialEq for Item {
    fn eq(self, other: Self) -> bool { self.counter.calls += 1; self.value == other.value }
}
fn same<T: PartialEq>(a: T, b: T) -> bool { a == b }
fn arrays<T: PartialEq>(a: [T], b: [T]) -> bool { a == b }
fn main() -> i32 {
    val count = Counter { calls: 0 };
    val a = Item { value: 1, counter: count };
    val b = Item { value: 2, counter: count };
    val xs = [a, b];
    if xs == [b, a] || count.calls != 1 { return 1; }
    if !same(xs, xs) || count.calls != 3 { return 2; }
    if xs == [a] || count.calls != 3 { return 3; }
    if !arrays(xs, [a, b]) || count.calls != 5 { return 4; }
    if !(xs === xs && xs !== [a, b]) { return 5; }
    val nan = [0.0f32 / 0.0f32];
    if nan == nan { return 6; }
    if !([0.0f32] == [-0.0f32]) || !([[1, 2]] == [[1, 2]]) { return 7; }
    42
}
"#,
    );
}

#[test]
fn intrinsic_standard_traits_box_without_changing_operations() {
    execute(
        r#"
use std::fmt::{Debug, Display};
use std::hash::Hash;
mod model {
    use std::fmt::Debug;
    pub struct Entry { pub val value: i32 }
    pub trait Inspect: Debug { fn value(self) -> i32; }
    impl Inspect for Entry { fn value(self) -> i32 { self.value } }
    pub fn boxed(value: Entry) -> Debug { value }
}
struct Item { val value: i32 }
trait Inspect: Debug { fn value(self) -> i32; }
impl Inspect for Item { fn value(self) -> i32 { self.value } }
fn boxed_tuple<T: Debug>(value: T) -> Debug { (value,) }
fn main() -> i32 {
    val d: Debug = 42;
    val s: Display = 42;
    val h: Hash = 42;
    if d.debug() != 42.debug() || s.display() != 42.display() || h.hash() != 42.hash() { return 1; }
    val tuple = (42, "value");
    val tuple_debug: Debug = tuple;
    val tuple_hash: Hash = tuple;
    if tuple_debug.debug() != tuple.debug() || tuple_hash.hash() != tuple.hash() { return 2; }
    val item = Item { value: 42 };
    val item_debug: Debug = item;
    val inspect: Inspect = item;
    val parent: Debug = inspect;
    if item_debug.debug() != item.debug() || parent.debug() != item.debug() || inspect.value() != 42 { return 3; }
    if boxed_tuple(42).debug() != (42,).debug() { return 5; }
    val remote_object = model::Entry { value: 42 };
    val remote: model::Inspect = remote_object;
    val remote_debug: Debug = remote;
    if remote_debug.debug() != model::boxed(remote_object).debug() || remote.value() != 42 { return 6; }
    val text: Display = "hello";
    val flag: Debug = true;
    if text.display() != "hello" || flag.debug() != true.debug() { return 4; }
    42
}
"#,
    );
}
