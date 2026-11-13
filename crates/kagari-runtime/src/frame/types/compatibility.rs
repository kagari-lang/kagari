//! Compare complete reified layout graphs while preserving lexical type scopes.
use crate::{
    error::RuntimeError,
    frame::types::{arguments::type_parameter, bindings::TypeBindings},
    host::HostRegistryId,
    module::{ExecutableModule, InstallationId, ModuleKey, dispatch::identity::PublicationMapping},
};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_types::ty::{NominalTy, Ty, substitution::TypeSubstitution};
use std::{borrow::Cow, collections::HashSet, ptr};

#[derive(Clone, Copy)]
pub(crate) struct TypeView<'a> {
    pub(crate) ty: &'a Ty,
    pub(crate) owner: &'a ExecutableModule,
    pub(crate) environment: Option<&'a TypeBindings>,
    application: Option<&'a Application<'a>>,
    prepared: Option<&'a Ty>,
    closed_scope: Option<&'a DefinitionTable>,
}

struct Application<'a> {
    declaration: DefinitionId,
    arguments: Vec<TypeView<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TypeIdentity {
    ty: Ty,
    owners: Vec<(HostRegistryId, InstallationId, ModuleKey)>,
}

impl TypeIdentity {
    /// Normalize a lookup key only; lexical value/layout scopes are not rewritten.
    pub(crate) fn transported(&self, mapping: &PublicationMapping) -> Result<Self, RuntimeError> {
        Ok(Self {
            ty: mapping.remap_closed(&self.ty, &Default::default())?,
            owners: self.owners.clone(),
        })
    }

    /// A checked publication bridge may compare frozen contracts across epochs,
    /// but never across runtime/installation boundaries (including script traits).
    pub(crate) fn same_contract(&self, other: &Self) -> bool {
        self.ty == other.ty
            && self.owners.len() == other.owners.len()
            && self
                .owners
                .iter()
                .zip(&other.owners)
                .all(|(a, b)| a.0 == b.0 && a.1 == b.1)
    }
}

impl<'a> TypeView<'a> {
    pub(crate) fn new(
        ty: &'a Ty,
        owner: &'a ExecutableModule,
        environment: Option<&'a TypeBindings>,
    ) -> Self {
        Self {
            ty,
            owner,
            environment,
            application: None,
            prepared: None,
            closed_scope: None,
        }
    }

    /// Carry the closed result already checked by a type argument or storage
    /// contract. Keep the source expression and its lexical provenance for layout
    /// comparison; a closed spelling alone does not identify a nominal generation.
    pub(crate) fn prepared(
        expression: &'a Ty,
        closed: &'a Ty,
        owner: &'a ExecutableModule,
        environment: Option<&'a TypeBindings>,
    ) -> Self {
        debug_assert!(closed.is_concrete());
        Self {
            prepared: Some(closed),
            ..Self::new(expression, owner, environment)
        }
    }

    pub(crate) fn parameter(self, index: usize) -> Option<Self> {
        Some(Self {
            ty: type_parameter(self.ty, index)?,
            prepared: self.prepared.and_then(|ty| type_parameter(ty, index)),
            ..self
        })
    }

    pub(crate) fn with_closed_scope(self, definitions: &'a DefinitionTable) -> Self {
        Self {
            closed_scope: Some(definitions),
            ..self
        }
    }

