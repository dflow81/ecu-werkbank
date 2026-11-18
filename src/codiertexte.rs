//! Codier-Helfer: eigene Texte zu Codier-Bytes und -Bits
//! (`<Datenordner>/codiertexte.toml`).
//!
//! Die Texte gehören zu einem Steuergerät, erkannt an seiner Request-CAN-ID
//! (z. B. 714 = Kombi beim Golf 6). Die ist bei UDS immer bekannt, auch wenn
//! das Steuergerät nur über den Suchlauf gefunden wurde. TP-2.0-Steuergeräte
//! bekommen 200 + Adresse (Bordnetz 20 → 220), siehe [`tp20_key`].
//!
//! Ohne eigenen Text nimmt ecu-werkbank den Namen aus deinen `[[coding]]`-Labels.
//! So lernst du die Codierung selbst: Bit ändern (oder vorher/nachher
//! vergleichen), Wirkung am Auto ansehen, Text eintragen.

use crate::labels::Coding;
use crate::tr;
use anyhow::{Context, Result};
use std::collections::BTreeMap;

fn path() -> std::path::PathBuf {
    crate::history::data_dir().join("codiertexte.toml")
}

/// Text-Schlüssel eines TP-2.0-Steuergeräts: 0x200 + Adresse (Bordnetz 0x20 →
/// „220“). Kollidiert nicht mit UDS-IDs (0x7xx).
pub fn tp20_key(adresse: u8) -> u16 {
    0x200 + u16::from(adresse)
}

/// Schlüssel: „714-03“ für Byte 3, „714-03.4“ für Byte 3 Bit 4 (CAN-ID hex).
pub fn schluessel(tx: u16, byte: usize, bit: Option<u8>) -> String {
    match bit {
        Some(b) => format!("{tx:03X}-{byte:02}.{b}"),
        None => format!("{tx:03X}-{byte:02}"),
    }
}

