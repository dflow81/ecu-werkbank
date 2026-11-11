//! Mitschnitt-Lernhilfe: aus dem Mitschnitt eines Werkstattgeräts fertige
//! Label-Einträge vorschlagen — Stellfunktionen (0x2F/0x31), Codierung (0x2E
//! auf die Codier-Kennung), Anpassungen (0x2E andere Kennungen, KWP 0x2E/0x3B)
//! und gelesene Messwerte (0x22 / KWP 0x21).
//!
//! Optional mit Vergleichs-Mitschnitt (gleicher Ablauf ohne die Aktion): dann
//! werden nur Befehle vorgeschlagen, die im Vergleich nicht vorkommen.

use crate::tr;
use crate::canlog::{Decoder, Message, Proto};
use std::collections::{BTreeMap, BTreeSet};

/// Ein Vorschlag.
#[derive(Debug, Clone, PartialEq)]
pub struct Vorschlag {
    /// Kurzbeschreibung für die Anzeige
    pub titel: String,
    /// TOML-Schnipsel für die Label-Datei
    pub toml: String,
}

fn key(m: &Message) -> (u32, Vec<u8>) {
    // Dienst + Kennung (ohne Nutzdaten) als Vergleichsschlüssel
    let n = match m.data.first() {
        Some(0x2F) | Some(0x2E) | Some(0x22) => 3,
        Some(0x31) => 4,
        Some(0x21) | Some(0x3B) | Some(0x1A) => 2,
        _ => 1,
    };
    (m.id, m.data.iter().take(n).copied().collect())
}

/// Antwort-ID zur Anfrage: nächste ECU-Nachricht mit SID + 0x40.
fn response_id(msgs: &[Message], i: usize) -> Option<u32> {
    let sid = *msgs[i].data.first()?;
    msgs[i + 1..].iter().take(8).find(|m| !m.from_tester && m.data.first() == Some(&(sid + 0x40))).map(|m| m.id)
}

