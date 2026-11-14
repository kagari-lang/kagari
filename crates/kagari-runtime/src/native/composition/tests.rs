//! Source-free nominal checks and optional origin forwarding at the policy boundary.
use super::*;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    error::RuntimeErrorKind,
    error_trace::ErrorTrace,
    module::ExecutableModule,
    native::types::VariantRef,
    value::{
        aggregates::enumerations::EnumObject,
        carriers::{RootedValue, ScopedValue},
    },
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::layout::{EnumLayout, EnumVariantLayout};
use kagari_types::{scalar::BuiltinType, ty::NominalTy};

#[derive(Debug)]
struct Unused;

impl PairPolicy for Unused {
    fn validate(&self, _: &PairContext<'_>) -> NativeResult<()> {
        unreachable!()
    }

    fn on_output(&self, _: &mut PairContext<'_>, _: usize) -> NativeResult<PairDecision> {
        unreachable!()
    }
}

fn fixture(runtime: &mut Runtime) -> (ExecutableModule, Vec<TypeArgument>, VariantRef, VariantRef) {
    let definitions = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("pair-origin.kgr");
    let root = definitions.intern_root(&identity).unwrap();
    let declared = definitions
        .intern_child(root, DefinitionKind::Enum, "Outcome", 0)
        .unwrap();
    let foreign = definitions
        .intern_child(root, DefinitionKind::Enum, "Other", 0)
        .unwrap();
    let error = definitions
        .intern_child(declared, DefinitionKind::Variant, "Err", 0)
        .unwrap();
    let other = definitions
        .intern_child(foreign, DefinitionKind::Variant, "Err", 0)
        .unwrap();
    let scalar = Ty::Builtin(BuiltinType::I32);
    let boolean = Ty::Builtin(BuiltinType::Bool);
    let pair = Ty::Tuple(vec![scalar.clone(), scalar.clone()]);
    let types = [
        (declared, scalar.clone(), scalar.clone()),
        (declared, pair.clone(), scalar.clone()),
        (declared, pair, boolean.clone()),
        (foreign, scalar.clone(), scalar),
    ];
    let layouts = types
        .iter()
        .map(|(declaration, success, payload)| EnumLayout {
            declaration: *declaration,
            arguments: vec![success.clone(), payload.clone()],
            variants: vec![EnumVariantLayout {
                declaration: if *declaration == declared {
                    error
                } else {
                    other
                },
                payload: vec![payload.clone()],
                reports_failure: true,
            }],
        })
        .collect();
    let owner_program = load_bytecode(
        runtime,
        "pair-origin",
        ProgramFixture {
            root: ModuleRef::new(0),
            exports: Default::default(),
            modules: vec![BytecodeModule {
                identity,
                enumerations: layouts,
                ..Default::default()
            }],
        }
        .metadata(definitions.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    let types = types
        .into_iter()
        .map(|(declaration, success, payload)| {
            Ty::Enum(NominalTy {
                declaration,
                arguments: vec![success, payload],
                associated_types: Default::default(),
            })
        })
        .collect::<Vec<_>>();
    let types = DefinitionMetadata::checked(definitions.snapshot(), types, &Default::default())
        .unwrap()
        .import_into(runtime.definition_context(), &Default::default())
        .unwrap()
        .into_records();
    let types = runtime.resolve_type_arguments(&owner, &types).unwrap();
    (
        owner,
        types,
        VariantRef {
            id: error,
            definitions: definitions.snapshot(),
        },
        VariantRef {
            id: other,
            definitions: definitions.snapshot(),
        },
    )
}

#[test]
fn composition_preserves_original_optional_origin_and_checks_payload_identity() {
    let mut runtime = Runtime::default();
    let (owner, types, error, foreign) = fixture(&mut runtime);
    let contract = ColdComposition {
        owner: owner.clone(),
        inputs: [types[0].clone(), types[0].clone()],
        output: types[1].clone(),
        policy: Arc::new(Unused),
    };
    let baseline = runtime.collected_gc_stats().root_registrations;
    for origin in [None, Some(ErrorTrace::capture(runtime.resources()))] {
        let layout = runtime
            .checked_enum_variant(&owner, &types[0], &error)
            .unwrap();
        let root = runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let object =
                    EnumObject::alloc(cx, layout, &[ScopedValue::from(7_i32)], origin.clone())?;
                ScopedValue::managed(object).retain(cx)
            })
            .unwrap();
        let outputs = [Some(root), None];
        let forwarded = {
            let mut cx = PairContext {
                runtime: &mut runtime,
                contract: &contract,
                outputs: &outputs,
            };
            let same_output = cx.output(0).unwrap().unwrap();
            assert_eq!(
                cx.runtime.collected_gc_stats().root_registrations,
                baseline + 1
            );
            drop(same_output);
            let forwarded = cx.forward_variant(0, &types[1], &error).unwrap();
            for (target, variant, reason) in [
                (&types[2], &error, "different error payload type"),
                (&types[3], &error, "different nominal carrier"),
                (&types[1], &foreign, "foreign same-spelled variant"),
            ] {
                assert_eq!(
                    cx.forward_variant(0, target, variant).unwrap_err().kind(),
                    RuntimeErrorKind::ModuleValidation,
                    "{reason}"
                );
            }
            assert_eq!(
                cx.allocate_enum(&types[1], &error, vec![RootedValue::from(true)])
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::ScriptTrap,
                "malformed target payload must reach semantic payload admission"
            );
            forwarded
        };
        runtime.collect_garbage().unwrap();
        runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let value = forwarded.get(cx)?;
                let object = cx.downcast::<EnumObject>(value.object().unwrap())?;
                let after = cx.read(object, |object| object.origin().cloned());
                match (&origin, after) {
                    (None, None) => {}
                    (Some(before), Some(after)) => assert!(Arc::ptr_eq(before, &after)),
                    other => panic!("origin changed: {other:?}"),
                }
                assert_eq!(cx.read(object, EnumObject::len), 1);
                assert_eq!(
                    EnumObject::get(cx, object, 0)?.scalar_payload(),
                    Some((ValueType::I32, 7))
                );
                Ok(())
            })
            .unwrap();
        drop(outputs);
        drop(forwarded);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.collected_gc_stats().root_registrations, baseline);
    }
}
