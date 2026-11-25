//! „Fahrzeug auf einen Blick“: alles Wichtige zu einem Fahrzeug aus den
//! gespeicherten Daten (Fahrzeugordner, Historie, Wartung) — ohne Verbindung.

use crate::tr;
use crate::limits::Stufe;
use crate::report::ScanReport;
use crate::vehicle::Vehicle;

#[derive(Debug, Clone, Default)]
pub struct Blick {
    /// Kilometerstand und Datum („TT.MM.JJJJ“)
    pub km: Option<(u32, String)>,
    /// Gefahrene km pro Jahr (aus dem Verlauf, mind. 30 Tage Abstand)
    pub km_pro_jahr: Option<u32>,
    /// Fehler im letzten Scan und im Scan davor
    pub fehler: Option<usize>,
    pub fehler_vorher: Option<usize>,
    /// Steuergeräte im letzten Scan
    pub steuergeraete: Option<usize>,
    /// Datum des letzten Scans und wie viele Tage her
    pub letzter_scan: Option<(String, i64)>,
    /// Dringendster Wartungsposten: (Stufe, Name, Text)
    pub wartung: Option<(Stufe, String, String)>,
    /// (Tag seit 1970, km)
    pub km_verlauf: Vec<(i64, u32)>,
    /// (Tag seit 1970, Fehleranzahl) je Scan
    pub fehler_verlauf: Vec<(i64, usize)>,
    /// Steuergeräte mit Fehlern im letzten Scan: (Index im Scan, Titel, Anzahl, Stufe)
    pub module_mit_fehlern: Vec<(usize, String, usize, Stufe)>,
}

/// Tag (seit 1970) eines Scans: aus der Unix-Zeit, sonst aus dem Datumstext.
fn tag(r: &ScanReport) -> Option<i64> {
    if r.timestamp > 0 {
        Some((r.timestamp / 86_400) as i64)
    } else {
        crate::vehicle::days_of(&r.created)
    }
}

/// Übersicht für ein Fahrzeug aus seinen gespeicherten Scans.
pub fn blick(v: &Vehicle) -> Blick {
    let scans: Vec<ScanReport> = crate::history::list(&v.key).into_iter().map(|(_, r)| r).collect();
    blick_aus(v, &scans, &crate::vehicle::today())
}

/// Wie [`blick`], mit vorgegebenen Scans (älteste zuerst) und Datum (für Tests).
pub fn blick_aus(v: &Vehicle, scans: &[ScanReport], heute: &str) -> Blick {
    let mut b = Blick::default();
    let heute_tag = crate::vehicle::days_of(heute);

    // Kilometerverlauf: Scans mit km + der im Fahrzeug eingetragene Stand
    let mut km: Vec<(i64, u32)> = scans.iter().filter_map(|r| Some((tag(r)?, r.km?))).collect();
    if let (Some(k), Some(t)) = (v.km, crate::vehicle::days_of(&v.km_datum)) {
        km.push((t, k));
    }
    km.sort_unstable();
    // je Tag nur den höchsten Stand
    km.dedup_by(|b, a| {
        if a.0 == b.0 {
            a.1 = a.1.max(b.1);
            true
        } else {
            false
        }
    });
    if let (Some(first), Some(last)) = (km.first(), km.last()) {
        let tage = last.0 - first.0;
        if tage >= 30 && last.1 > first.1 {
            b.km_pro_jahr = Some(((f64::from(last.1 - first.1)) * 365.0 / tage as f64).round() as u32);
        }
    }
    b.km = match (v.km, km.last()) {
        (Some(k), _) if !v.km_datum.is_empty() || km.is_empty() => Some((k, v.km_datum.clone())),
        (_, Some(&(t, k))) => Some((k, datum_von_tag(t))),
        (Some(k), None) => Some((k, String::new())),
        (None, None) => None,
    };
    b.km_verlauf = km;

    // Fehler
    b.fehler_verlauf = scans.iter().filter_map(|r| Some((tag(r)?, r.total_dtcs()))).collect();
    if let Some(last) = scans.last() {
        b.fehler = Some(last.total_dtcs());
        b.steuergeraete = Some(last.modules.len());
        let tage = match (tag(last), heute_tag) {
            (Some(a), Some(h)) => (h - a).max(0),
            _ => 0,
        };
        b.letzter_scan = Some((last.created.split(' ').next().unwrap_or("").to_string(), tage));
        for (i, m) in last.modules.iter().enumerate() {
            let Some(d) = &m.dtcs else { continue };
            if d.is_empty() {
                continue;
            }
            let aktiv = d.iter().any(|e| e.status.contains("aktuell"));
            b.module_mit_fehlern.push((i, m.title(), d.len(), if aktiv { Stufe::Warnung } else { Stufe::Hinweis }));
        }
    }
    if scans.len() >= 2 {
        b.fehler_vorher = Some(scans[scans.len() - 2].total_dtcs());
    }

    // Dringendster Wartungsposten (bekannte vor „bitte eintragen“)
    let mut w: Vec<(Stufe, bool, String, String)> = v
        .wartung
        .iter()
        .map(|w| {
            let f = w.faellig(v.km, heute);
            let bekannt = w.letzte_km.is_some() || !w.letztes_datum.is_empty();
            (f.stufe, bekannt, w.name.clone(), f.text)
        })
        .collect();
    w.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
    b.wartung = w.into_iter().next().map(|(s, _, n, t)| (s, n, t));
    b
}