/// Vorschläge aus einem Mitschnitt (`vergleich` = optional ohne Aktion).
pub fn lernen(log: &str, vergleich: Option<&str>) -> Vec<Vorschlag> {
    let d = Decoder::decode_text(log);
    let skip: BTreeSet<(u32, Vec<u8>)> = vergleich
        .map(|v| Decoder::decode_text(v).messages.iter().filter(|m| m.from_tester).map(key).collect())
        .unwrap_or_default();
    let msgs = &d.messages;
    let mut out = Vec::new();

    // --- UDS-Stellglieder (0x2F, Steuerparameter 0x03) ---
    let mut io: BTreeMap<(u32, u16), (Option<u32>, Vec<Vec<u8>>)> = BTreeMap::new();
    // --- Routinen (0x31 01) ---
    let mut rt: BTreeMap<(u32, u16), (Option<u32>, Vec<Vec<u8>>)> = BTreeMap::new();
    // --- Schreiben (0x2E) mit vorher gelesenem Wert ---
    let mut wr: Vec<(u32, u16, Option<Vec<u8>>, Vec<u8>)> = Vec::new();
    // --- gelesene Messwerte ---
    let mut reads: BTreeMap<(u32, u16), Vec<u8>> = BTreeMap::new();
    let mut blocks: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    let mut kwp_writes: Vec<(u8, Vec<u8>)> = Vec::new();

    let mut last_read: BTreeMap<(u32, u16), Vec<u8>> = BTreeMap::new();
    for (i, m) in msgs.iter().enumerate() {
        // Antworten auf 0x22 merken (Wert vor dem Schreiben)
        if !m.from_tester && m.proto == Proto::Uds && m.data.first() == Some(&0x62) && m.data.len() >= 3 {
            let did = u16::from_be_bytes([m.data[1], m.data[2]]);
            if let Some(req) = msgs[..i].iter().rev().find(|q| q.from_tester && q.data.first() == Some(&0x22)) {
                last_read.insert((req.id, did), m.data[3..].to_vec());
                reads.entry((req.id, did)).or_insert_with(|| m.data[3..].to_vec());
            }
        }
        if !m.from_tester || skip.contains(&key(m)) {
            continue;
        }
        let a = |k: usize| m.data.get(k).copied().unwrap_or(0);
        match (m.proto, m.data.first()) {
            (Proto::Uds, Some(0x2F)) if m.data.len() >= 4 && a(3) == 0x03 => {
                let e = io.entry((m.id, u16::from_be_bytes([a(1), a(2)]))).or_insert((None, vec![]));
                e.0 = e.0.or(response_id(msgs, i));
                e.1.push(m.data[4..].to_vec());
            }
            (Proto::Uds, Some(0x31)) if m.data.len() >= 4 && a(1) == 0x01 => {
                let e = rt.entry((m.id, u16::from_be_bytes([a(2), a(3)]))).or_insert((None, vec![]));
                e.0 = e.0.or(response_id(msgs, i));
                e.1.push(m.data[4..].to_vec());
            }
            (Proto::Uds, Some(0x2E)) if m.data.len() >= 4 => {
                let did = u16::from_be_bytes([a(1), a(2)]);
                wr.push((m.id, did, last_read.get(&(m.id, did)).cloned(), m.data[3..].to_vec()));
            }
            (Proto::Kwp, Some(0x21)) => *blocks.entry((m.address.unwrap_or(0), a(1))).or_default() += 1,
            (Proto::Kwp, Some(0x2E)) | (Proto::Kwp, Some(0x3B)) => kwp_writes.push((m.address.unwrap_or(0), m.data.clone())),
            _ => {}
        }
    }

    let hex = |d: &[u8]| d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
    let val = |d: &[u8]| d.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b));
    for ((tx, did), (rx, values)) in &io {
        let nums: Vec<u64> = values.iter().map(|v| val(v)).collect();
        let (lo, hi) = (nums.iter().min().copied().unwrap_or(0), nums.iter().max().copied().unwrap_or(0));
        let bytes = values.iter().map(Vec::len).max().unwrap_or(2).max(1);
        let rx = rx.unwrap_or_else(|| u16::try_from(*tx).map(|t| u32::from(crate::uds::antwort_id(t))).unwrap_or(*tx + 8));
        out.push(Vorschlag {
            titel: tr!("Stellglied {did} an {tx}: {a} Sollwert(e) {lo}…{hi} (roh)", did = format!("{:#06x}", did), tx = format!("{:#05x}", tx), a = values.len(), lo = lo, hi = hi),
            toml: crate::platzhalter!("[[stellfunktion]]\nname = \"Stellglied {did}\"   # Namen anpassen, z. B. \"Leerlaufdrehzahl anheben\"\nart = \"uds-io\"\ntx = {tx}\nrx = {rx}\nid = {a}\nbytes = {bytes}\nfaktor = 1.0          # Rohwert → Wert; im Mitschnitt gesehen: {lo} … {hi}\noffset = 0.0\nmin = {lo}.0          # Sicherheitsgrenzen bewusst eng: nur gesehene Werte\nmax = {hi}.0\nrueckmeldung_pid = 0x0C\nmax_dauer_s = 300\neinheit = \"\"\n", did = format!("{:#06x}", did), tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx), a = format!("{:#06x}", did), bytes = bytes, lo = lo, hi = hi),
        });
    }
    for ((tx, id), (rx, values)) in &rt {
        let rx = rx.unwrap_or_else(|| u16::try_from(*tx).map(|t| u32::from(crate::uds::antwort_id(t))).unwrap_or(*tx + 8));
        let data: Vec<String> = values.iter().map(|v| hex(v)).collect();
        out.push(Vorschlag {
            titel: tr!("Routine {id} an {tx} ({a}× gestartet)", id = format!("{:#06x}", id), tx = format!("{:#05x}", tx), a = values.len()),
            toml: crate::platzhalter!("[[stellfunktion]]\nname = \"Routine {id}\"   # Namen anpassen\nart = \"uds-routine\"\ntx = {tx}\nrx = {rx}\nid = {a}\nbytes = {b}\nmin = 0.0             # Parameter im Mitschnitt: {c}\nmax = 0.0\n", id = format!("{:#06x}", id), tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx), a = format!("{:#06x}", id), b = values.iter().map(Vec::len).max().unwrap_or(0), c = data.join(", ")),
        });
    }
    for (tx, did, before, after) in &wr {
        let mut t = String::new();
        let title;
        if let Some(b) = before.as_ref().filter(|b| b.len() == after.len()) {
            let mut bits = Vec::new();
            for (i, (x, y)) in b.iter().zip(after).enumerate() {
                for bit in 0..8u8 {
                    if (x >> bit) & 1 != (y >> bit) & 1 {
                        bits.push((i, bit, (y >> bit) & 1 == 1));
                    }
                }
            }
            title = tr!("Schreiben {did} an {tx}: {a} zu {b} ({c} Bit geändert)", did = format!("{:#06x}", did), tx = format!("{:#05x}", tx), a = hex(b), b = hex(after), c = bits.len());
            t += &crate::platzhalter!("# Kennung {did} geschrieben: vorher {a}  →  nachher {b}\n", did = format!("{:#06x}", did), a = hex(b), b = hex(after));
            if *did == crate::backup::VAG_CODING_DID || bits.len() <= 8 {
                t += "# Codier-Bits (Namen eintragen):\n";
                for (byte, bit, on) in bits {
                    t += &crate::platzhalter!("[[coding]]\nbyte = {byte}\nbit = {bit}\nname = \"Byte {byte} Bit {bit} ({a})\"\n", byte = byte, bit = bit, a = if on { "eingeschaltet" } else { "ausgeschaltet" });
                }
            }
        } else {
            title = tr!("Schreiben {did} an {tx}: {a}", did = format!("{:#06x}", did), tx = format!("{:#05x}", tx), a = hex(after));
            t += &crate::platzhalter!("# Kennung {did} geschrieben: {a} (vorheriger Wert nicht im Mitschnitt)\n", did = format!("{:#06x}", did), a = hex(after));
        }
        if *did != crate::backup::VAG_CODING_DID {
            t += &crate::platzhalter!("[[adaptation]]\nchannel = {did}\nname = \"Anpassung {a}\"   # Namen anpassen\n", did = did, a = format!("{:#06x}", did));
        } else {
            t += "# (im [ecu]-Kopf: coding_did = 0x0600)\n";
        }
        out.push(Vorschlag { titel: title, toml: t });
    }
    for (addr, data) in &kwp_writes {
        out.push(Vorschlag {
            titel: tr!("KWP-Schreibbefehl an {addr}: {a}", addr = format!("{:#04x}", addr), a = hex(data)),
            toml: crate::platzhalter!("# TP 2.0 / KWP an Steuergerät {addr}: {a}\n# (Anpassung/Codierung bei älteren Fahrzeugen — Aufbau noch zu prüfen)\n", addr = format!("{:#04x}", addr), a = hex(data)),
        });
    }
    let interesting: Vec<_> = reads.iter().filter(|((tx, did), _)| !skip.contains(&(*tx, vec![0x22, (*did >> 8) as u8, *did as u8]))).collect();
    if !interesting.is_empty() {
        let mut t = String::from("# Vom Werkstattgerät gelesene Kennungen (Namen/Skalierung eintragen):\n");
        for ((tx, did), v) in &interesting {
            t += &crate::platzhalter!("[[measuring]]\nid = {did}   # an {tx}, Beispielwert {a}\nname = \"\"\nunit = \"\"\nfactor = 1.0\noffset = 0.0\n", did = format!("{:#06x}", did), tx = format!("{:#05x}", tx), a = hex(v));
        }
        out.push(Vorschlag { titel: tr!("{a} gelesene Kennung(en) als Messwerte", a = interesting.len()), toml: t });
    }
    if !blocks.is_empty() {
        let mut t = String::from("# Vom Werkstattgerät gelesene Messwertblöcke (TP 2.0):\n");
        for ((addr, b), n) in &blocks {
            t += &crate::platzhalter!("[[measuring]]\nid = {b}   # Steuergerät {addr}, {n}× gelesen\nname = \"\"\nunit = \"\"\nfactor = 1.0\noffset = 0.0\n", b = b, addr = format!("{:#04x}", addr), n = n);
        }
        out.push(Vorschlag { titel: tr!("{a} Messwertblock/-blöcke", a = blocks.len()), toml: t });
    }
    out
}

