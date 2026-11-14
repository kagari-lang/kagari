//! String owns its constructor, complete method definitions and scoped UTF-8 bodies.
use crate::foundation::{arrays::operations::index_argument, default_impl, enumeration, type_docs};
use kagari_runtime::{
    error::{RuntimeError, reason::IndexError},
    native::{
        assembly::draft::ModuleDraft,
        binding::{Codec, NativeBinding, NativeResult},
        context::NativeCall,
        declarations::MethodDecl,
        primitive::NativePrimitive,
        types::Type,
    },
    value::view::ValueView,
};

use kagari_types::{declaration::native::NativeTypeConstructor, scalar::BuiltinType};
type Entry = for<'call> fn(&mut NativeCall<'call>) -> NativeResult<()>;

pub(crate) const TYPE_DOCS: (&str, &str) = (
    "An immutable UTF-8 string. len counts bytes and slice requires valid byte boundaries. Transformations produce strings without changing the receiver. String has no indexing operator; concrete traversal providers are installed separately.",
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
                .implementation_send(NativeBinding::scoped(codecs, Codec::Exact(result), entry)))?;
        }


        Ok(())
    })?;
    default_impl(
        module,
        string.clone(),
        NativeBinding::scoped(vec![], Codec::Exact(string), |cx| {
            cx.encode_result(String::new())
        }),
        "Return an empty immutable String.",
    )
}

fn text<R, const N: usize>(
    cx: &mut NativeCall<'_>,
    indices: [usize; N],
    read: impl FnOnce([&str; N]) -> R,
) -> NativeResult<R> {
    let mut values = [ValueView::unit(); N];
    for (value, index) in values.iter_mut().zip(indices) {
        *value = cx.argument(index)?;
    }
    cx.read_strings(&values, read)
}

fn is_empty(cx: &mut NativeCall<'_>) -> NativeResult<()> {
    let empty = text(cx, [0], |[text]| text.is_empty())?;
    cx.set_result(empty.into())
}

fn contains(cx: &mut NativeCall<'_>) -> NativeResult<()> {
    let result = text(cx, [0, 1], |[text, pattern]| text.contains(pattern))?;
    cx.set_result(result.into())
}

fn starts_with(cx: &mut NativeCall<'_>) -> NativeResult<()> {
    let result = text(cx, [0, 1], |[text, pattern]| text.starts_with(pattern))?;
    cx.set_result(result.into())
}

fn ends_with(cx: &mut NativeCall<'_>) -> NativeResult<()> {
    let result = text(cx, [0, 1], |[text, pattern]| text.ends_with(pattern))?;
    cx.set_result(result.into())
}

fn find(cx: &mut NativeCall<'_>) -> NativeResult<()> {
    let result = text(cx, [0, 1], |[text, pattern]| {
        text.find(pattern).map(|index| index as u64)
    })?;
    cx.encode_result(result)
}

fn slice(cx: &mut NativeCall<'_>) -> NativeResult<()> {
    let start = index_argument(cx, 1)?;
    let end = index_argument(cx, 2)?;
    let result = text(cx, [0], |[source]| {
        usize::try_from(start).ok().zip(usize::try_from(end).ok())
            .and_then(|(start, end)| source.get(start..end)).map(str::to_owned)
    })?.ok_or_else(|| RuntimeError::from(IndexError::Provider(format!("String slice {start}..{end} requires ordered byte offsets within bounds and on UTF-8 boundaries"))))?;
    cx.encode_result(result)
}

fn trim(cx: &mut NativeCall<'_>) -> NativeResult<()> {
    let result = text(cx, [0], |[text]| text.trim().to_owned())?;
    cx.encode_result(result)
}

fn trim_start(cx: &mut NativeCall<'_>) -> NativeResult<()> {
    let result = text(cx, [0], |[text]| text.trim_start().to_owned())?;
    cx.encode_result(result)
}

fn trim_end(cx: &mut NativeCall<'_>) -> NativeResult<()> {
    let result = text(cx, [0], |[text]| text.trim_end().to_owned())?;
    cx.encode_result(result)
}

fn replace(cx: &mut NativeCall<'_>) -> NativeResult<()> {
    let result = text(cx, [0, 1, 2], |[text, from, to]| text.replace(from, to))?;
    cx.encode_result(result)
}

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Immutable UTF-8 strings. Length and offsets count bytes; slicing validates UTF-8 boundaries. Concrete traversal and split providers are installed separately.",
    "fn clean(text: String) -> String { text.trim() }",
);
