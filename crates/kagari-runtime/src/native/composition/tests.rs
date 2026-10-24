//! Source-free nominal checks and optional origin forwarding at the policy boundary.
use super::*;
use crate::test_support::native::program::ProgramFixture;
use crate::{error_trace::ErrorTrace, module::LoadedModule};
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

    fn on_output(&self, _: &PairContext<'_>, _: usize) -> NativeResult<PairDecision> {
        unreachable!()
    }
}

fn fixture(runtime: &mut Runtime) -> (LoadedModule, Vec<TypeArgument>, VariantRef, VariantRef) {
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
    let owner = runtime
        .load_program(
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
        values: [Value::Unit; 2],
        policy: Arc::new(Unused),
    };
    for origin in [None, Some(ErrorTrace::capture(runtime.resources()))] {
        let layout = runtime
            .checked_enum_variant(&owner, &types[0], &error)
            .unwrap();
        let original = runtime
            .gc()
            .alloc_enum_origin(EnumTag::Declared(layout), vec![Value::I32(7)], || {
                origin.clone()
            })
            .unwrap();
        let root = runtime.root_value(Value::Enum(original)).unwrap();
        let outputs = [Some(root), None];
        let cx = PairContext {
            runtime: &runtime,
            contract: &contract,
            outputs: &outputs,
        };
        let forwarded = cx.forward_variant(0, &types[1], &error).unwrap();
        let value = cx.value(&forwarded).unwrap();
        match (&origin, runtime.gc().result_error_trace(&value)) {
            (None, None) => {}
            (Some(before), Some(after)) => assert!(Arc::ptr_eq(before, &after)),
            other => panic!("origin changed: {other:?}"),
        }
        assert!(
            cx.forward_variant(0, &types[2], &error).is_err(),
            "different error payload type"
        );
        assert!(
            cx.forward_variant(0, &types[3], &error).is_err(),
            "different nominal carrier"
        );
        assert!(
            cx.forward_variant(0, &types[1], &foreign).is_err(),
            "foreign same-spelled variant"
        );
        assert!(
            cx.allocate_enum(&types[1], &error, vec![Value::Bool(true)])
                .is_err(),
            "malformed target payload"
        );
        runtime.collect_garbage().unwrap();
        assert_eq!(
            runtime
                .gc()
                .enum_view(match value {
                    Value::Enum(id) => id,
                    _ => unreachable!(),
                })
                .unwrap()
                .fields,
            vec![Value::I32(7)]
        );
    }
}
