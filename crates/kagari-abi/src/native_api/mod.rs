//! Authoritative native API declarations shared by compilation, installation and tooling.
use crate::{
    callable::CallableImplementation,
    native_import::callables::NativeCallableRequirement,
    types::{
        AbiType, ConstraintAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi,
        NativeDeclaration, NominalAbiType, PublicAbiItem, TraitAbi, TypeAbi, TypeAbiKind,
        native::NativeTypeConstructor,
        substitution::{TypeSubstitution, resolve_associated_outputs},
        verify::{native_bounds_valid, validate, validate_native_declarations},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    error::Error,
    fmt,
};

pub mod render;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeApiError(pub String);

impl fmt::Display for NativeApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native API: {}", self.0)
    }
}
impl Error for NativeApiError {}

#[derive(Debug, Clone)]
pub struct NativeImplementation {
    pub generic_params: Vec<GenericParameterAbi>,
    /// Obligations inherited by every method's registered callable template.
    pub bounds: Vec<GenericBoundAbi>,
    pub trait_type: Option<NominalAbiType>,
    pub for_type: AbiType,
    pub methods: Vec<FunctionAbi>,
}

/// Rust registration definitions own these records. Generated text is a projection.
/// The initial API supports native storage, traits, generic impls and free functions;
/// Free functions retain ordinary checked trait bounds and associated projections.
/// Impl and inherent-method bounds use the same checked declaration model.
/// Registered default applications retain their explicit native template mappings.
/// Associated type families and method generics remain later steps.
#[derive(Debug, Clone)]
pub struct NativeModule {
    pub identity: ModuleIdentity,
    pub types: Vec<TypeAbi>,
    pub traits: Vec<TraitAbi>,
    pub implementations: Vec<NativeImplementation>,
    pub functions: Vec<FunctionAbi>,
    /// Registered templates remain addressable by identity without public exports.
    pub private_functions: BTreeSet<DefinitionId>,
    pub documentation: BTreeMap<DefinitionId, String>,
    /// Ordered callback slots owned by each native declaration. The compiler
    /// specializes these requirements; generated source does not select targets.
    pub callable_requirements: BTreeMap<DefinitionId, Vec<NativeCallableRequirement>>,
}

impl NativeModule {
    pub fn new(identity: ModuleIdentity) -> Self {
        Self {
            identity,
            types: vec![],
            traits: vec![],
            implementations: vec![],
            functions: vec![],
            private_functions: BTreeSet::new(),
            documentation: BTreeMap::new(),
            callable_requirements: BTreeMap::new(),
        }
    }

    pub fn definition(&self, kind: DefinitionKind, name: &str) -> DefinitionId {
        DefinitionId {
            module: self.identity.clone(),
            path: vec![DefinitionPathSegment {
                kind,
                name: name.into(),
                occurrence: 0,
            }],
        }
    }

    pub fn implementation_id(&self, index: usize) -> DefinitionId {
        let mut id = self.definition(DefinitionKind::Impl, "");
        id.path[0].occurrence =
            u32::try_from(index).expect("native implementation slot exceeds identity range");
        id
    }

    pub fn method_id(owner: &DefinitionId, name: &str) -> DefinitionId {
        let mut id = owner.clone();
        id.path.push(DefinitionPathSegment {
            kind: DefinitionKind::Method,
            name: name.into(),
            occurrence: 0,
        });
        id
    }

