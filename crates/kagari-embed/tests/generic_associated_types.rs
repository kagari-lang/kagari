use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("families.kgr", source),
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
    }
}

#[test]
fn identity_type_family_specializes_static_generic_calls() {
    execute(
        r#"
trait Family { type Item<T>; fn make<T>(self, value: T) -> Self::Item<T>; }
struct Number {}
impl Family for Number { type Item<U> = U; fn make<V>(self, value: V) -> V { value } }
fn make<T: Family>(x: T) -> T::Item<i32> { x.make(42) }
fn main() -> i32 { make(Number {}) }
"#,
    );
}

#[test]
fn type_families_construct_gc_objects_and_default_methods() {
    execute(
        r#"
trait Family { type Item<T>; fn make<T>(self, value: T) -> Self::Item<T>; fn again<T>(self, value: T) -> Self::Item<T> { self.make(value) } }
struct Holder<T> { val value: T }
struct Number {}
impl Family for Number { type Item<U> = Holder<U>; fn make<V>(self, value: V) -> Holder<V> { Holder { value } } }
fn make<T: Family>(x: T) -> T::Item<i32> { x.again(42) }
fn main() -> i32 { val result: <Number as Family>::Item<i32> = make(Number {}); result.value }
"#,
    );
}

#[test]
fn inherited_families_and_generic_trait_arguments_substitute_independent_binders() {
    execute(
        r#"
trait Family<A> { type Item<T>; fn make<T>(self, first: A, value: T) -> Self::Item<T>; }
trait Child: Family<i32> {}
struct Number {}
impl Family<i32> for Number { type Item<U> = (i32, U); fn make<V>(self, first: i32, value: V) -> (i32, V) { (first, value) } }
impl Child for Number {}
fn make<T: Child>(x: T) -> T::Item<i32> { x.make(20, 22) }
fn main() -> i32 { val pair = make(Number {}); match pair { (a, b) => a + b, } }
"#,
    );
}

#[test]
fn family_input_and_output_bounds_follow_the_trait_contract() {
    execute(
        r#"
trait Family { type Item<T: Comparable>: Comparable; fn make<T: Comparable>(self, value: T) -> Self::Item<T>; }
struct Number {}
impl Family for Number { type Item<U> = U; fn make<V: Comparable>(self, value: V) -> V { value } }
fn make<T: Family>(x: T) -> T::Item<i32> { x.make(42) }
fn main() -> i32 { make(Number {}) }
"#,
    );
}

#[test]
fn generic_impl_families_where_bounds_and_nested_outputs_execute() {
    execute(
        r#"
trait Read { fn read(self) -> i32; }
struct Holder<T> { val value: T }
impl Read for Holder<i32> { fn read(self) -> i32 { self.value } }
trait Family<A> { type Item<T> where T: Comparable; fn make<T: Comparable>(self, value: T) -> Self::Item<T>; }
impl<A> Family<A> for Holder<A> { type Item<U> = [(A, U)] where U: Comparable; fn make<V: Comparable>(self, value: V) -> [(A, V)] { [(self.value, value)] } }
fn make<A, F: Family<A>>(f: F, first: A) -> F::Item<i32> { f.make(22) }
fn main() -> i32 { val value = make(Holder { value: 20 }, 20); value[0][0] + value[0][1] }
"#,
    );
}

#[test]
fn output_trait_bounds_are_proved_under_family_inputs() {
    execute(
        r#"
trait Read { fn read(self) -> i32; }
struct Number { val value: i32 }
impl Read for Number { fn read(self) -> i32 { self.value } }
trait Family { type Item<T: Read>: Read; fn make<T: Read>(self, value: T) -> Self::Item<T>; }
struct Maker {}
impl Family for Maker { type Item<U> = U; fn make<V: Read>(self, value: V) -> V { value } }
fn make<F: Family>(f: F) -> F::Item<Number> { f.make(Number { value: 42 }) }
fn main() -> i32 { make(Maker {}).read() }
"#,
    );
}

