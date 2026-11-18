//! REST-Schnittstelle auf die Fahrzeug-Datenbank — **nur lesen**, nur auf
//! diesem Rechner (127.0.0.1), mit zufälligem Schlüssel im Pfad.
//!
//! ```text
//! GET /<schluessel>/api/v1/status
//! GET /<schluessel>/api/v1/quellen
//! GET /<schluessel>/api/v1/hersteller
//! GET /<schluessel>/api/v1/hersteller/{id}/steuergeraete
//! GET /<schluessel>/api/v1/steuergeraete/{id}
//! GET /<schluessel>/api/v1/fehler/{code}
//! GET /<schluessel>/api/v1/can/{id-hex}/signale
//! GET /<schluessel>/api/v1/fahrzeug?vin=WVW…
//! ```
//!
//! Die Schnittstelle schreibt nie — weder in die Datenbank (sie wird nur
//! lesend geöffnet) noch ans Auto. Für eigene Skripte (z. B. Python).

use super::Db;
use anyhow::{Context, Result};
use rusqlite::params;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;

/// Antwort auf einen Pfad (ohne Schlüssel-Präfix): (HTTP-Status, JSON).
pub fn antworten(db: &Db, methode: &str, pfad: &str) -> (u16, Value) {
    if methode != "GET" {
        return (405, json!({ "fehler": "nur GET (die Schnittstelle liest nur)" }));
    }
    let (weg, abfrage) = pfad.split_once('?').unwrap_or((pfad, ""));
    let teile: Vec<&str> = weg.trim_matches('/').split('/').collect();
    let r = match teile.as_slice() {
        ["api", "v1", "status"] => status(db),
        ["api", "v1", "quellen"] => quellen(db),
        ["api", "v1", "hersteller"] => hersteller(db),
        ["api", "v1", "hersteller", id, "steuergeraete"] => zahl(id).and_then(|id| steuergeraete(db, id)),
        ["api", "v1", "steuergeraete", id] => zahl(id).and_then(|id| steuergeraet(db, id)),
        ["api", "v1", "fehler", code] => fehler(db, code),
        ["api", "v1", "can", id, "signale"] => u32::from_str_radix(id.trim_start_matches("0x"), 16).map_err(|_| Fehler::Anfrage("CAN-ID hexadezimal".into())).and_then(|id| signale(db, id)),
        ["api", "v1", "fahrzeug"] => fahrzeug(db, abfrage),
        _ => Err(Fehler::NichtGefunden),
    };
    match r {
        Ok(v) => (200, v),
        Err(Fehler::NichtGefunden) => (404, json!({ "fehler": "nicht gefunden" })),
        Err(Fehler::Anfrage(t)) => (400, json!({ "fehler": t })),
        Err(Fehler::Intern(t)) => (500, json!({ "fehler": t })),
    }
}

enum Fehler {
    NichtGefunden,
    Anfrage(String),
    Intern(String),
}

impl From<rusqlite::Error> for Fehler {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => Fehler::NichtGefunden,
            e => Fehler::Intern(e.to_string()),
        }
    }
}

impl From<anyhow::Error> for Fehler {
    fn from(e: anyhow::Error) -> Self {
        Fehler::Intern(format!("{e:#}"))
    }
}

type Antwort = std::result::Result<Value, Fehler>;

/// Bedingung: Zeile stammt nicht aus selbst gekauften Herstellerdaten (ODX/PDX) —
/// die verlassen die Datenbank nie, auch nicht über diese Schnittstelle.
const TEILBAR: &str = "source_id NOT IN (SELECT id FROM data_source WHERE source_type IN ('ODX', 'PDX'))";

fn zahl(s: &str) -> std::result::Result<i64, Fehler> {
    s.parse().map_err(|_| Fehler::Anfrage(format!("„{s}“ ist keine Nummer")))
}

fn status(db: &Db) -> Antwort {
    let t: serde_json::Map<String, Value> = db.statistik()?.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    Ok(json!({ "schema_version": db.schema_version()?, "programm": env!("CARGO_PKG_VERSION"), "tabellen": t }))
}

