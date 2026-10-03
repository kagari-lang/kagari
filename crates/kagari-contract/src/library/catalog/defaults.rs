//! Canonical concrete declarations and their ordinary native implementation slots.
use crate::library::catalog::contracts::{applied_item, method, unit};
use crate::library::catalog::key::{self, RegistrationTrait};
use crate::{
    callable::CallableImplementation,
    declaration::{ImplDecl, ModuleDecl},
    native_import::callables::NativeCallableRequirement,
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::{
        Constraint, FnDecl, GenericBound, GenericParam, NominalTy, Ty, TypeDef, TypeDefKind,
        VariantDef, native::NativeTypeConstructor, substitution::TypeSubstitution,
    },
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionKind, DefinitionPath, associated_type_id},
    range::RangeKind,
};

fn define_type(
    module: &mut ModuleDecl,
    name: &str,
    layout: NativeTypeConstructor,
    parameters: &[&str],
    hash_key: bool,
) {
    let owner = module.definition(layout.declaration_kind(), name);
    let generic_params: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(position, _)| GenericParam {
            owner: owner.clone(),
            position,
        })
        .collect();
    let bounds = if hash_key {
        hash_bounds(generic_params[0].as_type())
    } else {
        vec![]
    };
    let variants = if let NativeTypeConstructor::Enum(kind) = layout {
        kind.variants()
            .iter()
            .map(|variant| {
                let name = format!("{variant:?}");
                VariantDef {
                    name: name.strip_prefix("Parse").unwrap_or(&name).to_owned(),
                    payload: variant
                        .payload()
                        .map(|position| vec![generic_params[position].as_type()])
                        .unwrap_or_default(),
                }
            })
            .collect()
    } else {
        vec![]
    };
    module.types.push(TypeDef {
        name: name.into(),
        kind: TypeDefKind::Native(layout),
        generic_params,
        bounds,
        fields: vec![],
        variants,
    });
    module
        .documentation
        .insert(owner, format!("Language-owned {name} type."));
}

fn hash_bounds(key: Ty) -> Vec<GenericBound> {
    vec![GenericBound {
        ty: key,
        constraints: vec![
            Constraint::Trait(key::applied(RegistrationTrait::Eq, vec![])),
            Constraint::Trait(key::applied(RegistrationTrait::Hash, vec![])),
            // The selected equality member belongs to PartialEq. Carry its
            // inherited obligation explicitly in the portable callback template.
            Constraint::Trait(key::applied(RegistrationTrait::PartialEq, vec![])),
        ],
    }]
}

fn parameters(owner: &DefinitionPath, names: &[&str]) -> Vec<GenericParam> {
    names
        .iter()
        .enumerate()
        .map(|(position, _)| GenericParam {
            owner: owner.clone(),
            position,
        })
        .collect()
}

fn binding(module: &ModuleDecl, family: &str, name: &str) -> DefinitionPath {
    module.definition(
        DefinitionKind::Function,
        &format!("$foundation_{family}_{name}"),
    )
}

fn implement(
    module: &mut ModuleDecl,
    kind: RegistrationTrait,
    generic_params: Vec<GenericParam>,
    applied: NominalTy,
    receiver: Ty,
    bounds: Vec<GenericBound>,
    family: &str,
) {
    let contract = module
        .traits
        .iter()
        .find(|contract| contract.name == kind.name())
        .expect("language contract")
        .clone();
    let bindings: Vec<_> = contract
        .methods
        .iter()
        .map(|method| (method.name.as_str(), binding(module, family, &method.name)))
        .collect();
    module
        .implement_trait(&contract, applied, receiver, generic_params, &bindings)
        .unwrap_or_else(|error| panic!("{} foundation implementation: {error}", kind.name()));
    module
        .implementations
        .last_mut()
        .expect("new implementation")
        .bounds = bounds;
    declare_key_calls(module, family);
}

