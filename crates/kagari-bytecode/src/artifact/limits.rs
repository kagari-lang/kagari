use crate::{
    BytecodeModule, BytecodeProgram,
    artifact::{
        ArtifactSignatures, DebugMetadata, KbcArtifact, MAX_ARTIFACT_BYTES, MAX_ARTIFACT_FUNCTIONS,
        MAX_ARTIFACT_INSTRUCTIONS, MAX_ARTIFACT_MODULES, MAX_ARTIFACT_NESTED_RECORDS,
        MAX_ARTIFACT_TABLE_RECORDS, exceeds_encoded_size,
    },
};
use kagari_abi::native_import::NativeWitnessImplementation;
use kagari_abi::types::{
    AbiType, AssociatedTypeAbi, ConstraintAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi,
    PublicAbiItem,
};
use kagari_common::{
    host_interface::{HostInterface, HostPathSegmentDeclaration, HostValueType},
    identity::DefinitionId,
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
    for import in &module.engine_imports {
        if !add(import.instance.arguments.len())
            || !add(import.signature.params.len())
            || !add(import.requirements.len())
            || !add_abi_bounds(&import.requirements, &mut add)
            || !add(import.witnesses.len())
        {
            return false;
        }
        for witness in &import.witnesses {
            if !add(witness.interface.arguments.len())
                || !add(witness.interface.associated_types.len())
            {
                return false;
            }
            if let NativeWitnessImplementation::Table(instance) = &witness.implementation
                && !add(instance.arguments.len())
            {
                return false;
            }
        }
    }
    for declaration in &module.native_declarations {
        let function = &declaration.function;
        if !add(function.generic_params.len())
            || !add(function.bounds.len())
            || !add_abi_bounds(&function.bounds, &mut add)
            || !add(function.params.len())
        {
            return false;
        }
    }
    for function in &module.functions {
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
        if !add(table.arguments.len()) || !add(table.methods.len()) {
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
            PublicAbiItem::Function(item) => {
                add(item.generic_params.len())
                    && add(item.bounds.len())
                    && add_abi_bounds(&item.bounds, &mut add)
                    && add(item.params.len())
            }
            PublicAbiItem::Const(_) => true,
            PublicAbiItem::Type(item) => {
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
            PublicAbiItem::Trait(item) => {
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
                    })
            }
            PublicAbiItem::InterfaceTable(item) => {
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
            })
        {
            return false;
        }
    }
    true
}

pub(super) fn add_abi_bounds(
    bounds: &[GenericBoundAbi],
    add: &mut impl FnMut(usize) -> bool,
) -> bool {
    bounds.iter().all(|bound| {
        add(bound.constraints.len())
            && bound.constraints.iter().all(|constraint| match constraint {
                ConstraintAbi::Trait(ty) => add(ty.arguments.len()),
                _ => true,
            })
    })
}

pub(super) fn generic_identity_limit(
    params: &[GenericParameterAbi],
    bounds: &[GenericBoundAbi],
) -> bool {
    params.iter().all(|param| param.owner.within_path_limit())
        && bounds.iter().all(|bound| {
            bound.ty.within_wire_limits()
                && bound.constraints.iter().all(|constraint| match constraint {
                    ConstraintAbi::Standard(_) => true,
                    ConstraintAbi::Trait(ty) => {
                        ty.declaration.within_path_limit()
                            && ty.arguments.iter().all(|arg| arg.within_wire_limits())
                            && ty.associated_types.iter().all(|(member, value)| {
                                member.within_path_limit() && value.within_wire_limits()
                            })
                    }
                })
        })
}

pub(super) fn associated_identity_limit(members: &[AssociatedTypeAbi]) -> bool {
    members.iter().all(|member| {
        member.declaration.within_path_limit()
            && generic_identity_limit(&member.generic_params, &member.parameter_bounds)
            && member.bounds.iter().all(|bound| match bound {
                ConstraintAbi::Standard(_) => true,
                ConstraintAbi::Trait(ty) => AbiType::Trait(ty.clone()).within_wire_limits(),
            })
    })
}

pub(super) fn function_abi_identity_limit(function: &FunctionAbi) -> bool {
    generic_identity_limit(&function.generic_params, &function.bounds)
}