    /// Bind implementations to an existing trait contract without repeating signatures.
    pub fn implement_trait(
        &mut self,
        contract: &TraitAbi,
        trait_type: NominalAbiType,
        for_type: AbiType,
        generic_params: Vec<GenericParameterAbi>,
        bindings: &[(&str, DefinitionId)],
    ) -> Result<(), NativeApiError> {
        let owner = NativeModule::new(trait_type.declaration.module.clone())
            .definition(DefinitionKind::Trait, &contract.name);
        if owner != trait_type.declaration {
            return Err(NativeApiError(
                "native trait contract has a different owner".into(),
            ));
        }
        validate(
            &[PublicAbiItem::Trait(contract.clone())],
            &owner.module,
            &CancellationToken::default(),
        )
        .map_err(|_| NativeApiError("invalid native trait contract".into()))?;
        if trait_type.arguments.len() != contract.generic_params.len()
            || trait_type.associated_types.len() != contract.associated_types.len()
            || contract.associated_types.iter().any(|member| {
                !trait_type
                    .associated_types
                    .contains_key(&member.declaration)
            })
        {
            return Err(NativeApiError(
                "trait arguments or required methods differ".into(),
            ));
        }
        let mut names = HashSet::new();
        if bindings.iter().any(|(name, _)| !names.insert(*name)) {
            return Err(NativeApiError("duplicate method binding".into()));
        }
        if bindings
            .iter()
            .any(|(name, _)| !contract.methods.iter().any(|method| method.name == *name))
        {
            return Err(NativeApiError("unknown trait method binding".into()));
        }
        let mut substitution =
            TypeSubstitution::for_owner(&trait_type.declaration, &trait_type.arguments);
        substitution.bind_receiver(&trait_type.declaration, &for_type);
        let cancel = CancellationToken::default();
        let resolve = |ty: &AbiType| {
            let ty = substitution
                .apply(ty, &cancel)
                .map_err(|error| NativeApiError(format!("invalid substitution: {error:?}")))?;
            resolve_associated_outputs(&ty, &trait_type, &cancel)
                .map_err(|error| NativeApiError(format!("invalid associated output: {error:?}")))
        };
        let mut methods = vec![];
        for method in &contract.methods {
            let binding = bindings.iter().find(|(name, _)| *name == method.name);
            let Some(binding) = binding else {
                if matches!(
                    method.implementation,
                    CallableImplementation::NativeDefault(_)
                ) {
                    continue;
                }
                return Err(NativeApiError(format!("missing method {}", method.name)));
            };
            if !method.method_policy.override_allowed {
                return Err(NativeApiError(format!(
                    "method {} forbids overriding",
                    method.name
                )));
            }
            let mut method = method.clone();
            method.implementation = CallableImplementation::Native(binding.1.clone());
            method.generic_params = generic_params.clone();
            method.params = method
                .params
                .into_iter()
                .map(|mut param| {
                    param.ty = resolve(&param.ty)?;
                    Ok(param)
                })
                .collect::<Result<_, NativeApiError>>()?;
            method.return_type = resolve(&method.return_type)?;
            methods.push(method);
        }
        let owner = self.implementation_id(self.implementations.len());
        let inherited_docs = contract
            .methods
            .iter()
            .filter_map(|method| {
                self.documentation
                    .get(&Self::method_id(&trait_type.declaration, &method.name))
                    .map(|doc| (Self::method_id(&owner, &method.name), doc.clone()))
            })
            .collect::<Vec<_>>();
        for (id, doc) in inherited_docs {
            self.documentation.entry(id).or_insert(doc);
        }
        self.implementations.push(NativeImplementation {
            bounds: vec![],
            generic_params,
            trait_type: Some(trait_type),
            for_type,
            methods,
        });
        Ok(())
    }

    pub fn native_declarations(&self) -> Vec<NativeDeclaration> {
        let mut result = self
            .functions
            .iter()
            .map(|function| NativeDeclaration {
                callable_requirements: self
                    .callable_requirements
                    .get(&self.definition(DefinitionKind::Function, &function.name))
                    .cloned()
                    .unwrap_or_default(),
                declaration: self.definition(DefinitionKind::Function, &function.name),
                function: function.clone(),
            })
            .collect::<Vec<_>>();
        for (index, implementation) in self.implementations.iter().enumerate() {
            let owner = self.implementation_id(index);
            result.extend(implementation.methods.iter().map(|function| {
                let mut function = function.clone();
                let mut bounds = BTreeMap::<AbiType, BTreeSet<ConstraintAbi>>::new();
                for bound in implementation.bounds.iter().chain(&function.bounds) {
                    bounds
                        .entry(bound.ty.clone())
                        .or_default()
                        .extend(bound.constraints.clone());
                }
                function.bounds = bounds
                    .into_iter()
                    .map(|(ty, constraints)| GenericBoundAbi {
                        ty,
                        constraints: constraints.into_iter().collect(),
                    })
                    .collect();
                NativeDeclaration {
                    callable_requirements: self
                        .callable_requirements
                        .get(&Self::method_id(&owner, &function.name))
                        .cloned()
                        .unwrap_or_default(),
                    declaration: Self::method_id(&owner, &function.name),
                    function,
                }
            }));
        }
        result
    }

