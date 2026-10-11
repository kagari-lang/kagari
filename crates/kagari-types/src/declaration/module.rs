//! Authoritative native API declarations shared by compilation, installation and tooling.
use crate::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, NativeDeclaration, TraitDef, TypeDef, TypeDefKind,
        names::{ExportName, NameNamespace},
        requirement::NativeCallableRequirement,
        verify::{
            Parameters, function_valid, native_bounds_valid, trait_valid, type_definition_valid,
            validate_native_declarations,
        },
    },
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty},
};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    error::Error,
    fmt,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationError(pub String);

impl fmt::Display for DeclarationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native API: {}", self.0)
    }
}

impl Error for DeclarationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplDecl {
    pub generic_params: Vec<GenericParam>,
    /// Obligations inherited by every method's registered callable template.
    pub bounds: Vec<GenericBound>,
    pub trait_type: Option<NominalTy>,
    pub for_type: Ty,
    pub methods: Vec<FnDecl>,
}

/// Rust registration definitions own these records. Generated text is a projection.
/// The initial API supports native storage, traits, generic impls and free functions;
/// Free functions retain ordinary checked trait bounds and associated projections.
/// Impl and inherent-method bounds use the same checked declaration model.
/// Registered default applications retain their explicit native template mappings.
/// Method binders retain their own identities; associated type families remain deferred.
#[derive(Debug, Clone)]
pub struct ModuleDecl {
    /// Explicit source prelude supplied by this registration closure.
    pub prelude: bool,
    pub identity: ModuleIdentity,
    /// Installed script package spelling, independent of the canonical package ID.
    pub package_alias: Option<String>,
    /// Owning modules required by the checked registration closure. NativeApi
    /// derives these edges from actual traits, templates and implementation facts.
    pub dependencies: BTreeSet<ModuleIdentity>,
    pub types: Vec<TypeDef>,
    /// Enum owners whose variants are also explicitly exported at module scope.
    pub variant_exports: BTreeSet<String>,
    /// Public type/trait/variant aliases retain the identity of their owner.
    pub exports: BTreeMap<ExportName, DefinitionId>,
    pub traits: Vec<TraitDef>,
    pub implementations: Vec<ImplDecl>,
    pub functions: Vec<FnDecl>,
    /// Registered templates remain addressable by identity without public exports.
    pub private_functions: BTreeSet<DefinitionId>,
    /// Full Markdown overview of this installed module.
    pub module_documentation: String,
    pub documentation: BTreeMap<DefinitionId, String>,
    /// Ordered callback slots owned by each native declaration. The compiler
    /// specializes these requirements; generated source does not select targets.
    pub callable_requirements: BTreeMap<DefinitionId, Vec<NativeCallableRequirement>>,
    pub concrete_results: BTreeMap<DefinitionId, Ty>,
}

impl ModuleDecl {
    pub fn new(identity: ModuleIdentity) -> Self {
        Self {
            prelude: false,
            package_alias: None,
            identity,
            dependencies: BTreeSet::new(),
            types: vec![],
            variant_exports: BTreeSet::new(),
            exports: BTreeMap::new(),
            traits: vec![],
            implementations: vec![],
            functions: vec![],
            private_functions: BTreeSet::new(),
            module_documentation: String::new(),
            documentation: BTreeMap::new(),
            callable_requirements: BTreeMap::new(),
            concrete_results: BTreeMap::new(),
        }
    }

    fn declaration_count_valid(&self) -> bool {
        self.types
            .len()
            .saturating_add(self.traits.len())
            .saturating_add(self.functions.len())
            .saturating_add(self.implementations.len())
            <= 4096
    }

    /// Inherent declarations are owned by the receiver's defining module.
    /// Built-in value families use their canonical core/alloc/std declaration owner.
    pub fn owns_inherent_receiver(
        &self,
        receiver: &Ty,
        owner: &impl Fn(&Ty) -> Option<ModuleIdentity>,
    ) -> bool {
        owner(receiver).is_some_and(|owner| owner == self.identity)
    }

