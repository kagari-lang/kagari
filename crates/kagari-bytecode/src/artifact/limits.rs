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
use kagari_contract::{callable::witness::OperationWitness, types::PublicItem};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{AssociatedTypeDef, FnDecl, requirement::NativeCallableRequirement},
    ty::{Constraint, GenericBound, Ty},
};
use std::slice;

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
    for import in &module.native_imports {
        if !add(import.instance.arguments.len())
            || !add(import.signature.params.len())
            || !import
                .host
                .as_ref()
                .is_none_or(|host| add(host.params.len()))
            || !add(import.requirements.len())
            || !add_abi_bounds(&import.requirements, &mut add)
            || !import
                .result_adapter
                .as_ref()
                .is_none_or(|adapter| add(adapter.implementation.arguments.len()))
            || !add_operations(&import.callables, &mut add)
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
            if let BytecodeInstruction::Call {
                callee: CallTarget::Shared { contract, .. },
                ..
            } = instruction
                && (!add(contract.arguments.len())
                    || !add(contract.instance.arguments.len())
                    || !add(contract.signature.params.len())
                    || !add_operations(&contract.operations, &mut add))
            {
                return false;
            }
            if let BytecodeInstruction::MakeInterface { arguments, .. } = instruction
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
                    || !add_operations(&contract.operations, &mut add))
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
        if let Some(comparison) = table
            .comparison
            .as_ref()
            .and_then(|comparison| comparison.selection())
            && (!add_operations(slice::from_ref(&comparison.operation), &mut add)
                || !add(comparison.conditions.len())
                || !add_abi_bounds(&comparison.conditions, &mut add)
                || !add(comparison.eligibility.len())
                || !comparison
                    .eligibility
                    .iter()
                    .all(|fact| add_abi_bounds(slice::from_ref(&fact.bound), &mut add)))
        {
            return false;
        }
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
    for ty in &module.host_interface.types {
        if !add(ty.fields.len()) || !add(ty.methods.len()) {
            return false;
        }
        for method in &ty.methods {
            if !add(method.params.len()) {
                return false;
            }
        }
    }
    for function in &module.host_interface.functions {
        if !add(function.params.len()) {
            return false;
        }
    }
    for path in &module.host_interface.paths {
        if !add(path.segments.len()) {
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
            PublicItem::Const(_) => true,
            PublicItem::Type(item) => {
                add(item.generic_params.len())
                    && add(item.bounds.len())
                    && add_abi_bounds(&item.bounds, &mut add)
                    && add(item.fields.len())
                    && add(item.variants.len())
                    && item
                        .variants
                        .iter()
                        .all(|variant| add(variant.payload.len()))
            }
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
    for contract in &module.trait_contracts {
        let item = &contract.abi;
        if !add(item.associated_consts.len())
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
            && bound.constraints.iter().all(|constraint| match constraint {
                Constraint::Trait(ty) => add(ty.arguments.len()),
                _ => true,
            })
    })
}

pub(super) fn bounds_type_limit(bounds: &[GenericBound]) -> bool {
    bounds.iter().all(|bound| {
        bound.ty.within_wire_limits()
            && bound.constraints.iter().all(|constraint| match constraint {
                Constraint::Standard(_) => true,
                Constraint::Trait(ty) => {
                    ty.arguments.iter().all(|arg| arg.within_wire_limits())
                        && ty
                            .associated_types
                            .values()
                            .all(|value| value.within_wire_limits())
                }
            })
    })
}

pub(super) fn associated_type_limit(members: &[AssociatedTypeDef]) -> bool {
    members.iter().all(|member| {
        bounds_type_limit(&member.parameter_bounds)
            && member.bounds.iter().all(|bound| match bound {
                Constraint::Standard(_) => true,
                Constraint::Trait(ty) => Ty::Trait(ty.clone()).within_wire_limits(),
            })
    })
}

pub(super) fn function_abi_type_limit(function: &FnDecl) -> bool {
    bounds_type_limit(&function.bounds)
}

