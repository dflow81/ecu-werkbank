//! Skripte (Rhai) für eigene Abläufe — **nur lesen** am Auto.
//!
//! Rhai ist eingebettet und abgeschottet: kein Datei- oder Netzzugriff, nur
//! die Funktionen unten. Schreibende Funktionen gibt es bewusst nicht; dafür
//! bleiben die Befehle mit Bestätigung (`--ja`) bzw. die GUI.
//!
//! ```rhai
//! // Drehzahl 10× im Abstand von 0,5 s
//! for i in 0..10 {
//!     print(`Drehzahl: ${obd(0x0C)} 1/min`);
//!     warte(500);
//! }
//! print(uds_text(0x7E0, 0xF190));          // Fahrgestellnummer
//! for w in kwp_block(0x01, 2) { print(w.text); }   // TP-2.0-Messwertblock
//! ```
//!
//! | Funktion | Ergebnis |
//! |---|---|
//! | `obd(pid)` | Messwert Modus 01 (Zahl) |
//! | `obd_vin()` | Fahrgestellnummer (OBD Modus 09) |
//! | `uds_lesen(tx, did)` | Rohbytes (Liste von Zahlen) |
//! | `uds_text(tx, did)` | Rohbytes als Text |
//! | `uds_fehler(tx)` | Fehler als Textliste |
//! | `kwp_block(adresse, block)` | Liste von `#{wert, einheit, text}` |
//! | `kwp_fehler(adresse)` | Fehler als Textliste |
//! | `fehlertext(code)` | Klartext aus Labels/Datenbank |
//! | `hex(liste)` | „01 A0 FF“ |
//! | `warte(ms)`, `zeit()` | Pause; Sekunden seit Start |

use crate::kwp::{self, Kwp};
use crate::labels::LabelDb;
use crate::transport::Backend;
use crate::uds::Uds;
use anyhow::Result;
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

type Ergebnis<T> = std::result::Result<T, Box<EvalAltResult>>;

fn fehler(e: anyhow::Error) -> Box<EvalAltResult> {
    format!("{e:#}").into()
}

fn u16_von(x: i64, was: &str) -> Ergebnis<u16> {
    u16::try_from(x).map_err(|_| format!("{was} {x} ungültig").into())
}

fn u8_von(x: i64, was: &str) -> Ergebnis<u8> {
    u8::try_from(x).map_err(|_| format!("{was} {x} ungültig").into())
}

/// Engine mit allen Funktionen. `ausgabe` bekommt jede `print`-Zeile;
/// `stopp` bricht das Skript beim nächsten Schritt ab.
pub fn engine(be: Backend, db: Arc<LabelDb>, ausgabe: impl Fn(&str) + Send + Sync + 'static, stopp: Arc<AtomicBool>) -> Engine {
    let mut e = Engine::new();
    // Schutz gegen Endlos-Rekursion und Speicherfresser; Endlosschleifen beendet `stopp`
    e.set_max_call_levels(32);
    e.set_max_expr_depths(64, 32);
    e.set_max_string_size(1 << 20);
    e.set_max_array_size(100_000);
    e.set_max_map_size(10_000);
    e.on_print(move |s| ausgabe(s));
    let s2 = stopp.clone();
    e.on_progress(move |_| if s2.load(Ordering::Relaxed) { Some(Dynamic::from("abgebrochen")) } else { None });
    let start = Instant::now();

    let b = be.clone();
    e.register_fn("obd", move |pid: i64| -> Ergebnis<f64> {
        let pid = u8_von(pid, "PID")?;
        let tp = b.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX).map_err(fehler)?;
        crate::obd::Obd::new(&*tp).read_value(pid).map_err(fehler)
    });
    let b = be.clone();
    e.register_fn("obd_vin", move || -> Ergebnis<String> {
        let tp = b.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX).map_err(fehler)?;
        crate::obd::Obd::new(&*tp).vin().map_err(fehler)
    });
    let b = be.clone();
    let uds_lesen = move |tx: i64, did: i64| -> Ergebnis<Vec<u8>> {
        let (tx, did) = (u16_von(tx, "CAN-ID")?, u16_von(did, "DID")?);
        let tp = b.open(tx, crate::uds::antwort_id(tx)).map_err(fehler)?;
        let uds = Uds::new(&*tp).mit_tx(tx);
        uds.diagnostic_session(crate::uds::SESSION_DEFAULT).map_err(fehler)?;
        uds.read_data_by_id(did).map_err(fehler)
    };
    let l2 = uds_lesen.clone();
    e.register_fn("uds_lesen", move |tx: i64, did: i64| -> Ergebnis<Array> { Ok(uds_lesen(tx, did)?.into_iter().map(|b| Dynamic::from(i64::from(b))).collect()) });
    e.register_fn("uds_text", move |tx: i64, did: i64| -> Ergebnis<String> { Ok(String::from_utf8_lossy(&l2(tx, did)?).trim_matches(|c: char| c.is_control() || c == ' ').to_string()) });
    let (b, d) = (be.clone(), db.clone());
    e.register_fn("uds_fehler", move |tx: i64| -> Ergebnis<Array> {
        let tx = u16_von(tx, "CAN-ID")?;
        let tp = b.open(tx, crate::uds::antwort_id(tx)).map_err(fehler)?;
        let uds = Uds::new(&*tp).mit_tx(tx);
        uds.diagnostic_session(crate::uds::SESSION_DEFAULT).map_err(fehler)?;
        Ok(uds
            .read_dtcs(0xFF)
            .map_err(fehler)?
            .iter()
            .map(|x| {
                let c = x.code();
                Dynamic::from(format!("{c}  {}  ({})", d.dtc_description(&c).unwrap_or(""), crate::report::uds_status_text(x.status)))
            })
            .collect())
    });
    let b = be.clone();
    e.register_fn("kwp_block", move |adresse: i64, block: i64| -> Ergebnis<Array> {
        let (a, bl) = (u8_von(adresse, "Adresse")?, u8_von(block, "Block")?);
        let mut ch = kwp::open_session(&b, a).map_err(fehler)?;
        let werte = Kwp::new(&mut ch).read_block(bl).map_err(fehler)?;
        Ok(werte
            .iter()
            .map(|v| {
                let mut m = Map::new();
                let (wert, einheit) = v.decode().map(|(x, u)| (Dynamic::from(x), u.to_string())).unwrap_or((Dynamic::UNIT, String::new()));
                m.insert("wert".into(), wert);
                m.insert("einheit".into(), einheit.into());
                m.insert("text".into(), v.display().into());
                Dynamic::from_map(m)
            })
            .collect())
    });
    let (b, d) = (be, db.clone());
    e.register_fn("kwp_fehler", move |adresse: i64| -> Ergebnis<Array> {
        let a = u8_von(adresse, "Adresse")?;
        let mut ch = kwp::open_session(&b, a).map_err(fehler)?;
        Ok(Kwp::new(&mut ch)
            .read_dtcs()
            .map_err(fehler)?
            .iter()
            .map(|x| {
                let c = x.vag_code();
                Dynamic::from(format!("{c}  {}  ({})", d.dtc_description(&c).unwrap_or(""), x.status_text()))
            })
            .collect())
    });
    e.register_fn("fehlertext", move |code: &str| -> String { db.dtc_description(code).unwrap_or("").to_string() });
    e.register_fn("hex", |a: Array| -> String { a.iter().filter_map(|x| x.as_int().ok()).map(|x| format!("{:02X}", x & 0xFF)).collect::<Vec<_>>().join(" ") });
    let s3 = stopp;
    e.register_fn("warte", move |ms: i64| {
        // in Stücken warten, damit „Stopp“ sofort wirkt
        let ende = Instant::now() + Duration::from_millis(ms.clamp(0, 3_600_000) as u64);
        while Instant::now() < ende && !s3.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(20).min(ende.saturating_duration_since(Instant::now())));
        }
    });
    e.register_fn("zeit", move || start.elapsed().as_secs_f64());
    e
}

