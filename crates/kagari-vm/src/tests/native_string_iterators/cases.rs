pub(super) const METHODS: &[&str] = &[
    "bytes",
    "char_indices",
    "split",
    "splitn",
    "split_whitespace",
    "lines",
];
pub(super) fn cases() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for method in METHODS {
        let scenarios = match *method {
            "split" => vec![
                ("", ",", 0),
                ("a,,b,", ",", 0),
                ("é😀é", "é", 0),
                ("é😀", "", 0),
                ("ababa", "aba", 0),
                ("a b", "absent", 0),
            ],
            "splitn" => vec![
                ("a,b,c", ",", 0),
                ("a,b,c", ",", 1),
                ("a,b,c", ",", 2),
                ("é😀", "", 2),
                ("é😀", "", 3),
                ("", ",", 8),
            ],
            "split_whitespace" => vec![
                ("", "", 0),
                (" a b ", "", 0),
                (" a　b\t\nc ", "", 0),
                ("　 \t\n", "", 0),
                ("é😀 ß", "", 0),
                ("a\r\nb", "", 0),
            ],
            "lines" => vec![
                ("", "", 0),
                ("a", "", 0),
                ("a\n", "", 0),
                ("a\r\nb\nc\r", "", 0),
                ("\n\n", "", 0),
                ("\r\n", "", 0),
            ],
            _ => vec![
                ("", "", 0),
                ("abc", "", 0),
                ("é😀", "", 0),
                ("é😀é", "", 0),
                ("　ß", "", 0),
                ("\r\n", "", 0),
            ],
        };
        for (index, (text, separator, count)) in scenarios.into_iter().enumerate() {
            let elements = match *method {
                "bytes" => text.bytes().map(|b| format!("{b}u8")).collect::<Vec<_>>(),
                "char_indices" => text
                    .char_indices()
                    .map(|(i, c)| format!("({i}usize,{:?})", c.to_string()))
                    .collect(),
                _ => {
                    let parts: Vec<_> = match *method {
                        "split" => text.split(separator).collect(),
                        "splitn" => text.splitn(count, separator).collect(),
                        "split_whitespace" => text.split_whitespace().collect(),
                        _ => text.lines().collect(),
                    };
                    parts.into_iter().map(|s| format!("{s:?}")).collect()
                }
            }
            .join(",");
            let item = match *method {
                "bytes" => "u8",
                "char_indices" => "(usize,String)",
                _ => "String",
            };
            let arguments = match *method {
                "split" => format!("separator({separator:?})"),
                "splitn" => format!("count({count}usize),separator({separator:?})"),
                _ => String::new(),
            };
            for qualified in [false, true] {
                let receiver = format!("source({text:?})");
                let call = if qualified {
                    format!(
                        "std::string::String::{method}({receiver}{}{arguments})",
                        if arguments.is_empty() { "" } else { "," }
                    )
                } else {
                    format!("{receiver}.{method}({arguments})")
                };
                out.push((format!("{method}_{index}_{qualified}"),format!(r#"
fn source(value:String)->String{{print("source");value}}fn separator(value:String)->String{{print("separator");value}}fn count(value:usize)->usize{{print("count");value}}
fn main()->i32{{val expected:ArrayList<{item}> =[{elements}];val iterator={call};print("created");val alias=iterator;std::debug::assert(alias.next()==expected.get(0usize),"first");print("first");var index=1usize;while index<expected.len(){{std::debug::assert(iterator.next()==expected.get(index),"items");print("item");index+=1usize;}}std::debug::assert(alias.next()==None && iterator.next()==None,"fused");print("done");42}}
"#)));
            }
        }
    }
    out
}
