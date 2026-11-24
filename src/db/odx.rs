//! ODX/PDX-Import (ASAM MCD-2D) — nur für Daten, die der Nutzer rechtmäßig
//! besitzt (z. B. gekauft). Die Daten bleiben ausschließlich in der eigenen
//! Datenbank: Quelle `ODX`/`PDX`, nie exportiert ([`super::darf_weitergeben`]),
//! nie über die REST-Schnittstelle ausgegeben.
//!
//! Übernommen wird ein bewusst kleiner Teil:
//! - Diagnose-Schichten (`BASE-VARIANT`, `ECU-VARIANT`) → Steuergeräte
//! - `DTC` → Fehlercodes mit Text
//! - `DIAG-SERVICE` mit Anfrage `0x22 <DID>` (ReadDataByIdentifier) → DIDs mit Namen
//!
//! PDX ist ein ZIP mit ODX-Dateien (gespeichert oder „deflate“).

use super::paket::{Datenpaket, PaketDid, PaketFehler, PaketHersteller, PaketQuelle, PaketSteuergeraet};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

/// Text eines direkten Kind-Elements.
fn kind_text<'a>(n: roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    n.children().find(|c| c.has_tag_name(name)).and_then(|c| c.text()).map(str::trim).filter(|t| !t.is_empty())
}

fn name(n: roxmltree::Node) -> String {
    kind_text(n, "LONG-NAME").or_else(|| kind_text(n, "SHORT-NAME")).unwrap_or("").to_string()
}

