//! Diagnostic spelling borrows names from the semantic snapshot that owns the IDs.
use crate::types::TypeId;
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_types::{range::RangeKind, surface as standard_surface};

fn name(definitions: &DefinitionTable, id: DefinitionId) -> &str {
    definitions
        .resolve(id)
        .expect("scoped diagnostic identity")
        .last()
        .expect("named diagnostic identity")
        .name
}

impl TypeId {
    /// Formats a diagnostic type spelling using borrowed names from its retained definition table.
    ///
    /// This is not a unique serialized identity or guaranteed round-trippable source.
    /// Associated members appear in canonical identity order, independent of allocation order.
    ///
    /// # Panics
    ///
    /// Panics if a named identity is absent from `definitions` or names a module root.
    pub fn display_name(&self, definitions: &DefinitionTable) -> String {
        enum Part<'a> {
            Type(&'a TypeId),
            Text(&'a str),
        }

        fn sequence<'a>(
            pending: &mut Vec<Part<'a>>,
            args: &'a [TypeId],
            open: &'a str,
            close: &'a str,
        ) {
            pending.push(Part::Text(close));
            for (index, ty) in args.iter().enumerate().rev() {
                pending.push(Part::Type(ty));
                if index > 0 {
                    pending.push(Part::Text(", "));
                }
            }
            pending.push(Part::Text(open));
        }

        let mut output = String::new();
        let mut pending = vec![Part::Type(self)];
        while let Some(part) = pending.pop() {
            match part {
                Part::Text(text) => output.push_str(text),
                Part::Type(ty) => match ty {
                    Self::Reference { target, access } => {
                        output.push_str(access.name());
                        output.push('<');
                        pending.push(Part::Text(">"));
                        pending.push(Part::Type(target));
                    }
                    Self::Inference(_) => output.push('_'),
                    Self::Unknown => output.push_str("<unknown>"),
                    Self::Error => output.push_str("<error>"),
                    Self::Host(id) => output.push_str(name(definitions, *id)),
                    Self::Builtin(ty) => output.push_str(
                        standard_surface::builtin_type_spec(*ty)
                            .map_or("<builtin>", |spec| spec.name),
                    ),
                    Self::Tuple(items) => sequence(&mut pending, items, "(", ")"),
                    Self::Function { params, result } => {
                        pending.push(Part::Type(result));
                        pending.push(Part::Text(" -> "));
                        sequence(&mut pending, params, "fn(", ")");
                    }
                    Self::Iter(item) => {
                        pending.push(Part::Text(">"));
                        pending.push(Part::Type(item));
                        pending.push(Part::Text("Iter<"));
                    }
                    Self::Array(item) => {
                        pending.push(Part::Text("]"));
                        pending.push(Part::Type(item));
                        pending.push(Part::Text("["));
                    }

                    Self::NativeObject(nominal)
                    | Self::Struct(nominal)
                    | Self::Enum(nominal)
                    | Self::Trait(nominal) => {
                        if !nominal.arguments.is_empty() || !nominal.associated_types.is_empty() {
                            pending.push(Part::Text(">"));
                            let mut members = nominal.associated_types.iter().collect::<Vec<_>>();
                            members.sort_by(|(left, _), (right, _)| {
                                definitions
                                    .compare(**left, **right)
                                    .expect("scoped associated members")
                            });
                            for (index, (member, ty)) in members.into_iter().enumerate().rev() {
                                pending.push(Part::Type(ty));
                                pending.push(Part::Text(" = "));
                                pending.push(Part::Text(name(definitions, *member)));
                                if index > 0 || !nominal.arguments.is_empty() {
                                    pending.push(Part::Text(", "));
                                }
                            }
                            for (index, ty) in nominal.arguments.iter().enumerate().rev() {
                                pending.push(Part::Type(ty));
                                if index > 0 {
                                    pending.push(Part::Text(", "));
                                }
                            }
                            pending.push(Part::Text("<"));
                        }
                        pending.push(Part::Text(name(definitions, nominal.declaration)));
                    }
                    Self::Generic(parameter) => output.push_str(&parameter.name),
                    Self::SelfType(_) => output.push_str("Self"),
                    Self::Projection {
                        receiver,
                        interface,
                        member,
                        arguments,
                    } => {
                        output.push_str(&format!(
                            "<{} as {}>::{}",
                            receiver.display_name(definitions),
                            name(definitions, interface.declaration),
                            name(definitions, *member)
                        ));
                        if !arguments.is_empty() {
                            output.push('<');
                            output.push_str(
                                &arguments
                                    .iter()
                                    .map(|ty| ty.display_name(definitions))
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            );
                            output.push('>');
                        }
                    }
                    Self::Range(item, kind) => {
                        if *kind != RangeKind::Full {
                            pending.push(Part::Text(">"));
                            pending.push(Part::Type(item));
                            pending.push(Part::Text("<"));
                        }
                        pending.push(Part::Text(kind.name()));
                    }
                },
            }
        }
        output
    }
}
