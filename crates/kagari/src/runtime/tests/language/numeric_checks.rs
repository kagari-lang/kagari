/// Bounded numeric predicate programs with a retained failure-to-expression map.
pub(super) struct NumericChecks {
    expressions: Vec<String>,
}

impl NumericChecks {
    pub(super) fn new(expressions: Vec<String>) -> Self {
        Self { expressions }
    }

    pub(super) fn source(&self) -> String {
        // Bound per-function MIR work without raising compiler resource limits.
        let mut source = String::new();
        let mut main = String::from("fn main() -> i32 {\n");
        for (group, checks) in self.expressions.chunks(6).enumerate() {
            source.push_str(&format!("fn checks{group}() -> i32 {{\n"));
            for (offset, check) in checks.iter().enumerate() {
                let index = group * 6 + offset + 1;
                source.push_str(&format!(
                    "{{ val passed = {check}; if !passed {{ return {index}; }} }}\n"
                ));
            }
            source.push_str("0 }\n");
            main.push_str(&format!(
                "{{ val result = checks{group}(); if result != 0 {{ return result; }} }}\n"
            ));
        }
        source.push_str(&main);
        source.push_str("0 }\n");
        source
    }

    pub(super) fn assert_result(&self, result: i32, route: &str, source: &str) {
        let expression = result
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| self.expressions.get(index))
            .map(String::as_str)
            .unwrap_or("invalid numeric check marker");
        assert_eq!(
            result, 0,
            "{route}: failed numeric check {result}: {expression}\n{source}"
        );
    }
}
