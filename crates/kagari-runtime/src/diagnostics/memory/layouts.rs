//! Pure layout facts and cross-version admission, measured separately from inputs.
use super::{Phases, compare, compile, preparation, provider, snapshot};
use crate::{
    Runtime,
    diagnostics::allocations::{self, Counts},
};
use kagari_bytecode::{
    instruction::{EnumId, StructId},
    program::BytecodeProgram,
};
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::slice;

const SOURCE: &str = r#"
    struct Marker {}
    fn marker() -> Marker { Marker {} }
    struct Holder<T> { val value: T }
    enum Wrapped<T> { Some(T), None }
    trait Build {
        fn make<T>(self, value: T) -> Holder<T> { Holder { value: value } }
        fn wrap<T>(self, value: T) -> Wrapped<T> { Wrapped::Some(value) }
    }
    impl Build for i32 {}
    fn main() -> i32 { val b: Build = 0; val h = b.make(7); val w = b.wrap(h.value); h.value }
"#;

fn layout_lifecycle(
    code: &BytecodeProgram,
    count: usize,
    enumeration: bool,
    prepare: bool,
) -> (Phases, Counts) {
    allocations::measure(|| {
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("memory", code.clone()).unwrap();
        let structure = loaded
            .bytecode
            .structures
            .iter()
            .position(|layout| !layout.arguments.iter().all(Ty::is_concrete))
            .unwrap();
        let variant = loaded
            .bytecode
            .enumerations
            .iter()
            .position(|layout| !layout.arguments.iter().all(Ty::is_concrete))
            .unwrap();
        let declaration = if enumeration {
            loaded.bytecode.enumerations[variant].declaration
        } else {
            loaded.bytecode.structures[structure].declaration
        };
        let marker = loaded
            .bytecode
            .structures
            .iter()
            .find(|layout| loaded.definition_name(layout.declaration) == Some("Marker"))
            .unwrap()
            .declaration;
        let nominal = Ty::Struct(NominalTy {
            declaration: marker,
            arguments: vec![],
            associated_types: Default::default(),
        });
        let inputs = (1..=count)
            .map(|width| {
                let ty = Ty::Tuple(vec![
                    nominal.clone(),
                    Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); width]),
                ]);
                let arguments = runtime
                    .resolve_type_arguments(&loaded, slice::from_ref(&ty))
                    .unwrap();
                (ty, arguments)
            })
            .collect::<Vec<_>>();
        let (setup, cold, warm, repeated) = preparation(&runtime, || {
            if prepare {
                for (ty, arguments) in &inputs {
                    let scope = runtime
                        .prepare_layout_scope(&loaded, declaration, arguments)
                        .unwrap();
                    assert!(scope.is_some());
                    if enumeration {
                        runtime
                            .modules
                            .applied_enum_variant(
                                &loaded,
                                EnumId::new(variant),
                                slice::from_ref(ty),
                                0,
                                scope,
                            )
                            .unwrap();
                    } else {
                        runtime
                            .modules
                            .applied_struct_layout(
                                &loaded,
                                StructId::new(structure),
                                slice::from_ref(ty),
                                scope,
                            )
                            .unwrap();
                    }
                }
            }
        });
        let staged = runtime
            .stage_reload_program(&loaded, "memory", code.clone())
            .unwrap();
        drop(runtime.publish_staged_reload(staged).unwrap());
        drop((inputs, nominal, loaded));
        runtime.collect_garbage().unwrap();
        let retired = snapshot();
        Phases {
            setup,
            cold,
            warm,
            repeated,
            retired,
        }
    })
}

fn admission_lifecycle(code: &BytecodeProgram, count: usize, prepare: bool) -> (Phases, Counts) {
    allocations::measure(|| {
        let mut runtime = Runtime::default();
        let old = runtime.load_program("memory", code.clone()).unwrap();
        let structure = StructId::new(
            old.bytecode
                .structures
                .iter()
                .position(|layout| !layout.arguments.iter().all(Ty::is_concrete))
                .unwrap(),
        );
        let variant = EnumId::new(
            old.bytecode
                .enumerations
                .iter()
                .position(|layout| !layout.arguments.iter().all(Ty::is_concrete))
                .unwrap(),
        );
        let staged = runtime
            .stage_reload_program(&old, "memory", code.clone())
            .unwrap();
        let loaded = runtime.publish_staged_reload(staged).unwrap();
        // Both sides prepare the complete producer/consumer layouts as setup inputs.
        let inputs = (1..=count)
            .map(|width| {
                let arguments = [Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); width])];
                (
                    runtime
                        .modules
                        .applied_struct_layout(&old, structure, &arguments, None)
                        .unwrap(),
                    runtime
                        .modules
                        .applied_struct_layout(&loaded, structure, &arguments, None)
                        .unwrap(),
                    runtime
                        .modules
                        .applied_enum_variant(&old, variant, &arguments, 0, None)
                        .unwrap(),
                    runtime
                        .modules
                        .applied_enum_variant(&loaded, variant, &arguments, 0, None)
                        .unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let (setup, cold, warm, repeated) = preparation(&runtime, || {
            if prepare {
                for (producer, consumer, producer_enum, consumer_enum) in &inputs {
                    assert!(consumer.matches(producer));
                    assert!(consumer_enum.matches_layout(producer_enum));
                }
            }
        });
        let staged = runtime
            .stage_reload_program(&loaded, "memory", code.clone())
            .unwrap();
        drop(runtime.publish_staged_reload(staged).unwrap());
        drop((inputs, old, loaded));
        runtime.collect_garbage().unwrap();
        let retired = snapshot();
        Phases {
            setup,
            cold,
            warm,
            repeated,
            retired,
        }
    })
}

#[test]
#[ignore = "manual descriptor allocation accounting; run release with --nocapture"]
fn layout_descriptor_retention() {
    let code = compile(SOURCE, &provider());
    for count in [1, 4, 160] {
        compare(&format!("struct applications={count}"), |prepare| {
            layout_lifecycle(&code, count, false, prepare)
        });
        compare(&format!("enum applications={count}"), |prepare| {
            layout_lifecycle(&code, count, true, prepare)
        });
        compare(&format!("admission pairs={count}x2"), |prepare| {
            admission_lifecycle(&code, count, prepare)
        });
    }
}
