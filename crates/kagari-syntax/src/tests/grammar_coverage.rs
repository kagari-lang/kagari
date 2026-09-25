use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{lexer::lex, tests::common, token::TokenKind};

const GRAMMAR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/kagari.ebnf"
));
const INVENTORY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/syntax-coverage.tsv"
));
const BRANCHES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/syntax-branches.tsv"
));
const QUANTIFIERS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/syntax-quantifiers.tsv"
));

#[derive(Clone, Copy)]
struct GapCase {
    id: &'static str,
    source: &'static str,
}

const GAPS: &[GapCase] = &[
    GapCase {
        id: "attribute",
        source: "@tag fn main() {}",
    },
    GapCase {
        id: "binding_condition",
        source: "fn main() { if val x = 1 { x }; }",
    },
];

fn grammar_rules() -> BTreeSet<String> {
    let mut rules = BTreeSet::new();
    let mut pending = None;
    for line in GRAMMAR.lines() {
        let trimmed = line.trim();
        if let Some((left, _)) = trimmed.split_once("::=") {
            let name = if left.trim().is_empty() {
                pending.take().expect("split EBNF rule name")
            } else {
                left.trim().to_owned()
            };
            assert!(
                name.chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'),
                "invalid EBNF rule name: {name}"
            );
            assert!(rules.insert(name.clone()), "duplicate EBNF rule: {name}");
        } else if !line.starts_with(char::is_whitespace)
            && !trimmed.starts_with("(*")
            && trimmed
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            pending = Some(trimmed.to_owned());
        }
    }
    rules
}

#[test]
fn grammar_rule_inventory_is_complete_and_witnesses_still_parse() {
    let mut inventory = BTreeMap::new();
    let mut sources = BTreeSet::new();
    let known_gaps = GAPS.iter().map(|case| case.id).collect::<BTreeSet<_>>();
    for line in INVENTORY.lines().skip(1).filter(|line| !line.is_empty()) {
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 4, "invalid inventory row: {line}");
        let [rule, status, evidence, note] =
            <[&str; 4]>::try_from(columns).expect("four inventory columns");
        assert!(
            inventory.insert(rule.to_owned(), status).is_none(),
            "duplicate inventory row: {rule}"
        );
        match status {
            "covered" | "partial" => {
                if evidence != "-" {
                    assert!(evidence.starts_with("examples/"), "invalid witness: {line}");
                    sources.insert(evidence);
                } else {
                    assert_eq!(status, "partial", "covered rule needs a witness: {line}");
                }
                if status == "partial" {
                    assert_ne!(note, "-", "partial rule needs a gap note: {line}");
                }
            }
            "missing" => {
                let gap = evidence
                    .strip_prefix("gap:")
                    .expect("missing rule needs a gap case");
                assert!(known_gaps.contains(gap), "unknown gap case: {line}");
                assert_ne!(note, "-", "missing rule needs a note: {line}");
            }
            "unverified" => {
                assert_eq!(evidence, "-", "unverified rule has a witness: {line}");
            }
            _ => panic!("invalid coverage status: {line}"),
        }
    }
    let declared = grammar_rules();
    let accounted = inventory.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        declared, accounted,
        "update docs/syntax-coverage.tsv when EBNF changes"
    );

    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for path in sources {
        let source =
            std::fs::read_to_string(repository.join(path)).expect("coverage witness exists");
        let parse = common::parse(&source);
        assert!(
            parse.diagnostics().is_empty(),
            "{path}: {:?}",
            parse.diagnostics()
        );
    }
    let mut counts = BTreeMap::<&str, usize>::new();
    for status in inventory.values() {
        *counts.entry(status).or_default() += 1;
    }
    println!("EBNF rule inventory: {counts:?}");
}

