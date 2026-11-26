use crate::{
    debug::{MirFunctionDebugMetadata, MirLexicalScope, MirLocalDebugInfo},
    function::{BasicBlock, MirFunction, MirLocal, MirModule, MirParameter, MirTemp},
    ids::{BlockId, InstanceId, LocalId, TempId},
    instruction::{Constant, Instruction as I, MirValue, PathRef, Terminator},
    verify::{Context, MirVerificationErrorKind, verify_function},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_contract::{
    contracts::ContractError, effects::EffectSet, slots::SemanticSlots,
    types::ConcreteFunctionIdentity,
};
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

fn block(instructions: Vec<I>, terminator: Terminator) -> BasicBlock {
    BasicBlock {
        instruction_spans: vec![Default::default(); instructions.len()],
        instruction_scopes: vec![0; instructions.len()],
        instructions,
        terminator: Some(terminator),
        terminator_span: Some(Default::default()),
        terminator_scope: Some(0),
    }
}

#[test]
fn typed_host_local_cannot_hide_path_provenance_at_retention_points() {
    let definitions = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("retention");
    let root = definitions.intern_root(&identity).unwrap();
    let declaration = definitions
        .intern_child(root, DefinitionKind::Function, "wait", 0)
        .unwrap();
    let host = Ty::Host(
        definitions
            .intern_child(root, DefinitionKind::Struct, "Player", 0)
            .unwrap(),
    );
    let core = definitions
        .intern_root(&ModuleIdentity::single_file("core"))
        .unwrap();
    let unit = Ty::Builtin(BuiltinType::Unit);
    let future = Ty::NativeObject(NominalTy {
        declaration: definitions
            .intern_child(core, DefinitionKind::AssociatedType, "Future", 0)
            .unwrap(),
        arguments: vec![unit.clone()],
        associated_types: Default::default(),
    });
    let physical = [
        ValueType::HostHandle,
        ValueType::HeapObject,
        ValueType::HostHandle,
        ValueType::Unit,
        ValueType::HeapObject,
        ValueType::I32,
    ];
    let value = |index: usize| MirValue {
        temp: TempId::new(index),
        ty: physical[index],
    };
    let player = LocalId::new(0);
    let id = InstanceId::new(0);
    let blocks = vec![
        block(
            vec![
                I::LoadLocal {
                    dst: value(0),
                    local: player,
                },
                I::LoadLocal {
                    dst: value(1),
                    local: LocalId::new(1),
                },
                I::MakePathView {
                    dst: value(2),
                    root_or_view: value(0),
                    path: PathRef {
                        declaration: None,
                        contract_fingerprint: 0,
                        root_ty: physical[0],
                        result_ty: physical[0],
                        read_only: true,
                        debug_name: "related".into(),
                    },
                    dynamic_args: Default::default(),
                },
                I::MakeTuple {
                    dst: value(4),
                    elements: [value(0), value(2)].into_iter().collect(),
                },
                I::LoadConst {
                    dst: value(5),
                    constant: Constant::I32(0),
                },
                // Peel a wrapper tuple before selecting a sibling of the view.
                I::MakeTuple {
                    dst: value(4),
                    elements: [value(4)].into_iter().collect(),
                },
                I::ReadAggregateIndex {
                    dst: value(4),
                    base: value(4),
                    index: value(5),
                },
                I::LoadConst {
                    dst: value(5),
                    constant: Constant::I32(0),
                },
                // Overwrite the original view; only the selected element can escape.
                I::ReadAggregateIndex {
                    dst: value(2),
                    base: value(4),
                    index: value(5),
                },
                I::StoreLocal {
                    local: player,
                    src: value(2),
                },
            ],
            Terminator::Jump(BlockId::new(1)),
        ),
        block(
            vec![
                I::LoadLocal {
                    dst: value(0),
                    local: player,
                },
                I::MakeFuture {
                    dst: value(1),
                    function: id,
                    arguments: [value(0), value(1)].into_iter().collect(),
                    future: future.clone(),
                },
                I::StoreLocal {
                    local: player,
                    src: value(2),
                },
                I::Await {
                    dst: value(3),
                    value: value(1),
                    future: future.clone(),
                },
            ],
            Terminator::Return(None),
        ),
    ];
    let types = [host.clone(), future.clone()];
    let params: Vec<_> = (0..2)
        .map(|index| MirParameter {
            name: format!("arg{index}"),
            local: LocalId::new(index),
            ty: physical[index],
        })
        .collect();
    let function = MirFunction {
        semantic: SemanticSlots {
            params: types.clone().into_iter().enumerate().collect(),
            locals: types.into_iter().enumerate().collect(),
            registers: [host.clone(), future, host, unit.clone()]
                .into_iter()
                .enumerate()
                .collect(),
            result: Some(unit),
            ..Default::default()
        },
        id,
        instance: ConcreteFunctionIdentity {
            declaration,
            arguments: vec![],
        },
        name: "wait".into(),
        locals: params
            .iter()
            .map(|p| MirLocal {
                name: p.name.clone(),
                ty: p.ty,
            })
            .collect(),
        debug: MirFunctionDebugMetadata {
            lexical_scopes: vec![MirLexicalScope {
                parent: None,
                local: None,
            }],
            locals: params
                .iter()
                .map(|p| MirLocalDebugInfo {
                    local: p.local,
                    name: p.name.clone(),
                    ty: p.ty,
                    is_parameter: true,
                    span: Default::default(),
                })
                .collect(),
            ..Default::default()
        },
        params,
        temps: physical.map(|ty| MirTemp { ty }).to_vec(),
        effects: blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .fold(Default::default(), |effects: EffectSet, i| {
                effects.union(i.effects())
            }),
        blocks,
        entry: BlockId::new(0),
        return_type: ValueType::Unit,
    };
    let module = MirModule {
        identity,
        functions: vec![function.clone()],
        source_name: String::new(),
        abi: Default::default(),
        module_slots: vec![],
        dependencies: vec![],
        structures: vec![],
        enumerations: vec![],
        native_targets: vec![],
        external_types: vec![],
        interface_instances: vec![],
    };
    let context = Context {
        definitions: &definitions.snapshot(),
        function: Some(id),
        block: None,
        instruction: None,
        span: None,
        cancel: &Default::default(),
    };
    let verify = |f: &MirFunction| verify_function(&module, f, context, &mut Default::default());
    verify(&function).unwrap();
    for (at, reason) in [
        (1, "cannot capture a host path view"),
        (3, "host capability live across await"),
    ] {
        let mut invalid = function.clone();
        invalid.blocks[0].instructions[7] = I::LoadConst {
            dst: value(5),
            constant: Constant::I32(1),
        };
        if at == 3 {
            // Capture the original handle, then park the projected debug local.
            invalid.blocks[0].instructions[9] = I::StoreLocal {
                local: player,
                src: value(0),
            };
        }
        let error = verify(&invalid).unwrap_err();
        assert_eq!(error.block, Some(BlockId::new(1)));
        assert_eq!(error.instruction, Some(at));
        assert_eq!(
            error.kind,
            MirVerificationErrorKind::Contract(ContractError::InvalidOperation { reason })
        );
    }
}
