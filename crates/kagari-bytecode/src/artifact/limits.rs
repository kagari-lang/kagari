use crate::instruction::ConstructionTarget;
use crate::{
    artifact::{
        ArtifactSignatures, DebugMetadata, KbcArtifact, MAX_ARTIFACT_BYTES, MAX_ARTIFACT_FUNCTIONS,
        MAX_ARTIFACT_INSTRUCTIONS, MAX_ARTIFACT_MODULES, MAX_ARTIFACT_NESTED_RECORDS,
        MAX_ARTIFACT_TABLE_RECORDS,
    },
    instruction::{BytecodeInstruction, CallTarget},
    module::BytecodeModule,
    program::BytecodeProgram,
};
use kagari_common::identity::table::DefinitionTable;
use kagari_contract::{
    callable::{
        construction::evidence::{
            ConstructionRequirement, ConstructionSelection, ConstructionWitness,
        },
        evidence::EvidenceRef,
        witness::OperationWitness,
    },
    types::PublicItem,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        AssociatedTypeDef, FnDecl, TypeDef, path::HostPathSegmentDeclaration,
        requirement::NativeCallableRequirement,
    },
    ty::{GenericBound, Ty},
};

pub(super) fn within_table_limit(lengths: impl IntoIterator<Item = usize>) -> bool {
    lengths
        .into_iter()
        .all(|length| length <= MAX_ARTIFACT_TABLE_RECORDS)
}

