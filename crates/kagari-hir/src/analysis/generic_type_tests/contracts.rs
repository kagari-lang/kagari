//! Ordinary application declarations used to exercise inference and recovery.
//! These bodies are test inputs, not a second implementation of library algorithms.
use crate::{AnalysisResult, AnalyzedModule, analyze_source};
use kagari_source::source::SourceFile;

const CONTRACTS: &str = r#"use std::collections::{HashMap, List, Map, Set};
use std::hash::{Hash};

fn take_bool(value: bool, message: String) {}
fn ordered_pair<T: OrderedNumber>(left: T, right: T) -> T { left }
fn ordered_three<T: OrderedNumber>(value: T, min: T, max: T) -> T { value }
fn signed_value<T: SignedNumber>(value: T) -> T { value }
fn check_equal<T: PartialEq>(left: T, right: T, message: String) -> bool { left == right }
fn put<T>(values: Vec<T>, value: T) { values.push(value); }
fn list_count<T>(values: List<T>) -> usize { values.len() }
fn map_count<K: Eq + Hash, V>(values: Map<K,V>) -> usize { values.len() }
fn set_count<T: Eq + Hash>(values: Set<T>) -> usize { values.len() }
fn take_text(value: String) {}
fn text_pair(value: String, other: String) {}
fn option_present<T>(value: Option<T>) -> bool { match value { Some(x) => true, None => false } }
fn result_present<T,E>(value: Result<T,E>) -> bool { match value { Ok(x) => true, Err(e) => false } }
fn get_or<T>(value: Option<T>, fallback: T) -> T { match value { Some(x) => x, None => fallback } }
fn result_or<T,E>(value: Result<T,E>, fallback: T) -> T { match value { Ok(x) => x, Err(e) => fallback } }
fn same_set<T: Eq + Hash>(left: Set<T>, right: Set<T>) -> Set<T> { left }
fn put_map<K: Eq + Hash,V>(values: HashMap<K,V>, key: K, value: V) { values.insert(key, value); }
"#;

pub(super) fn analyze_contracts(source: &SourceFile) -> AnalysisResult<AnalyzedModule> {
    analyze_source(&SourceFile::new(
        source.name(),
        format!("{CONTRACTS}\n{}", source.text()),
    ))
}