fn quellen(db: &Db) -> Antwort {
    Ok(Value::Array(
        db.quellen()?
            .into_iter()
            .map(|q| json!({ "id": q.id, "art": q.art.text(), "name": q.name, "version": q.version, "lizenz": q.lizenz, "herkunft": q.herkunft, "importiert": q.importiert, "eintraege": q.zeilen }))
            .collect(),
    ))
}

fn hersteller(db: &Db) -> Antwort {
    let mut st = db.con.prepare("SELECT id, name, vin_wmi, country FROM manufacturer ORDER BY name")?;
    let v = st
        .query_map([], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?, "wmi": r.get::<_, String>(2)?, "land": r.get::<_, String>(3)? })))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Value::Array(v))
}

fn steuergeraete(db: &Db, hersteller: i64) -> Antwort {
    let mut st = db.con.prepare(&format!("SELECT id, ecu_name, ecu_family, protocol, diagnostic_address, can_tx, can_rx, source_id FROM ecu WHERE manufacturer_id = ?1 AND {TEILBAR} ORDER BY diagnostic_address, id"))?;
    let v = st.query_map(params![hersteller], ecu_json)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Value::Array(v))
}

fn ecu_json(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "name": r.get::<_, String>(1)?,
        "gruppe": r.get::<_, String>(2)?,
        "protokoll": r.get::<_, String>(3)?,
        "adresse": r.get::<_, Option<i64>>(4)?,
        "tx": r.get::<_, Option<i64>>(5)?,
        "rx": r.get::<_, Option<i64>>(6)?,
        "quelle": r.get::<_, i64>(7)?,
    }))
}

fn steuergeraet(db: &Db, id: i64) -> Antwort {
    let mut e = db.con.query_row(&format!("SELECT id, ecu_name, ecu_family, protocol, diagnostic_address, can_tx, can_rx, source_id FROM ecu WHERE id = ?1 AND {TEILBAR}"), params![id], ecu_json)?;
    let liste = |sql: &str, f: fn(&rusqlite::Row) -> rusqlite::Result<Value>| -> std::result::Result<Value, Fehler> {
        let mut st = db.con.prepare(sql)?;
        let v = st.query_map(params![id], f)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Value::Array(v))
    };
    e["fehler"] = liste("SELECT dtc_code, title FROM dtc WHERE ecu_id = ?1 ORDER BY dtc_code", |r| Ok(json!({ "code": r.get::<_, String>(0)?, "text": r.get::<_, String>(1)? })))?;
    e["dids"] = liste("SELECT did, name, unit, factor, \"offset\" FROM did WHERE ecu_id = ?1 ORDER BY did", |r| {
        Ok(json!({ "did": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?, "einheit": r.get::<_, String>(2)?, "faktor": r.get::<_, f64>(3)?, "offset": r.get::<_, f64>(4)? }))
    })?;
    e["codierung"] = liste("SELECT byte_index, bit_index, label, requires, excludes FROM coding_item WHERE ecu_id = ?1 ORDER BY byte_index, bit_index", |r| {
        Ok(json!({ "byte": r.get::<_, i64>(0)?, "bit": r.get::<_, Option<i64>>(1)?, "name": r.get::<_, String>(2)?, "benoetigt": r.get::<_, String>(3)?, "schliesst_aus": r.get::<_, String>(4)? }))
    })?;
    e["anpassung"] = liste("SELECT channel, name FROM adaptation WHERE ecu_id = ?1 ORDER BY channel", |r| Ok(json!({ "kanal": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)? })))?;
    Ok(e)
}