/// „TT.MM.JJJJ“ aus Tagen seit 1970.
pub fn datum_von_tag(t: i64) -> String {
    // Umkehrung von days_of („civil_from_days“, H. Hinnant)
    let z = t + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{d:02}.{m:02}.{y}")
}

/// „heute“, „gestern“, „vor 5 Tagen“, „vor 3 Monaten“ …
pub fn vor_text(tage: i64) -> String {
    match tage {
        ..=0 => "heute".into(),
        1 => "gestern".into(),
        2..=59 => tr!("vor {tage} Tagen", tage = tage),
        60..=729 => tr!("vor {a} Monaten", a = tage * 12 / 365),
        _ => tr!("vor {a} Jahren", a = tage / 365),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{DtcEntry, ModuleReport};

    fn scan(datum: &str, km: Option<u32>, fehler: usize) -> ScanReport {
        let dtcs = (0..fehler)
            .map(|i| DtcEntry { code: format!("0{i}"), alt: None, text: String::new(), status: "aktuell vorhanden".into(), umgebung: vec![], frueher: vec![] })
            .collect();
        let m = ModuleReport { protocol: "TP 2.0".into(), address: Some(1), name: "Motor".into(), can: String::new(), ident: vec![], vin: None, dtcs: Some(dtcs) };
        ScanReport {
            fremd: false,
            readiness: Vec::new(),
            created: format!("{datum} 12:00"),
            timestamp: 0,
            transport: "Demo".into(),
            vin: None,
            note: String::new(),
            vehicle: String::new(),
            kennzeichen: String::new(),
            fahrzeug: String::new(),
            km,
            reparatur: String::new(),
            freeze: None,
            check: vec![],
            pruefwerte: Vec::new(),
            modules: vec![m],
            comparison: None,
        }
    }

    #[test]
    fn datum_hin_und_zurueck() {
        for d in ["01.01.1970", "29.02.2024", "28.10.2026", "31.12.2030"] {
            assert_eq!(datum_von_tag(crate::vehicle::days_of(d).unwrap()), d);
        }
        assert_eq!(vor_text(0), "heute");
        assert_eq!(vor_text(10), "vor 10 Tagen");
        assert_eq!(vor_text(400), "vor 13 Monaten");
    }

    #[test]
    fn blick_aus_historie() {
        let mut v = Vehicle { key: "t".into(), wartung: crate::vehicle::wartung_vorlage(), ..Default::default() };
        v.wartung[0].letzte_km = Some(150_000);
        v.wartung[0].letztes_datum = "01.01.2026".into();
        let scans = vec![scan("01.01.2026", Some(150_000), 1), scan("01.07.2026", Some(156_000), 3)];
        let b = blick_aus(&v, &scans, "11.07.2026");
        assert_eq!(b.km, Some((156_000, "01.07.2026".into())));
        assert_eq!(b.km_pro_jahr, Some(12_099));
        assert_eq!((b.fehler, b.fehler_vorher, b.steuergeraete), (Some(3), Some(1), Some(1)));
        assert_eq!(b.letzter_scan, Some(("01.07.2026".into(), 10)));
        assert_eq!(b.module_mit_fehlern.len(), 1);
        assert_eq!(b.module_mit_fehlern[0].3, Stufe::Warnung);
        assert_eq!(b.wartung.as_ref().map(|w| w.1.clone()), Some(v.wartung[0].name.clone()));
        assert_eq!(b.km_verlauf.len(), 2);
    }

    #[test]
    fn blick_ohne_daten() {
        let b = blick_aus(&Vehicle::default(), &[], "01.01.2026");
        assert!(b.km.is_none() && b.fehler.is_none() && b.letzter_scan.is_none());
    }
}
