//! Wartungstermine als Kalenderdatei (iCalendar, .ics) — lässt sich in jeden
//! Kalender (Handy, Thunderbird, Outlook …) importieren. Je fälligem Posten ein
//! ganztägiger Termin mit Erinnerung eine Woche vorher.
//!
//! Termine nach Zeit (z. B. HU alle 24 Monate) stehen fest. Bei Posten nur nach
//! km wird das Datum aus der bisherigen Fahrleistung geschätzt.

use crate::tr;
use crate::vehicle::{datum_aus_tagen, days_of, plus_monate, Vehicle};

/// Ein Termin.
#[derive(Debug, Clone, PartialEq)]
pub struct Termin {
    pub posten: String,
    /// "TT.MM.JJJJ"
    pub datum: String,
    /// true = aus km und Fahrleistung geschätzt
    pub geschaetzt: bool,
    pub hinweis: String,
}

/// Fälligkeitstermine aller Wartungsposten (frühester Termin je Posten).
/// Schon überfällige Termine landen auf `heute` (sonst erinnert der Kalender nie).
pub fn termine(v: &Vehicle, heute: &str) -> Vec<Termin> {
    let km_tag = crate::kosten::km_pro_tag(v);
    let mut out = Vec::new();
    for w in &v.wartung {
        let nach_zeit = w.monate.and_then(|m| plus_monate(&w.letztes_datum, m)).and_then(|d| Some((days_of(&d)?, d)));
        let nach_km = match (w.km_intervall, w.letzte_km, v.km, km_tag, days_of(&v.km_datum)) {
            (Some(iv), Some(letzte), Some(jetzt), Some(rate), Some(stand)) if rate > 0.0 => {
                let rest = i64::from(letzte) + i64::from(iv) - i64::from(jetzt);
                let tag = stand + (rest as f64 / rate).round() as i64;
                Some((tag, letzte + iv))
            }
            _ => None,
        };
        let t = match (nach_zeit, nach_km) {
            (Some((z, d)), Some((k, _))) if z <= k => Termin { posten: w.name.clone(), datum: d, geschaetzt: false, hinweis: tr!("nach Zeit ({a} Monate)", a = w.monate.unwrap_or(0)) },
            (_, Some((k, bei))) => Termin {
                posten: w.name.clone(),
                datum: datum_aus_tagen(k),
                geschaetzt: true,
                hinweis: tr!("bei {a} km — Datum aus der bisherigen Fahrleistung geschätzt", a = crate::report::group_thousands(bei)),
            },
            (Some((_, d)), None) => Termin { posten: w.name.clone(), datum: d, geschaetzt: false, hinweis: tr!("nach Zeit ({a} Monate)", a = w.monate.unwrap_or(0)) },
            (None, None) => continue,
        };
        let mut t = t;
        if let (Some(d), Some(h)) = (days_of(&t.datum), days_of(heute)) {
            if d < h {
                t.hinweis = tr!("überfällig seit {a} — {b}", a = &t.datum, b = &t.hinweis);
                t.datum = heute.to_string();
            }
        }
        out.push(t);
    }
    out.sort_by_key(|t| days_of(&t.datum));
    out
}

/// Text für iCalendar maskieren (\ ; , Zeilenumbruch).
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n")
}

/// Zeilen über 75 Byte falten (RFC 5545), ohne UTF-8-Zeichen zu zerschneiden.
fn falten(zeile: &str) -> String {
    let mut out = String::new();
    let mut laenge = 0;
    for c in zeile.chars() {
        if laenge + c.len_utf8() > 74 {
            out.push_str("\r\n ");
            laenge = 1;
        }
        out.push(c);
        laenge += c.len_utf8();
    }
    out + "\r\n"
}

fn yyyymmdd(datum: &str) -> Option<String> {
    let p: Vec<&str> = datum.split('.').collect();
    (p.len() == 3).then(|| format!("{}{}{}", p[2], p[1], p[0]))
}

/// Feste UID je Fahrzeug und Posten — erneutes Importieren ersetzt den Termin.
fn uid(v: &Vehicle, posten: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in v.key.bytes().chain(b"|".iter().copied()).chain(posten.bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}@ecu-werkbank")
}