fn fehler(db: &Db, code: &str) -> Antwort {
    let mut st = db.con.prepare("SELECT d.dtc_code, d.title, d.ecu_id, d.source_id, s.source_name FROM dtc d JOIN data_source s ON s.id = d.source_id WHERE d.dtc_code = ?1 AND s.source_type NOT IN ('ODX', 'PDX') ORDER BY d.source_id DESC")?;
    let v = st
        .query_map(params![code.trim().to_ascii_uppercase()], |r| {
            Ok(json!({ "code": r.get::<_, String>(0)?, "text": r.get::<_, String>(1)?, "steuergeraet": r.get::<_, Option<i64>>(2)?, "quelle": r.get::<_, i64>(3)?, "quelle_name": r.get::<_, String>(4)? }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if v.is_empty() {
        return Err(Fehler::NichtGefunden);
    }
    Ok(Value::Array(v))
}

fn signale(db: &Db, can_id: u32) -> Antwort {
    let mut st = db.con.prepare(
        "SELECT m.name, s.signal_name, s.start_bit, s.bit_length, s.little_endian, s.is_signed, s.factor, s.\"offset\", s.unit, m.manufacturer_id
         FROM can_signal s JOIN can_message m ON m.id = s.can_message_id WHERE m.can_id = ?1 ORDER BY m.id, s.id",
    )?;
    let v = st
        .query_map(params![can_id], |r| {
            Ok(json!({
                "botschaft": r.get::<_, String>(0)?, "signal": r.get::<_, String>(1)?, "start": r.get::<_, i64>(2)?, "laenge": r.get::<_, i64>(3)?,
                "intel": r.get::<_, i64>(4)? != 0, "vorzeichen": r.get::<_, i64>(5)? != 0, "faktor": r.get::<_, f64>(6)?, "offset": r.get::<_, f64>(7)?,
                "einheit": r.get::<_, String>(8)?, "hersteller": r.get::<_, Option<i64>>(9)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if v.is_empty() {
        return Err(Fehler::NichtGefunden);
    }
    Ok(Value::Array(v))
}

fn fahrzeug(db: &Db, abfrage: &str) -> Antwort {
    let vin = abfrage.split('&').find_map(|p| p.strip_prefix("vin=")).ok_or_else(|| Fehler::Anfrage("?vin=… fehlt".into()))?;
    match db.hersteller_zur_vin(vin)? {
        Some((id, name)) => Ok(json!({ "hersteller_id": id, "hersteller": name })),
        None => Err(Fehler::NichtGefunden),
    }
}

/// Zufälliger Schlüssel (32 Hex-Zeichen).
pub fn schluessel() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut s = String::new();
    for i in 0..2u8 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u8(i);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        s.push_str(&format!("{:016x}", h.finish()));
    }
    s
}

/// Server starten (blockiert). Nur 127.0.0.1. `bereit` bekommt die Adresse mit Schlüssel.
pub fn dienen(db_pfad: &std::path::Path, port: u16, schluessel: &str, bereit: impl FnOnce(&str)) -> Result<()> {
    let db = Db::nur_lesen(db_pfad)?;
    let listener = TcpListener::bind(("127.0.0.1", port)).with_context(|| crate::tr!("Port {port} belegt?", port = port))?;
    let port = listener.local_addr()?.port();
    let praefix = format!("/{schluessel}");
    bereit(&format!("http://127.0.0.1:{port}{praefix}/api/v1/"));
    for sock in listener.incoming() {
        let Ok(mut sock) = sock else { continue };
        let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        let mut buf = Vec::with_capacity(1024);
        let mut stueck = [0u8; 1024];
        while buf.len() < 8192 && !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            match sock.read(&mut stueck) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&stueck[..n]),
            }
        }
        let anfrage = String::from_utf8_lossy(&buf);
        let mut zeile = anfrage.split_whitespace();
        let (methode, pfad) = (zeile.next().unwrap_or(""), zeile.next().unwrap_or("/"));
        let (code, wert) = match pfad.strip_prefix(&praefix) {
            Some(rest) => antworten(&db, methode, rest),
            None => (404, json!({ "fehler": "nicht gefunden" })),
        };
        let text = wert.to_string();
        let grund = match code {
            200 => "OK",
            400 => "Bad Request",
            404 => "Not Found",
            405 => "Method Not Allowed",
            _ => "Internal Server Error",
        };
        let _ = write!(
            sock,
            "HTTP/1.1 {code} {grund}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{text}",
            text.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        let db = Db::im_speicher().unwrap();
        db.community_uebernehmen().unwrap();
        db.csv_text_importieren("code;text\nP0301;Zylinder 1 Zündaussetzer\n", "f.csv", "").unwrap();
        let dbc = crate::dbc::Dbc::lesen("BO_ 640 Motor_1: 8 XXX\n SG_ Drehzahl : 16|16@1+ (0.25,0) [0|16256] \"U/min\" XXX\n");
        db.dbc_importieren(&dbc, "t.dbc", "MIT", "", None).unwrap();
        db
    }

    #[test]
    fn wege() {
        let db = db();
        let (c, v) = antworten(&db, "GET", "/api/v1/status");
        assert_eq!(c, 200);
        assert_eq!(v["schema_version"], super::super::SCHEMA_VERSION);
        let (_, h) = antworten(&db, "GET", "/api/v1/hersteller");
        let vw = h[0]["id"].as_i64().unwrap();
        let (c, sg) = antworten(&db, "GET", &format!("/api/v1/hersteller/{vw}/steuergeraete"));
        assert_eq!(c, 200);
        let bordnetz = sg.as_array().unwrap().iter().find(|e| e["adresse"] == 9).unwrap();
        assert_eq!(bordnetz["name"], "Bordnetz");
        let (c, e) = antworten(&db, "GET", &format!("/api/v1/steuergeraete/{}", bordnetz["id"]));
        assert_eq!(c, 200);
        assert!(e["fehler"].is_array());
        let (c, f) = antworten(&db, "GET", "/api/v1/fehler/p0301");
        assert_eq!((c, f[0]["text"].as_str()), (200, Some("Zylinder 1 Zündaussetzer")));
        let (c, s) = antworten(&db, "GET", "/api/v1/can/280/signale");
        assert_eq!((c, s[0]["signal"].as_str()), (200, Some("Drehzahl")));
        let (c, f) = antworten(&db, "GET", "/api/v1/fahrzeug?vin=WVWZZZ1KZAM000017");
        assert_eq!((c, f["hersteller"].as_str()), (200, Some("Volkswagen")));
    }

    #[test]
    fn fehlerfaelle() {
        let db = db();
        assert_eq!(antworten(&db, "POST", "/api/v1/status").0, 405, "schreibt nie");
        assert_eq!(antworten(&db, "DELETE", "/api/v1/quellen").0, 405);
        assert_eq!(antworten(&db, "GET", "/api/v1/gibtsnicht").0, 404);
        assert_eq!(antworten(&db, "GET", "/api/v1/steuergeraete/abc").0, 400);
        assert_eq!(antworten(&db, "GET", "/api/v1/steuergeraete/999999").0, 404);
        assert_eq!(antworten(&db, "GET", "/api/v1/fehler/X0000").0, 404);
        assert_eq!(antworten(&db, "GET", "/api/v1/can/zz/signale").0, 400);
        assert_eq!(antworten(&db, "GET", "/api/v1/fahrzeug").0, 400);
        assert_ne!(schluessel(), schluessel());
        assert_eq!(schluessel().len(), 32);
    }

    #[test]
    fn server_mit_schluessel() {
        let dir = std::env::temp_dir().join(format!("ecuwb-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("t.sqlite");
        Db::oeffnen(&p).unwrap().community_uebernehmen().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let p2 = p.clone();
        std::thread::spawn(move || {
            let _ = dienen(&p2, 0, "geheim", |url| tx.send(url.to_string()).unwrap());
        });
        let url = rx.recv().unwrap();
        let port = url.split(':').nth(2).unwrap().split('/').next().unwrap().to_string();
        let hole = |pfad: &str| {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
            write!(s, "GET {pfad} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut a = String::new();
            s.read_to_string(&mut a).unwrap();
            a
        };
        assert!(hole("/geheim/api/v1/hersteller").starts_with("HTTP/1.1 200"));
        assert!(hole("/falsch/api/v1/hersteller").starts_with("HTTP/1.1 404"), "ohne Schlüssel nichts");
        assert!(hole("/api/v1/hersteller").starts_with("HTTP/1.1 404"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