#[test]
fn grammar_alternatives_are_accounted_for() {
    let productions = syntax_productions();
    let mut expected = BTreeMap::new();
    for (rule, alternatives) in productions {
        if alternatives.len() > 1 {
            for (index, production) in alternatives.into_iter().enumerate() {
                expected.insert((rule.clone(), index + 1), production);
            }
        }
    }
    let known_gaps = GAPS.iter().map(|case| case.id).collect::<BTreeSet<_>>();
    let mut actual = BTreeMap::new();
    let mut witnesses = BTreeSet::new();
    let mut counts = BTreeMap::<&str, usize>::new();
    for line in BRANCHES.lines().skip(1).filter(|line| !line.is_empty()) {
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 6, "invalid branch row: {line}");
        let [rule, index, status, evidence, note, production] =
            <[&str; 6]>::try_from(columns).expect("six branch columns");
        let index = index.parse::<usize>().expect("numeric branch index");
        assert!(
            actual
                .insert((rule.to_owned(), index), production.to_owned())
                .is_none(),
            "duplicate branch: {rule}#{index}"
        );
        *counts.entry(status).or_default() += 1;
        match status {
            "witnessed" => {
                assert!(evidence.starts_with("examples/"), "invalid witness: {line}");
                witnesses.insert(evidence);
            }
            "missing" => {
                let gap = evidence
                    .strip_prefix("gap:")
                    .expect("missing branch needs gap case");
                assert!(known_gaps.contains(gap), "unknown gap case: {line}");
                assert_ne!(note, "-", "missing branch needs a note: {line}");
            }
            "unverified" => assert_eq!(evidence, "-", "unverified branch has witness: {line}"),
            _ => panic!("invalid branch status: {line}"),
        }
    }
    assert_eq!(
        expected, actual,
        "update docs/syntax-branches.tsv when EBNF alternatives change"
    );
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for path in witnesses {
        let source = std::fs::read_to_string(repository.join(path)).expect("branch witness exists");
        let parse = common::parse(&source);
        assert!(
            parse.diagnostics().is_empty(),
            "{path}: {:?}",
            parse.diagnostics()
        );
    }
    println!("EBNF top-level alternatives: {counts:?}");
}

#[test]
fn grammar_optional_and_repeated_forms_are_accounted_for() {
    let expected = syntax_quantifiers();
    let mut actual = BTreeMap::new();
    for line in QUANTIFIERS.lines().skip(1).filter(|line| !line.is_empty()) {
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 4, "invalid quantifier row: {line}");
        let [rule, index, operator, operand] =
            <[&str; 4]>::try_from(columns).expect("four quantifier columns");
        let index = index.parse::<usize>().expect("numeric quantifier index");
        assert!(
            actual
                .insert(
                    (rule.to_owned(), index),
                    (operator.to_owned(), operand.to_owned())
                )
                .is_none(),
            "duplicate quantifier: {rule}#{index}"
        );
    }
    assert_eq!(
        expected, actual,
        "update docs/syntax-quantifiers.tsv when EBNF changes"
    );
    println!(
        "EBNF optional/repetition positions inventoried: {}",
        actual.len()
    );
}

#[test]
fn known_grammar_gaps_remain_explicit() {
    for case in GAPS {
        let parse = common::parse(case.source);
        assert!(
            !parse.diagnostics().is_empty(),
            "{} is now accepted; update grammar coverage and add positive coverage",
            case.id
        );
    }
}

#[test]
fn unrecognized_ebnf_terminals_are_an_explicit_baseline() {
    let mut missing = BTreeSet::new();
    for terminal in grammar_terminals() {
        let tokens = lex(&terminal);
        if tokens.len() != 2 || tokens[0].kind == TokenKind::Unknown {
            missing.insert(terminal);
        }
    }
    let baseline = ["@"].into_iter().map(str::to_owned).collect();
    assert_eq!(
        missing, baseline,
        "EBNF terminal/lexer drift; update the lexer or reviewed baseline"
    );
}