#[test]
fn invalid_family_declarations_projections_and_dynamic_interfaces_are_rejected() {
    for source in [
        "trait Family { type Item<T>; } struct N {} impl Family for N {}",
        "trait Family { type Item<T>; } struct N {} impl Family for N { type Item = i32; }",
        "trait Family { type Item<T>; } struct N {} impl Family for N { type Item<U, V> = U; }",
        "trait Family { type Item<T>; } struct N {} impl Family for N { type Item<U> = Unknown; }",
        "trait Family { type Item<T>; } struct N {} impl Family for N { type Item<U> = Self::Item<U>; }",
        "trait Family { type Item<T>; } struct N {} impl Family for N { type Item<U> = Self::Other<U>; type Other<U> = U; }",
        "trait Family { type Item<T>; } fn f<T: Family>(x: T) -> T::Item { 0 }",
        "trait Family { type Item<T>; } fn f<T: Family>(x: T) -> T::Item<i32, bool> { 0 }",
        "trait Family { type Item<T>; } fn f(x: Family) {}",
        "trait Family { type Item<T>; } trait Child: Family {} fn f(x: Child) {}",
        "trait Family { type Item<T>; } fn f<T: Family<Item=i32>>(x: T) {}",
        "trait Family { type Item<T: Comparable>; } struct N {} impl Family for N { type Item<U: Comparable + HashKey> = U; }",
        "trait Family { type Item<T>: Comparable; } struct N {} impl Family for N { type Item<U> = U; }",
        "trait Family { type Item<T: HashKey>; } struct N {} impl Family for N { type Item<U> = U; } fn f(x: <N as Family>::Item<f32>) {}",
        "trait Family { type Item<T> = T; }",
        "trait Family { type Item<T: HashKey>; } struct N {} impl Family for N { type Item<U> = i32; } fn main()->i32 { val x: <N as Family>::Item<f32> = 42; x }",
        "trait Family { type A<T>; type B<T>; } struct N {} impl Family for N { type A<U> = Self::B<U>; type B<U> = Self::A<U>; }",
        "trait Family { type Item<'a>; }",
        "trait Family { type Item<const N: i32>; }",
        "trait Family { type Item<T, T>; }",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(
                    SourceFile::new("invalid.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn imported_families_and_defaults_keep_declaration_owned_binders() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "pub struct Holder<T> { pub val value: T } pub trait Family<A> { type Item<T>; fn make<T>(self, first: A, value: T) -> Self::Item<T>; fn again<T>(self, first:A, value:T) -> Self::Item<T> { self.make(first, value) } }",
        ),
        (
            "root",
            "use pkg::model::{Family, Holder}; struct N {} impl Family<i32> for N { type Item<U> = Holder<(i32, U)>; fn make<V>(self, first:i32, value:V) -> Holder<(i32,V)> { Holder { value: (first,value) } } } fn main() -> i32 { val item: <N as Family<i32>>::Item<i32> = N {}.again(20, 22); item.value[0] + item.value[1] }",
        ),
    ] {
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
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root.unwrap(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    for encoded in [false, true] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
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
fn unused_family_metadata_is_verified_before_loading() {
    use kagari_hir::{builtin::surface::StandardTypeConstraint, types::BuiltinType};
    use kagari_ir::module::{
        PublicAbiItem,
        abi::{AbiType, ConstraintAbi, GenericBoundAbi},
    };
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("families.kgr", "pub trait Family { type Item<T: Comparable>: Comparable; fn make<T: Comparable>(self, value:T)->Self::Item<T>; } struct N {} impl Family for N { type Item<U> = U; fn make<V: Comparable>(self, value:V)->V { value } } fn main()->i32 { 42 }"), Default::default(), Default::default()).unwrap();
    for mutation in 0..10 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation < 3 {
            let record = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    if let PublicAbiItem::Trait(record) = item {
                        Some(record)
                    } else {
                        None
                    }
                })
                .unwrap();
            match mutation {
                0 => {
                    record.associated_types[0].generic_params[0]
                        .owner
                        .path
                        .pop();
                }
                1 => record.associated_types[0].generic_params[0].position = 1,
                _ => {
                    let AbiType::Projection { arguments, .. } = &mut record.methods[0].return_type
                    else {
                        panic!("projection")
                    };
                    arguments.clear();
                }
            }
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
            let family = &mut table.associated_type_families[0];
            match mutation {
                3 => family.generic_params[0].owner = table.declaration.clone(),
                4 => family.generic_params[0].position = 1,
                5 => family.generic_params.clear(),
                6 => family.value = AbiType::Builtin(BuiltinType::Unit),
                7 => family.bounds.push(GenericBoundAbi {
                    ty: AbiType::Parameter {
                        owner: family.generic_params[0].owner.clone(),
                        position: 0,
                    },
                    constraints: vec![ConstraintAbi::Standard(StandardTypeConstraint::HashKey)],
                }),
                8 => family.declaration.path.last_mut().unwrap().name = "Unknown".into(),
                _ => {
                    family.value = AbiType::Projection {
                        receiver: Box::new(table.for_type.clone()),
                        interface: Box::new(match &table.trait_type {
                            AbiType::Trait(v) => v.clone(),
                            _ => unreachable!(),
                        }),
                        member: family.declaration.clone(),
                        arguments: vec![AbiType::Parameter {
                            owner: family.generic_params[0].owner.clone(),
                            position: 0,
                        }],
                    }
                }
            }
        }
        assert!(
            BytecodeArtifact::from_program(program, Default::default()).is_err(),
            "accepted mutation {mutation}"
        );
    }
}