pub(super) fn module_nested_count_limit(module: &BytecodeModule, total: &mut usize) -> bool {
    let mut add = |length: usize| {
        *total = total.saturating_add(length);
        length <= MAX_ARTIFACT_NESTED_RECORDS && *total <= MAX_ARTIFACT_TABLE_RECORDS
    };
    if !add(module.trait_references.len()) {
        return false;
    }
    for import in &module.native_imports {
        if !add(import.instance.arguments.len())
            || !add(import.signature.params.len())
            || !add(import.requirements.len())
            || !add_abi_bounds(&import.requirements, &mut add)
            || !import
                .result_adapter
                .as_ref()
                .is_none_or(|adapter| add(adapter.implementation.arguments.len()))
            || !add_evidence(&import.callables, &[], &mut add)
        {
            return false;
        }
    }
    for declaration in &module.native_declarations {
        let function = &declaration.function;
        if !add(function.generic_params.len())
            || !add(function.bounds.len())
            || !add_abi_bounds(&function.bounds, &mut add)
            || !add(function.params.len())
            || !add_function_contract(function, &mut add)
            || !add(declaration.callable_requirements.len())
            || !declaration
                .callable_requirements
                .iter()
                .all(|required| add_callable_requirement(required, &mut add))
        {
            return false;
        }
    }
    for function in &module.functions {
        for instruction in &function.instructions {
            if let Some(arguments) = instruction.layout_arguments()
                && (!add(arguments.len()) || !arguments.iter().all(Ty::within_wire_limits))
            {
                return false;
            }
            if let BytecodeInstruction::MakeInterface {
                target: ConstructionTarget::Bound(required),
                ..
            } = instruction
                && !add_construction_requirement(required, &mut add)
            {
                return false;
            }
            if let BytecodeInstruction::Call {
                callee: CallTarget::Shared { contract, .. },
                ..
            } = instruction
                && (!add(contract.arguments.len())
                    || !add(contract.instance.arguments.len())
                    || !add(contract.signature.params.len())
                    || !add_evidence(&contract.operations, &contract.constructions, &mut add))
            {
                return false;
            }
            if let BytecodeInstruction::MakeInterface {
                target: ConstructionTarget::Declared { arguments, .. },
                ..
            } = instruction
                && (!add(arguments.len()) || !arguments.iter().all(Ty::within_wire_limits))
            {
                return false;
            }
            if let BytecodeInstruction::Call {
                callee: CallTarget::InterfaceMethod { contract, .. },
                ..
            } = instruction
                && (!add(contract.arguments.len())
                    || !add(contract.normalizations.len())
                    || !add_evidence(&contract.operations, &contract.constructions, &mut add))
            {
                return false;
            }
        }
        if let Some(body) = &function.metadata.semantic.generic
            && (!add(body.parameters.len()) || !add_abi_bounds(&body.bounds, &mut add))
        {
            return false;
        }

        if function
            .metadata
            .semantic
            .protocol_adapter
            .as_ref()
            .is_some_and(|required| !add_callable_requirement(required, &mut add))
        {
            return false;
        }
        if function
            .identity
            .as_ref()
            .is_some_and(|identity| !add(identity.arguments.len()))
        {
            return false;
        }
    }
    for record in &module.function_table {
        if record
            .identity
            .as_ref()
            .is_some_and(|identity| !add(identity.arguments.len()))
        {
            return false;
        }
    }
    for table in &module.interface_tables {
        if let Some(view) = &table.view
            && (!add(view.results.len())
                || !add(view.interface.arguments.len())
                || !add(view.interface.associated_types.len())
                || view
                    .results
                    .iter()
                    .any(|adapter| !add(adapter.implementation.arguments.len())))
        {
            return false;
        }
        if !add(table.arguments.len())
            || !add(table.methods.len())
            || !add(table.parents.len())
            || table.parents.iter().any(|parent| {
                !add(parent.interface.arguments.len())
                    || !add(parent.interface.associated_types.len())
                    || !add(parent.implementation.arguments.len())
            })
            || table.methods.iter().any(|slot| !add(slot.arguments.len()))
        {
            return false;
        }
    }
    for layout in &module.structures {
        if !add(layout.arguments.len()) || !add(layout.fields.len()) {
            return false;
        }
    }
    for layout in &module.enumerations {
        if !add(layout.arguments.len()) || !add(layout.variants.len()) {
            return false;
        }
        for variant in &layout.variants {
            if !add(variant.payload.len()) {
                return false;
            }
        }
    }

    for path in module.path_declarations.iter() {
        if !add(path.segments.len()) {
            return false;
        }
    }
    for declaration in &module.private_types {
        if !add_type_definition(&declaration.definition, &mut add) {
            return false;
        }
    }
    for item in &module.public_items {
        let valid = match item {
            PublicItem::Function(item) => {
                add(item.generic_params.len())
                    && add(item.bounds.len())
                    && add_abi_bounds(&item.bounds, &mut add)
                    && add(item.params.len())
                    && add_function_contract(item, &mut add)
            }
            PublicItem::Const(_) | PublicItem::Static(_) => true,
            PublicItem::Type(item) => add_type_definition(item, &mut add),
            PublicItem::Trait(item) => {
                if !add(item.associated_consts.len()) {
                    return false;
                }
                add(item.generic_params.len())
                    && add(item.bounds.len())
                    && add_abi_bounds(&item.bounds, &mut add)
                    && add(item.associated_types.len())
                    && item.associated_types.iter().all(|member| {
                        add(member.bounds.len())
                            && add(member.parameter_bounds.len())
                            && add(member.generic_params.len())
                            && add_abi_bounds(&member.parameter_bounds, &mut add)
                    })
                    && add(item.methods.len())
                    && item.methods.iter().all(|method| {
                        add(method.generic_params.len())
                            && add(method.bounds.len())
                            && add_abi_bounds(&method.bounds, &mut add)
                            && add(method.params.len())
                            && add_function_contract(method, &mut add)
                    })
            }
            PublicItem::InherentTable(item) => {
                add(item.generic_params.len())
                    && add(item.bounds.len())
                    && add_abi_bounds(&item.bounds, &mut add)
                    && add(item.methods.len())
                    && item.methods.iter().all(|method| {
                        add(method.generic_params.len())
                            && add(method.bounds.len())
                            && add_abi_bounds(&method.bounds, &mut add)
                            && add(method.params.len())
                            && add_function_contract(method, &mut add)
                    })
            }
            PublicItem::InterfaceTable(item) => {
                if !add(item.associated_type_families.len())
                    || !item.associated_type_families.iter().all(|family| {
                        add(family.generic_params.len())
                            && add(family.bounds.len())
                            && add_abi_bounds(&family.bounds, &mut add)
                    })
                    || !add(item.associated_consts.len())
                {
                    return false;
                }
                add(item.generic_params.len())
                    && add(item.bounds.len())
                    && add_abi_bounds(&item.bounds, &mut add)
                    && add(item.methods.len())
                    && item.methods.iter().all(|method| {
                        add(method.generic_params.len())
                            && add(method.bounds.len())
                            && add_abi_bounds(&method.bounds, &mut add)
                            && add(method.params.len())
                            && add_function_contract(method, &mut add)
                    })
            }
        };
        if !valid {
            return false;
        }
    }
    for contract in module
        .trait_contracts
        .iter()
        .chain(&module.trait_references)
    {
        let item = &contract.abi;
        if !add(item.associated_consts.len())
            || !add(item.supertraits.len())
            || !item
                .supertraits
                .iter()
                .all(|parent| add(parent.arguments.len()))
            || !add(item.generic_params.len())
            || !add(item.bounds.len())
            || !add_abi_bounds(&item.bounds, &mut add)
            || !add(item.associated_types.len())
            || !item.associated_types.iter().all(|member| {
                add(member.bounds.len())
                    && add(member.generic_params.len())
                    && add_abi_bounds(&member.parameter_bounds, &mut add)
            })
            || !add(item.methods.len())
            || !item.methods.iter().all(|method| {
                add(method.generic_params.len())
                    && add(method.bounds.len())
                    && add_abi_bounds(&method.bounds, &mut add)
                    && add(method.params.len())
                    && add_function_contract(method, &mut add)
            })
        {
            return false;
        }
    }
    true
}

