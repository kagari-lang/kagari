//! Publish assembled records under their canonical owners without duplicate declarations.
use crate::{catalog::assembly_identity, namespaces};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
        mapping::{DefinitionMapper, DefinitionRecord},
    },
};
use kagari_types::{
    declaration::{FnDecl, module::ModuleDecl},
    ty::GenericBound,
};
use std::collections::BTreeMap;

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
    if name.starts_with("__default_") {
        return namespaces::module("std", "collections");
    }
    if name.starts_with("$foundation_string_") {
        return namespaces::type_owner("String");
    }
    if name.starts_with("$foundation_list_") {
        return namespaces::type_owner("Vec");
    }
    if name.starts_with("$foundation_map_") || name.starts_with("$foundation_set_") {
        return namespaces::module("std", "collections");
    }
    if name.starts_with("$foundation_Range") {
        return namespaces::module("core", "ops");
    }
    if matches!(
        name,
        "$foundation_from_str" | "$foundation_try_from" | "$foundation_sum" | "$foundation_product"
    ) {
        return namespaces::module("core", "num");
    }
    assert!(
        matches!(name, "$foundation_cursor_next"),
        "unknown foundation function {name}"
    );
    namespaces::module("core", "iter")
}

fn bounds(bounds: &mut [GenericBound]) {
    for bound in bounds.iter_mut() {
        bound.constraints.sort();
    }
    bounds.sort_by(|left, right| left.ty.cmp(&right.ty));
}

fn function(function: &mut FnDecl) {
    bounds(&mut function.bounds);
}

pub(super) fn finish(assembly: ModuleDecl) -> Vec<ModuleDecl> {
    let mut implementations = BTreeMap::new();
    let mut counts = BTreeMap::<ModuleIdentity, u32>::new();
    for (index, implementation) in assembly.implementations.iter().enumerate() {
        let owner = namespaces::receiver_owner(&implementation.for_type)
            .expect("foundation receiver owner");
        let occurrence = counts.entry(owner.clone()).or_default();
        implementations.insert(index as u32, (owner, *occurrence));
        *occurrence += 1;
    }
    let assembly_id = assembly_identity();
    let mapped = assembly
        .map_identities(&mut DefinitionMapper::new(
            &mut |id: &DefinitionPath| {
                assert_eq!(id.module, assembly_id);
                let mut id = id.clone();
                let root = &mut id.path[0];
                id.module = match root.kind {
                    DefinitionKind::Trait => namespaces::trait_owner(&root.name),
                    DefinitionKind::AssociatedType | DefinitionKind::Enum => {
                        namespaces::type_owner(&root.name)
                    }
                    DefinitionKind::Function => function_owner(&root.name),
                    DefinitionKind::Impl => {
                        let (owner, occurrence) = &implementations[&root.occurrence];
                        root.occurrence = *occurrence;
                        owner.clone()
                    }
                    _ => panic!("unsupported foundation definition"),
                };
                Ok(id)
            },
            &CancellationToken::default(),
        ))
        .expect("foundation ownership mapping");
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
        entry(&mut modules, &id.module).private_functions.insert(id);
    }
    for (id, doc) in mapped.documentation {
        entry(&mut modules, &id.module)
            .documentation
            .insert(id, doc);
    }
    for (id, calls) in mapped.callable_requirements {
        entry(&mut modules, &id.module)
            .callable_requirements
            .insert(id, calls);
    }
    for (id, result) in mapped.concrete_results {
        entry(&mut modules, &id.module)
            .concrete_results
            .insert(id, result);
    }
    // std exposes the same declarations, while canonical definitions remain in core/alloc.
    let owners: Vec<_> = modules
        .values()
        .filter(|module| module.identity.package.0 != "std")
        .cloned()
        .collect();
    for owner in owners {
        let facade = entry(
            &mut modules,
            &namespaces::module("std", &owner.identity.path.join("::")),
        );
        for item in &owner.traits {
            facade.exports.insert(
                item.name.clone(),
                owner.definition(DefinitionKind::Trait, &item.name),
            );
        }
        for ty in &owner.types {
            let kind = ty.kind.definition_kind();
            facade
                .exports
                .insert(ty.name.clone(), owner.definition(kind, &ty.name));
        }
    }
    let foundation_owners: Vec<_> = modules
        .keys()
        .filter(|id| id.package.0 != "std" || id.path == ["collections"])
        .cloned()
        .collect();
    let prelude = entry(&mut modules, &namespaces::prelude());
    prelude.prelude = true;
    // Syntax and implicit implementations need the complete installed foundation,
    // independently of which declaration names the prelude brings into scope.
    prelude.dependencies.extend(foundation_owners);
    for name in [
        "Iterator",
        "Iterable",
        "FromIterator",
        "PartialEq",
        "Eq",
        "PartialOrd",
        "Ord",
        "From",
        "Into",
        "TryFrom",
        "TryInto",
        "Fn",
    ] {
        prelude.exports.insert(
            name.into(),
            ModuleDecl::new(namespaces::trait_owner(name)).definition(DefinitionKind::Trait, name),
        );
    }
    for (name, kind) in [
        ("String", DefinitionKind::AssociatedType),
        ("Vec", DefinitionKind::AssociatedType),
        ("Option", DefinitionKind::Enum),
        ("Result", DefinitionKind::Enum),
    ] {
        prelude.exports.insert(
            name.into(),
            ModuleDecl::new(namespaces::type_owner(name)).definition(kind, name),
        );
    }
    for (owner, variant) in [
        ("Option", "Some"),
        ("Option", "None"),
        ("Result", "Ok"),
        ("Result", "Err"),
    ] {
        let mut id =
            ModuleDecl::new(namespaces::type_owner(owner)).definition(DefinitionKind::Enum, owner);
        id.path.push(DefinitionPathSegment {
            kind: DefinitionKind::Variant,
            name: variant.into(),
            occurrence: 0,
        });
        prelude.exports.insert(variant.into(), id);
    }
    for module in modules.values_mut() {
        // Changing owned paths changes canonical constraint ordering too.
        for ty in &mut module.types {
            bounds(&mut ty.bounds);
        }
        for item in &mut module.traits {
            bounds(&mut item.bounds);
            for member in &mut item.associated_types {
                member.bounds.sort();
            }
            for method in &mut item.methods {
                function(method);
            }
        }
        for item in &mut module.implementations {
            bounds(&mut item.bounds);
            for method in &mut item.methods {
                function(method);
            }
        }
        for item in &mut module.functions {
            function(item);
        }
        let mut dependencies = Vec::new();
        module
            .visit_definitions(
                &mut |id| {
                    if id.module != module.identity {
                        dependencies.push(id.module.clone());
                    }
                    Ok(())
                },
                &Default::default(),
            )
            .unwrap();
        module.dependencies.extend(dependencies);
        module
            .validate(&namespaces::receiver_owner)
            .unwrap_or_else(|error| panic!("{}: {error}", module.identity));
    }
    modules.into_values().collect()
}