pub(super) fn module_abi_type_limit(
    module: &BytecodeModule,
    definitions: &DefinitionTable,
) -> bool {
    let valid = |ty: &Ty| ty.within_wire_limits();
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
            table.comparison.as_ref().is_none_or(|comparison| {
                if !valid(comparison.receiver()) {
                    return false;
                }
                let Some(comparison) = comparison.selection() else {
                    return true;
                };
                bounds_type_limit(&comparison.conditions)
                    && comparison
                        .eligibility
                        .iter()
                        .all(|fact| bounds_type_limit(slice::from_ref(&fact.bound)))
                    && module.public_items.iter().any(|item| match item {
                        PublicItem::InterfaceTable(contract)
                            if contract.declaration == table.declaration =>
                        {
                            comparison
                                .operation
                                .structurally_valid(&contract.generic_params, definitions)
                        }
                        _ => false,
                    })
            }) && table.view.as_ref().is_none_or(|view| {
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
        && module.trait_contracts.iter().all(|contract| {
            bounds_type_limit(&contract.abi.bounds)
                && associated_type_limit(&contract.abi.associated_types)
                && contract.abi.methods.iter().all(|method| {
                    function_abi_type_limit(method)
                        && method.params.iter().all(|param| valid(&param.ty))
                        && valid(&method.return_type)
                })
        })
        && module.public_items.iter().all(|item| match item {
            PublicItem::Function(item) => {
                function_abi_type_limit(item)
                    && item.params.iter().all(|param| valid(&param.ty))
                    && valid(&item.return_type)
            }
            PublicItem::Const(item) => valid(&item.ty),
            PublicItem::Type(item) => {
                bounds_type_limit(&item.bounds)
                    && item.fields.iter().all(|field| valid(&field.ty))
                    && item
                        .variants
                        .iter()
                        .all(|variant| variant.payload.iter().all(&valid))
            }
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
                module.host_interface.types.len(),
                module.host_interface.functions.len(),
                module.native_imports.len(),
                module.native_declarations.len(),
                module.host_interface.paths.len(),
                module.module_slots.len(),
                module.constants.len(),
                module.types.len(),
                module.structures.len(),
                module.enumerations.len(),
                module.interface_tables.len(),
                module.paths.len(),
                module.function_table.len(),
                module.public_items.len(),
                module.trait_contracts.len(),
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
                metadata.semantic.locals.len(),
                metadata.semantic.registers.len(),
                metadata.params.len(),
                metadata.locals.len(),
                metadata.registers.len(),
                metadata.roots.locals.len(),
                metadata.roots.registers.len(),
                metadata.control_flow_targets.len(),
                debug.source_spans.len(),
                debug.line_table.len(),
                debug.safe_debug_points.len(),
                debug.local_live_ranges.len(),
                debug.captured_bindings.len(),
                debug.frame_layout.params.len(),
                debug.frame_layout.locals.len(),
                debug.frame_layout.registers.len(),
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
                function.frame_layout.locals.len(),
                function.frame_layout.registers.len(),
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
            !within_table_limit([
                item.params.len(),
                item.locals.len(),
                item.registers.len(),
                item.roots.locals.len(),
                item.roots.registers.len(),
            ])
        })
    {
        return Some("artifact metadata record limit exceeded");
    }
    None
}

fn add_operations(operations: &[OperationWitness], add: &mut impl FnMut(usize) -> bool) -> bool {
    add(operations.len())
        && operations
            .iter()
            .flat_map(OperationWitness::walk)
            .all(|operation| {
                add_callable_requirement(operation.requirement(), add)
                    && match operation {
                        OperationWitness::Selected(selected) => {
                            add(selected.instance.arguments.len())
                                && add(selected.signature.params.len())
                        }
                        OperationWitness::SharedAdapter(adapter) => {
                            add(adapter.call.arguments.len())
                                && add(adapter.call.signature.params.len())
                                && add(adapter.call.operations.len())
                        }
                        OperationWitness::Forward(_) => true,
                        OperationWitness::SharedMethod(selected) => {
                            add(selected.implementation.arguments.len())
                                && add(selected.operations.len())
                        }
                    }
            })
}