/// Alle Vorschläge als eine Label-Datei (mit [ecu]-Kopf).
pub fn als_label_datei(v: &[Vorschlag], address: u16, name: &str) -> String {
    let mut s = crate::platzhalter!("# Aus Mitschnitt gelernt ({a}) — bitte Namen, Einheiten und Grenzen prüfen!\n\n[ecu]\naddress = {address}\nname = \"{name}\"\n\n", a = crate::report::now_local(), address = format!("{:#04x}", address), name = name);
    for x in v {
        s += &format!("# --- {} ---\n{}\n", x.titel, x.toml);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_from_demo_action_log() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap();
        let action = crate::canlog::demo_log_aktion();
        let plain = crate::canlog::demo_log();
        let v = lernen(&action, Some(&plain));
        let all = v.iter().map(|x| x.titel.clone()).collect::<Vec<_>>().join(" | ");
        assert!(all.contains("Stellglied 0xf1c0"), "{all}");
        assert!(all.contains("1000…1400"), "{all}");
        assert!(all.contains("Schreiben 0x0600"), "{all}");
        let file = als_label_datei(&v, 0x01, "Motor (gelernt)");
        // Muss als Label-Datei gültig sein
        crate::labels::LabelDb::check_file(&file).unwrap();
        let lf = crate::labels::LabelFile::parse(&file).unwrap();
        assert_eq!(lf.stellfunktion.len(), 1);
        assert_eq!(lf.stellfunktion[0].id, crate::sim::DEMO_IDLE_DID);
        assert!(!lf.coding.is_empty());
    }
}
