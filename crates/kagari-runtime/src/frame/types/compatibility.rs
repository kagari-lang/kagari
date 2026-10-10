//! Compare complete reified layout graphs while preserving lexical type scopes.
use crate::{
    frame::types::bindings::TypeBindings,
    host::HostRegistryId,
    module::{LoadedModule, ModuleKey},
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{NominalTy, Ty, substitution::TypeSubstitution};
use std::{borrow::Cow, collections::HashSet, ptr};

#[derive(Clone, Copy)]
pub(crate) struct TypeView<'a> {
    pub(crate) ty: &'a Ty<DefinitionId>,
    pub(crate) owner: &'a LoadedModule,
    pub(crate) environment: Option<&'a TypeBindings>,
    application: Option<&'a Application<'a>>,
}

struct Application<'a> {
    declaration: &'a DefinitionId,
    arguments: Vec<TypeView<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TypeIdentity {
    ty: Ty<DefinitionId>,
    owners: Vec<(HostRegistryId, ModuleKey)>,
}

impl<'a> TypeView<'a> {
    pub(crate) fn new(
        ty: &'a Ty<DefinitionId>,
        owner: &'a LoadedModule,
        environment: Option<&'a TypeBindings>,
    ) -> Self {
        Self {
            ty,
            owner,
            environment,
            application: None,
        }
    }

    fn child(self, ty: &'a Ty<DefinitionId>) -> Self {
        Self { ty, ..self }
    }

    pub(crate) fn normalized(mut self) -> Option<Self> {
        loop {
            match self.ty {
                Ty::Parameter { owner, position } => {
                    self = if let Some(application) = self.application
                        && application.declaration == owner
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
                    let (ty, owner) =
                        environment.associated_output(&receiver, &interface, *member)?;
                    self = Self::new(ty, owner, None);
                }
                _ => break,
            }
        }
        Some(self)
    }

    pub(crate) fn closed(self) -> Option<Cow<'a, Ty<DefinitionId>>> {
        let view = self.normalized()?;
        if view.ty.is_concrete() {
            return Some(Cow::Borrowed(view.ty));
        }
        if let Some(application) = view.application {
            let arguments = application
                .arguments
                .iter()
                .map(|ty| ty.closed().map(Cow::into_owned))
                .collect::<Option<Vec<_>>>()?;
            let ty = TypeSubstitution::for_owner(application.declaration, &arguments)
                .apply(view.ty, &Default::default())
                .ok()?;
            return (ty.is_concrete() && ty.within_wire_limits()).then_some(Cow::Owned(ty));
        }
        view.environment?.resolve(view.ty).ok().map(Cow::Owned)
    }

    pub(crate) fn identity(self) -> Option<TypeIdentity> {
        let mut owners = vec![];
        self.collect_owners(&mut owners).then_some(())?;
        Some(TypeIdentity {
            ty: self.closed()?.into_owned(),
            owners,
        })
    }

    fn collect_owners(self, owners: &mut Vec<(HostRegistryId, ModuleKey)>) -> bool {
        let Some(view) = self.normalized() else {
            return false;
        };
        if matches!(view.ty, Ty::Struct(_) | Ty::Enum(_)) {
            owners.push(view.owner.program_identity());
        }
        children_match(view.ty, view.ty, |a, _| {
            view.child(a).collect_owners(owners)
        })
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
        if left.owner.key() == right.owner.key()
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
        children_match(left.ty, right.ty, |a, b| {
            left.child(a).compare(right.child(b), visited)
        })
    }

    fn application(self) -> Option<Application<'a>> {
        let nominal = nominal(self.ty)?;
        Some(Application {
            declaration: &nominal.declaration,
            arguments: nominal.arguments.iter().map(|ty| self.child(ty)).collect(),
        })
    }

    fn compare_fields(
        self,
        other: Self,
        closed: &Ty<DefinitionId>,
        visited: &mut HashSet<(TypeIdentity, TypeIdentity)>,
    ) -> bool {
        let (Some(left), Some(right)) = (self.application(), other.application()) else {
            return false;
        };
        let mut compare = |a: &Ty<DefinitionId>,
                           a_owner: &LoadedModule,
                           b: &Ty<DefinitionId>,
                           b_owner: &LoadedModule| {
            TypeView {
                ty: a,
                owner: a_owner,
                environment: None,
                application: Some(&left),
            }
            .compare(
                TypeView {
                    ty: b,
                    owner: b_owner,
                    environment: None,
                    application: Some(&right),
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
                a.layout() == b.layout()
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
                a.apply(&ty.arguments, &Default::default())
                    == b.apply(&ty.arguments, &Default::default())
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
            let mut check = |ty: &Ty<DefinitionId>, owner: &LoadedModule| {
                TypeView {
                    ty,
                    owner,
                    environment: None,
                    application: Some(&application),
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
        children_match(view.ty, view.ty, |a, _| {
            view.child(a).check_heap_type(visited)
        })
    }
}

fn nominal(ty: &Ty<DefinitionId>) -> Option<&NominalTy<DefinitionId>> {
    match ty {
        Ty::Struct(ty) | Ty::Enum(ty) => Some(ty),
        _ => None,
    }
}

fn children_match<'a>(
    left: &'a Ty<DefinitionId>,
    right: &'a Ty<DefinitionId>,
    mut test: impl FnMut(&'a Ty<DefinitionId>, &'a Ty<DefinitionId>) -> bool,
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
        (Ty::Array(a, _), Ty::Array(b, _))
        | (Ty::Set(a, _), Ty::Set(b, _))
        | (Ty::Iter(a), Ty::Iter(b))
        | (Ty::Range(a, _), Ty::Range(b, _)) => test(a, b),
        (
            Ty::Map {
                key: a, value: av, ..
            },
            Ty::Map {
                key: b, value: bv, ..
            },
        ) => test(a, b) && test(av, bv),
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
