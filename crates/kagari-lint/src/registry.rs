//! The finite first-release rule registry; no dynamic passes or rule groups.

use crate::policy::Level;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LintId {
    UnusedBinding,
    UnusedParameter,
    UnnecessaryVar,
    NonSnakeCase,
    NonUpperCamelCase,
    NonUpperCaseGlobals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Syntax,
    Semantic,
}

pub struct Rule {
    pub id: LintId,
    pub name: &'static str,
    pub stage: Stage,
    pub default_level: Level,
    pub rationale: &'static str,
    pub scope: &'static str,
    pub example: &'static str,
    pub limitation: &'static str,
}

pub const RULES: [Rule; 6] = [
    Rule {
        default_level: Level::Warn,
        id: LintId::UnusedBinding,
        name: "unused_binding",
        stage: Stage::Semantic,
        rationale: "A local binding has no resolved reads.",
        scope: "Local and pattern bindings, including shadowing and closure reads; underscore names express intent.",
        example: "Warn: val unused = record_event(); Pass: val _unused = record_event();",
        limitation: "The initializer may have effects. No deletion or executable edit is suggested. Writes alone are not reads.",
    },
    Rule {
        default_level: Level::Warn,
        id: LintId::UnusedParameter,
        name: "unused_parameter",
        stage: Stage::Semantic,
        rationale: "A source-body parameter has no resolved reads.",
        scope: "Function and closure parameters; excludes bodyless signatures, self and underscore names.",
        example: "Warn: fn f(value:i32) {} Pass: fn f(_value:i32) {}",
        limitation: "Only positively eligible bodies are inspected; recovered facts are not proof of absence.",
    },
    Rule {
        default_level: Level::Warn,
        id: LintId::UnnecessaryVar,
        name: "unnecessary_var",
        stage: Stage::Semantic,
        rationale: "An initialized local variable is never rebound and can use val.",
        scope: "Ordinary local var bindings; direct and compound assignments count as rebinding, member mutation does not.",
        example: "Warn: var count=1; consume(count); Pass: var count=1; count+=1;",
        limitation: "Deferred initialization and captured bindings are conservatively excluded because changing capture mode needs separate evidence.",
    },
    Rule {
        default_level: Level::Warn,
        id: LintId::NonSnakeCase,
        name: "non_snake_case",
        stage: Stage::Semantic,
        rationale: "Value/member names use snake_case.",
        scope: "Functions, methods, modules, fields, locals and parameters; trait implementation methods follow their declaration.",
        example: "Warn: fn BadName() {} Pass: fn good_name() {}",
        limitation: "Uses Unicode case predicates without normalization; current source identifiers are ASCII. Leading underscores are permitted. Invalid shared contracts suppress semantic naming checks.",
    },
    Rule {
        default_level: Level::Warn,
        id: LintId::NonUpperCamelCase,
        name: "non_upper_camel_case",
        stage: Stage::Syntax,
        rationale: "Type-like names use UpperCamelCase.",
        scope: "Named types, traits, generic type parameters and enum variants. Acronyms such as HTTPServer are accepted.",
        example: "Warn: struct bad_name {} Pass: struct GoodName {}",
        limitation: "Leading underscores are permitted, internal underscores are discouraged. No identifier normalization or ASCII conversion is suggested.",
    },
    Rule {
        default_level: Level::Warn,
        id: LintId::NonUpperCaseGlobals,
        name: "non_upper_case_globals",
        stage: Stage::Syntax,
        rationale: "Declared constants and module statics use UPPER_CASE.",
        scope: "Const and module static declarations; ordinary val and var bindings are excluded.",
        example: "Warn: const limit:i32=1; Pass: const LIMIT:i32=1;",
        limitation: "Uses Unicode case predicates without changing source spelling or resolution.",
    },
];

impl LintId {
    pub fn rule(self) -> &'static Rule {
        RULES
            .iter()
            .find(|rule| rule.id == self)
            .expect("finite lint registry")
    }

    pub fn parse(name: &str) -> Option<Self> {
        RULES
            .iter()
            .find(|rule| rule.name == name)
            .map(|rule| rule.id)
    }
}