/// Skript ausführen.
pub fn ausfuehren(be: Backend, db: Arc<LabelDb>, quelltext: &str, ausgabe: impl Fn(&str) + Send + Sync + 'static, stopp: Arc<AtomicBool>) -> Result<()> {
    let e = engine(be, db, ausgabe, stopp);
    e.run(quelltext).map_err(|err| match *err {
        EvalAltResult::ErrorTerminated(..) => anyhow::anyhow!(crate::tr!("Skript abgebrochen.")),
        other => anyhow::anyhow!(crate::tr!("Skript: {e}", e = other)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn laufen(code: &str) -> (Result<()>, Vec<String>) {
        let zeilen = Arc::new(Mutex::new(Vec::new()));
        let z2 = zeilen.clone();
        let r = ausfuehren(Backend::Demo, Arc::new(LabelDb::default()), code, move |s| z2.lock().unwrap().push(s.to_string()), Arc::default());
        let v = zeilen.lock().unwrap().clone();
        (r, v)
    }

    #[test]
    fn lesen_in_der_simulation() {
        let (r, z) = laufen(
            r#"
            let n = obd(0x0C);
            print(`Drehzahl ${n}`);
            print(uds_text(0x7E0, 0xF190));
            print(hex(uds_lesen(0x7E0, 0xF190)));
            let b = kwp_block(0x01, 1);
            print(`${b.len()} Felder`);
            print(type_of(uds_fehler(0x7E0)));
            print(zeit() >= 0.0);
            print(obd_vin());
        "#,
        );
        r.unwrap();
        assert!(z[0].starts_with("Drehzahl "), "{z:?}");
        assert!(z[1].starts_with("WVW"), "Fahrgestellnummer aus der Simulation: {z:?}");
        assert!(z[2].starts_with("57 56 57"), "{z:?}");
        assert_eq!(z[3], "4 Felder");
        assert_eq!(z[4], "array");
        assert_eq!(z[5], "true");
        assert!(z[6].starts_with("WVW"), "{z:?}");
    }

    #[test]
    fn fehler_und_grenzen() {
        let (r, _) = laufen("obd(999)");
        assert!(format!("{:#}", r.unwrap_err()).contains("PID 999"));
        let (r, _) = laufen("fn f(x) { f(x + 1) } f(0)");
        assert!(r.is_err(), "Endlos-Rekursion wird begrenzt");
        let (r, _) = laufen("uds_schreiben(0x7E0, 0x0600, [1])");
        assert!(r.is_err(), "es gibt keine Schreibfunktion");
        // Stopp beendet auch Endlosschleifen
        let stopp = Arc::new(AtomicBool::new(false));
        let s2 = stopp.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            s2.store(true, Ordering::Relaxed);
        });
        let r = ausfuehren(Backend::Demo, Arc::new(LabelDb::default()), "loop { warte(10); }", |_| {}, stopp);
        assert!(format!("{:#}", r.unwrap_err()).contains("abgebrochen"));
    }
}
