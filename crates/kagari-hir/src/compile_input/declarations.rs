//! Declaration metadata and registered contracts without a source body arena.

use crate::{
    aggregates::implementations::ImplementationSignature,
    callable::AppliedCallSignature,
    compile_input::program::AdmissionError,
    declarations::Declarations,
    hir::item::{
        Item, Module,
        adt::{Enum, OpaqueType, Struct},
        alias::TypeAlias,
        behavior::{Impl, TraitDef},
        function::Function,
        module::ModuleDecl,
        storage::{ConstItem, StaticItem},
    },
    typeck::{GenericBounds, constraints, scalar::ScalarValue, table::TypeTable},
    types::{GenericParameterType, NominalType, TypeId},
};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_types::declaration::{NativeDeclaration, TraitDef as RegisteredTrait};
use std::{borrow::Cow, collections::BTreeMap};

/// Borrowed source declaration metadata in one admitted module.
///
/// For `pub struct Point { val x: i32 }`, `items` contains its struct handle and
/// `structs` retains the name, visibility and field order. The matching
/// [`super::environment::CheckedEnvironment::aggregates`] owns the selected field
/// type and identity. Consumers use that contract instead of resolving the written
/// type reference again.
///
/// Function/initializer headers retain their root IDs, but this view provides no
/// arena in which to dereference them. Use the enclosing checked module's body
/// directory. Plain declaration IDs require this same module; these slices are
/// metadata projections and cannot authorize a new compilation on their own.
#[derive(Clone, Copy)]
pub struct DeclarationHeaders<'a> {
    /// Top-level declarations in source order, excluding import leaves.
    pub items: &'a [Item],
    /// All named functions, including local functions and trait/impl methods.
    pub functions: &'a [Function],
    /// Scalar declarations, including associated initializers.
    pub constants: &'a [ConstItem],
    /// Runtime static declarations; checking has not executed their initializers.
    pub statics: &'a [StaticItem],
    /// Child-module headers; each child's body belongs to its own checked module.
    pub modules: &'a [ModuleDecl],
    /// Struct metadata; semantic member types belong to the aggregate catalog.
    pub structs: &'a [Struct],
    /// Native-backed type headers with separately registered representation facts.
    pub opaque_types: &'a [OpaqueType],
    /// Transparent aliases; normalized templates belong to the declaration environment.
    pub aliases: &'a [TypeAlias],
    /// Enum headers, retaining authored variant order.
    pub enums: &'a [Enum],
    /// Trait headers, retaining method and associated-member order.
    pub traits: &'a [TraitDef],
    /// Trait and inherent implementation headers, including empty implementations.
    pub implementations: &'a [Impl],
}

impl<'a> DeclarationHeaders<'a> {
    pub(super) fn new(module: &'a Module) -> Self {
        Self {
            items: &module.items,
            functions: &module.functions,
            constants: &module.consts,
            statics: &module.statics,
            modules: &module.modules,
            structs: &module.structs,
            opaque_types: &module.opaque_types,
            aliases: &module.type_aliases,
            enums: &module.enums,
            traits: &module.traits,
            implementations: &module.impls,
        }
    }

    pub(super) fn contains(self, item: Item) -> bool {
        match item {
            Item::Function(id) => self
                .functions
                .get(id.index())
                .is_some_and(|row| row.id == id),
            Item::Const(id) => self
                .constants
                .get(id.index())
                .is_some_and(|row| row.id == id),
            Item::Static(id) => self.statics.get(id.index()).is_some_and(|row| row.id == id),
            Item::Module(id) => self.modules.get(id.index()).is_some_and(|row| row.id == id),
            Item::Struct(id) => self.structs.get(id.index()).is_some_and(|row| row.id == id),
            Item::OpaqueType(id) => self
                .opaque_types
                .get(id.index())
                .is_some_and(|row| row.id == id),
            Item::TypeAlias(id) => self.aliases.get(id.index()).is_some_and(|row| row.id == id),
            Item::Enum(id) => self.enums.get(id.index()).is_some_and(|row| row.id == id),
            Item::Trait(id) => self.traits.get(id.index()).is_some_and(|row| row.id == id),
            Item::Impl(id) => self
                .implementations
                .get(id.index())
                .is_some_and(|row| row.id == id),
        }
    }
}