fn declare_key_calls(module: &mut ModuleDecl, family: &str) {
    if !matches!(family, "map" | "set") {
        return;
    }
    let index = module.implementations.len() - 1;
    let implementation = &module.implementations[index];
    let key = implementation.generic_params[0].as_type();
    let requirements: Vec<_> = [
        (RegistrationTrait::Hash, "hash"),
        (RegistrationTrait::PartialEq, "eq"),
    ]
    .into_iter()
    .map(|(protocol, name)| {
        let interface = key::applied(protocol, vec![]);
        NativeCallableRequirement {
            receiver: key.clone(),
            member: ModuleDecl::method_id(&interface.declaration, name),
            interface,
            arguments: vec![],
        }
    })
    .collect();
    let owner = module.implementation_id(index);
    let methods: Vec<_> = implementation
        .methods
        .iter()
        .filter(|method| {
            matches!(
                method.name.as_str(),
                "new" | "get" | "contains" | "contains_key" | "insert" | "insert_fluent" | "remove"
            )
        })
        .map(|method| ModuleDecl::method_id(&owner, &method.name))
        .collect();
    for method in methods {
        module
            .callable_requirements
            .insert(method, requirements.clone());
    }
}

pub(super) fn declare(module: &mut ModuleDecl) {
    define_type(module, "String", NativeTypeConstructor::String, &[], false);
    for (name, kind, parameters) in [
        ("Option", StandardEnum::Option, &["T"][..]),
        ("Result", StandardEnum::Result, &["T", "E"][..]),
        ("Ordering", StandardEnum::Ordering, &[][..]),
        ("Bound", StandardEnum::Bound, &["T"][..]),
        ("ParseError", StandardEnum::ParseError, &[][..]),
        ("TryFromIntError", StandardEnum::TryFromIntError, &[][..]),
        ("Infallible", StandardEnum::Infallible, &[][..]),
    ] {
        define_type(
            module,
            name,
            NativeTypeConstructor::Enum(kind),
            parameters,
            false,
        );
        if matches!(
            kind,
            StandardEnum::Option
                | StandardEnum::Result
                | StandardEnum::Ordering
                | StandardEnum::Bound
        ) {
            module.variant_exports.insert(name.into());
        }
    }
    for kind in [
        RangeKind::Exclusive,
        RangeKind::Inclusive,
        RangeKind::From,
        RangeKind::To,
        RangeKind::ToInclusive,
        RangeKind::Full,
    ] {
        let names: &[&str] = if kind == RangeKind::Full { &[] } else { &["T"] };
        define_type(
            module,
            kind.name(),
            NativeTypeConstructor::Range(kind),
            names,
            false,
        );
        range_implementations(module, kind);
    }
    define_type(
        module,
        "ArrayList",
        NativeTypeConstructor::Array,
        &["T"],
        false,
    );
    define_type(
        module,
        "HashMap",
        NativeTypeConstructor::Map,
        &["K", "V"],
        true,
    );
    define_type(module, "HashSet", NativeTypeConstructor::Set, &["T"], true);
    define_type(
        module,
        "CollectionCursor",
        NativeTypeConstructor::Iter,
        &["T"],
        false,
    );
    for (family, names, protocols) in [
        (
            "list",
            &["T"][..],
            &[
                RegistrationTrait::List,
                RegistrationTrait::MutableList,
                RegistrationTrait::Index,
                RegistrationTrait::Iterable,
            ][..],
        ),
        (
            "map",
            &["K", "V"][..],
            &[
                RegistrationTrait::Map,
                RegistrationTrait::MutableMap,
                RegistrationTrait::Iterable,
            ][..],
        ),
        (
            "set",
            &["T"][..],
            &[
                RegistrationTrait::Set,
                RegistrationTrait::MutableSet,
                RegistrationTrait::Iterable,
            ][..],
        ),
        ("cursor", &["T"][..], &[RegistrationTrait::Iterator][..]),
    ] {
        for kind in protocols {
            let owner = module.implementation_id(module.implementations.len());
            let params = parameters(&owner, names);
            let items: Vec<_> = params.iter().map(GenericParam::as_type).collect();
            let item = if family == "map" {
                Ty::Tuple(items.clone())
            } else {
                items[0].clone()
            };
            let receiver = storage(family, &items);
            let mut applied = if kind.iteration() {
                applied_item(*kind, item.clone())
            } else if *kind == RegistrationTrait::Index {
                key::applied(*kind, vec![Ty::Builtin(BuiltinType::USize)])
            } else {
                key::applied(*kind, items.clone())
            };
            if *kind == RegistrationTrait::Index {
                applied.associated_types.insert(
                    associated_type_id(&applied.declaration, "Output"),
                    item.clone(),
                );
            }
            if *kind == RegistrationTrait::Iterable {
                applied.associated_types.insert(
                    associated_type_id(&applied.declaration, "Iter"),
                    Ty::Iter(Box::new(item)),
                );
            }
            let bounds = if matches!(family, "map" | "set") {
                hash_bounds(items[0].clone())
            } else {
                vec![]
            };
            implement(module, *kind, params, applied, receiver, bounds, family);
        }
        if family != "cursor" {
            inherent(module, family, names);
        }
    }
}

