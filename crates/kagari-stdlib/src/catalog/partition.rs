//! Publish assembled records under their canonical owners without duplicate declarations.
use crate::{
    catalog::{assembly_identity, identities},
    namespaces,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        map::DefinitionContext,
        mapping::DefinitionMapper,
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionTable,
    },
};
use kagari_types::{
    declaration::{
        FnDecl,
        module::{DeclarationError, ModuleDecl},
    },
    ty::{GenericBound, NominalTy, Ty},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    mem,
};

fn entry<'a>(
    modules: &'a mut BTreeMap<ModuleIdentity, ModuleDecl>,
    id: &ModuleIdentity,
) -> &'a mut ModuleDecl {
    modules.entry(id.clone()).or_insert_with(|| {
        let mut module = ModuleDecl::new(id.clone());
        module.package_alias = match id.package.0.as_str() {
            "kagari-core" => Some("core".into()),
            "kagari-alloc" => Some("alloc".into()),
            _ => None,
        };
        module
    })
}

fn function_owner(name: &str) -> ModuleIdentity {
    if name.starts_with("$foundation_task_") {
        return namespaces::type_owner("Task");
    }

    panic!("unknown remaining foundation function {name}")
}

fn bounds(
    bounds: &mut Vec<GenericBound>,
    definitions: &DefinitionTable,
) -> Result<(), DeclarationError> {
    *bounds = GenericBound::normalize(
        mem::take(bounds),
        definitions,
        &CancellationToken::default(),
    )
    .map_err(|error| DeclarationError(format!("foundation bounds: {error:?}")))?;
    Ok(())
}

fn function(function: &mut FnDecl, definitions: &DefinitionTable) -> Result<(), DeclarationError> {
    bounds(&mut function.bounds, definitions)
}

