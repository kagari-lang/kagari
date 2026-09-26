//! Declaration queries do not depend on the runtime or invoke host callbacks.
use kagari_common::host_interface::{
    HostFunctionDeclaration, HostInterface, HostInterfaceError, HostTraitImplementationDeclaration,
    HostTypeDeclaration, HostValueType,
};
use std::{collections::HashMap, sync::Arc};

use crate::types::{BuiltinType, NominalType, TypeId};
use kagari_common::{
    Diagnostic, DiagnosticKind, Span,
    cancellation::{CancellationToken, Cancelled},
    identity::ModuleIdentity,
};

#[cfg(test)]
mod facade_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod type_tests;

/// Scoped to one immutable declaration input, never a runtime binding slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostFunctionId {
    revision: u64,
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostModuleId {
    revision: u64,
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostTypeId {
    revision: u64,
    index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostSourcePathStep {
    Member(String),
    Index(TypeId),
}

#[derive(Debug)]
pub struct HostDeclarations {
    revision: u64,
    interface: HostInterface,
    paths: HashMap<String, HostFunctionId>,
    methods: HashMap<(kagari_common::identity::DefinitionId, String), HostFunctionId>,
    type_paths: HashMap<String, HostTypeId>,
    type_identities: HashMap<kagari_common::identity::DefinitionId, HostTypeId>,
    modules: Vec<String>,
}

impl HostDeclarations {
    pub(crate) fn type_declarations(&self) -> &[HostTypeDeclaration] {
        &self.interface.types
    }
    pub fn trait_type(implementation: &HostTraitImplementationDeclaration) -> NominalType {
        NominalType {
            declaration: implementation.trait_id.clone(),
            arguments: implementation
                .trait_arguments
                .iter()
                .map(signature_type)
                .collect(),
            associated_types: implementation
                .associated_types
                .iter()
                .map(|output| (output.declaration.clone(), signature_type(&output.ty)))
                .collect(),
        }
    }

    pub fn interface_implementation(
        &self,
        trait_type: &NominalType,
        receiver: &TypeId,
    ) -> Option<&HostTraitImplementationDeclaration> {
        let TypeId::Host(id) = receiver else {
            return None;
        };
        let host = self.type_declaration(self.nominal_type(id)?)?;
        host.trait_implementations
            .iter()
            .find(|implementation| Self::matches_trait_application(implementation, trait_type))
    }
    fn matches_trait_application(
        implementation: &HostTraitImplementationDeclaration,
        trait_type: &NominalType,
    ) -> bool {
        Self::trait_type(implementation).satisfies(trait_type)
    }

    pub fn implements(&self, trait_type: &NominalType, receiver: &TypeId) -> bool {
        let TypeId::Host(host_id) = receiver else {
            return false;
        };
        self.nominal_type(host_id)
            .and_then(|id| self.type_declaration(id))
            .is_some_and(|host| {
                host.trait_implementations.iter().any(|implementation| {
                    Self::matches_trait_application(implementation, trait_type)
                })
            })
    }

    pub fn trait_method_binding(
        &self,
        trait_method: &kagari_common::identity::DefinitionId,
        trait_type: &NominalType,
        receiver: &TypeId,
    ) -> Option<HostFunctionId> {
        let TypeId::Host(host_id) = receiver else {
            return None;
        };
        let host = self.type_declaration(self.nominal_type(host_id)?)?;
        let implementation = host
            .trait_implementations
            .iter()
            .find(|implementation| Self::matches_trait_application(implementation, trait_type))?;
        let binding = implementation
            .methods
            .iter()
            .find(|binding| &binding.trait_method == trait_method)?;
        let method_name = &host
            .methods
            .iter()
            .find(|method| method.id == binding.host_method)?
            .name;
        self.method(host_id, method_name)
    }

    /// Check host method tables only where the target script trait is defined.
    /// This keeps diagnostics owned by one source file and makes signature
    /// changes invalidate the result through the aggregate catalog.
    pub(crate) fn validate_trait_implementations(
        &self,
        aggregates: &crate::aggregates::AggregateCatalog,
        module: &ModuleIdentity,
        cancel: &CancellationToken,
    ) -> Result<crate::DiagnosticBuffer, Cancelled> {
        let mut diagnostics = crate::DiagnosticBuffer::new();
        for host in &self.interface.types {
            cancel.check()?;
            for implementation in &host.trait_implementations {
                cancel.check()?;
                let standard =
                    crate::builtin::traits::StandardTrait::from_id(&implementation.trait_id);
                if &implementation.trait_id.module != module && standard.is_none() {
                    continue;
                }
                let trait_signature = aggregates.trait_(&implementation.trait_id);
                let mut report = |reason: String| {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                            trait_name: implementation
                                .trait_id
                                .path
                                .last()
                                .map(|segment| segment.name.clone())
                                .unwrap_or_default(),
                            type_name: host.symbol.clone(),
                            reason,
                        })
                        .with_span(Span::default()),
                    );
                };
                if standard.is_some_and(|kind| kind.equality_protocol()) {
                    report(
                        "standard equality and hashing cannot be overridden by host bindings"
                            .into(),
                    );
                    continue;
                }
                let Some(trait_signature) = trait_signature else {
                    report("trait declaration is missing from its source module".into());
                    continue;
                };
                if !trait_signature.associated_consts.is_empty()
                    || !trait_signature.associated_type_parameters.is_empty()
                {
                    report("native host trait tables cannot supply associated constants or generic associated types; use a script impl".into());
                    continue;
                }
                if trait_signature.generic_params.len() != implementation.trait_arguments.len() {
                    report(format!(
                        "trait type argument count differs: expected {}, found {}",
                        trait_signature.generic_params.len(),
                        implementation.trait_arguments.len()
                    ));
                    continue;
                }
                let interface = Self::trait_type(implementation);
                if interface.associated_types.len() != trait_signature.associated_types.len()
                    || trait_signature
                        .associated_types
                        .keys()
                        .any(|member| !interface.associated_types.contains_key(member))
                {
                    report(
                        "associated type definitions must match the complete trait output schema"
                            .into(),
                    );
                    continue;
                }
                let substitution: crate::types::TypeSubstitution = trait_signature
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(implementation.trait_arguments.iter().map(signature_type))
                    .collect();
                let receiver = TypeId::Host(host.id.clone());
                for (member, constraints) in &trait_signature.associated_types {
                    let actual = &interface.associated_types[member];
                    for constraint in constraints {
                        let satisfied = match constraint {
                            crate::typeck::ConstraintTarget::Standard(standard) => {
                                crate::typeck::type_satisfies_standard_constraint(
                                    actual,
                                    *standard,
                                    &Default::default(),
                                )
                            }
                            crate::typeck::ConstraintTarget::Trait(required) => {
                                let TypeId::Trait(required) = TypeId::Trait(required.clone())
                                    .with_associated_types(&interface)
                                    .with_self(&trait_signature.id, &receiver)
                                    .instantiate(&substitution)
                                else {
                                    unreachable!("trait bound")
                                };
                                aggregates.implementation_count(&required, actual)
                                    + usize::from(self.implements(&required, actual))
                                    == 1
                            }
                        };
                        if !satisfied {
                            report(format!(
                                "associated type `{}` does not satisfy its bound",
                                member.path.last().unwrap().name
                            ));
                        }
                    }
                }
                for (parameter, argument) in trait_signature
                    .generic_params
                    .iter()
                    .zip(&implementation.trait_arguments)
                {
                    cancel.check()?;
                    let actual = signature_type(argument);
                    for constraint in trait_signature
                        .bounds
                        .get(&TypeId::Generic(parameter.clone()))
                        .into_iter()
                        .flatten()
                    {
                        let satisfied = match constraint {
                            crate::typeck::ConstraintTarget::Standard(standard) => {
                                satisfies_standard_constraint(argument, *standard)
                            }
                            crate::typeck::ConstraintTarget::Trait(required) => {
                                let TypeId::Trait(required) = TypeId::Trait(required.clone())
                                    .with_associated_types(&interface)
                                    .with_self(&trait_signature.id, &receiver)
                                    .instantiate(&substitution)
                                else {
                                    unreachable!("trait bound")
                                };
                                aggregates.implementation_count(&required, &actual)
                                    + usize::from(self.implements(&required, &actual))
                                    == 1
                            }
                        };
                        if !satisfied {
                            report(format!(
                                "trait type argument `{}` does not satisfy its bound",
                                actual.display_name()
                            ));
                        }
                    }
                }
                for method in &trait_signature.methods {
                    cancel.check()?;
                    let Some(binding) = implementation
                        .methods
                        .iter()
                        .find(|binding| binding.trait_method == method.id)
                    else {
                        report(format!("missing method `{}`", method.name));
                        continue;
                    };
                    let host_method = host
                        .methods
                        .iter()
                        .find(|candidate| candidate.id == binding.host_method)
                        .expect("validated host method binding");
                    if method.generic_params.len() != trait_signature.generic_params.len() {
                        report(format!(
                            "method `{}` cannot have generic parameters",
                            method.name
                        ));
                        continue;
                    }
                    let receiver = TypeId::Host(host.id.clone());
                    let expected = method
                        .params
                        .iter()
                        .map(|parameter| {
                            parameter
                                .ty
                                .with_associated_types(&interface)
                                .with_self(&trait_signature.id, &receiver)
                                .instantiate(&substitution)
                        })
                        .collect::<Vec<_>>();
                    if expected.first() != Some(&receiver)
                        || expected.len() != host_method.params.len() + 1
                    {
                        report(format!(
                            "method `{}` receiver or parameter count differs",
                            method.name
                        ));
                        continue;
                    }
                    for (index, (expected, actual)) in
                        expected[1..].iter().zip(&host_method.params).enumerate()
                    {
                        if *expected != signature_type(&actual.ty) {
                            report(format!(
                                "method `{}` parameter {} expected `{}`, found `{}`",
                                method.name,
                                index + 1,
                                expected.display_name(),
                                signature_type(&actual.ty).display_name()
                            ));
                        }
                    }
                    let expected_return = method
                        .return_type
                        .with_associated_types(&interface)
                        .with_self(&trait_signature.id, &receiver)
                        .instantiate(&substitution);
                    let actual_return = signature_type(&host_method.return_type);
                    if expected_return != actual_return {
                        report(format!(
                            "method `{}` return type expected `{}`, found `{}`",
                            method.name,
                            expected_return.display_name(),
                            actual_return.display_name()
                        ));
                    }
                }
                for binding in &implementation.methods {
                    cancel.check()?;
                    if !trait_signature
                        .methods
                        .iter()
                        .any(|method| method.id == binding.trait_method)
                    {
                        report(format!(
                            "extra trait method binding `{}`",
                            binding
                                .trait_method
                                .path
                                .last()
                                .map(|segment| segment.name.as_str())
                                .unwrap_or_default()
                        ));
                    }
                }
            }
        }
        Ok(diagnostics)
    }

    pub fn new(mut interface: HostInterface) -> Result<Arc<Self>, HostInterfaceError> {
        interface.validate()?;
        let mut present = interface
            .functions
            .iter()
            .map(|f| f.id.clone())
            .collect::<std::collections::HashSet<_>>();
        for owner in &interface.types {
            for method in &owner.methods {
                if present.insert(method.id.clone()) {
                    interface.functions.push(owner.method_contract(&method.id)?);
                }
            }
        }
        interface.validate()?;
        if interface
            .functions
            .iter()
            .map(|function| &function.symbol)
            .chain(interface.types.iter().map(|ty| &ty.symbol))
            .any(|symbol| {
                symbol.split('.').any(|segment| {
                    let mut chars = segment.chars();
                    !chars
                        .next()
                        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
                        || !chars.all(|ch| ch == '_' || ch.is_alphanumeric())
                }) || symbol.split('.').next() == Some("std")
            })
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let revision = NEXT
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |n| n.checked_add(1),
            )
            .expect("host declaration revision exhausted");
        let paths: HashMap<_, _> = interface
            .functions
            .iter()
            .enumerate()
            .filter(|(_, declaration)| declaration.method_owner().is_none())
            .map(|(index, declaration)| {
                (
                    declaration.symbol.replace('.', "::"),
                    HostFunctionId { revision, index },
                )
            })
            .collect();
        let methods = interface
            .functions
            .iter()
            .enumerate()
            .filter_map(|(index, declaration)| {
                Some((
                    (
                        declaration.method_owner()?,
                        declaration.id.path.last()?.name.clone(),
                    ),
                    HostFunctionId { revision, index },
                ))
            })
            .collect();
        let mut modules = std::collections::BTreeSet::new();
        let type_paths: HashMap<_, _> = interface
            .types
            .iter()
            .enumerate()
            .map(|(index, declaration)| {
                (
                    declaration.symbol.replace('.', "::"),
                    HostTypeId { revision, index },
                )
            })
            .collect();
        let type_identities = interface
            .types
            .iter()
            .enumerate()
            .map(|(index, declaration)| (declaration.id.clone(), HostTypeId { revision, index }))
            .collect();
        for symbol in interface
            .functions
            .iter()
            .filter(|function| function.method_owner().is_none())
            .map(|function| &function.symbol)
            .chain(interface.types.iter().map(|ty| &ty.symbol))
        {
            let path = symbol.replace('.', "::");
            for (offset, _) in path.match_indices("::") {
                modules.insert(path[..offset].to_owned());
            }
        }
        if modules
            .iter()
            .any(|module| paths.contains_key(module) || type_paths.contains_key(module))
        {
            return Err(HostInterfaceError::DuplicateDeclaration);
        }
        Ok(Arc::new(Self {
            revision,
            interface,
            paths,
            methods,
            type_paths,
            type_identities,
            modules: modules.into_iter().collect(),
        }))
    }

    pub fn empty() -> Arc<Self> {
        static EMPTY: std::sync::OnceLock<Arc<HostDeclarations>> = std::sync::OnceLock::new();
        EMPTY
            .get_or_init(|| Self::new(HostInterface::default()).unwrap())
            .clone()
    }

    pub fn resolve(&self, path: &str) -> Option<HostFunctionId> {
        self.paths.get(path).copied()
    }
    pub fn method(
        &self,
        owner: &kagari_common::identity::DefinitionId,
        name: &str,
    ) -> Option<HostFunctionId> {
        self.methods.get(&(owner.clone(), name.to_owned())).copied()
    }
    pub fn module(&self, path: &str) -> Option<HostModuleId> {
        self.modules
            .binary_search_by(|candidate| candidate.as_str().cmp(path))
            .ok()
            .map(|index| HostModuleId {
                revision: self.revision,
                index,
            })
    }
    pub(crate) fn members_of_module(
        &self,
        module: HostModuleId,
    ) -> Vec<(String, crate::resolver::ResolvedName)> {
        if module.revision != self.revision {
            return Vec::new();
        }
        let Some(path) = self.modules.get(module.index) else {
            return Vec::new();
        };
        let prefix = format!("{path}::");
        let mut members = std::collections::BTreeMap::new();
        for name in self
            .paths
            .keys()
            .chain(self.type_paths.keys())
            .chain(self.modules.iter())
        {
            if let Some(member) = name.strip_prefix(&prefix)
                && !member.contains("::")
                && let Some(resolved) = self.resolve_name(name).or_else(|| {
                    self.module(name)
                        .map(crate::resolver::ResolvedName::HostModule)
                })
            {
                members.insert(member.to_owned(), resolved);
            }
        }
        members.into_iter().collect()
    }
    pub fn function(&self, id: HostFunctionId) -> Option<&HostFunctionDeclaration> {
        (id.revision == self.revision)
            .then(|| self.interface.functions.get(id.index))
            .flatten()
    }
    pub(crate) fn resolve_name_in(
        &self,
        module: HostModuleId,
        path: &str,
    ) -> Option<crate::resolver::ResolvedName> {
        if module.revision != self.revision {
            return None;
        }
        self.resolve_name(&format!("{}::{path}", self.modules.get(module.index)?))
    }
    pub(crate) fn resolve_name(&self, path: &str) -> Option<crate::resolver::ResolvedName> {
        self.resolve(path)
            .map(crate::resolver::ResolvedName::HostFunction)
            .or_else(|| {
                self.resolve_type(path)
                    .map(crate::resolver::ResolvedName::HostType)
            })
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn source_path(
        &self,
        root: &kagari_common::identity::DefinitionId,
        steps: &[HostSourcePathStep],
    ) -> Result<
        (
            kagari_common::host_interface::HostPathDeclaration,
            kagari_common::host_interface::HostPathContract,
        ),
        &'static str,
    > {
        use kagari_common::host_interface::HostPathSegmentDeclaration as Segment;
        let mut matches = self.interface.paths.iter().filter(|path| {
            if &path.root != root || path.segments.len() != steps.len() {
                return false;
            }
            let mut source_slots = std::collections::HashSet::new();
            path.segments
                .iter()
                .zip(steps)
                .all(|(segment, source)| match (segment, source) {
                    (Segment::Field(id), HostSourcePathStep::Member(name)) => {
                        self.field(id).is_some_and(|field| field.name == *name)
                    }
                    (Segment::Virtual(virtual_step), HostSourcePathStep::Member(name)) => {
                        virtual_step.name == *name
                    }
                    (Segment::Index(index), HostSourcePathStep::Index(ty)) => {
                        source_slots.insert(index.slot) && signature_type(&index.index) == *ty
                    }
                    _ => false,
                })
        });
        let declaration = matches.next().ok_or("path has no declared host contract")?;
        if matches.next().is_some() {
            return Err("path has ambiguous host declarations");
        }
        let contract = declaration
            .contract(&self.interface)
            .map_err(|_| "invalid declared host path")?;
        Ok((declaration.clone(), contract))
    }

    pub fn resolve_type(&self, path: &str) -> Option<HostTypeId> {
        self.type_paths.get(path).copied()
    }
    pub fn nominal_type(
        &self,
        declaration: &kagari_common::identity::DefinitionId,
    ) -> Option<HostTypeId> {
        self.type_identities.get(declaration).copied()
    }
    pub fn type_declaration(&self, id: HostTypeId) -> Option<&HostTypeDeclaration> {
        (id.revision == self.revision)
            .then(|| self.interface.types.get(id.index))
            .flatten()
    }
    pub fn field(
        &self,
        id: &kagari_common::identity::DefinitionId,
    ) -> Option<&kagari_common::host_interface::HostFieldDeclaration> {
        let mut owner = id.clone();
        owner.path.pop()?;
        self.type_declaration(self.nominal_type(&owner)?)?
            .fields
            .iter()
            .find(|field| &field.id == id)
    }
}

