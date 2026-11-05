//! Owner-scoped projections of recorded facts for semantic elaboration.
//!
//! Optional fields mean an operation does not use that kind of fact. Elaboration
//! must require the fields selected by its operation kind; an absent required
//! fact is an error, never permission to resolve source again.

use crate::{
    compile_input::body::CheckedBodyInput,
    hir::ids::{ExprId, LocalId, ParamId, PatternId, PlaceId},
    resolver::resolved::ResolvedName,
    typeck::{
        scalar::ScalarValue,
        table::{
            ResolvedAssociatedConst, ResolvedCall, ResolvedEnumConstructor, ResolvedHostPath,
            ResolvedHostPlacePath, ResolvedInterfaceCoercion, ResolvedIteration,
            ResolvedStructInit, propagation::ResolvedPropagation,
        },
    },
    types::{NominalType, TypeId},
};
use kagari_common::{identity::table::DefinitionId, span::Span};

/// Recorded operation selections for one expression in an admitted body.
///
/// `ty` describes the source value before implicit callable/interface conversions.
/// A method-selector callee is structural syntax and need not have a value type.
/// Every actually emitted value must have a complete type after elaboration.
pub struct ExpressionFacts<'a> {
    /// Checked value type before implicit conversion; selector-only syntax may have none.
    pub ty: Option<TypeId>,
    /// Final value type after the already-selected implicit conversions.
    pub result_type: Option<TypeId>,
    /// Resolved binding identity for a named read or mutation root.
    pub binding: Option<ResolvedName>,
    /// Selected callable/protocol application, including its receiver and applied signature.
    pub call: Option<ResolvedCall>,
    /// Decoded constant value; no literal parsing is needed downstream.
    pub scalar: Option<&'a ScalarValue>,
    /// Canonical field declaration, independent of source spelling.
    pub field: Option<&'a DefinitionId>,
    /// Constructor fields in source order and fields supplied by an update base.
    pub structure: Option<&'a ResolvedStructInit>,
    /// Selected enum owner and variant for a constructor expression.
    pub enumeration: Option<&'a ResolvedEnumConstructor>,
    /// Associated member and proved interface; instantiated implementation selection remains downstream.
    pub associated_constant: Option<&'a ResolvedAssociatedConst>,
    /// Declared host read path, including root and ordered dynamic arguments.
    pub host_path: Option<&'a ResolvedHostPath>,
    /// Selected branch/residual contracts for `?`.
    pub propagation: Option<&'a ResolvedPropagation>,
    /// Iterable/Iterator contracts recorded on the `for` iterable expression.
    pub iteration: Option<&'a ResolvedIteration>,
    /// Semantic receiver of a protocol application, which can differ from its operand type.
    pub protocol_receiver: Option<&'a TypeId>,
    /// Concrete receiver and callable interface requiring a function-value adapter.
    pub callable_conversion: Option<&'a (TypeId, NominalType)>,
    /// Selected interface materialization or upcast after producing the source value.
    pub interface_conversion: Option<&'a ResolvedInterfaceCoercion>,
}

/// Recorded root, field/index and declared-host access facts for a mutation target.
pub struct PlaceFacts<'a> {
    /// Checked stored value type, distinct from whether computing the address completes.
    pub ty: Option<TypeId>,
    /// Resolved binding identity for a named read or mutation root.
    pub binding: Option<ResolvedName>,
    /// Canonical field declaration, independent of source spelling.
    pub field: Option<&'a DefinitionId>,
    /// Applied read interface, including reads needed by compound mutation.
    pub index_read: Option<&'a NominalType>,
    /// Applied write interface and its selected setter member.
    pub index_write: Option<(&'a NominalType, &'a DefinitionId)>,
    /// Declared host mutation path; preparing it must evaluate each dynamic argument once.
    pub host_path: Option<&'a ResolvedHostPlacePath>,
}

/// Checked scalar, constructor and field selections for a pattern.
///
/// A bare name can select a constant or unit variant instead of its provisional
/// local binding. Consumers inspect these selections before treating it as a binder.
pub struct PatternFacts<'a> {
    /// Decoded constant value; no literal parsing is needed downstream.
    pub scalar: Option<&'a ScalarValue>,
    /// Evaluated lower/upper range endpoints; inclusivity stays on the pattern node.
    pub range: Option<&'a (ScalarValue, ScalarValue)>,
    /// Canonical struct fields aligned with source pattern-field order.
    pub fields: Option<&'a [DefinitionId]>,
    /// Selected variant, including a bare name that does not introduce a binding.
    pub variant: Option<&'a DefinitionId>,
    /// Structural/resolved pattern classification shared with completion analysis.
    pub irrefutable: bool,
}

impl<'a> CheckedBodyInput<'a> {
    /// Projects operation facts only after validating the expression's complete local identity.
    pub fn expression_facts(self, id: ExprId) -> Option<ExpressionFacts<'a>> {
        self.expression(id)?;
        Some(ExpressionFacts {
            ty: self.types.expr_type(id),
            result_type: self.types.coerced_expr_type(self.definitions, id),
            binding: self.names.expr_resolution(id),
            call: self.types.call_resolution(id),
            scalar: self.types.scalar_value(id),
            field: self.types.expr_field(id),
            structure: self.types.struct_init(id),
            enumeration: self.types.enum_constructor(id),
            associated_constant: self.types.associated_const(id),
            host_path: self.types.host_path(id),
            propagation: self.types.propagation(id),
            iteration: self.types.iteration(id),
            protocol_receiver: self.types.protocol_receiver(id),
            callable_conversion: self.types.callable_coercion(id),
            interface_conversion: self.types.interface_coercion(id),
        })
    }

    /// Projects mutation facts only for a place in this body.
    pub fn place_facts(self, id: PlaceId) -> Option<PlaceFacts<'a>> {
        self.place(id)?;
        Some(PlaceFacts {
            ty: self.types.place_type(id),
            binding: self.names.place_resolution(id),
            field: self.types.place_field(id),
            index_read: self.types.place_index(id),
            index_write: self.types.place_index_write(id),
            host_path: self.types.host_place_path(id),
        })
    }

    /// Projects pattern selections only for a pattern in this body.
    pub fn pattern_facts(self, id: PatternId) -> Option<PatternFacts<'a>> {
        self.pattern(id)?;
        Some(PatternFacts {
            scalar: self.types.pattern_scalar_value(id),
            range: self.types.pattern_range(id),
            fields: self.types.pattern_fields(id),
            variant: self.types.pattern_variant(id),
            irrefutable: self.names.pattern_is_irrefutable(self.nodes, id),
        })
    }

    /// Returns a checked local type, including closure parameters and captures.
    pub fn local_type(self, id: LocalId) -> Option<TypeId> {
        self.owns(id.arena(), id.owner())
            .then(|| self.types.local_type(id))
            .flatten()
    }

    /// Returns the authored binding range for a checked local in this body.
    pub fn local_span(self, id: LocalId) -> Option<Span> {
        self.local_type(id)?;
        Some(self.origins.local_span(id))
    }

    /// Returns a parameter type from the matching enclosing function signature.
    pub fn parameter_type(self, id: ParamId) -> Option<&'a TypeId> {
        if !self.owns(id.arena(), id.owner()) {
            return None;
        }
        self.signature?
            .params
            .iter()
            .find(|parameter| parameter.id == id)
            .map(|parameter| &parameter.ty)
    }
}
