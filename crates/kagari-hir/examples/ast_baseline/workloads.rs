//! Fixed source recipes shared by correctness checks and measurements.

use std::{fmt::Write, fs, path::Path};

use blake3::Hasher;
use kagari_source::source::SourceFile;

pub struct Workload {
    pub name: &'static str,
    pub sources: Vec<SourceFile>,
    pub declarations: bool,
    pub erroneous: bool,
}

impl Workload {
    pub fn bytes(&self) -> usize {
        self.sources.iter().map(|source| source.text().len()).sum()
    }

    pub fn hash(&self) -> String {
        let mut hash = Hasher::new();
        for source in &self.sources {
            hash.update(&(source.text().len() as u64).to_le_bytes());
            hash.update(source.text().as_bytes());
        }
        hash.finalize().to_hex().to_string()
    }
}

fn functions(count: usize, trivia: bool) -> String {
    let mut text = String::new();
    for index in 0..count {
        if trivia {
            text.push_str("/// Calculation: \u{503c}\r\n/* outer /* inner */ retained */\r\n");
        }
        writeln!(text, "fn f{index}(x: i64) -> i64 {{ val y = x + 1; if y > 0 {{ helper(y, 2) }} else {{ 0 }} }}").unwrap();
    }
    text
}

pub fn inline_retention() -> Workload {
    let mut text = String::new();
    for index in 0..32 {
        writeln!(text, "mod child{index} {{\n{} }}", functions(4, true)).unwrap();
    }
    Workload {
        name: "inline_retention",
        sources: vec![SourceFile::new("inline_retention", text)],
        declarations: false,
        erroneous: false,
    }
}

pub fn load(root: &Path) -> Vec<Workload> {
    let mut workloads = Vec::new();
    for (name, count, trivia) in [
        ("small", 1, false),
        ("medium", 256, false),
        ("large", 4096, false),
        ("trivia", 256, true),
    ] {
        workloads.push(Workload {
            name,
            sources: vec![SourceFile::new(name, functions(count, trivia))],
            declarations: false,
            erroneous: false,
        });
    }
    let files = [
        "grammar-witnesses",
        "data-model",
        "control-flow",
        "expressions",
        "generic-associated-types",
        "where-bounds",
        "pattern-alternatives",
        "string-interpolation",
        "literals-and-comments",
        "inline-modules",
        "struct-update",
        "import-alias",
    ];
    let sources = files
        .into_iter()
        .map(|name| {
            let path = root.join("examples/syntax").join(format!("{name}.kgr"));
            // Normalize checked-in fixtures so Windows/Linux checkouts have identical inputs.
            let text = fs::read_to_string(path)
                .expect("tracked syntax fixture")
                .replace("\r\n", "\n");
            SourceFile::new(name, text)
        })
        .collect();
    workloads.push(Workload {
        name: "syntax_corpus",
        sources,
        declarations: false,
        erroneous: false,
    });
    let recovery = (0..64)
        .map(|i| format!("fn broken{i}() {{ val x = ; x + }}\n"))
        .collect::<String>();
    let limits = format!(
        "fn deep() {{ {}1{} }}\nfn after() {{ 2 }}",
        "(".repeat(160),
        ")".repeat(160)
    );
    let declarations = (0..256)
        .map(|i| format!("pub fn api{i}<T>(value: T) -> T;\n"))
        .collect::<String>();
    for (name, text, declarations, erroneous) in [
        ("recovery", recovery, false, true),
        ("limits", limits, false, true),
        ("declarations", declarations, true, false),
    ] {
        workloads.push(Workload {
            name,
            sources: vec![SourceFile::new(name, text)],
            declarations,
            erroneous,
        });
    }
    workloads
}