fn add_callable_requirement(
    required: &NativeCallableRequirement,
    add: &mut impl FnMut(usize) -> bool,
) -> bool {
    add(required.arguments.len())
        && add(required.interface.arguments.len())
        && add(required.interface.associated_types.len())
}

fn add_function_contract(function: &FnDecl, add: &mut impl FnMut(usize) -> bool) -> bool {
    match &function.implementation {
        CallableImplementation::NativeDefault(application) => add(application.arguments.len()),
        _ => true,
    }
}

pub(super) fn add_abi_bounds(bounds: &[GenericBound], add: &mut impl FnMut(usize) -> bool) -> bool {
    bounds.iter().all(|bound| {
        add(bound.constraints.len())
            && bound
                .constraints
                .iter()
                .all(|ty| add(ty.arguments.len()) && add(ty.associated_types.len()))
    })
}

pub(super) fn bounds_type_limit(bounds: &[GenericBound]) -> bool {
    bounds.iter().all(|bound| {
        bound.ty.within_wire_limits()
            && bound.constraints.iter().all(|ty| {
                ty.arguments.iter().all(Ty::within_wire_limits)
                    && ty.associated_types.values().all(Ty::within_wire_limits)
            })
    })
}

pub(super) fn associated_type_limit(members: &[AssociatedTypeDef]) -> bool {
    members.iter().all(|member| {
        bounds_type_limit(&member.parameter_bounds)
            && member
                .bounds
                .iter()
                .all(|ty| Ty::Trait(ty.clone()).within_wire_limits())
    })
}

pub(super) fn function_abi_type_limit(function: &FnDecl) -> bool {
    bounds_type_limit(&function.bounds)
}

fn add_type_definition(ty: &TypeDef, add: &mut impl FnMut(usize) -> bool) -> bool {
    add(ty.generic_params.len())
        && add(ty.bounds.len())
        && add_abi_bounds(&ty.bounds, add)
        && add(ty.fields.len())
        && add(ty.variants.len())
        && ty.variants.iter().all(|variant| add(variant.payload.len()))
}

fn type_definition_limit(ty: &TypeDef) -> bool {
    bounds_type_limit(&ty.bounds)
        && ty.fields.iter().all(|field| field.ty.within_wire_limits())
        && ty
            .variants
            .iter()
            .all(|variant| variant.payload.iter().all(Ty::within_wire_limits))
}