pub(crate) fn signature_type(ty: &HostValueType) -> TypeId {
    match ty {
        HostValueType::Tuple(types) => TypeId::Tuple(types.iter().map(signature_type).collect()),
        HostValueType::Array(element) => TypeId::Array(Box::new(signature_type(element))),
        HostValueType::Map { key, value } => TypeId::Map {
            key: Box::new(signature_type(key)),
            value: Box::new(signature_type(value)),
        },
        HostValueType::Set(element) => TypeId::Set(Box::new(signature_type(element))),
        HostValueType::Option(element) => TypeId::StandardEnum {
            kind: crate::builtin::surface::StandardEnum::Option,
            args: vec![signature_type(element)],
        },
        HostValueType::Result { ok, error } => TypeId::StandardEnum {
            kind: crate::builtin::surface::StandardEnum::Result,
            args: vec![signature_type(ok), signature_type(error)],
        },
        HostValueType::Opaque(id) => TypeId::Host(id.clone()),
        scalar => TypeId::Builtin(match scalar {
            HostValueType::Unit => BuiltinType::Unit,
            HostValueType::Bool => BuiltinType::Bool,
            HostValueType::I32 => BuiltinType::I32,
            HostValueType::I64 => BuiltinType::I64,
            HostValueType::F32 => BuiltinType::F32,
            HostValueType::F64 => BuiltinType::F64,
            HostValueType::String => BuiltinType::String,
            _ => unreachable!("composite handled above"),
        }),
    }
}

/// Reuse the language's standard-bound rule for portable host type arguments.
pub fn satisfies_standard_constraint(
    ty: &HostValueType,
    constraint: crate::builtin::surface::StandardTypeConstraint,
) -> bool {
    crate::typeck::type_satisfies_standard_constraint(
        &signature_type(ty),
        constraint,
        &Default::default(),
    )
}