pub(super) fn module_abi_type_limit(module: &BytecodeModule) -> bool {
    let valid = |ty: &AbiType| ty.within_wire_limits();
    module
        .engine_imports
        .iter()
        .all(|import| import.direct_operation().is_some())
        && module.native_declarations.iter().all(|declaration| {
            let function = &declaration.function;
            declaration.declaration.within_path_limit()
                && function_abi_identity_limit(function)
                && function.params.iter().all(|param| valid(&param.ty))
                && valid(&function.return_type)
        })
        && module.interface_tables.iter().all(|table| {
            table.declaration.within_path_limit()
                && table.arguments.iter().all(&valid)
                && table
                    .methods
                    .iter()
                    .all(|slot| slot.method.within_path_limit())
        })
        && module.functions.iter().all(|function| {
            function.identity.as_ref().is_none_or(|identity| {
                identity.declaration.within_path_limit() && identity.arguments.iter().all(&valid)
            })
        })
        && module.function_table.iter().all(|record| {
            record.identity.as_ref().is_none_or(|identity| {
                identity.declaration.within_path_limit() && identity.arguments.iter().all(&valid)
            })
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
            contract.declaration.within_path_limit()
                && generic_identity_limit(&contract.abi.generic_params, &contract.abi.bounds)
                && associated_identity_limit(&contract.abi.associated_types)
                && contract.abi.methods.iter().all(|method| {
                    function_abi_identity_limit(method)
                        && method.params.iter().all(|param| valid(&param.ty))
                        && valid(&method.return_type)
                })
        })
        && module.public_items.iter().all(|item| match item {
            PublicAbiItem::Function(item) => {
                function_abi_identity_limit(item)
                    && item.params.iter().all(|param| valid(&param.ty))
                    && valid(&item.return_type)
            }
            PublicAbiItem::Const(item) => valid(&item.ty),
            PublicAbiItem::Type(item) => {
                generic_identity_limit(&item.generic_params, &item.bounds)
                    && item.fields.iter().all(|field| valid(&field.ty))
                    && item
                        .variants
                        .iter()
                        .all(|variant| variant.payload.iter().all(&valid))
            }
            PublicAbiItem::Trait(item) => {
                associated_identity_limit(&item.associated_types)
                    && generic_identity_limit(&item.generic_params, &item.bounds)
                    && item.methods.iter().all(|method| {
                        function_abi_identity_limit(method)
                            && method.params.iter().all(|param| valid(&param.ty))
                            && valid(&method.return_type)
                    })
            }
            PublicAbiItem::InterfaceTable(item) => {
                item.declaration.within_path_limit()
                    && generic_identity_limit(&item.generic_params, &item.bounds)
                    && valid(&item.trait_type)
                    && valid(&item.for_type)
                    && item.associated_type_families.iter().all(|family| {
                        family.declaration.within_path_limit()
                            && generic_identity_limit(&family.generic_params, &family.bounds)
                            && valid(&family.value)
                    })
                    && item.methods.iter().all(|method| {
                        function_abi_identity_limit(method)
                            && method.params.iter().all(|param| valid(&param.ty))
                            && valid(&method.return_type)
                    })
            }
        })
}

pub(super) fn host_identity_limit(interface: &HostInterface) -> bool {
    let valid = |id: &DefinitionId| id.within_path_limit();
    let value = |ty: &HostValueType| ty.nominal_references().into_iter().all(valid);
    interface.types.iter().all(|ty| {
        valid(&ty.id)
            && ty
                .fields
                .iter()
                .all(|field| valid(&field.id) && value(&field.ty))
            && ty.methods.iter().all(|method| {
                valid(&method.id)
                    && method.params.iter().all(|param| value(&param.ty))
                    && value(&method.return_type)
            })
    }) && interface.functions.iter().all(|function| {
        valid(&function.id)
            && function.params.iter().all(|param| value(&param.ty))
            && value(&function.return_type)
    }) && interface.paths.iter().all(|path| {
        valid(&path.root)
            && path.segments.iter().all(|segment| match segment {
                HostPathSegmentDeclaration::Field(id) => valid(id),
                HostPathSegmentDeclaration::Index(index) => {
                    value(&index.collection) && value(&index.index) && value(&index.result)
                }
                HostPathSegmentDeclaration::Virtual(virtual_step) => value(&virtual_step.result),
            })
    })
}

pub(super) fn program_count_limit(program: &BytecodeProgram) -> Option<&'static str> {
    if program.modules.len() > MAX_ARTIFACT_MODULES {
        return Some("too many modules");
    }
    let mut functions = 0usize;
    let mut instructions = 0usize;
    let mut operand_records = 0usize;
    let mut module_records = 0usize;
    let mut nested_records = 0usize;
    for module in &program.modules {
        if !module.identity.within_path_limit()
            || !host_identity_limit(&module.host_interface)
            || module.structures.iter().any(|layout| {
                !layout.declaration.within_path_limit()
                    || layout
                        .fields
                        .iter()
                        .any(|field| !field.declaration.within_path_limit())
            })
            || module.enumerations.iter().any(|layout| {
                !layout.declaration.within_path_limit()
                    || layout
                        .variants
                        .iter()
                        .any(|variant| !variant.declaration.within_path_limit())
            })
        {
            return Some("identity path segment limit exceeded");
        }
        if !module_nested_count_limit(module, &mut nested_records) {
            return Some("nested module record limit exceeded");
        }
        if !module_abi_type_limit(module) {
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
                module.engine_imports.len(),
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
                metadata.instruction_budgets.len(),
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

pub(super) fn artifact_count_limit(artifact: &KbcArtifact) -> Option<&'static str> {
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
    if let Some(reason) = program_count_limit(&artifact.program) {
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
        verification.security_profile_requirements.len(),
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
    if exceeds_encoded_size(artifact) {
        return Some("artifact encoded size limit exceeded");
    }
    None
}