pub(super) fn finish(
    assembly: ModuleDecl,
    source: &DefinitionTable,
) -> Result<(DefinitionContext, Vec<ModuleDecl>), DeclarationError> {
    let context = DefinitionContext::new().map_err(identities::invalid)?;
    let mut implementations = BTreeMap::new();
    let mut counts = BTreeMap::<ModuleIdentity, u32>::new();
    for (index, implementation) in assembly.implementations.iter().enumerate() {
        let owner = match &implementation.for_type {
            Ty::Trait(nominal) => {
                let declaration = source
                    .resolve(nominal.declaration)
                    .map_err(identities::invalid)?;
                if declaration.module() == &assembly.identity {
                    namespaces::trait_owner(declaration.segments().next().expect("trait root").name)
                } else {
                    declaration.module().clone()
                }
            }
            Ty::Enum(nominal) | Ty::NativeObject(nominal)
                if source
                    .resolve(nominal.declaration)
                    .map_err(identities::invalid)?
                    .module()
                    == &assembly.identity =>
            {
                namespaces::type_owner(
                    source
                        .resolve(nominal.declaration)
                        .map_err(identities::invalid)?
                        .segments()
                        .next()
                        .expect("nominal root")
                        .name,
                )
            }
            receiver => {
                namespaces::receiver_owner(receiver, source).expect("foundation receiver owner")
            }
        };
        let occurrence = counts.entry(owner.clone()).or_default();
        implementations.insert(index as u32, (owner, *occurrence));
        *occurrence += 1;
    }
    let mut references = HashSet::new();
    let cancel = CancellationToken::default();
    assembly
        .visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            &cancel,
        )
        .map_err(identities::invalid)?;
    let mut identities = HashMap::new();
    for id in references {
        let view = source.resolve(id).map_err(identities::invalid)?;
        if view.module() != &assembly_identity() {
            return Err(DeclarationError(
                "foreign foundation assembly identity".into(),
            ));
        }
        let segments = view.segments().collect::<Vec<_>>();
        let root = segments
            .first()
            .ok_or_else(|| DeclarationError("missing foundation declaration segment".into()))?;
        let (module, occurrence) = match root.kind {
            DefinitionKind::Trait => (namespaces::trait_owner(root.name), root.occurrence),
            DefinitionKind::AssociatedType | DefinitionKind::Enum => {
                (namespaces::type_owner(root.name), root.occurrence)
            }
            DefinitionKind::Function => (function_owner(root.name), root.occurrence),
            DefinitionKind::Impl => implementations
                .get(&root.occurrence)
                .cloned()
                .ok_or_else(|| DeclarationError("unknown foundation implementation".into()))?,
            _ => return Err(DeclarationError("unsupported foundation identity".into())),
        };
        let mut target = context.intern_root(&module).map_err(identities::invalid)?;
        for (index, segment) in segments.iter().enumerate() {
            target = context
                .intern_child(
                    target,
                    segment.kind,
                    segment.name,
                    if index == 0 {
                        occurrence
                    } else {
                        segment.occurrence
                    },
                )
                .map_err(identities::invalid)?;
        }
        identities.insert(id, target);
    }
    let mapped = assembly
        .remap_definitions(&mut DefinitionMapper::new(
            &mut |id| Ok(identities[id]),
            &cancel,
        ))
        .map_err(identities::invalid)?;
    let mapped_definitions = context.snapshot();
    let mut modules = BTreeMap::new();
    for ty in mapped.types {
        entry(&mut modules, &namespaces::type_owner(&ty.name))
            .types
            .push(ty);
    }
    for name in mapped.variant_exports {
        entry(&mut modules, &namespaces::type_owner(&name))
            .variant_exports
            .insert(name);
    }
    for item in mapped.traits {
        entry(&mut modules, &namespaces::trait_owner(&item.name))
            .traits
            .push(item);
    }
    for (index, item) in mapped.implementations.into_iter().enumerate() {
        entry(&mut modules, &implementations[&(index as u32)].0)
            .implementations
            .push(item);
    }
    for function in mapped.functions {
        entry(&mut modules, &function_owner(&function.name))
            .functions
            .push(function);
    }
    for id in mapped.private_functions {
        entry(
            &mut modules,
            mapped_definitions
                .resolve(id)
                .map_err(identities::invalid)?
                .module(),
        )
        .private_functions
        .insert(id);
    }
    for (id, doc) in mapped.documentation {
        entry(
            &mut modules,
            mapped_definitions
                .resolve(id)
                .map_err(identities::invalid)?
                .module(),
        )
        .documentation
        .insert(id, doc);
    }
    for (id, calls) in mapped.callable_requirements {
        entry(
            &mut modules,
            mapped_definitions
                .resolve(id)
                .map_err(identities::invalid)?
                .module(),
        )
        .callable_requirements
        .insert(id, calls);
    }
    for (id, result) in mapped.concrete_results {
        entry(
            &mut modules,
            mapped_definitions
                .resolve(id)
                .map_err(identities::invalid)?
                .module(),
        )
        .concrete_results
        .insert(id, result);
    }
    for module in modules.values() {
        module.collect_definitions(&context)?;
    }
    let definitions = context.snapshot();
    for module in modules.values_mut() {
        // Publication changes owner identities; restore canonical semantic ordering.
        for ty in &mut module.types {
            bounds(&mut ty.bounds, &definitions)?;
        }
        for item in &mut module.traits {
            bounds(&mut item.bounds, &definitions)?;
            for member in &mut item.associated_types {
                member.bounds = NominalTy::normalize_bounds(
                    mem::take(&mut member.bounds),
                    &definitions,
                    &cancel,
                )
                .map_err(|error| {
                    DeclarationError(format!("foundation associated bounds: {error:?}"))
                })?;
            }
            for method in &mut item.methods {
                function(method, &definitions)?;
            }
        }
        for item in &mut module.implementations {
            bounds(&mut item.bounds, &definitions)?;
            for method in &mut item.methods {
                function(method, &definitions)?;
            }
        }
        for item in &mut module.functions {
            function(item, &definitions)?;
        }
        let mut dependencies = Vec::new();
        module
            .visit_definition_ids(
                &mut |id| {
                    let owner = definitions.resolve(*id)?.module();
                    if owner != &module.identity {
                        dependencies.push(owner.clone());
                    }
                    Ok(())
                },
                &Default::default(),
            )
            .map_err(identities::invalid)?;
        module.dependencies.extend(dependencies);
        module.collect_definitions(&context)?;
        // Complete validation belongs to the shared batch seal, after new owners join.
    }
    Ok((context, modules.into_values().collect()))
}
