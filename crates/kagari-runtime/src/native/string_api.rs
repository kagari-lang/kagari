//! Immutable String helpers and primitive parsing from actual Rust functions.
macro_rules! string_module {
    ($(($integer:ident, $rust:ty)),*; $($other:ty),*) => {
        #[kagari_native_macros::native_module("std::string", runtime = crate)]
        pub mod string {
            use std::string::String as RustString;
            use crate::{
                native::numeric_api::numeric::{$($integer),*},
                native_value::{NativeCall, NativeResult, NativeValue,
                    parse::ParseFailure, result::NativeResultValue,
                    text::{NativeTextBuffer, copy_text, size_error}},
            };

            /// An immutable, checked UTF-8 script string.
            #[native_type]
            pub struct String(RustString);
            impl From<RustString> for String {
                fn from(text: RustString) -> Self { Self(text) }
            }
            impl String {
                /// Borrow this adapter's owned text, without borrowing the script heap.
                pub fn as_str(&self) -> &str { &self.0 }
                /// Move out the owned Rust text.
                pub fn into_string(self) -> RustString { self.0 }
            }
            /// Recoverable parser failures; these are values rather than VM traps.
            #[native_type]
            pub type ParseError = ParseFailure;

            #[native_trait]
            pub trait FromStr {
                type Err: NativeValue;
                fn from_str(#[context] call: &NativeCall, text: RustString)
                    -> NativeResult<NativeResultValue<Self, Self::Err>>;
            }

            $(
                #[native_impl]
                impl FromStr for $integer {
                    type Err = ParseError;
                    /// Parse decimal input without whitespace, prefixes or separators.
                    fn from_str(#[context] call: &NativeCall, text: RustString)
                        -> NativeResult<NativeResultValue<Self, Self::Err>> {
                        call.charge_work(text.len() as u64)?;
                        let parsed = text.parse::<$rust>().map(Self).map_err(ParseFailure::integer);
                        NativeResultValue::from_result(call, parsed)
                    }
                }
            )*
            $(
                #[native_impl]
                impl FromStr for $other {
                    type Err = ParseError;
                    /// Parse the full primitive spelling, with no implicit trimming.
                    fn from_str(#[context] call: &NativeCall, text: RustString)
                        -> NativeResult<NativeResultValue<Self, Self::Err>> {
                        call.charge_work(text.len() as u64)?;
                        let parsed = if text.is_empty() { Err(ParseFailure::Empty) }
                            else { text.parse::<Self>().map_err(|_| ParseFailure::InvalidSyntax) };
                        NativeResultValue::from_result(call, parsed)
                    }
                }
            )*

            #[native_impl]
            impl String {
                /// Return the UTF-8 byte length.
                pub fn len_bytes(&self) -> usize { self.0.len() }
                /// Count Unicode scalar values, not grapheme clusters.
                pub fn len_chars(&self, #[context] call: &NativeCall) -> NativeResult<usize> {
                    scan(call, &[&self.0])?;
                    Ok(self.0.chars().count())
                }
                /// Test whether the immutable string contains no bytes.
                pub fn is_empty(&self) -> bool { self.0.is_empty() }
                /// Test whether every byte is ASCII; the empty string is ASCII.
                pub fn is_ascii(&self, #[context] call: &NativeCall) -> NativeResult<bool> {
                    scan(call, &[&self.0])?;
                    Ok(self.0.is_ascii())
                }
                /// Compare ASCII letters without case folding other Unicode scalars.
                pub fn eq_ignore_ascii_case(&self, other: RustString, #[context] call: &NativeCall) -> NativeResult<bool> {
                    scan(call, &[&self.0, &other])?;
                    Ok(self.0.eq_ignore_ascii_case(&other))
                }
                /// Concatenate immutable contents; overflow or allocation failure traps.
                pub fn concat(&self, rhs: RustString, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    let bytes = self.0.len().checked_add(rhs.len()).ok_or_else(size_error)?;
                    let mut buffer = NativeTextBuffer::new(call, bytes)?;
                    buffer.push_str(&self.0)?;
                    buffer.push_str(&rhs)?;
                    Ok(buffer.finish())
                }
                /// Test a UTF-8 substring; an empty needle matches.
                pub fn contains(&self, needle: RustString, #[context] call: &NativeCall) -> NativeResult<bool> {
                    scan(call, &[&self.0, &needle])?;
                    Ok(self.0.contains(&needle))
                }
                /// Test a byte-exact prefix; an empty prefix matches.
                pub fn starts_with(&self, needle: RustString, #[context] call: &NativeCall) -> NativeResult<bool> {
                    scan(call, &[&self.0, &needle])?;
                    Ok(self.0.starts_with(&needle))
                }
                /// Test a byte-exact suffix; an empty suffix matches.
                pub fn ends_with(&self, needle: RustString, #[context] call: &NativeCall) -> NativeResult<bool> {
                    scan(call, &[&self.0, &needle])?;
                    Ok(self.0.ends_with(&needle))
                }
                /// Check a UTF-8 boundary, including the end of the string.
                pub fn is_char_boundary(&self, index: usize) -> bool { self.0.is_char_boundary(index) }
                /// Return a byte range, or None for reversed, out-of-range or non-boundary indices.
                pub fn slice(&self, start: usize, end: usize, #[context] call: &NativeCall) -> NativeResult<Option<RustString>> {
                    self.0.get(start..end).map(|text| copy_text(call, text)).transpose()
                }
                /// Find the first matching byte offset; an empty needle matches at zero.
                pub fn find(&self, needle: RustString, #[context] call: &NativeCall) -> NativeResult<Option<usize>> {
                    scan(call, &[&self.0, &needle])?;
                    Ok(self.0.find(&needle))
                }
                /// Find the last matching byte offset; an empty needle matches at the byte length.
                pub fn rfind(&self, needle: RustString, #[context] call: &NativeCall) -> NativeResult<Option<usize>> {
                    scan(call, &[&self.0, &needle])?;
                    Ok(self.0.rfind(&needle))
                }
                /// Strip a matching prefix once, preserving all remaining scalars.
                pub fn strip_prefix(&self, prefix: RustString, #[context] call: &NativeCall) -> NativeResult<Option<RustString>> {
                    scan(call, &[&self.0, &prefix])?;
                    self.0.strip_prefix(&prefix).map(|text| copy_text(call, text)).transpose()
                }
                /// Strip a matching suffix once, preserving all remaining scalars.
                pub fn strip_suffix(&self, suffix: RustString, #[context] call: &NativeCall) -> NativeResult<Option<RustString>> {
                    scan(call, &[&self.0, &suffix])?;
                    self.0.strip_suffix(&suffix).map(|text| copy_text(call, text)).transpose()
                }
                /// Split the first match into two strings; an empty separator matches at zero.
                pub fn split_once(&self, separator: RustString, #[context] call: &NativeCall) -> NativeResult<Option<(RustString, RustString)>> {
                    scan(call, &[&self.0, &separator])?;
                    split_result(call, self.0.split_once(&separator))
                }
                /// Split the last match; an empty separator matches at the byte length.
                pub fn rsplit_once(&self, separator: RustString, #[context] call: &NativeCall) -> NativeResult<Option<(RustString, RustString)>> {
                    scan(call, &[&self.0, &separator])?;
                    split_result(call, self.0.rsplit_once(&separator))
                }
                /// Remove Unicode whitespace from both ends.
                pub fn trim(&self, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    scan(call, &[&self.0])?;
                    copy_text(call, self.0.trim())
                }
                /// Remove leading Unicode whitespace.
                pub fn trim_start(&self, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    scan(call, &[&self.0])?;
                    copy_text(call, self.0.trim_start())
                }
                /// Remove trailing Unicode whitespace.
                pub fn trim_end(&self, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    scan(call, &[&self.0])?;
                    copy_text(call, self.0.trim_end())
                }
                /// Lowercase ASCII letters without changing other scalars.
                pub fn to_ascii_lowercase(&self, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    scan(call, &[&self.0])?;
                    let mut text = copy_text(call, &self.0)?;
                    text.make_ascii_lowercase();
                    Ok(text)
                }
                /// Uppercase ASCII letters without changing other scalars.
                pub fn to_ascii_uppercase(&self, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    scan(call, &[&self.0])?;
                    let mut text = copy_text(call, &self.0)?;
                    text.make_ascii_uppercase();
                    Ok(text)
                }
                /// Apply context-sensitive Unicode lowercase mappings, without locale or normalization.
                pub fn to_lowercase(&self, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    scan(call, &[&self.0])?;
                    let text = self.0.to_lowercase();
                    call.charge_work(text.len() as u64)?;
                    Ok(text)
                }
                /// Apply Unicode uppercase mappings, including multi-scalar expansions.
                pub fn to_uppercase(&self, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    scan(call, &[&self.0, &self.0])?;
                    let bytes = self.0.chars().flat_map(char::to_uppercase).try_fold(0usize, |length, character| {
                        call.charge_work(0)?;
                        length.checked_add(character.len_utf8()).ok_or_else(size_error)
                    })?;
                    let mut buffer = NativeTextBuffer::new(call, bytes)?;
                    for character in self.0.chars().flat_map(char::to_uppercase) { buffer.push_char(character)?; }
                    Ok(buffer.finish())
                }
                /// Repeat contents count times. Empty input or zero count returns empty text; size/allocation failure traps.
                pub fn repeat(&self, count: usize, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    if self.0.is_empty() { return Ok(RustString::new()); }
                    let bytes = self.0.len().checked_mul(count).ok_or_else(size_error)?;
                    let mut buffer = NativeTextBuffer::new(call, bytes)?;
                    for _ in 0..count { buffer.push_str(&self.0)?; }
                    Ok(buffer.finish())
                }
                /// Replace non-overlapping matches; an empty pattern inserts at every scalar boundary, including both ends.
                pub fn replace(&self, from: RustString, to: RustString, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    replace_text(call, &self.0, &from, &to, usize::MAX)
                }
                /// Replace at most count matches from left to right; zero preserves contents.
                pub fn replacen(&self, from: RustString, to: RustString, count: usize, #[context] call: &NativeCall) -> NativeResult<RustString> {
                    replace_text(call, &self.0, &from, &to, count)
                }
            }

            fn scan(call: &NativeCall, texts: &[&str]) -> NativeResult<()> {
                let bytes = texts.iter().try_fold(0u64, |total, text| total.checked_add(text.len() as u64).ok_or_else(size_error))?;
                call.charge_work(bytes)
            }

            fn split_result(call: &NativeCall, split: Option<(&str, &str)>) -> NativeResult<Option<(RustString, RustString)>> {
                split.map(|(left, right)| Ok((copy_text(call, left)?, copy_text(call, right)?))).transpose()
            }

            fn replace_text(call: &NativeCall, text: &str, from: &str, to: &str, count: usize) -> NativeResult<RustString> {
                scan(call, &[text, text, from])?;
                let mut bytes = text.len();
                for (_, matched) in text.match_indices(from).take(count) {
                    call.charge_work(0)?;
                    bytes = bytes.checked_sub(matched.len()).and_then(|bytes| bytes.checked_add(to.len())).ok_or_else(size_error)?;
                }
                let mut buffer = NativeTextBuffer::new(call, bytes)?;
                let mut previous = 0;
                for (index, matched) in text.match_indices(from).take(count) {
                    buffer.push_str(&text[previous..index])?;
                    buffer.push_str(to)?;
                    previous = index + matched.len();
                }
                buffer.push_str(&text[previous..])?;
                Ok(buffer.finish())
            }
        }
    };
}

string_module! {
    (I8, i8), (I16, i16), (I32, i32), (I64, i64), (ISize, i64),
    (U8, u8), (U16, u16), (U32, u32), (U64, u64), (USize, u64);
    f32, f64, bool
}