/// Zahl wie in ODX (dezimal, manchmal 0x…).
fn zahl(s: &str) -> Option<u32> {
    let s = s.trim();
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

/// Eine ODX-Datei lesen → Steuergeräte mit Fehlern und DIDs.
pub fn steuergeraete_aus_odx(xml: &str) -> Result<Vec<PaketSteuergeraet>> {
    let doc = roxmltree::Document::parse_with_options(xml, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).context("ODX ist kein gültiges XML")?;
    if !doc.root_element().has_tag_name("ODX") {
        bail!(crate::tr!("Keine ODX-Datei (Wurzel ist <{a}>)", a = doc.root_element().tag_name().name()));
    }
    // Anfragen: ID → DID (SID 0x22 + Parameter mit SEMANTIC="ID")
    let mut did_von_anfrage: HashMap<&str, u16> = HashMap::new();
    for req in doc.descendants().filter(|n| n.has_tag_name("REQUEST")) {
        let Some(id) = req.attribute("ID") else { continue };
        let mut sid = None;
        let mut did = None;
        for p in req.descendants().filter(|n| n.has_tag_name("PARAM")) {
            let wert = kind_text(p, "CODED-VALUE").and_then(zahl);
            match p.attribute("SEMANTIC") {
                Some("SERVICE-ID") => sid = wert,
                Some("ID") | Some("DATA-ID") => did = did.or(wert),
                _ => {}
            }
        }
        if let (Some(0x22), Some(d)) = (sid, did) {
            if let Ok(d) = u16::try_from(d) {
                did_von_anfrage.insert(id, d);
            }
        }
    }
    let mut out = Vec::new();
    for schicht in doc.descendants().filter(|n| n.has_tag_name("BASE-VARIANT") || n.has_tag_name("ECU-VARIANT")) {
        let mut sg = PaketSteuergeraet { name: name(schicht), protokoll: "UDS".into(), ..Default::default() };
        if sg.name.is_empty() {
            continue;
        }
        for d in schicht.descendants().filter(|n| n.has_tag_name("DTC")) {
            let code = kind_text(d, "DISPLAY-TROUBLE-CODE")
                .map(str::to_string)
                .or_else(|| kind_text(d, "TROUBLE-CODE").and_then(zahl).map(|c| format!("{c:06X}")));
            let text = kind_text(d, "TEXT").unwrap_or("").to_string();
            if let Some(code) = code {
                sg.fehler.push(PaketFehler { code, text });
            }
        }
        for dienst in schicht.descendants().filter(|n| n.has_tag_name("DIAG-SERVICE")) {
            let Some(r) = dienst.children().find(|c| c.has_tag_name("REQUEST-REF")).and_then(|r| r.attribute("ID-REF")) else { continue };
            if let Some(&did) = did_von_anfrage.get(r) {
                if !sg.dids.iter().any(|d| d.did == did) {
                    sg.dids.push(PaketDid { did, name: name(dienst), einheit: String::new(), faktor: 1.0, offset: 0.0 });
                }
            }
        }
        out.push(sg);
    }
    Ok(out)
}

/// Dateien aus einem ZIP (PDX) lesen: (Name, Inhalt). Unterstützt „stored“ und „deflate“.
pub fn zip_eintraege(daten: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let u16le = |i: usize| -> Result<u16> { Ok(u16::from_le_bytes(daten.get(i..i + 2).context("ZIP zu kurz")?.try_into()?)) };
    let u32le = |i: usize| -> Result<u32> { Ok(u32::from_le_bytes(daten.get(i..i + 4).context("ZIP zu kurz")?.try_into()?)) };
    // Zentralverzeichnis über den End-Datensatz finden (Größen stimmen dort immer)
    let ende = (0..daten.len().saturating_sub(21)).rev().find(|&i| daten[i..].starts_with(b"PK\x05\x06")).context(crate::tr!("Kein ZIP (PDX) — Ende-Kennung fehlt"))?;
    let anzahl = usize::from(u16le(ende + 10)?);
    let mut pos = u32le(ende + 16)? as usize;
    let mut out = Vec::new();
    for _ in 0..anzahl {
        if !daten.get(pos..).is_some_and(|d| d.starts_with(b"PK\x01\x02")) {
            bail!(crate::tr!("ZIP-Verzeichnis beschädigt"));
        }
        let methode = u16le(pos + 10)?;
        let gepackt = u32le(pos + 20)? as usize;
        let groesse = u32le(pos + 24)? as usize;
        let (nl, el, kl) = (usize::from(u16le(pos + 28)?), usize::from(u16le(pos + 30)?), usize::from(u16le(pos + 32)?));
        let lokal = u32le(pos + 42)? as usize;
        let name = String::from_utf8_lossy(daten.get(pos + 46..pos + 46 + nl).context("ZIP-Name")?).to_string();
        pos += 46 + nl + el + kl;
        if name.ends_with('/') {
            continue;
        }
        if !daten.get(lokal..).is_some_and(|d| d.starts_with(b"PK\x03\x04")) {
            bail!(crate::tr!("ZIP-Eintrag {a} beschädigt", a = name));
        }
        let start = lokal + 30 + usize::from(u16le(lokal + 26)?) + usize::from(u16le(lokal + 28)?);
        let roh = daten.get(start..start + gepackt).context(crate::tr!("ZIP-Eintrag {a} abgeschnitten", a = name))?;
        let inhalt = match methode {
            0 => roh.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(roh, groesse.max(1) * 2 + 1024).map_err(|e| anyhow::anyhow!(crate::tr!("ZIP-Eintrag {a} entpacken: {e:?}", a = name, e = e)))?,
            m => bail!(crate::tr!("ZIP-Eintrag {a}: Packverfahren {m} wird nicht unterstützt", a = name, m = m)),
        };
        out.push((name, inhalt));
    }
    Ok(out)
}

/// ODX- oder PDX-Datei lesen und als Datenpaket aufbereiten (für [`super::Db::paket_importieren_lokal`]).
pub fn lesen(pfad: &std::path::Path, hersteller: &str) -> Result<(Datenpaket, super::QuellArt)> {
    let daten = std::fs::read(pfad).with_context(|| crate::tr!("{a} lesen", a = pfad.display()))?;
    let pdx = daten.starts_with(b"PK");
    let mut steuergeraete = Vec::new();
    if pdx {
        for (name, inhalt) in zip_eintraege(&daten)? {
            let n = name.to_ascii_lowercase();
            if n.ends_with(".odx") || n.contains(".odx-") {
                steuergeraete.extend(steuergeraete_aus_odx(&String::from_utf8_lossy(&inhalt)).with_context(|| crate::tr!("in {a}", a = name))?);
            }
        }
    } else {
        steuergeraete = steuergeraete_aus_odx(&String::from_utf8_lossy(&daten))?;
    }
    if steuergeraete.is_empty() {
        bail!(crate::tr!("Keine Steuergeräte (BASE-VARIANT/ECU-VARIANT) in {a} gefunden.", a = pfad.display()));
    }
    let dateiname = pfad.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let p = Datenpaket {
        quelle: PaketQuelle { name: dateiname, version: String::new(), lizenz: crate::tr!("eigene Lizenz des Nutzers — nicht weitergeben"), herkunft: pfad.display().to_string(), nur_lokal: false },
        hersteller: vec![PaketHersteller { name: hersteller.to_string(), wmi: Vec::new(), steuergeraete }],
        ..Default::default()
    };
    Ok((p, if pdx { super::QuellArt::Pdx } else { super::QuellArt::Odx }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Erfundenes Mini-ODX im Aufbau der Norm (keine echten Herstellerdaten).
    const BEISPIEL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ODX MODEL-VERSION="2.2.0">
  <DIAG-LAYER-CONTAINER ID="DLC_Test">
    <SHORT-NAME>DLC_Test</SHORT-NAME>
    <BASE-VARIANTS>
      <BASE-VARIANT ID="BV_Testgeraet">
        <SHORT-NAME>BV_Testgeraet</SHORT-NAME>
        <LONG-NAME>Teststeuergerät</LONG-NAME>
        <DIAG-DATA-DICTIONARY-SPEC>
          <DTC-DOPS>
            <DTC-DOP ID="DOP_DTC">
              <SHORT-NAME>DOP_DTC</SHORT-NAME>
              <DTCS>
                <DTC ID="DTC_1"><SHORT-NAME>DTC_1</SHORT-NAME><TROUBLE-CODE>1234</TROUBLE-CODE><DISPLAY-TROUBLE-CODE>P04D2</DISPLAY-TROUBLE-CODE><TEXT>Erfundener Fehler</TEXT></DTC>
                <DTC ID="DTC_2"><SHORT-NAME>DTC_2</SHORT-NAME><TROUBLE-CODE>0x9A0B11</TROUBLE-CODE><TEXT>Ohne Anzeigecode</TEXT></DTC>
              </DTCS>
            </DTC-DOP>
          </DTC-DOPS>
        </DIAG-DATA-DICTIONARY-SPEC>
        <DIAG-COMMS>
          <DIAG-SERVICE ID="DS_Lesen_Test" SEMANTIC="DATA">
            <SHORT-NAME>DS_Lesen_Test</SHORT-NAME>
            <LONG-NAME>Testwert lesen</LONG-NAME>
            <REQUEST-REF ID-REF="RQ_Lesen_Test"/>
          </DIAG-SERVICE>
          <DIAG-SERVICE ID="DS_Reset">
            <SHORT-NAME>DS_Reset</SHORT-NAME>
            <REQUEST-REF ID-REF="RQ_Reset"/>
          </DIAG-SERVICE>
        </DIAG-COMMS>
        <REQUESTS>
          <REQUEST ID="RQ_Lesen_Test">
            <SHORT-NAME>RQ_Lesen_Test</SHORT-NAME>
            <PARAMS>
              <PARAM SEMANTIC="SERVICE-ID" xsi:type="CODED-CONST" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><SHORT-NAME>SID</SHORT-NAME><BYTE-POSITION>0</BYTE-POSITION><CODED-VALUE>34</CODED-VALUE></PARAM>
              <PARAM SEMANTIC="ID" xsi:type="CODED-CONST" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><SHORT-NAME>DID</SHORT-NAME><BYTE-POSITION>1</BYTE-POSITION><CODED-VALUE>61708</CODED-VALUE></PARAM>
            </PARAMS>
          </REQUEST>
          <REQUEST ID="RQ_Reset">
            <SHORT-NAME>RQ_Reset</SHORT-NAME>
            <PARAMS>
              <PARAM SEMANTIC="SERVICE-ID"><SHORT-NAME>SID</SHORT-NAME><CODED-VALUE>17</CODED-VALUE></PARAM>
            </PARAMS>
          </REQUEST>
        </REQUESTS>
      </BASE-VARIANT>
    </BASE-VARIANTS>
  </DIAG-LAYER-CONTAINER>
</ODX>"#;

    #[test]
    fn odx_lesen() {
        let s = steuergeraete_aus_odx(BEISPIEL).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Teststeuergerät");
        assert_eq!(s[0].fehler.len(), 2);
        assert_eq!(s[0].fehler[0].code, "P04D2");
        assert_eq!(s[0].fehler[1].code, "9A0B11");
        assert_eq!(s[0].dids.len(), 1, "nur 0x22-Dienste");
        assert_eq!((s[0].dids[0].did, s[0].dids[0].name.as_str()), (0xF10C, "Testwert lesen"));
        assert!(steuergeraete_aus_odx("<kein/>").is_err());
        assert!(steuergeraete_aus_odx("kaputt <").is_err());
    }

    /// Minimales ZIP mit einem Eintrag bauen (gespeichert oder deflate).
    fn zip(name: &str, inhalt: &[u8], deflate: bool) -> Vec<u8> {
        let gepackt = if deflate { miniz_oxide::deflate::compress_to_vec(inhalt, 6) } else { inhalt.to_vec() };
        let methode: u16 = if deflate { 8 } else { 0 };
        let mut z = Vec::new();
        z.extend(b"PK\x03\x04");
        z.extend(20u16.to_le_bytes());
        z.extend(0u16.to_le_bytes());
        z.extend(methode.to_le_bytes());
        z.extend([0u8; 8]); // Zeit, Datum, CRC (nicht geprüft)
        z.extend((gepackt.len() as u32).to_le_bytes());
        z.extend((inhalt.len() as u32).to_le_bytes());
        z.extend((name.len() as u16).to_le_bytes());
        z.extend(0u16.to_le_bytes());
        z.extend(name.as_bytes());
        z.extend(&gepackt);
        let zv = z.len();
        z.extend(b"PK\x01\x02");
        z.extend(20u16.to_le_bytes());
        z.extend(20u16.to_le_bytes());
        z.extend(0u16.to_le_bytes());
        z.extend(methode.to_le_bytes());
        z.extend([0u8; 8]);
        z.extend((gepackt.len() as u32).to_le_bytes());
        z.extend((inhalt.len() as u32).to_le_bytes());
        z.extend((name.len() as u16).to_le_bytes());
        z.extend([0u8; 12]); // Extra, Kommentar, Disk, Attribute
        z.extend(0u32.to_le_bytes()); // Offset lokaler Kopf
        z.extend(name.as_bytes());
        let zv_len = z.len() - zv;
        z.extend(b"PK\x05\x06");
        z.extend([0u8; 4]);
        z.extend(1u16.to_le_bytes());
        z.extend(1u16.to_le_bytes());
        z.extend((zv_len as u32).to_le_bytes());
        z.extend((zv as u32).to_le_bytes());
        z.extend(0u16.to_le_bytes());
        z
    }

    #[test]
    fn pdx_gespeichert_und_gepackt() {
        for deflate in [false, true] {
            let z = zip("daten/test.odx-d", BEISPIEL.as_bytes(), deflate);
            let e = zip_eintraege(&z).unwrap();
            assert_eq!(e[0].0, "daten/test.odx-d");
            assert_eq!(e[0].1, BEISPIEL.as_bytes());
        }
        assert!(zip_eintraege(b"kein zip").is_err());
    }

    #[test]
    fn import_bleibt_lokal() {
        let dir = std::env::temp_dir().join(format!("ecuwb-odx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("paket.pdx");
        std::fs::write(&p, zip("a.odx-d", BEISPIEL.as_bytes(), true)).unwrap();
        let (paket, art) = lesen(&p, "Testmarke").unwrap();
        assert_eq!(art, super::super::QuellArt::Pdx);
        let db = super::super::Db::im_speicher().unwrap();
        let e = db.paket_importieren_lokal(&paket, art).unwrap();
        assert_eq!((e.steuergeraete, e.fehler, e.messwerte), (1, 2, 1));
        assert_eq!(db.fehlertext("P04D2").unwrap().as_deref(), Some("Erfundener Fehler"), "lokal nutzbar");
        let (export, weg) = db.paket_exportieren("x", "CC0-1.0").unwrap();
        assert!(export.hersteller.is_empty() && export.fehler.is_empty(), "nie im Export");
        assert!(weg.iter().any(|w| w.contains("paket.pdx")));
        assert_eq!(super::super::api::antworten(&db, "GET", "/api/v1/fehler/P04D2").0, 404, "nie über die Schnittstelle");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
