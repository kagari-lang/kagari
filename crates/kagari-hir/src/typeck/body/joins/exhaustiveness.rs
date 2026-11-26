//! Exhaustiveness for enum scrutinees, after pattern checking has selected constructors.
//! Specializing whole arm rows (rather than unioning coverage per field) preserves
//! correlations between payloads. Only the current column expands or-patterns.

use crate::{
    aggregates::EnumSignature,
    collections::HashSet,
    hir::{
        expr::{ExprKind, MatchArm},
        ids::{ExprId, PatternId},
        pattern::PatternKind,
    },
    resolver::resolved::ResolvedName,
    typeck::{body::BodyChecker, completion, scalar::ScalarValue},
    types::{GenericParameterType, NominalType, TypeId, TypeSubstitution},
};
use kagari_common::identity::table::DefinitionId;
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::scalar::BuiltinType;

// Pattern matrices can encode exponential searches. Exhaustion is a limit
// diagnostic, never evidence that the match covers (or misses) a value.
const WORK_LIMIT: usize = 1_000_000;
type Cell = Option<PatternId>;
type Row = Vec<Cell>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Covers,
    MayMutateAndContinue,
}

#[derive(Clone)]
struct ArmRow {
    action: Action,
    cells: Row,
}

struct Matrix {
    columns: Vec<TypeId>,
    rows: Vec<ArmRow>,
    // Prefix of columns reached through a writable field. Until guards are
    // specialized on stable facts, these columns must not select a single value.
    deferred: usize,
}

#[derive(Clone, Copy)]
enum Constructor {
    Variant(DefinitionId),
    Product,
    Point(i128),
    Other,
}

enum Stop {
    Cancelled,
    Limit,
}

struct Coverage<'a, 'db> {
    checker: &'a BodyChecker<'db>,
    remaining: usize,
}

impl BodyChecker<'_> {
    pub(super) fn check_enum_coverage(&mut self, site: ExprId, ty: &TypeId, arms: &[MatchArm]) {
        let TypeId::Enum(owner) = ty else { return };
        let Some(signature) = self.aggregates.enumeration(&owner.declaration) else {
            return;
        };
        let mut coverage = Coverage {
            checker: self,
            remaining: WORK_LIMIT,
        };
        let kind = match coverage.uncovered(owner, signature, arms) {
            Ok(missing) if missing.is_empty() => return,
            Ok(uncovered_variants) => DiagnosticKind::NonExhaustiveEnumMatch {
                enum_name: ty.display_name(self.declarations.definitions()),
                uncovered_variants,
            },
            Err(Stop::Cancelled) => {
                self.stopped = true;
                return;
            }
            Err(Stop::Limit) => DiagnosticKind::CompileLimitExceeded {
                resource: "enum match coverage",
                limit: WORK_LIMIT,
            },
        };
        self.diagnostics.push(
            Diagnostic::error(kind)
                .with_span(self.lowered.module.expr(site).origin.diagnostic_span()),
        );
    }
}

impl Coverage<'_, '_> {
    fn spend(&mut self, work: usize) -> Result<(), Stop> {
        self.checker.cancel.check().map_err(|_| Stop::Cancelled)?;
        self.remaining = self.remaining.checked_sub(work).ok_or(Stop::Limit)?;
        Ok(())
    }

