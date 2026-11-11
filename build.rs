//! Sammelt beim Bauen alle übersetzbaren Texte (`tr!("…")`, `trn!("…")`) aus
//! dem Quellcode in `$OUT_DIR/texte.rs` — daraus entsteht die Sprachvorlage.

use std::path::Path;

fn sammeln(dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            sammeln(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            println!("cargo:rerun-if-changed={}", p.display());
            let s = std::fs::read_to_string(&p).unwrap_or_default();
            for marker in ["tr!(", "trn!(", "trs!("] {
                let mut rest = s.as_str();
                while let Some(i) = rest.find(marker) {
                    // nur das Makro selbst, nicht z. B. include_str!(
                    let davor = rest[..i].chars().next_back();
                    rest = &rest[i + marker.len()..];
                    if davor.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let t = rest.trim_start();
                    if !t.starts_with('"') {
                        continue;
                    }
                    // Literal bis zum schließenden, nicht maskierten Anführungszeichen
                    let b = t.as_bytes();
                    let mut j = 1;
                    while j < b.len() {
                        match b[j] {
                            b'\\' => j += 2,
                            b'"' => break,
                            _ => j += 1,
                        }
                    }
                    if j < b.len() {
                        out.push(t[..=j].to_string());
                    }
                }
            }
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=lang");
    let mut v = Vec::new();
    sammeln(Path::new("src"), &mut v);
    v.sort();
    v.dedup();
    let body = format!("&[\n{}]\n", v.iter().map(|s| format!("    {s},\n")).collect::<String>());
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("texte.rs");
    std::fs::write(out, body).unwrap();
}
