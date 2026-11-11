//! Minimaler ZIP-Schreiber (ohne Kompression, „stored“) — für die Sicherung
//! eines Fahrzeugordners. Jedes Entpackprogramm kann die Datei öffnen.

use anyhow::{Context, Result};
use crate::tr;
use std::io::Write;
use std::path::Path;

fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    });
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = t[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// DOS-Datum/-Zeit aus "TT.MM.JJJJ HH:MM".
fn dos_time() -> (u16, u16) {
    let now = crate::report::now_local();
    let (d, t) = now.split_once(' ').unwrap_or((&now, "00:00"));
    let p: Vec<u16> = d.split('.').filter_map(|x| x.parse().ok()).collect();
    let h: Vec<u16> = t.split(':').filter_map(|x| x.parse().ok()).collect();
    if p.len() != 3 || h.len() != 2 {
        return (0, 0x21);
    }
    let date = ((p[2].saturating_sub(1980)) << 9) | (p[1] << 5) | p[0];
    let time = (h[0] << 11) | (h[1] << 5);
    (time, date)
}

/// Alle Dateien unter `dir` (rekursiv) nach `out` packen, Pfade mit `prefix/`.
/// Gibt die Anzahl der Dateien zurück.
pub fn zip_dir(dir: &Path, out: &Path, prefix: &str) -> Result<usize> {
    zip_dirs(&[(dir, prefix)], out)
}

/// Mehrere Ordner in ein ZIP, jeder unter seinem eigenen Präfix.
pub fn zip_dirs(dirs: &[(&Path, &str)], out: &Path) -> Result<usize> {
    // (Datei, Ordner, Präfix)
    let mut files: Vec<(std::path::PathBuf, &Path, &str)> = Vec::new();
    for &(dir, prefix) in dirs {
        let mut stack = vec![dir.to_path_buf()];
        let mut eigene = Vec::new();
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).with_context(|| format!("Lesen: {}", d.display()))?.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    eigene.push(p);
                }
            }
        }
        eigene.sort();
        files.extend(eigene.into_iter().map(|f| (f, dir, prefix)));
    }
    let (time, date) = dos_time();
    let mut w = std::io::BufWriter::new(std::fs::File::create(out).with_context(|| format!("Anlegen: {}", out.display()))?);
    let mut central = Vec::new();
    let mut offset: u32 = 0;
    for (f, dir, prefix) in &files {
        let data = std::fs::read(f)?;
        let rel = f.strip_prefix(dir).unwrap_or(f).to_string_lossy().replace('\\', "/");
        let name = format!("{prefix}/{rel}");
        let crc = crc32(&data);
        let size = u32::try_from(data.len()).context(crate::trs!("Datei zu groß für ZIP"))?;
        let nb = name.as_bytes();
        // Local file header (UTF-8-Namen: Bit 11)
        let mut h = Vec::with_capacity(30 + nb.len());
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes());
        h.extend_from_slice(&0x0800u16.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes()); // stored
        h.extend_from_slice(&time.to_le_bytes());
        h.extend_from_slice(&date.to_le_bytes());
        h.extend_from_slice(&crc.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&(nb.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(nb);
        w.write_all(&h)?;
        w.write_all(&data)?;
        // Central directory entry
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(nb.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 6]); // extra, comment, disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attr
        central.extend_from_slice(&0u32.to_le_bytes()); // external attr
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(nb);
        offset = offset
            .checked_add(h.len() as u32 + size)
            .context(crate::trs!("ZIP größer als 4 GB"))?;
    }
    w.write_all(&central)?;
    let mut end = Vec::new();
    end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    end.extend_from_slice(&[0; 4]);
    end.extend_from_slice(&(files.len() as u16).to_le_bytes());
    end.extend_from_slice(&(files.len() as u16).to_le_bytes());
    end.extend_from_slice(&(central.len() as u32).to_le_bytes());
    end.extend_from_slice(&offset.to_le_bytes());
    end.extend_from_slice(&0u16.to_le_bytes());
    w.write_all(&end)?;
    w.flush()?;
    Ok(files.len())
}

/// Einen sicheren Zielpfad aus einem ZIP-Eintragsnamen bauen (keine
/// absoluten Pfade, kein „..“ nach oben). `None` = Eintrag überspringen.
fn ziel_sicher(dest: &Path, name: &str) -> Option<std::path::PathBuf> {
    let name = name.replace('\\', "/");
    if name.is_empty() || name.ends_with('/') {
        return None; // Verzeichnis-Eintrag
    }
    let mut p = dest.to_path_buf();
    for teil in name.split('/') {
        match teil {
            "" | "." => continue,
            ".." => return None, // Ausbruch aus dem Zielordner verhindern
            _ => p.push(teil),
        }
    }
    (p.starts_with(dest)).then_some(p)
}