    fn uncovered(
        &mut self,
        owner: &NominalType,
        signature: &EnumSignature,
        arms: &[MatchArm],
    ) -> Result<Vec<String>, Stop> {
        let mut variants = vec![Vec::new(); signature.variants.len()];
        for arm in arms {
            let action = if let Some(guard) = arm.guard
                && completion::expr_can_complete(
                    &self.checker.lowered.module,
                    self.checker.names,
                    self.checker.type_table,
                    guard,
                    self.checker.cancel,
                )
                .map_err(|_| Stop::Cancelled)?
            {
                if self.guard_is_read_only(guard) {
                    continue;
                }
                Action::MayMutateAndContinue
            } else {
                // A diverging guard cannot fall through once its pattern matches.
                Action::Covers
            };
            for head in self.alternatives(Some(arm.pattern))? {
                let Some(id) = head else {
                    if action == Action::Covers {
                        return Ok(Vec::new());
                    }
                    for (variant, rows) in signature.variants.iter().zip(&mut variants) {
                        self.spend(1 + variant.payload.len())?;
                        rows.push(ArmRow {
                            action,
                            cells: vec![None; variant.payload.len()],
                        });
                    }
                    continue;
                };
                if let Some(variant) = self
                    .checker
                    .type_table
                    .pattern_variant(id)
                    .and_then(|v| self.checker.aggregates.variant(v))
                    && variant.owner == owner.declaration
                {
                    let (slot, ctor, arity) = (
                        variant.slot,
                        Constructor::Variant(variant.id),
                        variant.payload.len(),
                    );
                    self.spend(1 + arity)?;
                    if let Some(row) = self.specialize(Some(id), ctor, arity) {
                        variants[slot].push(ArmRow { action, cells: row });
                    }
                }
            }
        }
        let subst = substitution(owner, &signature.generic_params);
        let mut missing = Vec::new();
        for (variant, rows) in signature.variants.iter().zip(variants) {
            let columns = variant
                .payload
                .iter()
                .map(|ty| ty.instantiate(&subst))
                .collect();
            if !self.covered(Matrix {
                columns,
                rows,
                deferred: 0,
            })? {
                missing.push(variant.name.clone());
            }
        }
        Ok(missing)
    }

    // A read of a lexical value or ordinary struct field cannot invoke user
    // code. In particular, do not assume calls, operators or statics are pure.
    fn guard_is_read_only(&self, mut id: ExprId) -> bool {
        loop {
            match &self.checker.lowered.module.expr(id).kind {
                ExprKind::Literal(_) => return true,
                ExprKind::Name { .. } => {
                    return matches!(
                        self.checker.names.expr_resolution(id),
                        Some(
                            ResolvedName::Param(_)
                                | ResolvedName::Local(_)
                                | ResolvedName::Const(_)
                        )
                    );
                }
                ExprKind::Field { receiver, .. }
                    if self
                        .checker
                        .type_table
                        .expr_field(id)
                        .and_then(|field| self.checker.aggregates.field(field))
                        .is_some_and(|field| {
                            self.checker.aggregates.structure(&field.owner).is_some()
                        }) =>
                {
                    id = *receiver
                }
                _ => return false,
            }
        }
    }

    fn kind(&self, id: PatternId) -> &PatternKind {
        &self.checker.lowered.module.pattern(id).kind
    }

    fn wildcard(&self, cell: Cell) -> bool {
        cell.is_none_or(|id| match self.kind(id) {
            PatternKind::Wildcard => true,
            PatternKind::Name { .. } => !self.checker.names.pattern_variants.contains_key(&id),
            _ => false,
        })
    }

    fn alternatives(&mut self, cell: Cell) -> Result<Vec<Cell>, Stop> {
        let mut heads = Vec::new();
        let mut pending = vec![cell];
        while let Some(cell) = pending.pop() {
            self.spend(1)?;
            if self.wildcard(cell) {
                return Ok(vec![None]);
            }
            let id = cell.expect("non-wildcard pattern");
            if let PatternKind::Or(children) = self.kind(id) {
                pending.extend(children.iter().copied().map(Some));
            } else {
                heads.push(cell);
            }
        }
        Ok(heads)
    }