/// Alle eigenen Texte.
pub fn alle() -> BTreeMap<String, String> {
    std::fs::read_to_string(path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

/// Text setzen (`bit` None = Text zum ganzen Byte); leerer Text löscht.
pub fn setzen(tx: u16, byte: usize, bit: Option<u8>, text: &str) -> Result<()> {
    let mut m = alle();
    let k = schluessel(tx, byte, bit);
    if text.trim().is_empty() {
        m.remove(&k);
    } else {
        m.insert(k, text.trim().to_string());
    }
    let p = path();
    std::fs::create_dir_all(p.parent().expect("hat Eltern"))?;
    std::fs::write(&p, toml::to_string(&m)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

/// Geänderte Bits von `alt` nach `neu`: (Byte, Bit, alt, neu).
/// Fehlende Bytes zählen als 0.
/// „Byte.Bit“ lesen, z. B. "1.3" → (1, 3).
pub fn bit_stelle(s: &str) -> Option<(usize, u8)> {
    let (a, b) = s.trim().split_once('.')?;
    let bit: u8 = b.trim().parse().ok()?;
    (bit < 8).then_some((a.trim().parse().ok()?, bit))
}

fn bit_an(bytes: &[u8], byte: usize, bit: u8) -> Option<bool> {
    bytes.get(byte).map(|b| b & (1 << bit) != 0)
}

/// Abhängigkeiten aus den Labels prüfen (`benoetigt`, `schliesst_aus`).
/// Gibt Hinweise in Klartext zurück; leer = alles passt.
pub fn regeln_pruefen(labels: &[Coding], bytes: &[u8]) -> Vec<String> {
    let name = |byte: usize, bit: u8| {
        labels
            .iter()
            .find(|c| usize::from(c.byte) == byte && c.bit == Some(bit))
            .map(|c| format!("„{}“ (Byte {byte} Bit {bit})", c.name))
            .unwrap_or_else(|| crate::tr!("Byte {a} Bit {b}", a = byte, b = bit))
    };
    let mut out = Vec::new();
    for c in labels {
        let Some(bit) = c.bit else { continue };
        if bit_an(bytes, usize::from(c.byte), bit) != Some(true) {
            continue;
        }
        let selbst = name(usize::from(c.byte), bit);
        for r in &c.benoetigt {
            match bit_stelle(r) {
                Some((b, x)) if bit_an(bytes, b, x) == Some(false) => out.push(crate::tr!("{a} braucht {b} — das ist aus.", a = selbst, b = name(b, x))),
                Some(_) => {}
                None => out.push(crate::tr!("{a}: Regel „{r}“ nicht verstanden (Form: Byte.Bit)", a = selbst, r = r)),
            }
        }
        for r in &c.schliesst_aus {
            match bit_stelle(r) {
                Some((b, x)) if bit_an(bytes, b, x) == Some(true) => out.push(crate::tr!("{a} verträgt sich nicht mit {b} — beide sind an.", a = selbst, b = name(b, x))),
                Some(_) => {}
                None => out.push(crate::tr!("{a}: Regel „{r}“ nicht verstanden (Form: Byte.Bit)", a = selbst, r = r)),
            }
        }
    }
    out
}

pub fn unterschiede(alt: &[u8], neu: &[u8]) -> Vec<(usize, u8, bool, bool)> {
    let mut v = Vec::new();
    for i in 0..alt.len().max(neu.len()) {
        let (x, y) = (alt.get(i).copied().unwrap_or(0), neu.get(i).copied().unwrap_or(0));
        for bit in 0..8u8 {
            let (a, b) = ((x >> bit) & 1 == 1, (y >> bit) & 1 == 1);
            if a != b {
                v.push((i, bit, a, b));
            }
        }
    }
    v
}

/// Ein Byte als Bits, höchstes zuerst: „0010 0100“.
pub fn bits(b: u8) -> String {
    format!("{:04b} {:04b}", b >> 4, b & 0x0F)
}

/// Eigene Texte (und Label-Namen als Ersatz).
#[derive(Debug, Clone, Default)]
pub struct Texte {
    m: BTreeMap<String, String>,
}

impl Texte {
    pub fn laden() -> Self {
        Self { m: alle() }
    }

    /// Eigener Text (ohne Labels).
    pub fn eigen(&self, tx: u16, byte: usize, bit: Option<u8>) -> Option<&str> {
        self.m.get(&schluessel(tx, byte, bit)).map(String::as_str)
    }

    /// Name eines Bits (`bit` Some) oder Bytes: eigener Text, sonst Label.
    pub fn name(&self, tx: u16, labels: &[Coding], byte: usize, bit: Option<u8>) -> Option<String> {
        if let Some(t) = self.eigen(tx, byte, bit) {
            return Some(t.to_string());
        }
        labels
            .iter()
            .find(|c| c.byte as usize == byte && c.bit == bit && !c.name.is_empty())
            .map(|c| c.name.clone())
    }

    /// Wie viele Bits und Bytes dieses Steuergeräts haben einen eigenen Text.
    pub fn anzahl(&self, tx: u16) -> usize {
        let p = format!("{tx:03X}-");
        self.m.keys().filter(|k| k.starts_with(&p)).count()
    }

    /// Alle eigenen Texte eines Steuergeräts: (Byte, Bit, Text), sortiert.
    pub fn fuer(&self, tx: u16) -> Vec<(usize, Option<u8>, String)> {
        let p = format!("{tx:03X}-");
        let mut v: Vec<_> = self
            .m
            .iter()
            .filter_map(|(k, t)| {
                let rest = k.strip_prefix(&p)?;
                let (byte, bit) = match rest.split_once('.') {
                    Some((a, b)) => (a.parse().ok()?, Some(b.parse().ok()?)),
                    None => (rest.parse().ok()?, None),
                };
                Some((byte, bit, t.clone()))
            })
            .collect();
        v.sort_by_key(|(a, b, _)| (*a, b.map(|x| x as i16).unwrap_or(-1)));
        v
    }

    /// Codierung als Textzeilen: jedes Byte mit Bits, darunter die benannten
    /// Bits mit ein/aus. Für Terminal und Bericht.
    pub fn aufschluesseln(&self, tx: u16, labels: &[Coding], bytes: &[u8]) -> Vec<String> {
        let mut z = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            let name = self.name(tx, labels, i, None).map(|n| format!("  {n}")).unwrap_or_default();
            z.push(tr!("Byte {i} = {h}  ({b}){name}", i = format!("{i:02}"), h = format!("{b:02X}"), b = bits(b), name = name));
            for bit in 0..8u8 {
                if let Some(n) = self.name(tx, labels, i, Some(bit)) {
                    let an = (b >> bit) & 1 == 1;
                    z.push(format!("    Bit {bit} {}  {n}", if an { crate::trs!("■ ein") } else { crate::trs!("□ aus") }));
                }
            }
        }
        z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abhaengigkeiten() {
        let c = |byte, bit, name: &str, ben: &[&str], aus: &[&str]| Coding {
            byte,
            bit: Some(bit),
            name: name.into(),
            benoetigt: ben.iter().map(|s| s.to_string()).collect(),
            schliesst_aus: aus.iter().map(|s| s.to_string()).collect(),
        };
        let labels = vec![c(0, 1, "Komfortblinken", &["0.0"], &[]), c(0, 0, "Blinker", &[], &[]), c(1, 2, "Tagfahrlicht", &[], &["1.3"]), c(1, 3, "Standlicht dauernd", &[], &[])];
        assert!(regeln_pruefen(&labels, &[0b0000_0011, 0b0000_0100]).is_empty());
        let h = regeln_pruefen(&labels, &[0b0000_0010, 0b0000_1100]);
        assert_eq!(h.len(), 2, "{h:?}");
        assert!(h[0].contains("„Komfortblinken“") && h[0].contains("„Blinker“"), "{h:?}");
        assert!(h[1].contains("verträgt sich nicht"), "{h:?}");
        // aus = keine Prüfung; kaputte Regel wird gemeldet
        assert!(regeln_pruefen(&labels, &[0, 0]).is_empty());
        let kaputt = vec![c(0, 0, "X", &["1"], &[])];
        assert!(regeln_pruefen(&kaputt, &[1])[0].contains("nicht verstanden"));
        assert_eq!(bit_stelle(" 2.7 "), Some((2, 7)));
        assert_eq!(bit_stelle("2.8"), None);
    }

    #[test]
    fn texte_und_unterschiede() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("vag-ct-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);

        assert_eq!(unterschiede(&[0x01, 0x80], &[0x03, 0x00, 0x10]), vec![(0, 1, false, true), (1, 7, true, false), (2, 4, false, true)]);
        assert_eq!(bits(0x24), "0010 0100");

        let labels = vec![Coding { byte: 0, bit: Some(0), name: "Label-Bit".into(), benoetigt: vec![], schliesst_aus: vec![] }];
        let t = Texte::laden();
        assert_eq!(t.name(0x714, &labels, 0, Some(0)).as_deref(), Some("Label-Bit"));
        assert_eq!(t.name(0x714, &labels, 0, Some(1)), None);

        setzen(0x714, 0, Some(0), "Tagfahrlicht").unwrap();
        setzen(0x714, 3, None, "Ländervariante").unwrap();
        setzen(0x714, 0, Some(1), "  ").unwrap(); // leer: nichts
        let t = Texte::laden();
        assert_eq!(t.name(0x714, &labels, 0, Some(0)).as_deref(), Some("Tagfahrlicht")); // eigener Text vor Label
        assert_eq!(t.name(0x7E0, &labels, 0, Some(0)).as_deref(), Some("Label-Bit")); // anderes Steuergerät
        assert_eq!(t.anzahl(0x714), 2);
        assert_eq!(t.fuer(0x714), vec![(0, Some(0), "Tagfahrlicht".to_string()), (3, None, "Ländervariante".to_string())]);

        let z = t.aufschluesseln(0x714, &labels, &[0x01, 0x00, 0x00, 0x2A]);
        assert_eq!(z[0], "Byte 00 = 01  (0000 0001)");
        assert_eq!(z[1], "    Bit 0 ■ ein  Tagfahrlicht");
        assert_eq!(z.last().unwrap(), "Byte 03 = 2A  (0010 1010)  Ländervariante");

        setzen(0x714, 0, Some(0), "").unwrap();
        assert_eq!(Texte::laden().anzahl(0x714), 1);
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
