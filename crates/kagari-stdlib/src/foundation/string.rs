//! String owns its constructor, complete method definitions and scoped UTF-8 bodies.
mod documentation;
use crate::{
    foundation::{
        Entry, arrays::operations::index_argument, default_impl, enumeration, interface,
        string::documentation as docs, type_docs,
    },
    identity,
};
use kagari_common::identity::DefinitionKind;
use kagari_contract::operations::{IterOp, StringIterKind};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        assembly::draft::ModuleDraft,
        binding::{Codec, NativeBinding, NativeResult},
        context::CallContext,
        declarations::MethodDecl,
        primitive::NativePrimitive,
        types::Type,
    },
    value::Value,
};
use kagari_types::{
    declaration::{
        TypeDefKind,
        native::{NativeStorageLayout, NativeTypeConstructor},
    },
    scalar::BuiltinType,
};
use std::cell::Ref;

pub(crate) const TYPE_DOCS: (&str, &str) = docs::OVERVIEW;

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let reference = module.define_constructor("String", NativeTypeConstructor::String)?;
    type_docs(module, &reference, "String", TYPE_DOCS, &[])?;
    let string = Type::scalar(BuiltinType::String);
    let option = enumeration(module, "Option", 1)?.apply([Type::u64()])?;
    let list = interface(module, "List", 1)?.apply([string.clone()])?;
    let vector = module
        .reference_type(
            identity::type_id("Vec", DefinitionKind::AssociatedType),
            TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element: 0 }),
            1,
        )?
        .apply([string.clone()])?;
    module.inherent(string.clone(), |methods| {
        methods.define(
            MethodDecl::instance("len")
                .returns(Type::u64())
                .documentation(docs::method("len"))
                .implementation_send(NativeBinding::primitive(NativePrimitive::StringByteLength)),
        )?;
        for (name, parameters, result, entry) in [
            ("is_empty", vec![], Type::bool(), is_empty as Entry),
            (
                "contains",
                vec![("pattern", string.clone())],
                Type::bool(),
                contains,
            ),
            (
                "starts_with",
                vec![("pattern", string.clone())],
                Type::bool(),
                starts_with,
            ),
            (
                "ends_with",
                vec![("pattern", string.clone())],
                Type::bool(),
                ends_with,
            ),
            ("find", vec![("pattern", string.clone())], option, find),
            (
                "slice",
                vec![("start", Type::u64()), ("end", Type::u64())],
                string.clone(),
                slice,
            ),
            ("trim", vec![], string.clone(), trim),
            ("trim_start", vec![], string.clone(), trim_start),
            ("trim_end", vec![], string.clone(), trim_end),
            (
                "replace",
                vec![("from", string.clone()), ("to", string.clone())],
                string.clone(),
                replace,
            ),
        ] {
            let mut method = MethodDecl::instance(name)
                .returns(result.clone())
                .documentation(docs::method(name));
            let mut codecs = vec![Codec::Exact(string.clone())];
            for (name, ty) in parameters {
                codecs.push(Codec::Exact(ty.clone()));
                method = method.parameter(name, ty);
            }
            methods.define(method.implementation_send(NativeBinding::new(
                codecs,
                Codec::Exact(result),
                entry,
            )))?;
        }
        for (name, kind, item) in [
            (
                "chars",
                StringIterKind::Chars,
                Type::scalar(BuiltinType::Char),
            ),
            (
                "char_indices",
                StringIterKind::CharIndices,
                Type::tuple([Type::u64(), Type::scalar(BuiltinType::Char)])?,
            ),
            (
                "bytes",
                StringIterKind::Bytes,
                Type::scalar(BuiltinType::U8),
            ),
        ] {
            let result = Type::iterator(item)?;
            methods.define(
                MethodDecl::instance(name)
                    .returns(result.clone())
                    .documentation(docs::method(name))
                    .implementation_send(NativeBinding::new(
                        vec![Codec::Exact(string.clone())],
                        Codec::Exact(result),
                        move |cx| cx.iter_operation(0, IterOp::String(kind)),
                    )),
            )?;
        }
        methods.define(
            MethodDecl::instance("split")
                .parameter("separator", string.clone())
                .returns(list)
                .documentation(docs::method("split"))
                .produces(vector.clone())
                .implementation_send(NativeBinding::new(
                    vec![Codec::Exact(string.clone()), Codec::Exact(string.clone())],
                    Codec::Exact(vector),
                    split,
                )),
        )?;
        Ok(())
    })?;
    default_impl(
        module,
        string.clone(),
        NativeBinding::new(vec![], Codec::Exact(string), |cx| {
            cx.encode_result(String::new())
        }),
        "Return an empty immutable String.",
    )
}

fn text<'a>(cx: &'a CallContext<'_>, index: usize) -> NativeResult<Ref<'a, str>> {
    let Value::Str(id) = cx.argument(index)? else {
        return Err(RuntimeError::module_validation("string argument"));
    };
    cx.heap()
        .string(id)
        .ok_or_else(|| RuntimeError::module_validation("invalid string argument"))
}

fn is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(Value::Bool(text(cx, 0)?.is_empty()))
}

fn contains(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(Value::Bool(text(cx, 0)?.contains(&*text(cx, 1)?)))
}

fn starts_with(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(Value::Bool(text(cx, 0)?.starts_with(&*text(cx, 1)?)))
}

fn ends_with(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(Value::Bool(text(cx, 0)?.ends_with(&*text(cx, 1)?)))
}

fn find(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = text(cx, 0)?.find(&*text(cx, 1)?).map(|index| index as u64);
    cx.encode_result(result)
}

fn slice(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let start = index_argument(cx, 1)?;
    let end = index_argument(cx, 2)?;
    let source = text(cx, 0)?;
    let result = usize::try_from(start)
        .ok()
        .zip(usize::try_from(end).ok())
        .and_then(|(start, end)| source.get(start..end))
        .map(str::to_owned)
        .ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::IndexOutOfBounds,
                format!("String slice {start}..{end} requires ordered byte offsets within bounds and on UTF-8 boundaries"),
            )
        })?;
    drop(source);
    cx.encode_result(result)
}

fn trim(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = text(cx, 0)?.trim().to_owned();
    cx.encode_result(result)
}

fn trim_start(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = text(cx, 0)?.trim_start().to_owned();
    cx.encode_result(result)
}

fn trim_end(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = text(cx, 0)?.trim_end().to_owned();
    cx.encode_result(result)
}

fn replace(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = text(cx, 0)?.replace(&*text(cx, 1)?, &text(cx, 2)?);
    cx.encode_result(result)
}

fn split(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let parts = {
        let source = text(cx, 0)?;
        let separator = text(cx, 1)?;
        let mut parts = Vec::new();
        for part in source.split(&*separator) {
            cx.poll()?;
            parts
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("native collected values"))?;
            parts.push(part.to_owned());
        }
        cx.poll()?;
        parts
    };
    cx.encode_result(parts)
}

pub(super) const MODULE_DOCS: (&str, &str) = docs::OVERVIEW;