    pub fn validate(&self) -> Result<(), NativeApiError> {
        let fail = || NativeApiError("invalid or unsupported native declaration".into());
        if self.identity.package.0.is_empty()
            || self.identity.path.is_empty()
            || self.identity.path.iter().any(|name| !identifier(name))
            || self.types.len()
                + self.traits.len()
                + self.functions.len()
                + self.implementations.len()
                > 4096
        {
            return Err(fail());
        }
        let mut names = HashSet::new();
        let mut items = vec![];
        for ty in &self.types {
            if !identifier(&ty.name)
                || !names.insert(&ty.name)
                || !ty.bounds.is_empty()
                || ty.kind != TypeAbiKind::Native(NativeTypeConstructor::Array)
            {
                return Err(fail());
            }
            items.push(PublicAbiItem::Type(ty.clone()));
        }
        for item in &self.traits {
            if !identifier(&item.name)
                || !names.insert(&item.name)
                || !item.bounds.is_empty()
                || item.associated_types.iter().any(|member| {
                    member
                        .declaration
                        .path
                        .last()
                        .is_none_or(|part| !identifier(&part.name))
                        || !member.generic_params.is_empty()
                        || !member.parameter_bounds.is_empty()
                })
                || !item.associated_consts.is_empty()
                || item.methods.iter().any(|method| {
                    !identifier(&method.name)
                        || !method.bounds.is_empty()
                        || !method.generic_params.is_empty()
                        || !matches!(
                            method.implementation,
                            CallableImplementation::Required
                                | CallableImplementation::NativeDefault(_)
                        )
                })
            {
                return Err(fail());
            }
            items.push(PublicAbiItem::Trait(item.clone()));
        }
        for function in &self.functions {
            if !identifier(&function.name) || !names.insert(&function.name) {
                return Err(fail());
            }
            items.push(PublicAbiItem::Function(function.clone()));
        }
        if self.private_functions.len() > self.functions.len()
            || self.private_functions.iter().any(|id| {
                !self.functions.iter().any(|function| {
                    self.definition(DefinitionKind::Function, &function.name) == *id
                })
            })
        {
            return Err(fail());
        }
        for (index, implementation) in self.implementations.iter().enumerate() {
            let owner = self.implementation_id(index);
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
                &CancellationToken::default(),
            ) {
                return Err(fail());
            }
            for method in &implementation.methods {
                if !identifier(&method.name)
                    || !method_names.insert(&method.name)
                    || method.generic_params != implementation.generic_params
                    || (implementation.trait_type.is_some() && !method.bounds.is_empty())
                    || !native_bounds_valid(
                        &method.bounds,
                        &method.generic_params,
                        &CancellationToken::default(),
                    )
                {
                    return Err(fail());
                }
            }
        }
        let declarations = self.native_declarations();
        for required in declarations
            .iter()
            .flat_map(|declaration| &declaration.callable_requirements)
        {
            if required.interface.declaration.module == self.identity
                && !self.traits.iter().any(|contract| {
                    self.definition(DefinitionKind::Trait, &contract.name)
                        == required.interface.declaration
                        && contract.methods.iter().any(|method| {
                            Self::method_id(&required.interface.declaration, &method.name)
                                == required.member
                                && method.generic_params.len() == required.arguments.len()
                        })
                })
            {
                return Err(fail());
            }
        }
        if self.callable_requirements.len() > 4096
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
        let cancel = CancellationToken::default();
        validate(&items, &self.identity, &cancel).map_err(|_| fail())?;
        validate_native_declarations(&declarations, &self.identity, &cancel).map_err(|_| fail())?;
        self.validate_implementations()?;
        self.validate_supported_types()?;
        Ok(())
    }

    fn validate_supported_types(&self) -> Result<(), NativeApiError> {
        let mut functions = self.functions.iter().collect::<Vec<_>>();
        for contract in &self.traits {
            functions.extend(&contract.methods);
            for parent in &contract.supertraits {
                supported_type(&AbiType::Trait(parent.clone()))?;
            }
            for member in &contract.associated_types {
                for bound in &member.bounds {
                    supported_constraint(bound)?;
                }
            }
        }
        for implementation in &self.implementations {
            functions.extend(&implementation.methods);
            supported_type(&implementation.for_type)?;
            if let Some(trait_type) = &implementation.trait_type {
                supported_type(&AbiType::Trait(trait_type.clone()))?;
            }
            for bound in &implementation.bounds {
                supported_type(&bound.ty)?;
                for constraint in &bound.constraints {
                    supported_constraint(constraint)?;
                }
            }
        }
        for function in functions {
            supported_type(&function.return_type)?;
            for parameter in &function.params {
                supported_type(&parameter.ty)?;
            }
            for bound in &function.bounds {
                supported_type(&bound.ty)?;
                for constraint in &bound.constraints {
                    supported_constraint(constraint)?;
                }
            }
        }
        Ok(())
    }

    fn validate_implementations(&self) -> Result<(), NativeApiError> {
        let contracts = self
            .traits
            .iter()
            .map(|contract| {
                (
                    self.definition(DefinitionKind::Trait, &contract.name),
                    contract.clone(),
                )
            })
            .collect();
        self.check_implementations(&contracts, false)
    }

    /// Validate method contracts against a complete installed native catalog.
    /// Generic applicability and parent witnesses retain their ordinary checked
    /// HIR/portable proofs; this check does not infer an implementation body.
    pub fn validate_trait_implementations(
        &self,
        contracts: &BTreeMap<DefinitionId, TraitAbi>,
    ) -> Result<(), NativeApiError> {
        self.check_implementations(contracts, true)
    }

    fn check_implementations(
        &self,
        contracts: &BTreeMap<DefinitionId, TraitAbi>,
        require_external: bool,
    ) -> Result<(), NativeApiError> {
        let invalid =
            || NativeApiError("trait implementation differs from its registered contract".into());
        for (index, implementation) in self.implementations.iter().enumerate() {
            let Some(trait_type) = &implementation.trait_type else {
                continue;
            };
            let Some(contract) = contracts.get(&trait_type.declaration) else {
                if !require_external && trait_type.declaration.module != self.identity {
                    continue;
                }
                return Err(invalid());
            };
            let bindings = implementation
                .methods
                .iter()
                .map(|method| {
                    let CallableImplementation::Native(binding) = &method.implementation else {
                        return Err(invalid());
                    };
                    Ok((method.name.as_str(), binding.clone()))
                })
                .collect::<Result<Vec<_>, NativeApiError>>()?;
            let mut expected = self.clone();
            expected.implementations.truncate(index);
            expected.implement_trait(
                contract,
                trait_type.clone(),
                implementation.for_type.clone(),
                implementation.generic_params.clone(),
                &bindings,
            )?;
            if expected.implementations[index].methods != implementation.methods {
                return Err(invalid());
            }
            let substitution =
                TypeSubstitution::for_owner(&trait_type.declaration, &trait_type.arguments);
            for parent in &contract.supertraits {
                if parent.declaration.module != self.identity {
                    continue;
                }
                let parent = substitution
                    .apply_nominal(parent, &CancellationToken::default())
                    .map_err(|_| invalid())?;
                let required =
                    self.normalized(&AbiType::Trait(parent), &implementation.generic_params)?;
                let target =
                    self.normalized(&implementation.for_type, &implementation.generic_params)?;
                let mut found = false;
                for candidate in &self.implementations {
                    let Some(candidate_trait) = &candidate.trait_type else {
                        continue;
                    };
                    if self.normalized(
                        &AbiType::Trait(candidate_trait.clone()),
                        &candidate.generic_params,
                    )? == required
                        && self.normalized(&candidate.for_type, &candidate.generic_params)?
                            == target
                    {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(NativeApiError(
                        "missing native supertrait implementation".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn normalized(
        &self,
        ty: &AbiType,
        parameters: &[GenericParameterAbi],
    ) -> Result<AbiType, NativeApiError> {
        let owner = self.implementation_id(0);
        let arguments = parameters
            .iter()
            .enumerate()
            .map(|(position, _)| AbiType::Parameter {
                owner: owner.clone(),
                position,
            })
            .collect::<Vec<_>>();
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in parameters.iter().zip(&arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        substitution
            .apply(ty, &CancellationToken::default())
            .map_err(|error| NativeApiError(format!("invalid generic template: {error:?}")))
    }
}

// Keep runtime registration independent of declaration text and source analysis.
fn supported_type(ty: &AbiType) -> Result<(), NativeApiError> {
    let invalid = || NativeApiError("unsupported type in initial native API".into());
    if !ty.within_wire_limits() {
        return Err(invalid());
    }
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            AbiType::Builtin(_) | AbiType::Parameter { .. } | AbiType::SelfType(_) => {}
            AbiType::Array(item, _) => pending.push(item),
            AbiType::Tuple(items) | AbiType::StandardEnum { args: items, .. } => {
                pending.extend(items);
            }
            AbiType::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            AbiType::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => {
                if !arguments.is_empty()
                    || member
                        .path
                        .last()
                        .is_none_or(|part| !identifier(&part.name))
                {
                    return Err(invalid());
                }
                pending.push(receiver);
                supported_type(&AbiType::Trait(*interface.clone()))?;
            }
            AbiType::Trait(nominal) => {
                if nominal
                    .declaration
                    .path
                    .last()
                    .is_none_or(|name| !identifier(&name.name))
                    || nominal
                        .associated_types
                        .keys()
                        .any(|id| id.path.last().is_none_or(|name| !identifier(&name.name)))
                {
                    return Err(invalid());
                }
                pending.extend(&nominal.arguments);
                pending.extend(nominal.associated_types.values());
            }
            _ => return Err(invalid()),
        }
    }
    Ok(())
}

fn supported_constraint(constraint: &ConstraintAbi) -> Result<(), NativeApiError> {
    let ConstraintAbi::Trait(trait_type) = constraint else {
        return Err(NativeApiError(
            "native declarations require named trait constraints".into(),
        ));
    };
    supported_type(&AbiType::Trait(trait_type.clone()))
}

fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
