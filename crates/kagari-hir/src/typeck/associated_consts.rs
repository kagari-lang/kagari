//! Scalar associated constants share the ordinary const-safe evaluator.

use crate::{
    DiagnosticBuffer,
    aggregates::{AggregateCatalog, InherentConstSignature},
    collections::{HashMap, HashSet},
    declarations::Declarations,
    lower::LoweredModule,
    typeck::{
        inference,
        members::MemberQuery,
        state::TypeCheckState,
        table::TypeTable,
        ty::{TypeContext, resolve_type_in},
    },
    types::{TypeId, TypeSubstitution, semantic::ImplHeader},
};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, table::DefinitionId},
    span::Span,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::scalar::BuiltinType;

fn error(name: &str, reason: &str, span: Span) -> Diagnostic {
    Diagnostic::error(DiagnosticKind::InvalidAssociatedConst {
        name: name.into(),
        reason: reason.into(),
    })
    .with_span(span)
}

pub(crate) fn scalar_type(ty: &TypeId) -> bool {
    matches!(
        ty,
        TypeId::Builtin(
            BuiltinType::Unit
                | BuiltinType::Bool
                | BuiltinType::I8
                | BuiltinType::I16
                | BuiltinType::I32
                | BuiltinType::I64
                | BuiltinType::U8
                | BuiltinType::U16
                | BuiltinType::U32
                | BuiltinType::U64
                | BuiltinType::F32
                | BuiltinType::F64
                | BuiltinType::Char
        )
    )
}

pub(super) fn prepare(
    lowered: &LoweredModule,
    declarations: &Declarations,
    table: &mut TypeCheckState,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let owners = lowered
        .module
        .traits
        .iter()
        .map(|item| {
            (
                &item.associated_consts,
                &item.generic_params,
                Some(item.id),
                None,
            )
        })
        .chain(lowered.module.impls.iter().map(|item| {
            (
                &item.associated_consts,
                &item.generic_params,
                None,
                Some(item.id),
            )
        }));
    for (members, generics, self_type, implementation) in owners {
        let mut names = HashSet::default();
        for member in members {
            if cancel.check().is_err() {
                return;
            }
            let span = lowered
                .module
                .type_ref(member.name_ref)
                .origin
                .diagnostic_span();
            if !names.insert(&member.name) {
                diagnostics.push(error(&member.name, "duplicate declaration", span));
            }
            let ty = resolve_type_in(
                &lowered.module,
                member.ty,
                TypeContext {
                    declarations,
                    generics,
                    self_type,
                    implementation,
                },
                table,
                cancel,
            );
            if !scalar_type(&ty) {
                diagnostics.push(error(
                    &member.name,
                    "requires a v1 const-safe scalar type",
                    span,
                ));
            }
            if implementation.is_some() && member.initializer.is_none() {
                diagnostics.push(error(
                    &member.name,
                    "impl constants require an initializer",
                    span,
                ));
            }
        }
    }
}

/// Select a direct definition only after checking its typed receiver, requirements and visibility.
pub(super) fn select_inherent<'a>(
    query: &MemberQuery<'a>,
    name: &str,
    importer: &ModuleIdentity,
) -> Result<Option<(DefinitionId, &'a InherentConstSignature)>, &'static str> {
    let mut selected = None;
    let mut hidden = false;
    for (id, item) in query.aggregates.inherent_consts() {
        query.cancel.check().map_err(|_| "selection cancelled")?;
        if item.site.name != name {
            continue;
        }
        let mut substitution = TypeSubstitution::default();
        inference::infer(
            &item.owner,
            query.receiver,
            &item.generic_params,
            &mut substitution,
            query.cancel,
            None,
        )
        .map_err(|_| "selection cancelled")?;
        if item.owner.instantiate(&substitution) != *query.receiver
            || !item
                .generic_params
                .iter()
                .all(|p| substitution.contains_key(p))
            || !query.bounds_allow(&item.bounds, &substitution, None, false)
        {
            continue;
        }
        if !item.visibility.allows(
            query
                .definitions
                .resolve(id)
                .expect("scoped inherent constant")
                .module(),
            importer,
        ) {
            hidden = true;
            continue;
        }
        if selected.is_some() {
            return Err("ambiguous inherent constant");
        }
        selected = Some((id, item));
    }
    if selected.is_none() && hidden {
        Err("constant is not visible in this module")
    } else {
        Ok(selected)
    }
}

pub(crate) fn validate(
    lowered: &LoweredModule,
    declarations: &Declarations,
    catalog: &AggregateCatalog,
    table: &TypeTable,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let mut inherent_headers: HashMap<&str, Vec<ImplHeader>> = HashMap::default();
    for item in &lowered.module.impls {
        if cancel.check().is_err() {
            return;
        }
        if item.trait_ref.is_none() {
            if let Some(receiver) = item
                .for_type
                .and_then(|ty| table.type_ref(ty))
                .filter(|ty| !ty.ty.is_unresolved())
            {
                for member in &item.associated_consts {
                    if cancel.check().is_err() {
                        return;
                    }
                    let header = ImplHeader::lower(&receiver.ty, &[]);
                    let previous = inherent_headers.entry(&member.name).or_default();
                    if previous.iter().any(|other| header.overlaps(other)) {
                        diagnostics.push(error(
                            &member.name,
                            "duplicate inherent constant on overlapping receivers",
                            member.origin.diagnostic_span(),
                        ));
                    }
                    previous.push(header);
                }
            }
            continue;
        }
        let contract = declarations
            .impl_identity(item.id)
            .and_then(|id| catalog.implementation_signature(id))
            .and_then(|signature| catalog.trait_(&signature.trait_type.declaration));
        let mut defined = HashSet::default();
        for member in &item.associated_consts {
            let id = contract.and_then(|owner| {
                declarations.definitions().lookup_child(
                    owner.id,
                    DefinitionKind::Const,
                    &member.name,
                    0,
                )
            });
            let expected = id
                .as_ref()
                .and_then(|id| contract?.associated_consts.get(id));
            let span = lowered
                .module
                .type_ref(member.name_ref)
                .origin
                .diagnostic_span();
            let Some(expected) = expected else {
                diagnostics.push(error(
                    &member.name,
                    "expected a constant declared by the implemented trait",
                    span,
                ));
                continue;
            };
            defined.insert(id.expect("constant identity"));
            if table
                .type_ref(member.ty)
                .is_none_or(|ty| ty.ty != expected.ty)
            {
                diagnostics.push(error(
                    &member.name,
                    "type differs from the trait declaration",
                    span,
                ));
            }
        }
        if let Some(contract) = contract {
            for (id, member) in &contract.associated_consts {
                if member.initializer.is_none() && !defined.contains(id) {
                    diagnostics.push(error(
                        declarations
                            .definitions()
                            .resolve(*id)
                            .expect("scoped associated constant")
                            .last()
                            .expect("associated constant")
                            .name,
                        "missing definition in trait implementation",
                        lowered.module.impls[item.id.index()]
                            .origin
                            .diagnostic_span(),
                    ));
                }
            }
        }
    }
}