/// Die .ics-Datei.
pub fn ics(v: &Vehicle, termine: &[Termin]) -> String {
    let name = v.label();
    let jetzt = crate::report::unix_now();
    let stempel = {
        let d = datum_aus_tagen((jetzt / 86_400) as i64);
        let s = jetzt % 86_400;
        format!("{}T{:02}{:02}{:02}Z", yyyymmdd(&d).unwrap_or_default(), s / 3600, s / 60 % 60, s % 60)
    };
    let mut out = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//ecu-werkbank//Wartung//DE\r\nCALSCALE:GREGORIAN\r\n");
    for t in termine {
        let Some(tag) = yyyymmdd(&t.datum) else { continue };
        let folge = days_of(&t.datum).map(|d| yyyymmdd(&datum_aus_tagen(d + 1)).unwrap_or_default()).unwrap_or_default();
        let titel = if t.geschaetzt { tr!("{a}: {b} fällig (ca.)", a = name, b = t.posten) } else { tr!("{a}: {b} fällig", a = name, b = t.posten) };
        out += "BEGIN:VEVENT\r\n";
        out += &falten(&format!("UID:{}", uid(v, &t.posten)));
        out += &format!("DTSTAMP:{stempel}\r\n");
        out += &format!("DTSTART;VALUE=DATE:{tag}\r\nDTEND;VALUE=DATE:{folge}\r\n");
        out += &falten(&format!("SUMMARY:{}", esc(&titel)));
        out += &falten(&format!("DESCRIPTION:{}", esc(&format!("{}\n{}", t.hinweis, tr!("Erstellt mit ECUWerkbank")))));
        out += "TRANSP:TRANSPARENT\r\n";
        out += "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-P7D\r\n";
        out += &falten(&format!("DESCRIPTION:{}", esc(&titel)));
        out += "END:VALARM\r\nEND:VEVENT\r\n";
    }
    out + "END:VCALENDAR\r\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::{Tankung, Wartung};

    #[test]
    fn termine_und_datei() {
        let mut v = Vehicle { key: "WVW1".into(), kennzeichen: "BZ AB 1".into(), km: Some(110_000), km_datum: "01.03.2026".into(), ..Default::default() };
        v.wartung.push(Wartung { name: "HU / AU".into(), monate: Some(24), letztes_datum: "31.01.2025".into(), ..Default::default() });
        v.wartung.push(Wartung { name: "Öl".into(), km_intervall: Some(15_000), monate: Some(12), letzte_km: Some(100_000), letztes_datum: "01.03.2025".into() });
        v.wartung.push(Wartung { name: "Ohne Daten".into(), monate: Some(12), ..Default::default() });
        // Öl nach Zeit (01.03.2026) früher als nach km; nach Datum sortiert
        let t = termine(&v, "01.01.2026");
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].posten.as_str(), t[0].datum.as_str(), t[0].geschaetzt), ("Öl", "01.03.2026", false));
        assert_eq!((t[1].posten.as_str(), t[1].datum.as_str()), ("HU / AU", "31.01.2027"));
        // längeres Zeitintervall: dann zählen die km, Datum aus der Fahrleistung geschätzt
        v.tankbuch.push(Tankung { datum: "01.01.2026".into(), km: 107_000, liter: 40.0, preis: None, voll: true, notiz: String::new() });
        v.wartung[1].monate = Some(24);
        let rate = crate::kosten::km_pro_tag(&v).unwrap(); // 10.000 km in 365 Tagen
        let t = termine(&v, "01.01.2026");
        let oel = t.iter().find(|t| t.posten == "Öl").unwrap();
        assert!(oel.geschaetzt);
        assert_eq!(oel.datum, datum_aus_tagen(days_of("01.03.2026").unwrap() + (5000.0 / rate).round() as i64));
        let s = ics(&v, &t);
        assert!(s.starts_with("BEGIN:VCALENDAR\r\n") && s.ends_with("END:VCALENDAR\r\n"));
        assert_eq!(s.matches("BEGIN:VEVENT").count(), 2);
        assert!(s.contains("DTSTART;VALUE=DATE:20270131\r\nDTEND;VALUE=DATE:20270201"));
        assert!(s.contains("TRIGGER:-P7D"));
        assert!(s.lines().all(|l| l.len() <= 75));
        // überfällig → heute
        let t = termine(&v, "01.06.2027");
        let hu = t.iter().find(|t| t.posten == "HU / AU").unwrap();
        assert_eq!(hu.datum, "01.06.2027");
        assert!(hu.hinweis.contains("31.01.2027"));
    }

    #[test]
    fn datum_rechnen() {
        assert_eq!(plus_monate("31.01.2026", 1).as_deref(), Some("28.02.2026"));
        assert_eq!(plus_monate("31.01.2024", 1).as_deref(), Some("29.02.2024"));
        assert_eq!(plus_monate("15.11.2026", 24).as_deref(), Some("15.11.2028"));
        assert_eq!(datum_aus_tagen(days_of("08.11.2026").unwrap()), "08.11.2026");
        assert_eq!(datum_aus_tagen(0), "01.01.1970");
    }
}
