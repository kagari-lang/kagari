//! Forward references carry identity and expected shape, never fabricated contracts.
use crate::native::{
    assembly::conflict, binding::NativeResult, catalog::DeclarationCatalog, types::Type,
};
use kagari_common::identity::{
    DefinitionKind,
    map::DefinitionContext,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::{
        TypeDefKind, native::NativeTypeConstructor, requirement::NativeCallableRequirement,
    },
    range::RangeKind,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::collections::BTreeMap;

/// The semantic representation expected from a reference, in addition to its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceKind {
    Type(TypeDefKind),
    Trait,
}

/// An explicit unresolved reference. Only assembly sealing establishes existence,
/// category and arity; `apply` constructs a semantic input, not a checked TypeRef.
#[derive(Clone, Debug)]
pub struct DraftReference {
    pub(crate) id: DefinitionId,
    pub(crate) definitions: DefinitionTable,
    pub(crate) kind: ReferenceKind,
    pub(crate) arity: usize,
}

impl DraftReference {
    pub fn id(&self) -> DefinitionId {
        self.id
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    /// Refer to a parameter of the expected declaration, not an implementation binder.
    pub fn parameter(&self, position: usize) -> NativeResult<Type> {
        if position >= self.arity {
            return Err(conflict("forward reference parameter index"));
        }
        Type::from_semantic(
            Ty::Parameter {
                owner: self.id,
                position,
            },
            &self.definitions,
        )
    }

    /// The symbolic receiver of a trait contract, substituted by implementation lowering.
    pub fn receiver(&self) -> NativeResult<Type> {
        if self.kind != ReferenceKind::Trait {
            return Err(conflict("symbolic receiver requires a trait reference"));
        }
        Type::from_semantic(Ty::SelfType(self.id), &self.definitions)
    }

    /// Apply the declared expectation without inventing a completed provider.
    /// Argument scopes are composed by the ordinary Type mapping machinery.
    pub fn apply(&self, arguments: impl IntoIterator<Item = Type>) -> NativeResult<Type> {
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        if arguments.len() != self.arity {
            return Err(conflict("forward reference argument count"));
        }
        let marker = Type::from_semantic(Ty::SelfType(self.id), &self.definitions)?;
        let combined = Type::tuple([marker].into_iter().chain(arguments))?;
        let Ty::Tuple(items) = combined.abi() else {
            unreachable!("tuple composition")
        };
        let Ty::SelfType(id) = items[0] else {
            unreachable!("identity marker")
        };
        let arguments = items[1..].to_vec();
        let nominal = NominalTy {
            declaration: id,
            arguments,
            associated_types: BTreeMap::new(),
        };
        let ty = match self.kind {
            ReferenceKind::Trait => Ty::Trait(nominal),
            ReferenceKind::Type(TypeDefKind::Enum) => Ty::Enum(nominal),
            ReferenceKind::Type(TypeDefKind::Struct) => Ty::Struct(nominal),
            ReferenceKind::Type(TypeDefKind::NativeStorage(_)) => Ty::NativeObject(nominal),
            ReferenceKind::Type(TypeDefKind::Host(_)) if self.arity == 0 => Ty::Host(id),
            ReferenceKind::Type(TypeDefKind::Host(_)) => {
                return Err(conflict("external identities cannot be generic"));
            }
            ReferenceKind::Type(TypeDefKind::Native(constructor)) => {
                constructor_type(constructor, nominal.arguments)?
            }
        };
        Type::from_semantic(
            ty,
            combined.definitions().expect("reference identity scope"),
        )
    }

    pub(crate) fn validate(&self, catalog: &DeclarationCatalog) -> NativeResult<()> {
        if self.definitions.id() != catalog.definitions().id() {
            return Err(conflict("foreign assembly reference"));
        }
        let valid = match self.kind {
            ReferenceKind::Trait => catalog
                .traits
                .get_id(self.id)
                .is_some_and(|item| item.generic_params.len() == self.arity),
            ReferenceKind::Type(kind) => catalog
                .types
                .get_id(self.id)
                .is_some_and(|item| item.kind == kind && item.generic_params.len() == self.arity),
        };
        if !valid {
            return Err(conflict(
                "unresolved reference or category/arity disagreement",
            ));
        }
        Ok(())
    }
}

/// A checked-scope forward member expectation. Existence and generic arity are
/// checked against the complete trait at sealing, including unused references.
#[derive(Clone, Debug)]
pub struct DraftMember {
    pub(crate) owner: DraftReference,
    pub(crate) id: DefinitionId,
    pub(crate) kind: DefinitionKind,
    pub(crate) arity: usize,
    pub(crate) name: String,
}

impl DraftMember {
    pub fn id(&self) -> DefinitionId {
        self.id
    }