pub(super) fn module_abi_type_limit(
    module: &BytecodeModule,
    definitions: &DefinitionTable,
) -> bool {
    let valid = |ty: &Ty| ty.within_wire_limits();
    if !module.path_declarations.iter().all(|path| {
        path.segments.len() <= 256
            && path.segments.iter().all(|segment| match segment {
                HostPathSegmentDeclaration::Field(_) => true,
                HostPathSegmentDeclaration::Index(index) => {
                    valid(&index.collection) && valid(&index.index) && valid(&index.result)
                }
                HostPathSegmentDeclaration::Virtual(step) => valid(&step.result),
            })
    }) {
        return false;
    }
    module
        .native_imports
        .iter()
        .all(|import| import.structurally_valid(definitions))
        && module.native_declarations.iter().all(|declaration| {
            let function = &declaration.function;
            function_abi_type_limit(function)
                && function.params.iter().all(|param| valid(&param.ty))
                && valid(&function.return_type)
                && declaration.concrete_result.as_ref().is_none_or(valid)
                && declaration.callable_requirements.iter().all(|required| {
                    valid(&required.receiver)
                        && valid(&Ty::Trait(required.interface.clone()))
                        && required.arguments.iter().all(&valid)
                })
        })
        && module.interface_tables.iter().all(|table| {
            table.view.as_ref().is_none_or(|view| {
                valid(&Ty::Trait(view.interface.clone()))
                    && view
                        .results
                        .iter()
                        .all(|adapter| adapter.implementation.arguments.iter().all(&valid))
            }) && table.arguments.iter().all(&valid)
                && table.parents.iter().all(|parent| {
                    valid(&Ty::Trait(parent.interface.clone()))
                        && parent.implementation.arguments.iter().all(&valid)
                })
                && table
                    .methods
                    .iter()
                    .all(|slot| slot.arguments.iter().all(&valid))
        })
        && module.functions.iter().all(|function| {
            function
                .identity
                .as_ref()
                .is_none_or(|identity| identity.arguments.iter().all(&valid))
        })
        && module.function_table.iter().all(|record| {
            record
                .identity
                .as_ref()
                .is_none_or(|identity| identity.arguments.iter().all(&valid))
        })
        && module.structures.iter().all(|layout| {
            layout.arguments.iter().all(&valid)
                && layout.fields.iter().all(|field| valid(&field.ty))
        })
        && module.enumerations.iter().all(|layout| {
            layout.arguments.iter().all(&valid)
                && layout
                    .variants
                    .iter()
                    .all(|variant| variant.payload.iter().all(&valid))
        })
        && module
            .trait_contracts
            .iter()
            .chain(&module.trait_references)
            .all(|contract| {
                bounds_type_limit(&contract.abi.bounds)
                    && contract
                        .abi
                        .associated_consts
                        .iter()
                        .all(|member| valid(&member.ty))
                    && contract
                        .abi
                        .supertraits
                        .iter()
                        .all(|parent| parent.arguments.iter().all(&valid))
                    && associated_type_limit(&contract.abi.associated_types)
                    && contract.abi.methods.iter().all(|method| {
                        function_abi_type_limit(method)
                            && method.params.iter().all(|param| valid(&param.ty))
                            && valid(&method.return_type)
                    })
            })
        && module
            .module_slots
            .iter()
            .all(|slot| valid(&slot.contract.ty))
        && module
            .private_types
            .iter()
            .all(|declaration| type_definition_limit(&declaration.definition))
        && module.public_items.iter().all(|item| match item {
            PublicItem::Function(item) => {
                function_abi_type_limit(item)
                    && item.params.iter().all(|param| valid(&param.ty))
                    && valid(&item.return_type)
            }
            PublicItem::Const(item) => valid(&item.ty),
            PublicItem::Static(item) => valid(&item.ty),
            PublicItem::Type(item) => type_definition_limit(item),
            PublicItem::Trait(item) => {
                associated_type_limit(&item.associated_types)
                    && bounds_type_limit(&item.bounds)
                    && item.methods.iter().all(|method| {
                        function_abi_type_limit(method)
                            && method.params.iter().all(|param| valid(&param.ty))
                            && valid(&method.return_type)
                    })
            }
            PublicItem::InherentTable(item) => {
                bounds_type_limit(&item.bounds)
                    && valid(&item.for_type)
                    && item.methods.iter().all(|method| {
                        function_abi_type_limit(method)
                            && method.params.iter().all(|param| valid(&param.ty))
                            && valid(&method.return_type)
                    })
            }
            PublicItem::InterfaceTable(item) => {
                bounds_type_limit(&item.bounds)
                    && valid(&item.trait_type)
                    && valid(&item.for_type)
                    && item
                        .associated_type_families
                        .iter()
                        .all(|family| bounds_type_limit(&family.bounds) && valid(&family.value))
                    && item.methods.iter().all(|method| {
                        function_abi_type_limit(method)
                            && method.params.iter().all(|param| valid(&param.ty))
                            && valid(&method.return_type)
                    })
            }
        })
}

