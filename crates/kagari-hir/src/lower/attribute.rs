//! Attribute payload parsing and attachment to already-known declaration identities.

use crate::{
    hir::attribute::{
        AttributeArgument, AttributeFact, AttributeForm, AttributePayload, AttributeTarget,
        AttributeValue,
    },
    lower::context::{Lowerer, syntax_span, token_span},
};
use kagari_syntax::ast::{
    borrow::{Ast, AttributeOwner, NodeData},
    item::{
        Attribute, AttributeArg, AttributePayload as AstAttributePayload,
        AttributeValue as AstAttributeValue,
    },
    misc::Path,
    site::{Required, SeparatedList},
};

impl Lowerer {
    pub(crate) fn attach_attributes<T: AttributeOwner + NodeData>(
        &mut self,
        owner: Ast<'_, T>,
        target: AttributeTarget,
    ) {
        self.source_map
            .insert_attribute_target(target, syntax_span(&owner));
        for attribute in owner.attributes() {
            if self.cancel.check().is_err() {
                break;
            }
            self.module.attributes.push(AttributeFact {
                target,
                name: attribute.name_text().unwrap_or_default(),
                form: attribute_form(attribute),
                span: token_span(&attribute),
            });
        }
    }
}

fn attribute_argument(arg: Ast<'_, AttributeArg>) -> AttributeArgument {
    let value = match arg.value() {
        Some(value) => attribute_value(value),
        None => AttributeValue::Missing,
    };
    AttributeArgument {
        name: arg.name_text(),
        value,
    }
}

fn attribute_value(value: Ast<'_, AstAttributeValue>) -> AttributeValue {
    match value.data() {
        AstAttributeValue::Literal(_) => AttributeValue::Literal(value.source_text().to_owned()),
        AstAttributeValue::Path(path) if attribute_path_is_complete(path) => {
            AttributeValue::Path(value.source_text().to_owned())
        }
        AstAttributeValue::List(list)
            if list.close.as_ref().is_some()
                && value.unexpected().next().is_none()
                && attribute_separators_are_present(&list.elements) =>
        {
            AttributeValue::List(attribute_arguments(&list.elements, value.elements()))
        }
        AstAttributeValue::Path(_) | AstAttributeValue::List(_) | AstAttributeValue::Error(_) => {
            AttributeValue::Malformed
        }
    }
}

fn attribute_path_is_complete(path: &Path) -> bool {
    match path {
        Path::Single(_) => true,
        Path::Segmented(path) => path
            .rest
            .iter()
            .all(|segment| segment.name.as_ref().is_some()),
    }
}

fn attribute_separators_are_present(list: &SeparatedList<AttributeArg>) -> bool {
    list.entries
        .iter()
        .all(|entry| !matches!(entry.separator, Some(Required::Missing { .. })))
}

fn attribute_arguments<'a>(
    list: &SeparatedList<AttributeArg>,
    mut arguments: impl Iterator<Item = Ast<'a, AttributeArg>>,
) -> Vec<AttributeArgument> {
    // Typed iterators omit missing entries; retain their original list positions.
    list.entries
        .iter()
        .map(|entry| match entry.element {
            Required::Present(_) => attribute_argument(arguments.next().expect("present argument")),
            Required::Missing { .. } => AttributeArgument {
                name: None,
                value: AttributeValue::Missing,
            },
        })
        .collect()
}

fn attribute_form(attribute: Ast<'_, Attribute>) -> AttributeForm {
    let data = attribute.data();
    let mut malformed = data.open.as_ref().is_none()
        || data.close.as_ref().is_none()
        || !data.path.as_ref().is_some_and(attribute_path_is_complete)
        || attribute.unexpected().next().is_some();
    // Dispatch on the raw payload: a written `=` with no value is not bare syntax.
    let payload = match &data.payload {
        None => AttributePayload::Bare,
        Some(AstAttributePayload::Value { value, .. }) => AttributePayload::Value(match value {
            Required::Missing { .. } => AttributeValue::Missing,
            Required::Present(_) => {
                attribute_value(attribute.value().expect("present assigned value"))
            }
        }),
        Some(AstAttributePayload::Arguments(args)) => {
            let view = attribute.args().expect("argument payload");
            malformed |= args.close.as_ref().is_none()
                || view.unexpected().next().is_some()
                || !attribute_separators_are_present(&args.arguments);
            AttributePayload::Arguments(attribute_arguments(&args.arguments, view.arguments()))
        }
    };
    if malformed {
        AttributeForm::Malformed {
            recovered: Some(payload),
        }
    } else {
        match payload {
            AttributePayload::Bare => AttributeForm::Bare,
            AttributePayload::Value(value) => AttributeForm::Value(value),
            AttributePayload::Arguments(arguments) => AttributeForm::Arguments(arguments),
        }
    }
}