    /// A method/associated-type parameter under the member's declaration binder.
    pub fn parameter(&self, position: usize) -> NativeResult<Type> {
        if position >= self.arity {
            return Err(conflict("forward member parameter index"));
        }
        Type::from_semantic(
            Ty::Parameter {
                owner: self.id,
                position,
            },
            &self.owner.definitions,
        )
    }

    /// Attach a non-generic associated output to its applied trait. Duplicate
    /// bindings and a different trait owner reject instead of replacing facts.
    pub fn bind(&self, interface: Type, output: Type) -> NativeResult<Type> {
        if self.kind != DefinitionKind::AssociatedType || self.arity != 0 {
            return Err(conflict(
                "associated binding requires a non-generic associated type",
            ));
        }
        let combined = self.compose(interface, [output])?;
        let Ty::Tuple(items) = combined.abi() else {
            unreachable!("tuple composition")
        };
        let Ty::SelfType(member) = items[1] else {
            unreachable!("member marker")
        };
        let Ty::Trait(mut interface) = items[2].clone() else {
            unreachable!("checked interface")
        };
        if interface
            .associated_types
            .insert(member, items[3].clone())
            .is_some()
        {
            return Err(conflict("duplicate associated output"));
        }
        Type::from_semantic(
            Ty::Trait(interface),
            combined.definitions().expect("member scope"),
        )
    }

    /// Form a symbolic associated projection without claiming a completed contract.
    pub fn projection(
        &self,
        receiver: Type,
        interface: Type,
        arguments: impl IntoIterator<Item = Type>,
    ) -> NativeResult<Type> {
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        if self.kind != DefinitionKind::AssociatedType || arguments.len() != self.arity {
            return Err(conflict("associated projection category or argument count"));
        }
        let combined = self.compose(interface, [receiver].into_iter().chain(arguments))?;
        let Ty::Tuple(items) = combined.abi() else {
            unreachable!("tuple composition")
        };
        let Ty::SelfType(member) = items[1] else {
            unreachable!("member marker")
        };
        let Ty::Trait(interface) = &items[2] else {
            unreachable!("checked interface")
        };
        Type::from_semantic(
            Ty::Projection {
                receiver: Box::new(items[3].clone()),
                interface: Box::new(interface.clone()),
                member,
                arguments: items[4..].to_vec(),
            },
            combined.definitions().expect("member scope"),
        )
    }

    /// Describe an ordered selected-call obligation. The originating assembly
    /// scope is retained until submission; this is not a completed MethodRef.
    pub fn requirement(
        &self,
        receiver: Type,
        interface: Type,
        arguments: impl IntoIterator<Item = Type>,
    ) -> NativeResult<DraftCallableRequirement> {
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        if self.kind != DefinitionKind::Method || arguments.len() != self.arity {
            return Err(conflict("callable requirement category or argument count"));
        }
        self.compose(interface.clone(), [])?;
        Ok(DraftCallableRequirement {
            member: self.clone(),
            receiver,
            interface,
            arguments,
        })
    }