pub(super) fn program_count_limit(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
) -> Option<&'static str> {
    if program.modules.len() > MAX_ARTIFACT_MODULES {
        return Some("too many modules");
    }
    let mut functions = 0usize;
    let mut instructions = 0usize;
    let mut operand_records = 0usize;
    let mut module_records = 0usize;
    let mut nested_records = 0usize;
    for module in &program.modules {
        if !module.identity.within_path_limit() {
            return Some("identity path segment limit exceeded");
        }
        if !module_nested_count_limit(module, &mut nested_records) {
            return Some("nested module record limit exceeded");
        }
        if !module_abi_type_limit(module, definitions) {
            return Some("ABI type resource limit exceeded");
        }
        functions = functions.saturating_add(module.functions.len());
        if functions > MAX_ARTIFACT_FUNCTIONS {
            return Some("too many functions");
        }
        module_records = module_records.saturating_add(
            [
                module.dependencies.len(),
                module.external_types.len(),
                module.native_imports.len(),
                module.native_declarations.len(),
                module.path_declarations.len(),
                module.module_slots.len(),
                module.constants.len(),
                module.types.len(),
                module.structures.len(),
                module.enumerations.len(),
                module.interface_tables.len(),
                module.paths.len(),
                module.function_table.len(),
                module.public_items.len(),
                module.private_types.len(),
                module.trait_contracts.len(),
                module.trait_references.len(),
            ]
            .into_iter()
            .fold(0usize, usize::saturating_add),
        );
        if module_records > MAX_ARTIFACT_TABLE_RECORDS {
            return Some("module table record limit exceeded");
        }
        if module
            .function_table
            .iter()
            .any(|record| record.params.len() > MAX_ARTIFACT_TABLE_RECORDS)
        {
            return Some("function table parameter record limit exceeded");
        }
        for function in &module.functions {
            instructions = instructions.saturating_add(function.instructions.len());
            if instructions > MAX_ARTIFACT_INSTRUCTIONS {
                return Some("too many instructions");
            }
            for instruction in &function.instructions {
                let count = instruction.operand_vector_len();
                if count > MAX_ARTIFACT_NESTED_RECORDS {
                    return Some("instruction operand record limit exceeded");
                }
                operand_records = operand_records.saturating_add(count);
                if operand_records > MAX_ARTIFACT_TABLE_RECORDS {
                    return Some("instruction operand aggregate limit exceeded");
                }
            }
            let metadata = &function.metadata;
            let debug = &metadata.debug;
            if !within_table_limit([
                metadata.semantic.params.len(),
                metadata.semantic.slots.len(),
                metadata.params.len(),
                metadata.parameter_slots.len(),
                metadata.origins.len(),
                metadata.instruction_origins.len(),
                metadata.slots.len(),
                metadata.roots.slots.len(),
                metadata.control_flow_targets.len(),
                debug.source_spans.len(),
                debug.line_table.len(),
                debug.safe_debug_points.len(),
                debug.local_live_ranges.len(),
                debug.captured_bindings.len(),
                debug.frame_layout.params.len(),
                debug.frame_layout.slots.len(),
            ]) {
                return Some("function metadata record limit exceeded");
            }
        }
    }
    None
}

pub(super) fn metadata_count_limit(
    debug: Option<&DebugMetadata>,
    signatures: Option<&ArtifactSignatures>,
) -> Option<&'static str> {
    if let Some(debug) = debug
        && (!within_table_limit([
            debug.source_files.len(),
            debug.debug_names.len(),
            debug.functions.len(),
        ]) || debug.functions.iter().any(|function| {
            !within_table_limit([
                function.source_spans.len(),
                function.line_table.len(),
                function.safe_debug_points.len(),
                function.local_live_ranges.len(),
                function.captured_bindings.len(),
                function.frame_layout.params.len(),
                function.frame_layout.slots.len(),
            ])
        }))
    {
        return Some("debug record limit exceeded");
    }
    if signatures.is_some_and(|signatures| signatures.signatures.len() > MAX_ARTIFACT_TABLE_RECORDS)
    {
        return Some("signature record limit exceeded");
    }
    None
}

