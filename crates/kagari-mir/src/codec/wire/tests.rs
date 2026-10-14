use super::{ProgramRecord, function::MirModuleRecord};
use crate::{
    codec::{Header, MirCodecError, decode_program, options},
    debug::{MirFunctionDebugMetadata, MirLexicalScope},
    function::{BasicBlock, MirFunction, MirModule, MirTemp},
    ids::{BlockId, InstanceId, TempId},
    instruction::{Constant, Instruction, MirValue, Terminator},
    program::{ProgramErrorKind, records::MirProgram, verify_program},
    verify::MirVerificationErrorKind,
};
use bincode::Options;
use kagari_abi::representation::ValueType;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, scoped::DefinitionReferences,
};
use kagari_contract::{exports::wire::ProgramExportsRecord, types::ConcreteFunctionIdentity};
use std::collections::HashSet;

fn module(context: &DefinitionContext, name: &str) -> MirModule {
    let identity = ModuleIdentity::single_file(name);
    let root = context.intern_root(&identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Function, "main", 0)
        .unwrap();
    MirModule {
        native_targets: vec![],
        interface_instances: vec![],
        external_types: vec![],
        host_types: vec![],
        dependencies: vec![],
        structures: vec![],
        enumerations: vec![],
        identity,
        source_name: String::new(),
        module_slots: vec![],
        abi: Default::default(),
        functions: vec![MirFunction {
            semantic: Default::default(),
            id: InstanceId::new(0),
            instance: ConcreteFunctionIdentity {
                declaration,
                arguments: vec![],
            },
            name: "main".into(),
            params: vec![],
            return_type: ValueType::Unit,
            locals: vec![],
            temps: vec![],
            blocks: vec![BasicBlock {
                instructions: vec![],
                instruction_spans: vec![],
                instruction_scopes: vec![],
                terminator: Some(Terminator::Return(None)),
                terminator_span: Some(Default::default()),
                terminator_scope: Some(0),
            }],
            entry: BlockId::new(0),
            effects: Default::default(),
            debug: MirFunctionDebugMetadata {
                lexical_scopes: vec![MirLexicalScope {
                    parent: None,
                    local: None,
                }],
                ..Default::default()
            },
        }],
    }
}

// Test-only access to the real file records permits malformed semantic facts
// without exposing an unverified encoder through the production codec API.
fn forged(context: &DefinitionContext, modules: &[MirModule]) -> Vec<u8> {
    let cancel = Default::default();
    let mut references = HashSet::new();
    modules
        .to_vec()
        .visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            &cancel,
        )
        .unwrap();
    let encoding = context.snapshot().encode(references).unwrap();
    let record = ProgramRecord {
        root: modules[0].identity.clone(),
        modules: modules
            .iter()
            .map(|module| MirModuleRecord::encode(module, &encoding, &cancel).unwrap())
            .collect(),
        exports: ProgramExportsRecord::encode(&Default::default(), &encoding, &cancel).unwrap(),
        definitions: encoding.table,
    };
    options().serialize(&(Header::current(), record)).unwrap()
}

#[test]
fn codec_rejects_forged_control_flow_instead_of_accepting_serialized_proof() {
    let context = DefinitionContext::new().unwrap();
    let mut module = module(&context, "root");
    verify_program(
        MirProgram {
            root: module.identity.clone(),
            modules: vec![module.clone()],
            exports: Default::default(),
        },
        &context,
        &Default::default(),
    )
    .unwrap();
    module.functions[0].blocks[0].terminator = Some(Terminator::Jump(BlockId::new(999)));
    assert!(matches!(
        decode_program(&forged(&context, &[module]), &Default::default()),
        Err(MirCodecError::Verification(error)) if matches!(error.kind,
            ProgramErrorKind::Verification(ref error)
                if error.kind == MirVerificationErrorKind::InvalidBlock(BlockId::new(999)))
    ));
}

#[test]
fn whole_program_analysis_budget_cannot_be_reset_by_splitting_modules() {
    let context = DefinitionContext::new().unwrap();
    let mut modules = (0..24)
        .map(|index| {
            let mut module = module(&context, &format!("member{index}"));
            let function = &mut module.functions[0];
            function.temps = vec![
                MirTemp {
                    ty: ValueType::Unit
                };
                32_768
            ];
            let block = &mut function.blocks[0];
            block.instructions = vec![
                Instruction::LoadConst {
                    dst: MirValue {
                        temp: TempId::new(0),
                        ty: ValueType::Unit
                    },
                    constant: Constant::Unit,
                };
                256
            ];
            block.instruction_spans = vec![Default::default(); 256];
            block.instruction_scopes = vec![0; 256];
            module
        })
        .collect::<Vec<_>>();
    verify_program(
        MirProgram {
            root: modules[0].identity.clone(),
            modules: vec![modules[0].clone()],
            exports: Default::default(),
        },
        &context,
        &Default::default(),
    )
    .unwrap();
    modules[0].dependencies = modules
        .iter()
        .skip(1)
        .map(|module| module.identity.clone())
        .collect();
    let error = decode_program(&forged(&context, &modules), &Default::default()).unwrap_err();
    assert!(
        matches!(error, MirCodecError::Verification(error) if matches!(error.kind,
            ProgramErrorKind::Verification(ref error) if matches!(error.kind,
                MirVerificationErrorKind::Limit { resource: "MIR analysis state bytes", .. }
            )
        ))
    );
}