#[test]
fn nested_families_and_inherited_input_bounds_normalize_without_runtime_dispatch() {
    execute(
        r#"
trait Read { fn read(self) -> i32; }
trait Child: Read {}
struct N { val value: i32 }
impl Read for N { fn read(self) -> i32 { self.value } }
impl Child for N {}
trait Family { type Item<T: Child>: Read; type Again<T: Child>: Read; fn make<T: Child>(self, value:T)->Self::Again<T>; }
struct Maker {}
impl Family for Maker { type Item<U> = U; type Again<U> = Self::Item<U>; fn make<V: Child>(self, value:V)->V { value } }
fn make<F: Family>(f:F)->F::Again<N> { f.make(N { value:42 }) }
fn main()->i32 { make(Maker {}).read() }
"#,
    );
}

#[test]
fn complete_family_metadata_cannot_make_a_dynamic_interface() {
    use kagari_ir::module::{
        PublicAbiItem,
        abi::{AbiType, AssociatedTypeAbi, AssociatedTypeFamilyAbi, GenericParameterAbi},
    };
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("dynamic.kgr", "pub trait Read { fn read(self)->i32; } struct N {} impl Read for N { fn read(self)->i32 { 42 } } fn main()->i32 { val x: Read = N {}; x.read() }"), Default::default(), Default::default()).unwrap();
    let mut program = artifact.program.clone();
    let module = &mut program.modules[program.root.index()];
    let (interface, implementation) = module
        .public_items
        .iter()
        .find_map(|item| {
            let PublicAbiItem::InterfaceTable(table) = item else {
                return None;
            };
            let AbiType::Trait(interface) = &table.trait_type else {
                return None;
            };
            Some((interface.declaration.clone(), table.declaration.clone()))
        })
        .unwrap();
    let member = kagari_hir::types::associated_type_id(&interface, "Item");
    let binder = kagari_hir::types::associated_type_id(&implementation, "Item");
    for item in &mut module.public_items {
        match item {
            PublicAbiItem::Trait(record) => record.associated_types.push(AssociatedTypeAbi {
                declaration: member.clone(),
                generic_params: vec![GenericParameterAbi {
                    owner: member.clone(),
                    position: 0,
                }],
                parameter_bounds: Vec::new(),
                bounds: Vec::new(),
            }),
            PublicAbiItem::InterfaceTable(table) => {
                table
                    .associated_type_families
                    .push(AssociatedTypeFamilyAbi {
                        declaration: member.clone(),
                        generic_params: vec![GenericParameterAbi {
                            owner: binder.clone(),
                            position: 0,
                        }],
                        bounds: Vec::new(),
                        value: AbiType::Parameter {
                            owner: binder.clone(),
                            position: 0,
                        },
                    })
            }
            _ => {}
        }
    }
    assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
}

#[test]
fn family_input_bounds_substitute_the_owning_self_type() {
    execute(
        r#"
trait Link<A> {}
struct N { val value:i32 }
struct Maker {}
impl Link<Maker> for N {}
trait Family { type Item<T: Link<Self>>; fn make<T: Link<Self>>(self, value:T)->Self::Item<T>; }
impl Family for Maker { type Item<U> = U where U: Link<Maker>; fn make<V: Link<Maker>>(self, value:V)->V { value } }
fn main()->i32 { val value: <Maker as Family>::Item<N> = Maker {}.make(N { value:42 }); value.value }
"#,
    );
}