pub(super) fn artifact_count_limit(
    artifact: &KbcArtifact,
    definitions: &DefinitionTable,
) -> Option<&'static str> {
    if artifact
        .portable_mir
        .as_ref()
        .is_some_and(|payload| payload.bytes.len() as u64 > MAX_ARTIFACT_BYTES)
    {
        return Some("portable MIR byte limit exceeded");
    }
    if !artifact.header.module_identity.within_path_limit()
        || !artifact
            .verification
            .loader
            .module_identity
            .within_path_limit()
        || artifact
            .verification
            .dependency_fingerprints
            .iter()
            .chain(&artifact.verification.loader.dependency_fingerprints)
            .any(|dependency| !dependency.module_id.within_path_limit())
    {
        return Some("identity path segment limit exceeded");
    }
    if let Some(reason) = program_count_limit(&artifact.program, definitions) {
        return Some(reason);
    }
    if let Some(reason) =
        metadata_count_limit(artifact.debug.as_ref(), artifact.signatures.as_ref())
    {
        return Some(reason);
    }
    let tables = &artifact.tables;
    let verification = &artifact.verification;
    if !within_table_limit([
        tables.sections.len(),
        tables.source_files.len(),
        tables.debug_names.len(),
        verification.function_layouts.len(),
        verification.function_effects.len(),
        verification.control_flow_targets.len(),
        verification.typed_path_fingerprints.len(),
        verification.public_abi_fingerprints.len(),
        verification.dependency_fingerprints.len(),
        verification.loader.dependency_fingerprints.len(),
        verification.loader.typed_path_fingerprints.len(),
        verification.loader.public_abi_fingerprints.len(),
    ]) || tables
        .sections
        .iter()
        .any(|section| section.record_count > MAX_ARTIFACT_TABLE_RECORDS)
        || verification
            .control_flow_targets
            .iter()
            .any(|item| item.targets.len() > MAX_ARTIFACT_TABLE_RECORDS)
        || verification.function_layouts.iter().any(|item| {
            !within_table_limit([item.params.len(), item.slots.len(), item.roots.slots.len()])
        })
    {
        return Some("artifact metadata record limit exceeded");
    }
    None
}

fn add_construction_requirement(
    required: &ConstructionRequirement,
    add: &mut impl FnMut(usize) -> bool,
) -> bool {
    add(required.interface.arguments.len())
        && add(required.interface.associated_types.len())
        && required.receiver.within_wire_limits()
        && required
            .interface
            .arguments
            .iter()
            .chain(required.interface.associated_types.values())
            .all(Ty::within_wire_limits)
}

fn add_evidence(
    operations: &[OperationWitness],
    constructions: &[ConstructionWitness],
    add: &mut impl FnMut(usize) -> bool,
) -> bool {
    if !add(operations.len()) || !add(constructions.len()) {
        return false;
    }
    let mut pending: Vec<_> = operations
        .iter()
        .map(EvidenceRef::Operation)
        .chain(constructions.iter().map(EvidenceRef::Construction))
        .collect();
    while let Some(node) = pending.pop() {
        let valid = match node {
            EvidenceRef::Operation(operation) => {
                add_callable_requirement(operation.requirement(), add)
                    && match operation {
                        OperationWitness::Selected(selected) => {
                            add(selected.instance.arguments.len())
                                && add(selected.signature.params.len())
                        }
                        OperationWitness::SharedAdapter(adapter) => {
                            add(adapter.call.arguments.len())
                                && add(adapter.call.signature.params.len())
                        }
                        OperationWitness::Forward(_) => true,
                        OperationWitness::SharedMethod(selected) => {
                            add(selected.implementation.arguments.len())
                        }
                    }
            }
            EvidenceRef::Construction(witness) => {
                add_construction_requirement(&witness.requirement, add)
                    && match &witness.selection {
                        ConstructionSelection::Declared { implementation, .. } => {
                            add(implementation.arguments.len())
                        }
                        ConstructionSelection::Interface | ConstructionSelection::Forward => true,
                    }
            }
        };
        let (operations, constructions) = node.children();
        if !valid || !add(operations.len()) || !add(constructions.len()) {
            return false;
        }
        pending.extend(operations.iter().map(EvidenceRef::Operation));
        pending.extend(constructions.iter().map(EvidenceRef::Construction));
    }
    true
}