fn range_implementations(module: &mut ModuleDecl, kind: RangeKind) {
    let owner = module.implementation_id(module.implementations.len());
    let params = parameters(&owner, &["T"]);
    let item = params[0].as_type();
    let layout = if kind == RangeKind::Full {
        Ty::Builtin(BuiltinType::Unit)
    } else {
        item.clone()
    };
    implement(
        module,
        RegistrationTrait::RangeBounds,
        params,
        key::applied(RegistrationTrait::RangeBounds, vec![item]),
        Ty::Range(Box::new(layout), kind),
        vec![],
        kind.name(),
    );
    if !kind.has_start() {
        return;
    }
    for scalar in [
        BuiltinType::I8,
        BuiltinType::I16,
        BuiltinType::I32,
        BuiltinType::I64,
        BuiltinType::ISize,
        BuiltinType::U8,
        BuiltinType::U16,
        BuiltinType::U32,
        BuiltinType::U64,
        BuiltinType::USize,
    ] {
        let item = Ty::Builtin(scalar);
        let mut applied = applied_item(RegistrationTrait::Iterable, item.clone());
        applied.associated_types.insert(
            associated_type_id(&applied.declaration, "Iter"),
            Ty::Iter(Box::new(item.clone())),
        );
        implement(
            module,
            RegistrationTrait::Iterable,
            vec![],
            applied,
            Ty::Range(Box::new(item), kind),
            vec![],
            kind.name(),
        );
    }
}

fn storage(family: &str, items: &[Ty]) -> Ty {
    match family {
        "list" => Ty::Array(Box::new(items[0].clone()), CollectionAccess::Mutable),
        "map" => Ty::Map {
            key: Box::new(items[0].clone()),
            value: Box::new(items[1].clone()),
            access: CollectionAccess::Mutable,
        },
        "set" => Ty::Set(Box::new(items[0].clone()), CollectionAccess::Mutable),
        "cursor" => Ty::Iter(Box::new(items[0].clone())),
        _ => unreachable!("foundation family"),
    }
}

fn inherent(module: &mut ModuleDecl, family: &str, names: &[&str]) {
    let owner = module.implementation_id(module.implementations.len());
    let parameters = parameters(&owner, names);
    let items: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
    let receiver = storage(family, &items);
    let bounds = if matches!(family, "map" | "set") {
        hash_bounds(items[0].clone())
    } else {
        vec![]
    };
    let mut new = method("new", vec![], receiver.clone());
    new.implementation = CallableImplementation::Native(binding(module, family, "new"));
    let mut methods: Vec<FnDecl> = vec![new];
    // Concrete mutators preserve fluent returns; capability methods return unit.
    for kind in [
        RegistrationTrait::MutableList,
        RegistrationTrait::MutableMap,
        RegistrationTrait::MutableSet,
    ] {
        if !matches!(
            (family, kind),
            ("list", RegistrationTrait::MutableList)
                | ("map", RegistrationTrait::MutableMap)
                | ("set", RegistrationTrait::MutableSet)
        ) {
            continue;
        }
        let contract = module
            .traits
            .iter()
            .find(|contract| contract.name == kind.name())
            .expect("mutable contract")
            .clone();
        let contract_owner = key::identity(kind);
        let mut substitution = TypeSubstitution::for_owner(&contract_owner, &items);
        substitution.bind_receiver(&contract_owner, &receiver);
        for mut method in contract.methods {
            if method.return_type != unit()
                || matches!(
                    method.implementation,
                    CallableImplementation::NativeDefault(_)
                )
            {
                continue;
            }
            for param in &mut method.params {
                param.ty = substitution
                    .apply(&param.ty, &Default::default())
                    .expect("foundation parameter");
            }
            method.return_type = receiver.clone();
            method.implementation = CallableImplementation::Native(binding(
                module,
                family,
                &format!("{}_fluent", method.name),
            ));
            methods.push(method);
        }
    }
    for method in &mut methods {
        method.generic_params = parameters.clone();
        method.bounds = bounds.clone();
    }
    module.implementations.push(ImplDecl {
        generic_params: parameters,
        bounds,
        trait_type: None,
        for_type: receiver,
        methods,
    });
    declare_key_calls(module, family);
}