    fn covered(&mut self, matrix: Matrix) -> Result<bool, Stop> {
        let mut pending = vec![matrix];
        while let Some(Matrix {
            mut columns,
            rows,
            deferred,
        }) = pending.pop()
        {
            self.spend(1)?;
            if rows.iter().any(|row| {
                row.action == Action::Covers && row.cells.iter().all(|&c| self.wildcard(c))
            }) {
                continue;
            }
            if rows.is_empty() {
                if !self.empty_product(&columns, &HashSet::default(), &mut Vec::new())? {
                    return Ok(false);
                }
                continue;
            }
            let has_barrier = rows
                .iter()
                .any(|row| row.action == Action::MayMutateAndContinue);
            if has_barrier && columns.len() == deferred {
                // For this stable constructor combination, a false guard may
                // change every reachable writable field. A complete segment
                // suffices; partial mutable coverage cannot cross the guard.
                if !self.covered_between_guards(&columns, &rows)? {
                    return Ok(false);
                }
                continue;
            }
            let ty = columns.pop().expect("nonempty non-wildcard row");
            let mut heads = Vec::new();
            for mut row in rows {
                let cell = row.cells.pop().expect("one cell per column");
                for head in self.alternatives(cell)? {
                    self.spend(1 + row.cells.len())?;
                    heads.push((head, row.clone()));
                }
            }
            if heads.iter().all(|(head, _)| head.is_none()) {
                if !self.empty_type(&ty, &HashSet::default(), &mut Vec::new())? {
                    pending.push(Matrix {
                        columns,
                        rows: heads.into_iter().map(|(_, tail)| tail).collect(),
                        deferred,
                    });
                }
                continue;
            }
            let writable = if has_barrier
                && let TypeId::Struct(owner) = &ty
                && let Some(sig) = self.checker.aggregates.structure(&owner.declaration)
            {
                sig.fields
                    .iter()
                    .map(|f| f.writeability.is_var())
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let next_deferred = deferred + writable.iter().filter(|&&v| v).count();
            for (ctor, fields) in self.constructors(&ty, &heads) {
                self.spend(1 + columns.len() + fields.len())?;
                let mut rows = Vec::new();
                for (head, tail) in &heads {
                    self.spend(1)?;
                    // Other represents an open domain rather than a single
                    // literal. Its guard might execute even when that literal
                    // cannot establish coverage of the whole domain.
                    let children = if matches!(ctor, Constructor::Other)
                        && tail.action == Action::MayMutateAndContinue
                    {
                        Some(vec![])
                    } else {
                        self.specialize(*head, ctor, fields.len())
                    };
                    if let Some(children) = children {
                        self.spend(1 + tail.cells.len() + children.len())?;
                        rows.push(ArmRow {
                            action: tail.action,
                            cells: extend_fields(tail.cells.clone(), children, &writable),
                        });
                    }
                }
                pending.push(Matrix {
                    columns: extend_fields(columns.clone(), fields, &writable),
                    rows,
                    deferred: next_deferred,
                });
            }
        }
        Ok(true)
    }

    fn covered_between_guards(
        &mut self,
        columns: &[TypeId],
        rows: &[ArmRow],
    ) -> Result<bool, Stop> {
        for segment in rows.split(|r| r.action == Action::MayMutateAndContinue) {
            self.spend((1 + columns.len()).saturating_mul(1 + segment.len()))?;
            if self.covered(Matrix {
                columns: columns.to_vec(),
                rows: segment.to_vec(),
                deferred: 0,
            })? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn constructors(
        &self,
        ty: &TypeId,
        heads: &[(Cell, ArmRow)],
    ) -> Vec<(Constructor, Vec<TypeId>)> {
        match ty {
            TypeId::Enum(owner) => {
                if let Some(sig) = self.checker.aggregates.enumeration(&owner.declaration) {
                    let subst = substitution(owner, &sig.generic_params);
                    return sig
                        .variants
                        .iter()
                        .map(|v| {
                            (
                                Constructor::Variant(v.id),
                                v.payload.iter().map(|ty| ty.instantiate(&subst)).collect(),
                            )
                        })
                        .collect();
                }
            }
            TypeId::Struct(owner) => {
                if let Some(sig) = self.checker.aggregates.structure(&owner.declaration) {
                    let subst = substitution(owner, &sig.generic_params);
                    return vec![(
                        Constructor::Product,
                        sig.fields
                            .iter()
                            .map(|f| {
                                self.checker.aggregates.normalize_type(
                                    self.checker.declarations.definitions(),
                                    &f.ty.instantiate(&subst),
                                )
                            })
                            .collect(),
                    )];
                }
            }
            TypeId::Tuple(items) => return vec![(Constructor::Product, items.clone())],
            TypeId::Builtin(BuiltinType::Unit) => return vec![(Constructor::Product, vec![])],
            TypeId::Builtin(BuiltinType::Never) => return vec![],
            TypeId::Builtin(builtin) => {
                let bounds = match builtin {
                    BuiltinType::Bool => Some((0, 1)),
                    BuiltinType::Char => Some((0, char_rank(char::MAX))),
                    ty => ty.integer_bounds(),
                };
                if let Some((min, max)) = bounds {
                    // Coverage is constant between consecutive literal/range boundaries.
                    let mut points = vec![min];
                    for (head, _) in heads {
                        if let Some((lo, hi)) = head.and_then(|id| self.interval(id)) {
                            points.push(lo);
                            if hi < max {
                                points.push(hi + 1);
                            }
                        }
                    }
                    points.sort_unstable();
                    points.dedup();
                    return points
                        .into_iter()
                        .map(|p| (Constructor::Point(p), vec![]))
                        .collect();
                }
            }
            _ => {}
        }
        // String and float literals cannot cover all values (including NaN).
        // Rows covering the remainder also cover every listed literal.
        vec![(Constructor::Other, vec![])]
    }

    fn specialize(&self, cell: Cell, ctor: Constructor, arity: usize) -> Option<Row> {
        let Some(id) = cell else {
            return Some(vec![None; arity]);
        };
        match (ctor, self.kind(id)) {
            (Constructor::Variant(v), kind)
                if self.checker.type_table.pattern_variant(id) == Some(&v) =>
            {
                let fields = match kind {
                    PatternKind::EnumVariant { fields, .. } => fields.elements.as_slice(),
                    PatternKind::Struct { fields, .. } => {
                        let mut row = vec![None; arity];
                        for (field, &slot) in fields
                            .iter()
                            .zip(self.checker.type_table.pattern_slots(id)?)
                        {
                            *row.get_mut(slot)? = Some(field.pattern);
                        }
                        return Some(row);
                    }
                    PatternKind::Name { .. } => &[],
                    _ => return None,
                };
                self.positional_row(id, fields, arity)
            }
            (Constructor::Product, PatternKind::Tuple(fields)) => {
                self.positional_row(id, &fields.elements, arity)
            }
            (Constructor::Product, PatternKind::Struct { fields, .. }) => {
                let mut row = vec![None; arity];
                for (field, id) in fields
                    .iter()
                    .zip(self.checker.type_table.pattern_fields(id)?)
                {
                    row[self.checker.aggregates.field(id)?.slot] = Some(field.pattern);
                }
                Some(row)
            }
            (Constructor::Point(p), _)
                if self.interval(id).is_some_and(|(lo, hi)| lo <= p && p <= hi) =>
            {
                Some(vec![])
            }
            _ => None,
        }
    }

    fn positional_row(&self, id: PatternId, fields: &[PatternId], arity: usize) -> Option<Row> {
        let slots = self.checker.type_table.pattern_slots(id)?;
        let mut row = vec![None; arity];
        for (&field, &slot) in fields.iter().zip(slots) {
            *row.get_mut(slot)? = Some(field);
        }
        Some(row)
    }

    fn interval(&self, id: PatternId) -> Option<(i128, i128)> {
        if let PatternKind::Range { inclusive, .. } = self.kind(id) {
            let (lo, hi) = self.checker.type_table.pattern_range(id)?;
            Some((rank(lo)?, rank(hi)? - i128::from(!inclusive)))
        } else {
            let p = rank(self.checker.type_table.pattern_scalar_value(id)?)?;
            Some((p, p))
        }
    }

    fn empty_product(
        &mut self,
        types: &[TypeId],
        empty_params: &HashSet<GenericParameterType>,
        visiting: &mut Vec<DefinitionId>,
    ) -> Result<bool, Stop> {
        for ty in types {
            if self.empty_type(ty, empty_params, visiting)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // Only prove emptiness from finite declaration evidence. An opaque, generic
    // or recursive edge is not evidence that a value cannot exist.
    fn empty_type(
        &mut self,
        ty: &TypeId,
        empty_params: &HashSet<GenericParameterType>,
        visiting: &mut Vec<DefinitionId>,
    ) -> Result<bool, Stop> {
        self.spend(1)?;
        match ty {
            TypeId::Builtin(BuiltinType::Never) => Ok(true),
            TypeId::Tuple(types) => self.empty_product(types, empty_params, visiting),
            TypeId::Generic(param) => Ok(empty_params.contains(param)),
            TypeId::Enum(owner) | TypeId::Struct(owner)
                if visiting.len() < 32 && !visiting.contains(&owner.declaration) =>
            {
                // Inhabitation depends on whether each parameter has values, not
                // the size of its instantiated type tree. Do not materialize a
                // chain of increasingly large generic payload types here.
                let enumeration = self.checker.aggregates.enumeration(&owner.declaration);
                let structure = self.checker.aggregates.structure(&owner.declaration);
                let parameters = enumeration
                    .map(|e| &e.generic_params)
                    .or_else(|| structure.map(|s| &s.generic_params));
                let Some(parameters) = parameters else {
                    return Ok(false);
                };
                let mut nested = HashSet::default();
                for (param, arg) in parameters.iter().zip(&owner.arguments) {
                    if self.empty_type(arg, empty_params, visiting)? {
                        nested.insert(param.clone());
                    }
                }
                visiting.push(owner.declaration);
                let empty = if let Some(sig) = enumeration {
                    let mut empty = true;
                    for v in &sig.variants {
                        if !self.empty_product(&v.payload, &nested, visiting)? {
                            empty = false;
                            break;
                        }
                    }
                    empty
                } else if let Some(sig) = structure {
                    let mut empty = false;
                    for field in &sig.fields {
                        if self.empty_type(&field.ty, &nested, visiting)? {
                            empty = true;
                            break;
                        }
                    }
                    empty
                } else {
                    false
                };
                visiting.pop();
                Ok(empty)
            }
            _ => Ok(false),
        }
    }
}

// The work stack pops from the end. Keep writable slots at the front in
// exactly the same order for the type columns and each specialized arm row.
fn extend_fields<T>(mut tail: Vec<T>, fields: Vec<T>, writable: &[bool]) -> Vec<T> {
    if !writable.contains(&true) {
        tail.extend(fields);
        return tail;
    }
    let mut deferred = Vec::new();
    for (field, &is_var) in fields.into_iter().zip(writable) {
        if is_var {
            deferred.push(field);
        } else {
            tail.push(field);
        }
    }
    deferred.append(&mut tail);
    deferred
}

fn substitution(owner: &NominalType, parameters: &[GenericParameterType]) -> TypeSubstitution {
    parameters
        .iter()
        .cloned()
        .zip(owner.arguments.iter().cloned())
        .collect()
}

fn rank(value: &ScalarValue) -> Option<i128> {
    match value {
        ScalarValue::Integer { value, .. } => Some(*value),
        ScalarValue::Bool(value) => Some(i128::from(*value)),
        ScalarValue::Char(value) => Some(char_rank(*value)),
        _ => None,
    }
}

fn char_rank(c: char) -> i128 {
    let code = u32::from(c);
    i128::from(if code >= 0xe000 { code - 0x800 } else { code })
}
