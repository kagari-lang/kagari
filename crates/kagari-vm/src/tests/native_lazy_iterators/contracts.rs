//! Source-free rejection of unbound traversal, callbacks and result publication.
use super::cases;
use crate::tests::common::compile_test_bytecode;
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::NativeWitnessImplementation,
    scalar::BuiltinType,
    standard::{bindings::NativeDefaultMethod, traits::StandardTrait},
    types::{AbiType, PublicAbiItem},
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    program::{BytecodeProgram, verify_program},
};

fn reject(program: &BytecodeProgram, label: &str) {
    assert!(verify_program(program).is_err(), "{label}");
    assert!(
        KbcArtifact::from_program(program.clone(), Default::default()).is_err(),
        "{label} KBC"
    );
}

#[test]
fn lazy_witness_order_preserves_every_budget_cut() {
    super::check_budget_cases_with_transform(
        |name| {
            matches!(
                name,
                "windows_array_2_2" | "chunks_array_2_2" | "flatten_native_false_false"
            )
        },
        |program| {
            for import in program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.native_imports)
            {
                import.witnesses.reverse();
            }
        },
    );
}

#[test]
fn native_list_source_and_readonly_result_require_distinct_applications() {
    let cases = cases::cases();
    for (name, operation) in [
        ("windows_array_2_2", NativeDefaultMethod::ListWindows),
        ("chunks_array_2_2", NativeDefaultMethod::ListChunks),
    ] {
        let source = &cases.iter().find(|(label, _)| label == name).unwrap().1;
        let program = compile_test_bytecode(source);
        let root = program.root.index();
        let index = program.modules[root]
            .native_imports
            .iter()
            .position(|import| import.binding == EngineNativeBinding::TraitDefault(operation))
            .unwrap();
        let contract = &program.modules[root].native_imports[index];
        let applications: Vec<_> = contract
            .witnesses
            .iter()
            .enumerate()
            .filter_map(|(index, witness)| {
                if StandardTrait::from_id(&witness.interface.declaration)
                    != Some(StandardTrait::List)
                {
                    return None;
                }
                let NativeWitnessImplementation::Table(target) = &witness.implementation else {
                    panic!("{name} native List table");
                };
                let table = program
                    .modules
                    .iter()
                    .flat_map(|module| &module.public_items)
                    .find_map(|item| match item {
                        PublicAbiItem::InterfaceTable(table)
                            if table.declaration == target.declaration =>
                        {
                            Some(table)
                        }
                        _ => None,
                    })
                    .unwrap();
                Some((index, table.native_bridge))
            })
            .collect();
        assert_eq!(applications.len(), 2, "{name} source and result");
        let source = applications.iter().find(|(_, bridge)| !bridge).unwrap().0;
        let result = applications.iter().find(|(_, bridge)| *bridge).unwrap().0;
        assert_eq!(
            contract.witnesses[source].receiver,
            contract.witnesses[result].receiver
        );
        assert_eq!(
            contract.witnesses[source].interface,
            contract.witnesses[result].interface
        );
        for removed in [source, result] {
            let mut forged = program.clone();
            forged.modules[root].native_imports[index]
                .witnesses
                .remove(removed);
            reject(&forged, &format!("{name} missing application {removed}"));
        }
        for (destination, replacement) in [(source, result), (result, source)] {
            let mut forged = program.clone();
            let witnesses = &mut forged.modules[root].native_imports[index].witnesses;
            witnesses[destination] = witnesses[replacement].clone();
            reject(
                &forged,
                &format!("{name} replaced application {destination}"),
            );
        }
    }
}
#[test]
fn lazy_constructor_contracts_reject_forged_types_witnesses_and_applications() {
    let cases = cases::cases();
    let mut checked = 0;
    for (name, operation) in [
        ("map_script_false", NativeDefaultMethod::Map),
        ("filter_script_false", NativeDefaultMethod::Filter),
        ("filter_map_script_false", NativeDefaultMethod::FilterMap),
        ("take_script_false", NativeDefaultMethod::Take),
        ("skip_script_false", NativeDefaultMethod::Skip),
        ("enumerate_script_false", NativeDefaultMethod::Enumerate),
        ("zip_script_false", NativeDefaultMethod::Zip),
        ("chain_script_false", NativeDefaultMethod::Chain),
        ("take_while_script_false", NativeDefaultMethod::TakeWhile),
        ("skip_while_script_false", NativeDefaultMethod::SkipWhile),
        ("inspect_script_false", NativeDefaultMethod::Inspect),
        ("fuse_script_false", NativeDefaultMethod::Fuse),
        ("flat_map_custom_script_false", NativeDefaultMethod::FlatMap),
        ("flatten_script_false_true", NativeDefaultMethod::Flatten),
        ("windows_script_2_2", NativeDefaultMethod::ListWindows),
        ("chunks_script_2_2", NativeDefaultMethod::ListChunks),
    ] {
        let source = &cases.iter().find(|(label, _)| label == name).unwrap().1;
        let program = compile_test_bytecode(source);
        let root = program.root.index();
        let index = program.modules[root]
            .native_imports
            .iter()
            .position(|import| import.binding == EngineNativeBinding::TraitDefault(operation))
            .unwrap();
        let contract = &program.modules[root].native_imports[index];
        let mut mutations = Vec::new();
        for mutation in 0..4 {
            let mut forged = program.clone();
            let import = &mut forged.modules[root].native_imports[index];
            match mutation {
                0 => {
                    import.signature.params.pop();
                }
                1 => {
                    import.signature.result =
                        AbiType::Iter(Box::new(AbiType::Builtin(BuiltinType::Bool)))
                }
                2 => import.binding_version += 1,
                _ => {
                    import.binding = EngineNativeBinding::TraitDefault(
                        if operation == NativeDefaultMethod::Map {
                            NativeDefaultMethod::Filter
                        } else {
                            NativeDefaultMethod::Map
                        },
                    )
                }
            }
            mutations.push(forged);
        }
        for witness in 0..contract.witnesses.len() {
            for mutation in 0..4 {
                let mut forged = program.clone();
                let import = &mut forged.modules[root].native_imports[index];
                match mutation {
                    0 => {
                        import.witnesses.remove(witness);
                    }
                    1 => import.witnesses[witness].receiver = AbiType::Builtin(BuiltinType::Bool),
                    2 => {
                        let copy = import.witnesses[witness].clone();
                        import.witnesses.push(copy);
                    }
                    _ => {
                        import.witnesses[witness]
                            .interface
                            .declaration
                            .path
                            .last_mut()
                            .unwrap()
                            .name = "Forged".into();
                    }
                }
                mutations.push(forged);
            }
            for method in &contract.witnesses[witness].methods {
                let mut forged = program.clone();
                let owner = forged
                    .modules
                    .iter_mut()
                    .find(|module| module.identity == method.declaration.module)
                    .unwrap();
                let target = owner
                    .functions
                    .iter_mut()
                    .find(|function| function.identity.as_ref() == Some(method))
                    .unwrap();
                target.metadata.semantic.result = Some(AbiType::Builtin(
                    if target.metadata.semantic.result == Some(AbiType::Builtin(BuiltinType::Bool))
                    {
                        BuiltinType::USize
                    } else {
                        BuiltinType::Bool
                    },
                ));
                mutations.push(forged);
            }
        }
        for (mutation, forged) in mutations.iter().enumerate() {
            reject(forged, &format!("{name}:{mutation}"));
            checked += 1;
        }
    }
    eprintln!("Checked {checked} native lazy mutations");
    assert!(checked >= 150, "{checked} checked mutations");
}