/// Exact installed declaration authority in its retained definition scope.
///
/// These records can retain projections and native passing policies that a
/// normalized source signature does not. They are borrowed from installation,
/// never reconstructed from a generated source spelling. Ordinary source modules
/// have no registration view.
#[derive(Clone, Copy)]
pub struct RegisteredDeclarations<'a> {
    /// Scope in which every identity in the following records is valid.
    pub definitions: &'a DefinitionTable,
    /// Installed callable and native template declarations.
    pub callables: &'a [NativeDeclaration],
    /// Exact foreign source trait requirements retained by installation.
    pub source_traits: &'a BTreeMap<DefinitionId, RegisteredTrait>,
}

/// Selected forwarding application proved by a generated native default body.
///
/// The target is canonical; arguments can still contain the default's binders.
/// Instance planning substitutes them under the declaring trait's environment.
/// This is a declaration recipe, not an executed call or a substitute for the
/// registered target's host policy.
#[derive(Clone, Copy)]
pub struct NativeDefaultCall<'a> {
    /// Selected native template or source callable declaration.
    pub declaration: DefinitionId,
    /// Applied type arguments in the selected target's binder order.
    pub arguments: &'a [TypeId],
    /// Checked parameter/result types of the forwarding application.
    pub signature: &'a AppliedCallSignature,
}

/// The type and evaluated value of one local scalar constant.
///
/// `const STEP: i32 = 2` supplies `i32` and the decoded integer value. No consumer
/// needs to lower or evaluate the initializer to construct a declaration contract.
#[derive(Clone, Copy)]
pub struct ConstantValue<'a> {
    /// Declared or inferred semantic type in this module's definition scope.
    pub ty: &'a TypeId,
    /// Evaluated scalar; no source expression or runtime initialization is required.
    pub value: &'a ScalarValue,
}

/// Receiver and binder contract of one trait or inherent implementation header.
///
/// Trait implementations borrow their existing aggregate contract. Inherent impls
/// have no trait entry, so admission gathers their already resolved header facts
/// using checking's bound projection. This also represents an empty inherent impl;
/// inferring the header from its first method would lose that declaration.
///
/// For `impl<T: Read> Box<T>`, `receiver` is `Box<T>`, `interface` is `None`, and
/// `bounds` retains the `T: Read` requirement under the impl's binder identity.
pub struct ImplementationInput<'a> {
    /// Canonical identity of the implementation, not a member method.
    pub declaration: DefinitionId,
    /// Receiver pattern before instance-specific substitution.
    pub receiver: &'a TypeId,
    /// Applied implemented trait, or `None` for inherent declarations.
    pub interface: Option<&'a NominalType>,
    /// Impl-owned binders in declaration order; method binders are separate.
    pub generic_params: Cow<'a, [GenericParameterType]>,
    /// Checked inline and where-clause constraints, still parameterized.
    pub bounds: Cow<'a, GenericBounds>,
}

impl<'a> ImplementationInput<'a> {
    pub(super) fn trait_implementation(signature: &'a ImplementationSignature) -> Self {
        Self {
            declaration: signature.id,
            receiver: &signature.for_type,
            interface: Some(&signature.trait_type),
            generic_params: Cow::Borrowed(&signature.generic_params),
            bounds: Cow::Borrowed(&signature.bounds),
        }
    }

    pub(super) fn inherent(
        header: &Impl,
        declarations: &Declarations,
        types: &'a TypeTable,
    ) -> Result<Self, AdmissionError> {
        let invalid = || AdmissionError::InvalidModule("incomplete inherent implementation header");
        let declaration = *declarations.impl_identity(header.id).ok_or_else(invalid)?;
        let receiver = &types
            .type_ref(header.for_type.ok_or_else(invalid)?)
            .ok_or_else(invalid)?
            .ty;
        if receiver.is_unresolved() {
            return Err(invalid());
        }
        let mut parameters = Vec::with_capacity(header.generic_params.len());
        for parameter in &header.generic_params {
            parameters.push(
                declarations
                    .generic_type(parameter.id)
                    .ok_or_else(invalid)?,
            );
            for bound in &parameter.bounds {
                types.constraint(bound.ty).ok_or_else(invalid)?;
            }
        }
        for bound in &header.bounds {
            let target = &types.type_ref(bound.target_ref).ok_or_else(invalid)?.ty;
            if target.is_unresolved() {
                return Err(invalid());
            }
            for requirement in &bound.traits {
                types.constraint(requirement.ty).ok_or_else(invalid)?;
            }
        }
        Ok(Self {
            declaration,
            receiver,
            interface: None,
            generic_params: Cow::Owned(parameters),
            bounds: Cow::Owned(constraints::implementation_bounds(
                header,
                declarations,
                types,
            )),
        })
    }
}
