//! String owns its constructor, complete method definitions and scoped UTF-8 bodies.
use crate::{
    foundation::{
        Entry, arrays::operations::index_argument, default_impl, enumeration, interface, type_docs,
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

pub(crate) const TYPE_DOCS: (&str, &str) = (
    "An immutable UTF-8 string. len counts bytes and slice requires valid byte boundaries. Transformations produce strings without changing the receiver. Explicit chars, char_indices and bytes methods return lazy cursors; String itself is not Iterable and has no indexing operator.",
    "fn greeting() -> String { \"  hello  \".trim() }",
);

fn documented(method: MethodDecl, text: &str, name: &str, arguments: &str) -> MethodDecl {
    method.documentation(format!("{text}\n\n# Examples\n\n```kgr\nfn example(values: String) {{ val result = values.{name}({arguments}); }}\n```"))
}

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
        methods.define(documented(MethodDecl::instance("len").returns(Type::u64()), "Return the UTF-8 byte length.", "len", "")
            .implementation_send(NativeBinding::primitive(NativePrimitive::StringByteLength)))?;
        for (name, parameters, result, entry, description, example) in [
            ("is_empty", vec![], Type::bool(), is_empty as Entry, "Return whether the string has no bytes.", ""),
            ("contains", vec![("pattern", string.clone())], Type::bool(), contains, "Test for a literal substring; the empty pattern matches.", "\"a\""),
            ("starts_with", vec![("pattern", string.clone())], Type::bool(), starts_with, "Test for a literal prefix; the empty pattern matches.", "\"a\""),
            ("ends_with", vec![("pattern", string.clone())], Type::bool(), ends_with, "Test for a literal suffix; the empty pattern matches.", "\"a\""),
            ("find", vec![("pattern", string.clone())], option, find, "Return the first matching byte offset, or None. An empty pattern matches at zero.", "\"a\""),
            ("slice", vec![("start", Type::u64()), ("end", Type::u64())], string.clone(), slice, "Copy the half-open byte range. Invalid order, bounds or UTF-8 boundaries trap.", "0, 1"),
            ("trim", vec![], string.clone(), trim, "Remove Unicode whitespace from both ends.", ""),
            ("trim_start", vec![], string.clone(), trim_start, "Remove leading Unicode whitespace.", ""),
            ("trim_end", vec![], string.clone(), trim_end, "Remove trailing Unicode whitespace.", ""),
            ("replace", vec![("from", string.clone()), ("to", string.clone())], string.clone(), replace, "Replace all non-overlapping literal matches. An empty pattern inserts at Unicode scalar boundaries, including both ends.", "\"a\", \"b\""),
        ] {
            let mut method = MethodDecl::instance(name).returns(result.clone());
            let mut codecs = vec![Codec::Exact(string.clone())];
            for (name, ty) in parameters { codecs.push(Codec::Exact(ty.clone())); method = method.parameter(name, ty); }
            methods.define(documented(method, description, name, example)
                .implementation_send(NativeBinding::new(codecs, Codec::Exact(result), entry)))?;
        }
        for (name, kind, item, description) in [
            ("chars", StringIterKind::Chars, Type::scalar(BuiltinType::Char), "Traverse Unicode scalars as char, not grapheme clusters. Combining marks and NUL are separate scalar items."),
            ("char_indices", StringIterKind::CharIndices, Type::tuple([Type::u64(), Type::scalar(BuiltinType::Char)])?, "Traverse (u64, char) pairs of UTF-8 byte offsets and Unicode scalars. For aé🙂 the offsets are 0, 1 and 3, not scalar ordinals."),
            ("bytes", StringIterKind::Bytes, Type::scalar(BuiltinType::U8), "Traverse UTF-8 encoded bytes as u8. For aé🙂 there are seven bytes but three Unicode scalars."),
        ] {
            let result = Type::iterator(item)?;
            let description = format!("{description} Return a fresh lazy CollectionCursor on each call, retaining the source String. Cursor copies share progress; next returns None at exhaustion. Iterable adapters consume this cursor lazily without an intermediate List.");
            methods.define(documented(MethodDecl::instance(name).returns(result.clone()), &description, name, "")
                .implementation_send(NativeBinding::new(vec![Codec::Exact(string.clone())], Codec::Exact(result), move |cx| cx.iter_operation(0, IterOp::String(kind)))))?;
        }
        methods.define(documented(MethodDecl::instance("split").parameter("separator", string.clone()).returns(list),
            "Return an eager List<String> of literal-separated fields, preserving endpoint empties. An empty separator splits at Unicode scalar boundaries with endpoint empties.", "split", "\"a\"")
            .produces(vector.clone()).implementation_send(NativeBinding::new(vec![Codec::Exact(string.clone()), Codec::Exact(string.clone())], Codec::Exact(vector), split)))?;
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

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Immutable UTF-8 strings. Length and offsets count bytes; slicing validates UTF-8 boundaries. Explicit lazy chars, char_indices and bytes cursors traverse scalars, byte-offset/scalar pairs and encoded bytes respectively.",
    "fn clean(text: String) -> String { text.trim() }",
);