    fn compose(
        &self,
        interface: Type,
        values: impl IntoIterator<Item = Type>,
    ) -> NativeResult<Type> {
        let marker = |id| Type::from_semantic(Ty::SelfType(id), &self.owner.definitions);
        let combined = Type::tuple(
            [marker(self.owner.id)?, marker(self.id)?, interface]
                .into_iter()
                .chain(values),
        )?;
        let Ty::Tuple(items) = combined.abi() else {
            unreachable!("tuple composition")
        };
        let Ty::SelfType(owner) = items[0] else {
            unreachable!("owner marker")
        };
        if !matches!(&items[2], Ty::Trait(interface) if interface.declaration == owner
            && interface.arguments.len() == self.owner.arity)
        {
            return Err(conflict(
                "member application belongs to another trait or has wrong arity",
            ));
        }
        Ok(combined)
    }

    pub(crate) fn check_scope(&self, context: &DefinitionContext) -> NativeResult<()> {
        if self.owner.definitions.id() != context.snapshot().id() {
            return Err(conflict("foreign assembly member reference"));
        }
        Ok(())
    }

    pub(crate) fn validate(&self, catalog: &DeclarationCatalog) -> NativeResult<()> {
        self.owner.validate(catalog)?;
        let definitions = catalog.definitions();
        if definitions.lookup_child(self.owner.id, self.kind, &self.name, 0) != Some(self.id) {
            return Err(conflict("forward member identity or owner disagreement"));
        }
        let contract = catalog
            .traits
            .get_id(self.owner.id)
            .expect("validated trait");
        let valid = match self.kind {
            DefinitionKind::Method => contract.methods.iter().any(|method| {
                method.name == self.name && method.generic_params.len() == self.arity
            }),
            DefinitionKind::AssociatedType => contract.associated_types.iter().any(|member| {
                member.declaration == self.id && member.generic_params.len() == self.arity
            }),
            _ => false,
        };
        if !valid {
            return Err(conflict("unresolved member or category/arity disagreement"));
        }
        Ok(())
    }
}

/// A same-assembly obligation, accepted by FunctionBuilder::requires_draft and
/// trait overrides. Semantic applicability remains the catalog proof check's job.
#[derive(Clone, Debug)]
pub struct DraftCallableRequirement {
    member: DraftMember,
    receiver: Type,
    interface: Type,
    arguments: Vec<Type>,
}

impl DraftCallableRequirement {
    pub(crate) fn lower(
        &self,
        context: &DefinitionContext,
    ) -> NativeResult<NativeCallableRequirement> {
        self.member.check_scope(context)?;
        let Ty::Trait(interface) = self.interface.adopt(context)? else {
            return Err(conflict(
                "callable requirement requires a trait application",
            ));
        };
        if interface.declaration != self.member.owner.id {
            return Err(conflict("callable member belongs to another trait"));
        }
        Ok(NativeCallableRequirement {
            receiver: self.receiver.adopt(context)?,
            interface,
            member: self.member.id,
            arguments: self
                .arguments
                .iter()
                .map(|ty| ty.adopt(context))
                .collect::<NativeResult<_>>()?,
        })
    }
}

fn constructor_type(
    constructor: NativeTypeConstructor,
    mut arguments: Vec<Ty>,
) -> NativeResult<Ty> {
    if arguments.len() != constructor.arity() {
        return Err(conflict("constructor reference argument count"));
    }
    Ok(match constructor {
        NativeTypeConstructor::String => Ty::Builtin(BuiltinType::String),

        NativeTypeConstructor::Iter => Ty::Iter(Box::new(arguments.remove(0))),
        NativeTypeConstructor::Range(RangeKind::Full) => {
            Ty::Range(Box::new(Ty::Builtin(BuiltinType::Unit)), RangeKind::Full)
        }
        NativeTypeConstructor::Range(kind) => Ty::Range(Box::new(arguments.remove(0)), kind),
        NativeTypeConstructor::Reference(access) => Ty::Reference {
            target: Box::new(arguments.remove(0)),
            access,
        },
    })
}