    /// Resolve registered native templates in this module's retained identity scope.
    pub fn native_declarations(
        &self,
        definitions: &DefinitionTable,
    ) -> Result<Vec<NativeDeclaration>, DeclarationError> {
        let mut result = Vec::new();
        for function in &self.functions {
            let id = self.definition(DefinitionKind::Function, &function.name, definitions)?;
            result.push(NativeDeclaration {
                declaration: id,
                function: function.clone(),
                concrete_result: self.concrete_results.get(&id).cloned(),
                callable_requirements: self
                    .callable_requirements
                    .get(&id)
                    .cloned()
                    .unwrap_or_default(),
            });
        }
        for (index, implementation) in self.implementations.iter().enumerate() {
            let owner = self.implementation_id(index, definitions)?;
            for function in &implementation.methods {
                let id = Self::method_id(owner, &function.name, definitions)?;
                let mut function = function.clone();
                function.bounds = GenericBound::normalize(
                    implementation
                        .bounds
                        .iter()
                        .chain(&function.bounds)
                        .cloned(),
                    definitions,
                    &CancellationToken::default(),
                )
                .map_err(|error| DeclarationError(format!("invalid native bounds: {error:?}")))?;
                result.push(NativeDeclaration {
                    declaration: id,
                    function,
                    concrete_result: self.concrete_results.get(&id).cloned(),
                    callable_requirements: self
                        .callable_requirements
                        .get(&id)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
        }
        Ok(result)
    }

    pub fn validate(
        &self,
        receiver_owner: &impl Fn(&Ty) -> Option<ModuleIdentity>,
        definitions: &DefinitionTable,
    ) -> Result<(), DeclarationError> {
        let fail = || DeclarationError("invalid or unsupported native declaration".into());
        if self.identity.package.0.is_empty()
            || self
                .package_alias
                .as_ref()
                .is_some_and(|alias| !identifier(alias))
            || self.identity.path.is_empty()
            || self.identity.path.iter().any(|name| !identifier(name))
            || self.dependencies.len() > 4096
            || self.dependencies.iter().any(|dependency| {
                dependency == &self.identity
                    || dependency.package.0.is_empty()
                    || dependency.path.is_empty()
                    || dependency.path.iter().any(|name| !identifier(name))
            })
            || !self.declaration_count_valid()
        {
            return Err(fail());
        }
        let mut names = HashSet::new();
        let cancel = CancellationToken::default();
        self.visit_definition_ids(&mut |id| Ok(definitions.resolve(*id).map(|_| ())?), &cancel)
            .map_err(|error| DeclarationError(format!("invalid native identities: {error}")))?;
        for ty in &self.types {
            for payload in ty.variants.iter().flat_map(|variant| &variant.payload) {
                supported_type(payload, definitions)?;
            }
            if !identifier(&ty.name)
                || !names.insert((NameNamespace::Type, &ty.name))
                || !matches!(
                    ty.kind,
                    TypeDefKind::Enum | TypeDefKind::Native(_) | TypeDefKind::NativeStorage(_)
                )
                || ty.variants.iter().any(|variant| !identifier(&variant.name))
            {
                return Err(fail());
            }
            let owner = self.definition(ty.kind.definition_kind(), &ty.name, definitions)?;
            if !type_definition_valid(ty, owner, &cancel, definitions) {
                return Err(fail());
            }
        }
        for item in &self.traits {
            if !identifier(&item.name)
                || !names.insert((NameNamespace::Type, &item.name))
                || !item.bounds.is_empty()
                || item.associated_types.iter().any(|member| {
                    definitions
                        .resolve(member.declaration)
                        .ok()
                        .and_then(|view| view.last())
                        .is_none_or(|part| !identifier(part.name))
                        || !member.generic_params.is_empty()
                        || !member.parameter_bounds.is_empty()
                })
                || !item.associated_consts.is_empty()
                || item.methods.iter().any(|method| {
                    !identifier(&method.name)
                        || !matches!(
                            method.implementation,
                            CallableImplementation::Required
                                | CallableImplementation::NativeDefault(_)
                        )
                })
            {
                return Err(fail());
            }
            let owner = self.definition(DefinitionKind::Trait, &item.name, definitions)?;
            if !trait_valid(item, owner, &cancel, definitions) {
                return Err(fail());
            }
        }
        for function in &self.functions {
            let owner = self.definition(DefinitionKind::Function, &function.name, definitions)?;
            if !identifier(&function.name) || !names.insert((NameNamespace::Value, &function.name))
            {
                return Err(fail());
            }
            if !(matches!(function.implementation, CallableImplementation::Native(_))
                || (function.generic_params.is_empty() && function.bounds.is_empty()))
                || !function_valid(
                    function,
                    owner,
                    &Parameters::new(),
                    None,
                    &cancel,
                    definitions,
                )
            {
                return Err(fail());
            }
        }
        for owner in &self.variant_exports {
            let Some(ty) = self.types.iter().find(|ty| ty.name == *owner) else {
                return Err(fail());
            };
            if !matches!(ty.kind, TypeDefKind::Enum) {
                return Err(fail());
            }
            for variant in &ty.variants {
                if !names.insert((NameNamespace::Value, &variant.name)) {
                    return Err(fail());
                }
            }
        }
        if self.exports.len() > 4096 {
            return Err(fail());
        }
        for (name, target) in &self.exports {
            let target = definitions.resolve(*target).map_err(|_| fail())?;
            let segments = target.segments().collect::<Vec<_>>();
            if !identifier(&name.name)
                || !names.insert((name.namespace, &name.name))
                || target
                    .last()
                    .and_then(|part| NameNamespace::of_definition(part.kind))
                    != Some(name.namespace)
                || *target.module() == self.identity
                || !(1..=2).contains(&segments.len())
                || !matches!(
                    segments[0].kind,
                    DefinitionKind::Trait | DefinitionKind::Enum | DefinitionKind::AssociatedType
                )
                || segments
                    .iter()
                    .any(|part| !identifier(part.name) || part.occurrence != 0)
                || (segments.len() == 2
                    && (segments[0].kind != DefinitionKind::Enum
                        || segments[1].kind != DefinitionKind::Variant))
            {
                return Err(fail());
            }
        }
        if self.private_functions.len() > self.functions.len()
            || self.private_functions.iter().any(|id| {
                !self.functions.iter().any(|function| {
                    self.definition(DefinitionKind::Function, &function.name, definitions)
                        .ok()
                        == Some(*id)
                })
            })
        {
            return Err(fail());
        }
        for (index, implementation) in self.implementations.iter().enumerate() {
            if implementation.trait_type.is_none()
                && !self.owns_inherent_receiver(&implementation.for_type, receiver_owner)
            {
                return Err(DeclarationError(
                    "inherent methods belong to the receiver's defining module".into(),
                ));
            }
            let owner = self.implementation_id(index, definitions)?;
            if implementation
                .generic_params
                .iter()
                .enumerate()
                .any(|(position, parameter)| {
                    parameter.owner != owner || parameter.position != position
                })
            {
                return Err(fail());
            }
            let mut method_names = HashSet::new();
            if !native_bounds_valid(
                &implementation.bounds,
                &implementation.generic_params,
                &cancel,
                definitions,
            ) {
                return Err(fail());
            }
            for method in &implementation.methods {
                if !identifier(&method.name)
                    || !method_names.insert(&method.name)
                    || !method
                        .generic_params
                        .starts_with(&implementation.generic_params)
                    || method.generic_params[implementation.generic_params.len()..]
                        .iter()
                        .enumerate()
                        .any(|(position, parameter)| {
                            Self::method_id(owner, &method.name, definitions).ok()
                                != Some(parameter.owner)
                                || parameter.position != position
                        })
                    || !native_bounds_valid(
                        &method.bounds,
                        &method.generic_params,
                        &cancel,
                        definitions,
                    )
                {
                    return Err(fail());
                }
            }
        }
        let declarations = self.native_declarations(definitions)?;
        for required in declarations
            .iter()
            .flat_map(|declaration| &declaration.callable_requirements)
        {
            if definitions
                .resolve(required.interface.declaration)
                .map_err(|_| fail())?
                .module()
                == &self.identity
                && !self.traits.iter().any(|contract| {
                    self.definition(DefinitionKind::Trait, &contract.name, definitions)
                        .ok()
                        == Some(required.interface.declaration)
                        && contract.methods.iter().any(|method| {
                            Self::method_id(
                                required.interface.declaration,
                                &method.name,
                                definitions,
                            )
                            .ok()
                                == Some(required.member)
                                && method.generic_params.len() == required.arguments.len()
                        })
                })
            {
                return Err(fail());
            }
        }
        if self.callable_requirements.len() > 4096
            || self.concrete_results.len() > 4096
            || self
                .concrete_results
                .keys()
                .any(|id| !declarations.iter().any(|decl| &decl.declaration == id))
            || self
                .callable_requirements
                .keys()
                .any(|id| !declarations.iter().any(|decl| &decl.declaration == id))
        {
            return Err(fail());
        }
        if declarations.iter().any(|item| {
            !matches!(
                item.function.implementation,
                CallableImplementation::Native(_)
            ) || item
                .function
                .params
                .iter()
                .any(|param| !identifier(&param.name))
        }) {
            return Err(fail());
        }
        validate_native_declarations(&declarations, &self.identity, definitions, &cancel)
            .map_err(|error| DeclarationError(format!("invalid native declarations: {error:?}")))?;
        self.validate_implementations(definitions)?;
        self.validate_supported_types(definitions)?;
        Ok(())
    }

    fn validate_supported_types(
        &self,
        definitions: &DefinitionTable,
    ) -> Result<(), DeclarationError> {
        for ty in &self.types {
            for bound in &ty.bounds {
                supported_type(&bound.ty, definitions)?;
                for constraint in &bound.constraints {
                    supported_constraint(constraint, definitions)?;
                }
            }
        }
        let mut functions = self.functions.iter().collect::<Vec<_>>();
        for contract in &self.traits {
            functions.extend(&contract.methods);
            for parent in &contract.supertraits {
                supported_type(&Ty::Trait(parent.clone()), definitions)?;
            }
            for member in &contract.associated_types {
                for bound in &member.bounds {
                    supported_constraint(bound, definitions)?;
                }
            }
        }
        for implementation in &self.implementations {
            functions.extend(&implementation.methods);
            supported_type(&implementation.for_type, definitions)?;
            if let Some(trait_type) = &implementation.trait_type {
                supported_type(&Ty::Trait(trait_type.clone()), definitions)?;
            }
            for bound in &implementation.bounds {
                supported_type(&bound.ty, definitions)?;
                for constraint in &bound.constraints {
                    supported_constraint(constraint, definitions)?;
                }
            }
        }
        for function in functions {
            supported_type(&function.return_type, definitions)?;
            for parameter in &function.params {
                supported_type(&parameter.ty, definitions)?;
            }
            for bound in &function.bounds {
                supported_type(&bound.ty, definitions)?;
                for constraint in &bound.constraints {
                    supported_constraint(constraint, definitions)?;
                }
            }
        }
        Ok(())
    }
}

// Keep runtime registration independent of declaration text and source analysis.
fn supported_type(ty: &Ty, definitions: &DefinitionTable) -> Result<(), DeclarationError> {
    let invalid = || DeclarationError("unsupported type in initial native API".into());
    if !ty.within_wire_limits() {
        return Err(invalid());
    }
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            Ty::Builtin(_) | Ty::Parameter { .. } | Ty::SelfType(_) => {}
            Ty::Array(item) | Ty::Set(item, _) | Ty::Range(item, _) | Ty::Iter(item) => {
                pending.push(item)
            }
            Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            Ty::Tuple(items) => {
                pending.extend(items);
            }
            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            Ty::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => {
                if !arguments.is_empty()
                    || definitions
                        .resolve(*member)
                        .ok()
                        .and_then(|view| view.last())
                        .is_none_or(|part| !identifier(part.name))
                {
                    return Err(invalid());
                }
                pending.push(receiver);
                supported_type(&Ty::Trait(*interface.clone()), definitions)?;
            }
            Ty::Trait(nominal) => {
                if definitions
                    .resolve(nominal.declaration)
                    .ok()
                    .and_then(|view| view.last())
                    .is_none_or(|name| !identifier(name.name))
                    || nominal.associated_types.keys().any(|id| {
                        definitions
                            .resolve(*id)
                            .ok()
                            .and_then(|view| view.last())
                            .is_none_or(|name| !identifier(name.name))
                    })
                {
                    return Err(invalid());
                }
                pending.extend(&nominal.arguments);
                pending.extend(nominal.associated_types.values());
            }
            Ty::NativeObject(nominal) | Ty::Enum(nominal) => {
                if !nominal.associated_types.is_empty() {
                    return Err(invalid());
                }
                pending.extend(&nominal.arguments);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(())
}

fn supported_constraint(
    constraint: &Constraint,
    definitions: &DefinitionTable,
) -> Result<(), DeclarationError> {
    match constraint {
        Constraint::Trait(trait_type) => {
            supported_type(&Ty::Trait(trait_type.clone()), definitions)
        }
        Constraint::Standard(kind) if kind.source_bound_name().is_some() => Ok(()),
        _ => Err(DeclarationError(
            "native declaration has no source-level constraint".into(),
        )),
    }
}

fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

mod identity;
mod implementation;
mod mapping;
