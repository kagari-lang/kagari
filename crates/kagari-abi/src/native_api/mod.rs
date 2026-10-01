//! Authoritative native API declarations shared by compilation, installation and tooling.
use crate::{
    callable::CallableImplementation,
    types::{
        AbiType, ConstraintAbi, FunctionAbi, GenericParameterAbi, NativeDeclaration,
        NominalAbiType, PublicAbiItem, TraitAbi, TypeAbi, TypeAbiKind,
        native::NativeTypeConstructor,
        substitution::TypeSubstitution,
        verify::{validate, validate_native_declarations},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity},
};
use std::{
    collections::{BTreeMap, HashSet},
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
    pub trait_type: Option<NominalAbiType>,
    pub for_type: AbiType,
    pub methods: Vec<FunctionAbi>,
}

/// Rust registration definitions own these records. Generated text is a projection.
/// The initial API supports native storage, traits, generic impls and free functions;
/// Free functions retain ordinary checked trait bounds; associated declarations
/// and implementation-level bounds remain separate migration steps.
#[derive(Debug, Clone)]
pub struct NativeModule {
    pub identity: ModuleIdentity,
    pub types: Vec<TypeAbi>,
    pub traits: Vec<TraitAbi>,
    pub implementations: Vec<NativeImplementation>,
    pub functions: Vec<FunctionAbi>,
    pub documentation: BTreeMap<DefinitionId, String>,
}

impl NativeModule {
    pub fn new(identity: ModuleIdentity) -> Self {
        Self {
            identity,
            types: vec![],
            traits: vec![],
            implementations: vec![],
            functions: vec![],
            documentation: BTreeMap::new(),
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
        trait_type: NominalAbiType,
        for_type: AbiType,
        generic_params: Vec<GenericParameterAbi>,
        bindings: &[(&str, DefinitionId)],
    ) -> Result<(), NativeApiError> {
        let contract = self
            .traits
            .iter()
            .find(|item| {
                self.definition(DefinitionKind::Trait, &item.name) == trait_type.declaration
            })
            .ok_or_else(|| NativeApiError("trait is absent from this native module".into()))?;
        if trait_type.arguments.len() != contract.generic_params.len()
            || bindings.len() != contract.methods.len()
        {
            return Err(NativeApiError(
                "trait arguments or required methods differ".into(),
            ));
        }
        let mut names = HashSet::new();
        if bindings.iter().any(|(name, _)| !names.insert(*name)) {
            return Err(NativeApiError("duplicate method binding".into()));
        }
        let mut substitution =
            TypeSubstitution::for_owner(&trait_type.declaration, &trait_type.arguments);
        substitution.bind_receiver(&trait_type.declaration, &for_type);
        let cancel = CancellationToken::default();
        let mut methods = vec![];
        for method in &contract.methods {
            let binding = bindings
                .iter()
                .find(|(name, _)| *name == method.name)
                .ok_or_else(|| NativeApiError(format!("missing method {}", method.name)))?;
            let mut method = method.clone();
            method.implementation = CallableImplementation::Native(binding.1.clone());
            method.generic_params = generic_params.clone();
            method.params = method
                .params
                .into_iter()
                .map(|mut param| {
                    param.ty = substitution.apply(&param.ty, &cancel).map_err(|error| {
                        NativeApiError(format!("invalid substitution: {error:?}"))
                    })?;
                    Ok(param)
                })
                .collect::<Result<_, NativeApiError>>()?;
            method.return_type = substitution
                .apply(&method.return_type, &cancel)
                .map_err(|error| NativeApiError(format!("invalid substitution: {error:?}")))?;
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
                declaration: self.definition(DefinitionKind::Function, &function.name),
                function: function.clone(),
            })
            .collect::<Vec<_>>();
        for (index, implementation) in self.implementations.iter().enumerate() {
            let owner = self.implementation_id(index);
            result.extend(
                implementation
                    .methods
                    .iter()
                    .map(|function| NativeDeclaration {
                        declaration: Self::method_id(&owner, &function.name),
                        function: function.clone(),
                    }),
            );
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
                || !item.associated_types.is_empty()
                || !item.associated_consts.is_empty()
                || item.methods.iter().any(|method| {
                    !identifier(&method.name)
                        || !method.bounds.is_empty()
                        || !method.generic_params.is_empty()
                        || method.implementation != CallableImplementation::Required
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
            for method in &implementation.methods {
                if !identifier(&method.name)
                    || !method_names.insert(&method.name)
                    || method.generic_params != implementation.generic_params
                    || !method.bounds.is_empty()
                {
                    return Err(fail());
                }
            }
        }
        let declarations = self.native_declarations();
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
        }
        for implementation in &self.implementations {
            functions.extend(&implementation.methods);
            supported_type(&implementation.for_type)?;
            if let Some(trait_type) = &implementation.trait_type {
                supported_type(&AbiType::Trait(trait_type.clone()))?;
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
                    let ConstraintAbi::Trait(trait_type) = constraint else {
                        return Err(NativeApiError(
                            "native declarations require named trait constraints".into(),
                        ));
                    };
                    supported_type(&AbiType::Trait(trait_type.clone()))?;
                }
            }
        }
        Ok(())
    }

    fn validate_implementations(&self) -> Result<(), NativeApiError> {
        let invalid =
            || NativeApiError("trait implementation differs from its registered contract".into());
        for (index, implementation) in self.implementations.iter().enumerate() {
            let Some(trait_type) = &implementation.trait_type else {
                continue;
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
                trait_type.clone(),
                implementation.for_type.clone(),
                implementation.generic_params.clone(),
                &bindings,
            )?;
            if expected.implementations[index].methods != implementation.methods {
                return Err(invalid());
            }
            let contract = self
                .traits
                .iter()
                .find(|item| {
                    self.definition(DefinitionKind::Trait, &item.name) == trait_type.declaration
                })
                .ok_or_else(invalid)?;
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

fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