fn grammar_terminals() -> BTreeSet<String> {
    let source = GRAMMAR.split("(* Lexical structure *)").next().unwrap();
    let bytes = source.as_bytes();
    let mut result = BTreeSet::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"(*") {
            index += 2;
            while index < bytes.len() && !bytes[index..].starts_with(b"*)") {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
        } else if bytes[index] == b'"' {
            let start = index + 1;
            index = start;
            while index < bytes.len() && bytes[index] != b'"' {
                index += 1;
            }
            assert!(index < bytes.len(), "unterminated EBNF terminal");
            result.insert(source[start..index].to_owned());
            index += 1;
        } else {
            index += 1;
        }
    }
    result
}

fn syntax_productions() -> BTreeMap<String, Vec<String>> {
    let source = GRAMMAR.split("(* Lexical structure *)").next().unwrap();
    let mut without_comments = String::new();
    let mut cursor = 0;
    while let Some(relative_start) = source[cursor..].find("(*") {
        let start = cursor + relative_start;
        without_comments.push_str(&source[cursor..start]);
        let end = source[start + 2..].find("*)").expect("closed EBNF comment") + start + 4;
        cursor = end;
    }
    without_comments.push_str(&source[cursor..]);

    let mut result = BTreeMap::new();
    let mut start = 0;
    let mut quoted = false;
    for (index, byte) in without_comments.bytes().enumerate() {
        if byte == b'"' {
            quoted = !quoted;
        } else if byte == b';' && !quoted {
            let production = without_comments[start..index].trim();
            start = index + 1;
            if production.is_empty() {
                continue;
            }
            let (name, right) = production.split_once("::=").expect("EBNF production");
            let name = name.trim().to_owned();
            let mut alternatives = Vec::new();
            let mut depth = 0;
            let mut branch_start = 0;
            let mut in_quotes = false;
            for (position, ch) in right.char_indices() {
                match ch {
                    '"' => in_quotes = !in_quotes,
                    '(' | '[' if !in_quotes => depth += 1,
                    ')' | ']' if !in_quotes => depth -= 1,
                    '|' if !in_quotes && depth == 0 => {
                        alternatives.push(normalize(&right[branch_start..position]));
                        branch_start = position + 1;
                    }
                    _ => {}
                }
            }
            alternatives.push(normalize(&right[branch_start..]));
            assert!(
                result.insert(name.clone(), alternatives).is_none(),
                "duplicate production: {name}"
            );
        }
    }
    result
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn syntax_quantifiers() -> BTreeMap<(String, usize), (String, String)> {
    let mut result = BTreeMap::new();
    for (rule, alternatives) in syntax_productions() {
        let mut ordinal = 0;
        for branch in alternatives {
            let bytes = branch.as_bytes();
            let mut quoted = vec![false; bytes.len()];
            let mut in_quotes = false;
            for (index, byte) in bytes.iter().enumerate() {
                if *byte == b'"' {
                    in_quotes = !in_quotes;
                }
                quoted[index] = in_quotes;
            }
            for (index, byte) in bytes.iter().enumerate() {
                if !quoted[index] && matches!(byte, b'?' | b'*' | b'+') {
                    ordinal += 1;
                    result.insert(
                        (rule.clone(), ordinal),
                        (
                            char::from(*byte).to_string(),
                            quantifier_operand(&branch, index, &quoted),
                        ),
                    );
                }
            }
        }
    }
    result
}

fn quantifier_operand(branch: &str, index: usize, quoted: &[bool]) -> String {
    let bytes = branch.as_bytes();
    let mut end = index;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end > 0 && bytes[end - 1] == b')' {
        let mut depth = 0;
        for start in (0..end).rev() {
            if quoted[start] {
                continue;
            }
            if bytes[start] == b')' {
                depth += 1;
            } else if bytes[start] == b'(' {
                depth -= 1;
                if depth == 0 {
                    return normalize(&branch[start..end]);
                }
            }
        }
        panic!("unbalanced quantified group in {branch}");
    }
    let mut start = end;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    assert!(start < end, "missing quantifier operand in {branch}");
    branch[start..end].to_owned()
}