/// Ein (mit [`zip_dir`]) erzeugtes ZIP nach `dest` entpacken. Unterstützt nur
/// „stored“ (ohne Kompression) — genau das, was dieses Modul schreibt.
/// Gibt die geschriebenen relativen Pfade zurück. `ueberschreiben=false`
/// bricht ab, wenn eine Zieldatei schon existiert.
pub fn unzip_to(zip: &Path, dest: &Path, ueberschreiben: bool) -> Result<Vec<String>> {
    let data = std::fs::read(zip).with_context(|| format!("Lesen: {}", zip.display()))?;
    let mut i = 0usize;
    let mut geschrieben = Vec::new();
    let rd_u16 = |d: &[u8], o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
    let rd_u32 = |d: &[u8], o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    while i + 4 <= data.len() {
        let sig = rd_u32(&data, i);
        if sig != 0x0403_4b50 {
            break; // ab hier kommt das Central Directory
        }
        if i + 30 > data.len() {
            anyhow::bail!(tr!("ZIP beschädigt (Kopf abgeschnitten)"));
        }
        let flags = rd_u16(&data, i + 6);
        let method = rd_u16(&data, i + 8);
        let csize = rd_u32(&data, i + 18) as usize;
        let name_len = rd_u16(&data, i + 26) as usize;
        let extra_len = rd_u16(&data, i + 28) as usize;
        let name_start = i + 30;
        let data_start = name_start + name_len + extra_len;
        if data_start + csize > data.len() {
            anyhow::bail!(tr!("ZIP beschädigt (Daten abgeschnitten)"));
        }
        if method != 0 {
            anyhow::bail!(tr!("ZIP mit Kompression wird nicht unterstützt — bitte „stored“ (z. B. aus akte-zip)."));
        }
        if flags & 0x0008 != 0 {
            anyhow::bail!(tr!("ZIP mit nachgestellter Größe wird nicht unterstützt."));
        }
        let name = String::from_utf8_lossy(&data[name_start..name_start + name_len]).into_owned();
        let inhalt = &data[data_start..data_start + csize];
        if let Some(ziel) = ziel_sicher(dest, &name) {
            if ziel.exists() && !ueberschreiben {
                anyhow::bail!(tr!("Gibt es schon: {a} (mit --ueberschreiben erzwingen)", a = ziel.display()));
            }
            if let Some(p) = ziel.parent() {
                std::fs::create_dir_all(p).with_context(|| crate::tr!("Ordner anlegen: {a}", a = p.display()))?;
            }
            std::fs::write(&ziel, inhalt).with_context(|| format!("Schreiben: {}", ziel.display()))?;
            geschrieben.push(name);
        }
        i = data_start + csize;
    }
    Ok(geschrieben)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn zip_roundtrip_with_unzip_tool() {
        let base = std::env::temp_dir().join(format!("vag-zip-{}", std::process::id()));
        let src = base.join("src");
        std::fs::create_dir_all(src.join("berichte")).unwrap();
        std::fs::write(src.join("fahrzeug.toml"), "kennzeichen = \"B XY 123\"\n").unwrap();
        std::fs::write(src.join("berichte/fehler.txt"), "Größe ü").unwrap();
        let out = base.join("a.zip");
        assert_eq!(zip_dir(&src, &out, "WVW").unwrap(), 2);
        // Gegenprobe mit python3 (falls vorhanden)
        if let Ok(o) = std::process::Command::new("python3")
            .args(["-c", "import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; print(sorted(z.namelist()))", out.to_str().unwrap()])
            .output()
        {
            let txt = String::from_utf8_lossy(&o.stdout);
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
            assert!(txt.contains("WVW/berichte/fehler.txt"), "{txt}");
        }
        // eigener Entpacker: Inhalt muss exakt zurückkommen
        let dest = base.join("out");
        let files = unzip_to(&out, &dest, false).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(std::fs::read_to_string(dest.join("WVW/fahrzeug.toml")).unwrap(), "kennzeichen = \"B XY 123\"\n");
        assert_eq!(std::fs::read_to_string(dest.join("WVW/berichte/fehler.txt")).unwrap(), "Größe ü");
        // ohne --ueberschreiben: zweiter Lauf bricht ab
        assert!(unzip_to(&out, &dest, false).is_err());
        assert!(unzip_to(&out, &dest, true).is_ok());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn unzip_wehrt_pfad_ausbruch_ab() {
        let dest = std::env::temp_dir().join(format!("vag-unzip-{}", std::process::id()));
        assert_eq!(ziel_sicher(&dest, "../../etc/passwd"), None);
        assert_eq!(ziel_sicher(&dest, "a/../../b"), None);
        assert_eq!(ziel_sicher(&dest, "WVW/ok.txt"), Some(dest.join("WVW/ok.txt")));
    }
}
