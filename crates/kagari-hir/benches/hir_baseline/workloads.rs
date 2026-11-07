//! Versioned, deterministic source inputs shared by the timing and memory executables.

use std::fmt::Write;

use blake3::Hasher;
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::{
    identity::FileId,
    source_database::{SourceDatabase, SourceLayer},
};

pub const ROOT: &str = "/hir-baseline";
pub const GENERATION: &str = "historical arithmetic chain; explicit root main; dependency functions split 32/32/31 plus shared value; no inline modules";

pub struct Input {
    pub name: &'static str,
    pub text: String,
}

pub struct Workload {
    pub name: String,
    pub size: usize,
    pub inputs: Vec<Input>,
    pub body_edit: String,
    pub signature_edit: Option<String>,
    pub edit_index: usize,
    pub query_index: usize,
    pub query_offset: usize,
    pub input_hash: String,
}

impl Workload {
    pub fn new(name: &str, size: usize) -> Self {
        let inputs = if name == "dependency_edit" {
            dependency(size)
        } else {
            vec![Input {
                name: "profile.kgr",
                text: arithmetic(size, name == "generic"),
            }]
        };
        let dependency = name == "dependency_edit";
        let edit_index = usize::from(dependency);
        let query_index = if dependency { 2 } else { 0 };
        let original = &inputs[edit_index].text;
        let body_edit = if dependency {
            original.replacen("{ 1 }", "{ 9 }", 1)
        } else {
            original.replacen("x + 0", "x + 9", 1)
        };
        assert_ne!(*original, body_edit);
        let signature_edit = dependency.then(|| original.replacen("i32", "i64", 1));
        let query_offset = inputs[query_index]
            .text
            .find(if dependency { "saved;" } else { "x + 0" })
            .unwrap();
        let mut workload = Self {
            name: name.into(),
            size,
            inputs,
            body_edit,
            signature_edit,
            edit_index,
            query_index,
            query_offset,
            input_hash: String::new(),
        };
        workload.input_hash = workload.hash();
        workload
    }

    pub fn scenario_names(&self, memory: bool) -> Vec<&'static str> {
        if memory {
            return vec!["lifecycle"];
        }
        let mut names = vec!["staged", "snapshot", "checked", "warm"];
        match (self.name.as_str(), self.size) {
            ("plain", 96) => names.extend(["body_edit", "lifecycle"]),
            ("generic", 96) => names.push("lifecycle"),
            ("dependency_edit", 96) => names.extend([
                "body_edit",
                "signature_edit",
                "selected",
                "selected_warm",
                "selected_signature_edit",
                "lifecycle",
            ]),
            _ => {}
        }
        names
    }

    pub fn source_bytes(&self) -> usize {
        self.inputs.iter().map(|input| input.text.len()).sum()
    }

    pub fn expected_functions(&self) -> usize {
        self.size + if self.name == "generic" { 4 } else { 1 }
    }

    pub fn sources(&self) -> (SourceDatabase, Vec<FileId>) {
        let mut sources = SourceDatabase::new(ROOT).unwrap();
        let ids = self
            .inputs
            .iter()
            .map(|input| {
                sources
                    .bind_module(
                        input.name,
                        ModuleIdentity {
                            package: PackageId("bench".into()),
                            path: vec![input.name.trim_end_matches(".kgr").into()],
                        },
                    )
                    .unwrap();
                sources
                    .set(input.name, input.text.clone(), SourceLayer::Base)
                    .unwrap()
            })
            .collect();
        (sources, ids)
    }

    pub fn edit(&self, sources: &mut SourceDatabase, signature: bool) {
        let replacement = self.replacement(signature);
        sources
            .set(
                self.inputs[self.edit_index].name,
                replacement.to_owned(),
                SourceLayer::Overlay,
            )
            .unwrap();
    }

    pub fn replacement(&self, signature: bool) -> &str {
        if signature {
            self.signature_edit.as_deref().unwrap()
        } else {
            &self.body_edit
        }
    }

    fn hash(&self) -> String {
        let mut hash = Hasher::new();
        // Length framing prevents filename/text/route concatenation ambiguities.
        let mut field = |text: &str| {
            hash.update(&(text.len() as u64).to_le_bytes());
            hash.update(text.as_bytes());
        };
        field("hir-baseline-inputs-v1");
        field(GENERATION);
        field(&self.name);
        field(&self.size.to_string());
        for input in &self.inputs {
            field("original");
            field(input.name);
            field(&input.text);
        }
        field("body_edit");
        field(self.inputs[self.edit_index].name);
        field(&self.body_edit);
        if let Some(text) = &self.signature_edit {
            field("signature_edit");
            field(self.inputs[self.edit_index].name);
            field(text);
        }
        field("timing");
        for name in self.scenario_names(false) {
            field(name);
        }
        field("memory");
        for name in self.scenario_names(true) {
            field(name);
        }
        hash.finalize().to_hex().to_string()
    }
}

fn arithmetic(size: usize, generic: bool) -> String {
    let mut text = String::new();
    if generic {
        text.push_str(
            "trait Read { fn read(self) -> i32; }\n\
             struct Boxed { val n: i32 }\n\
             impl Read for Boxed { fn read(self) -> i32 { self.n } }\n\
             fn read<T: Read>(x: T) -> i32 { x.read() }\n",
        );
    }
    let tail = if generic { "read(Boxed { n: f })" } else { "f" };
    for i in 0..size {
        writeln!(
            text,
            "pub fn f{i}(x: i32) -> i32 {{\n    val a = x + 0; val b = a + 1; val c = b + 2;\n    val d = c + 3; val e = d + 4; val f = e + 5; {tail}\n}}"
        )
        .unwrap();
    }
    text.push_str("fn main() -> i32 { f0(0) }\n");
    text
}

fn dependency(size: usize) -> Vec<Input> {
    assert_eq!(size, 96);
    let mut inputs = vec![
        Input {
            name: "root.kgr",
            text: "use bench::{consumer_a, consumer_b, independent};\nfn main() { consumer_a::f0(); consumer_b::f0(); independent::f0(0); }\n".into(),
        },
        Input {
            name: "shared.kgr",
            text: "pub fn value() -> i32 { 1 }\n".into(),
        },
    ];
    for name in ["consumer_a.kgr", "consumer_b.kgr"] {
        let mut text = String::from("use bench::shared;\n");
        for i in 0..32 {
            writeln!(
                text,
                "pub fn f{i}() {{ val saved = shared::value(); saved; }}"
            )
            .unwrap();
        }
        inputs.push(Input { name, text });
    }
    // The independent module has no generated main and no shared dependency.
    let text = arithmetic(31, false).replace("fn main() -> i32 { f0(0) }\n", "");
    inputs.push(Input {
        name: "independent.kgr",
        text,
    });
    inputs
}