    /// Select a resolved tree without discarding its lexical nominal provenance.
    pub(crate) fn resolved<'tree>(self, ty: &'tree Ty) -> TypeView<'tree>
    where
        'a: 'tree,
    {
        TypeView {
            ty,
            prepared: Some(ty),
            owner: self.owner,
            environment: self.environment,
            application: self.application,
            closed_scope: self.closed_scope,
        }
    }

    fn closed_definitions(self) -> &'a DefinitionTable {
        let mut table = self.owner.identity_admission().target();
        if let Some(scope) = self.closed_scope
            && scope.id() == table.id()
            && scope.len() > table.len()
        {
            table = scope;
        }
        if let Some(environment) = self.environment {
            table = environment.closed_definitions(table);
        }
        if let Some(application) = self.application {
            for argument in &application.arguments {
                let candidate = argument.closed_definitions();
                if candidate.id() == table.id() && candidate.len() > table.len() {
                    table = candidate;
                }
            }
        }
        table
    }

    fn canonical(self, ty: Cow<'a, Ty>) -> Option<Cow<'a, Ty>> {
        let admission = self.owner.identity_admission();
        if admission.source().id() == admission.target().id()
            || matches!(ty.as_ref(), Ty::Builtin(_))
        {
            return Some(ty);
        }
        admission
            .runtime_in(ty.as_ref(), self.closed_definitions(), &Default::default())
            .ok()
            .map(Cow::Owned)
    }

    fn children(self, other: Self, mut test: impl FnMut(&'a Ty, &'a Ty) -> bool) -> bool {
        let pair = match (self.ty, other.ty) {
            (Ty::Struct(a), Ty::Struct(b))
            | (Ty::Enum(a), Ty::Enum(b))
            | (Ty::Trait(a), Ty::Trait(b))
            | (Ty::NativeObject(a), Ty::NativeObject(b)) => Some((a, b)),
            _ => None,
        };
        let Some((a, b)) = pair else {
            return children_match(self.ty, other.ty, test);
        };
        if a.arguments.len() != b.arguments.len()
            || a.associated_types.len() != b.associated_types.len()
            || !a
                .arguments
                .iter()
                .zip(&b.arguments)
                .all(|(a, b)| test(a, b))
        {
            return false;
        }
        let ordered = |view: Self, nominal: &'a NominalTy| {
            let mut outputs = nominal
                .associated_types
                .iter()
                .map(|(id, ty)| {
                    let id = view
                        .owner
                        .identity_admission()
                        .runtime_id(*id)
                        .ok()
                        .or_else(|| view.closed_definitions().resolve(*id).ok().map(|_| *id))?;
                    Some((id, ty))
                })
                .collect::<Option<Vec<_>>>()?;
            outputs.sort_by_key(|(id, _)| *id);
            Some(outputs)
        };
        let (Some(a), Some(b)) = (ordered(self, a), ordered(other, b)) else {
            return false;
        };
        a.into_iter()
            .zip(b)
            .all(|((a_id, a), (b_id, b))| a_id == b_id && test(a, b))
    }

    pub(crate) fn reference_target(self) -> Option<Self> {
        let view = self.normalized()?;
        let Ty::Reference { target, .. } = view.ty else {
            return None;
        };
        Some(Self {
            ty: target,
            prepared: view.prepared.and_then(|ty| match ty {
                Ty::Reference { target, .. } => Some(target.as_ref()),
                _ => None,
            }),
            ..view
        })
    }

    pub(crate) fn function_parameter(self, index: usize) -> Option<Self> {
        let view = self.normalized()?;
        let Ty::Function { params, .. } = view.ty else {
            return None;
        };
        Some(Self {
            ty: params.get(index)?,
            prepared: view.prepared.and_then(|ty| match ty {
                Ty::Function { params, .. } => params.get(index),
                _ => None,
            }),
            ..view
        })
    }

    pub(crate) fn function_result(self) -> Option<Self> {
        let view = self.normalized()?;
        let Ty::Function { result, .. } = view.ty else {
            return None;
        };
        Some(Self {
            ty: result,
            prepared: view.prepared.and_then(|ty| match ty {
                Ty::Function { result, .. } => Some(result.as_ref()),
                _ => None,
            }),
            ..view
        })
    }

    fn child(self, ty: &'a Ty) -> Self {
        // An arbitrary lexical child has no parallel closed operand. Inherit
        // evidence only when the source itself is the checked closed tree.
        let prepared = self
            .prepared
            .filter(|closed| ptr::eq(*closed, self.ty))
            .map(|_| ty);
        Self {
            ty,
            prepared,
            ..self
        }
    }

    pub(crate) fn normalized(mut self) -> Option<Self> {
        loop {
            match self.ty {
                Ty::Parameter { owner, position } => {
                    self = if let Some(application) = self.application
                        && application.declaration == *owner
                    {
                        *application.arguments.get(*position)?
                    } else {
                        self.environment?
                            .argument(owner, *position)?
                            .view(self.owner)
                    };
                }
                Ty::Projection {
                    receiver,
                    interface,
                    member,
                    arguments,
                } if arguments.is_empty() => {
                    let environment = self.environment?;
                    let receiver = environment.resolve(receiver).ok()?;
                    let Ty::Trait(interface) = environment
                        .resolve(&Ty::Trait(interface.as_ref().clone()))
                        .ok()?
                    else {
                        return None;
                    };
                    self = environment.associated_output(&receiver, &interface, *member)?;
                }
                _ => break,
            }
        }
        Some(self)
    }

    pub(crate) fn closed(self) -> Option<Cow<'a, Ty>> {
        if let Some(ty) = self.prepared {
            return Some(Cow::Borrowed(ty));
        }
        let view = self.normalized()?;
        if let Some(ty) = view.prepared {
            return Some(Cow::Borrowed(ty));
        }
        if view.ty.is_concrete() {
            return view.canonical(Cow::Borrowed(view.ty));
        }
        if let Some(application) = view.application {
            let arguments = application
                .arguments
                .iter()
                .map(|ty| ty.closed().map(Cow::into_owned))
                .collect::<Option<Vec<_>>>()?;
            let ty = TypeSubstitution::for_owner(&application.declaration, &arguments)
                .apply(view.ty, &Default::default())
                .ok()?;
            return (ty.is_concrete() && ty.within_wire_limits())
                .then_some(ty)
                .and_then(|ty| view.canonical(Cow::Owned(ty)));
        }
        view.environment?
            .resolve(view.ty)
            .ok()
            .and_then(|ty| view.canonical(Cow::Owned(ty)))
    }

    pub(crate) fn identity(self) -> Option<TypeIdentity> {
        let mut owners = vec![];
        self.collect_owners(&mut owners).then_some(())?;
        Some(TypeIdentity {
            ty: self.closed()?.into_owned(),
            owners,
        })
    }

    fn installation_nominal(self) -> bool {
        match self.ty {
            Ty::Struct(_) | Ty::Enum(_) => true,
            Ty::Trait(nominal) => !self.owner.is_shared_native_trait(nominal.declaration),
            _ => false,
        }
    }

    fn collect_owners(self, owners: &mut Vec<(HostRegistryId, InstallationId, ModuleKey)>) -> bool {
        let Some(view) = self.normalized() else {
            return false;
        };
        if view.installation_nominal() {
            let (registry, publication) = view.owner.program_identity();
            owners.push((registry, view.owner.installation_id(), publication));
        }
        view.children(view, |a, _| view.child(a).collect_owners(owners))
    }

    /// Only script nominal nodes carry installation authority; lexical parameters
    /// are followed before checking, including those nested in shared native types.
    pub(crate) fn belongs_to_installation(self, owner: &ExecutableModule) -> bool {
        let Some(view) = self.normalized() else {
            return false;
        };
        if view.installation_nominal()
            && (view.owner.program_identity().0 != owner.program_identity().0
                || view.owner.installation_id() != owner.installation_id())
        {
            return false;
        }
        view.children(view, |a, _| view.child(a).belongs_to_installation(owner))
    }

    pub(crate) fn compatible(self, other: Self) -> bool {
        self.compare(other, &mut HashSet::new())
    }

    fn compare(self, other: Self, visited: &mut HashSet<(TypeIdentity, TypeIdentity)>) -> bool {
        let (Some(left), Some(right)) = (self.normalized(), other.normalized()) else {
            return false;
        };
        let (Some(a), Some(b)) = (left.closed(), right.closed()) else {
            return false;
        };
        if a != b {
            return false;
        }
        if left.installation_nominal() != right.installation_nominal()
            || (left.installation_nominal()
                && (left.owner.program_identity().0 != right.owner.program_identity().0
                    || left.owner.installation_id() != right.owner.installation_id()))
        {
            return false;
        }
        if left.owner.program_identity() == right.owner.program_identity()
            && left.ty == right.ty
            && match (left.environment, right.environment) {
                (None, None) => true,
                (Some(a), Some(b)) => ptr::eq(a, b),
                _ => false,
            }
            && match (left.application, right.application) {
                (None, None) => true,
                (Some(a), Some(b)) => ptr::eq(a, b),
                _ => false,
            }
        {
            return true;
        }
        if matches!(a.as_ref(), Ty::Struct(_) | Ty::Enum(_)) {
            let (Some(a_key), Some(b_key)) = (left.identity(), right.identity()) else {
                return false;
            };
            if !visited.insert((a_key, b_key)) {
                return true;
            }
            if !left.compare_fields(right, &a, visited) {
                return false;
            }
        }
        left.children(right, |a, b| left.child(a).compare(right.child(b), visited))
    }

    fn application(self) -> Option<Application<'a>> {
        let nominal = nominal(self.ty)?;
        Some(Application {
            declaration: self
                .owner
                .identity_admission()
                .lexical_id(nominal.declaration)?,
            arguments: nominal.arguments.iter().map(|ty| self.child(ty)).collect(),
        })
    }

    fn compare_fields(
        self,
        other: Self,
        closed: &Ty,
        visited: &mut HashSet<(TypeIdentity, TypeIdentity)>,
    ) -> bool {
        let (Some(left), Some(right)) = (self.application(), other.application()) else {
            return false;
        };
        let mut compare =
            |a: &Ty, a_owner: &ExecutableModule, b: &Ty, b_owner: &ExecutableModule| {
                TypeView {
                    ty: a,
                    owner: a_owner,
                    environment: None,
                    application: Some(&left),
                    prepared: None,
                    closed_scope: None,
                }
                .compare(
                    TypeView {
                        ty: b,
                        owner: b_owner,
                        environment: None,
                        application: Some(&right),
                        prepared: None,
                        closed_scope: None,
                    },
                    visited,
                )
            };
        match closed {
            Ty::Struct(ty) => {
                let (Some(a), Some(b)) = (
                    self.owner.find_struct_layout(ty),
                    other.owner.find_struct_layout(ty),
                ) else {
                    return false;
                };
                a.module()
                    .identity_admission()
                    .runtime_in(a.layout(), self.closed_definitions(), &Default::default())
                    .ok()
                    .zip(
                        b.module()
                            .identity_admission()
                            .runtime_in(b.layout(), other.closed_definitions(), &Default::default())
                            .ok(),
                    )
                    .is_some_and(|(a, b)| a == b)
                    && a.template().fields.iter().zip(&b.template().fields).all(
                        |(a_field, b_field)| {
                            compare(&a_field.ty, a.module(), &b_field.ty, b.module())
                        },
                    )
            }
            Ty::Enum(ty) => {
                let (Some(a), Some(b)) = (
                    self.owner.find_enum_definition(ty),
                    other.owner.find_enum_definition(ty),
                ) else {
                    return false;
                };
                let (a_owner, a_id) = a;
                let (b_owner, b_id) = b;
                let a = &a_owner.bytecode.enumerations[a_id.index()];
                let b = &b_owner.bytecode.enumerations[b_id.index()];
                let a_arguments = if a_owner
                    .identity_admission()
                    .equivalent(&a.arguments, &ty.arguments)
                {
                    &a.arguments
                } else {
                    &ty.arguments
                };
                let b_arguments = if b_owner
                    .identity_admission()
                    .equivalent(&b.arguments, &ty.arguments)
                {
                    &b.arguments
                } else {
                    &ty.arguments
                };
                a.apply(a_arguments, &Default::default())
                    .and_then(|layout| {
                        a_owner
                            .identity_admission()
                            .runtime_in(
                                layout.as_ref(),
                                self.closed_definitions(),
                                &Default::default(),
                            )
                            .ok()
                    })
                    .zip(
                        b.apply(b_arguments, &Default::default())
                            .and_then(|layout| {
                                b_owner
                                    .identity_admission()
                                    .runtime_in(
                                        layout.as_ref(),
                                        other.closed_definitions(),
                                        &Default::default(),
                                    )
                                    .ok()
                            }),
                    )
                    .is_some_and(|(a, b)| a == b)
                    && a.variants.iter().zip(&b.variants).all(|(a, b)| {
                        a.payload
                            .iter()
                            .zip(&b.payload)
                            .all(|(a, b)| compare(a, &a_owner, b, &b_owner))
                    })
            }
            _ => false,
        }
    }

    pub(crate) fn is_heap_type(self) -> bool {
        self.check_heap_type(&mut HashSet::new())
    }

    fn check_heap_type(self, visited: &mut HashSet<TypeIdentity>) -> bool {
        let Some(view) = self.normalized() else {
            return false;
        };
        let Some(closed) = view.closed() else {
            return false;
        };
        if matches!(
            closed.as_ref(),
            Ty::Host(_) | Ty::Parameter { .. } | Ty::SelfType(_) | Ty::Projection { .. }
        ) {
            return false;
        }
        if matches!(closed.as_ref(), Ty::Struct(_) | Ty::Enum(_)) {
            let Some(key) = view.identity() else {
                return false;
            };
            if !visited.insert(key) {
                return true;
            }
            let Some(application) = view.application() else {
                return false;
            };
            let mut check = |ty: &Ty, owner: &ExecutableModule| {
                TypeView {
                    ty,
                    owner,
                    environment: None,
                    application: Some(&application),
                    prepared: None,
                    closed_scope: None,
                }
                .check_heap_type(visited)
            };
            match closed.as_ref() {
                Ty::Struct(ty) => {
                    let Some(layout) = view.owner.find_struct_layout(ty) else {
                        return false;
                    };
                    if !layout
                        .template()
                        .fields
                        .iter()
                        .all(|field| check(&field.ty, layout.module()))
                    {
                        return false;
                    }
                }
                Ty::Enum(ty) => {
                    let Some((owner, id)) = view.owner.find_enum_definition(ty) else {
                        return false;
                    };
                    if !owner.bytecode.enumerations[id.index()]
                        .variants
                        .iter()
                        .flat_map(|variant| &variant.payload)
                        .all(|ty| check(ty, &owner))
                    {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        view.children(view, |a, _| view.child(a).check_heap_type(visited))
    }
}

fn nominal(ty: &Ty) -> Option<&NominalTy> {
    match ty {
        Ty::Struct(ty) | Ty::Enum(ty) => Some(ty),
        _ => None,
    }
}

fn children_match<'a>(
    left: &'a Ty,
    right: &'a Ty,
    mut test: impl FnMut(&'a Ty, &'a Ty) -> bool,
) -> bool {
    match (left, right) {
        (Ty::Struct(a), Ty::Struct(b))
        | (Ty::Enum(a), Ty::Enum(b))
        | (Ty::Trait(a), Ty::Trait(b))
        | (Ty::NativeObject(a), Ty::NativeObject(b)) => {
            a.arguments
                .iter()
                .zip(&b.arguments)
                .all(|(a, b)| test(a, b))
                && a.associated_types
                    .values()
                    .zip(b.associated_types.values())
                    .all(|(a, b)| test(a, b))
        }
        (Ty::Tuple(a), Ty::Tuple(b)) => a.iter().zip(b).all(|(a, b)| test(a, b)),
        (Ty::Array(a), Ty::Array(b))
        | (Ty::Reference { target: a, .. }, Ty::Reference { target: b, .. })
        | (Ty::Range(a, _), Ty::Range(b, _)) => test(a, b),

        (
            Ty::Function {
                params: a,
                result: ar,
            },
            Ty::Function {
                params: b,
                result: br,
            },
        ) => a.iter().zip(b).all(|(a, b)| test(a, b)) && test(ar, br),
        _ => true,
    }
}
