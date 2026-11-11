//! Kosten und Verbrauch eines Fahrzeugs aus Tankbuch und gewechselten Teilen.
//!
//! Verbrauch nach der Voll-zu-voll-Methode: Liter seit der letzten vollen
//! Füllung (inklusive Teilfüllungen dazwischen) geteilt durch die Strecke.

use crate::vehicle::{days_of, Tankung, Vehicle};
use std::collections::BTreeMap;

/// Ein berechneter Abschnitt zwischen zwei Vollbetankungen.
#[derive(Debug, Clone, PartialEq)]
pub struct Abschnitt {
    /// Datum der Füllung, die den Abschnitt abschließt
    pub datum: String,
    /// km-Stand dieser Füllung
    pub km: u32,
    pub strecke_km: u32,
    pub liter: f64,
    pub l_100km: f64,
}

/// Verbrauch über alle Abschnitte.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Verbrauch {
    /// älteste zuerst
    pub abschnitte: Vec<Abschnitt>,
    /// Schnitt über die ganze Strecke (Liter gesamt / km gesamt)
    pub schnitt: Option<f64>,
    /// Durchschnittspreis je Liter (nur Füllungen mit Preis)
    pub euro_pro_liter: Option<f64>,
}

pub fn verbrauch(tank: &[Tankung]) -> Verbrauch {
    let mut t: Vec<&Tankung> = tank.iter().collect();
    t.sort_by_key(|x| (x.km, days_of(&x.datum).unwrap_or(0)));
    let mut abschnitte = Vec::new();
    let mut start: Option<u32> = None;
    let mut liter = 0.0;
    for x in t {
        match start {
            // erst ab der ersten vollen Füllung lässt sich rechnen
            None if x.voll => start = Some(x.km),
            None => {}
            Some(s) => {
                liter += x.liter;
                if x.voll {
                    let strecke = x.km.saturating_sub(s);
                    if strecke > 0 {
                        abschnitte.push(Abschnitt { datum: x.datum.clone(), km: x.km, strecke_km: strecke, liter, l_100km: liter * 100.0 / f64::from(strecke) });
                    }
                    start = Some(x.km);
                    liter = 0.0;
                }
            }
        }
    }
    let (l, km): (f64, u32) = abschnitte.iter().fold((0.0, 0), |(l, k), a| (l + a.liter, k + a.strecke_km));
    let schnitt = (km > 0).then(|| l * 100.0 / f64::from(km));
    let mit_preis: Vec<&Tankung> = tank.iter().filter(|x| x.preis.is_some() && x.liter > 0.0).collect();
    let pl: f64 = mit_preis.iter().map(|x| x.liter).sum();
    let euro_pro_liter = (pl > 0.0).then(|| mit_preis.iter().filter_map(|x| x.preis).sum::<f64>() / pl);
    Verbrauch { abschnitte, schnitt, euro_pro_liter }
}

/// Kosten eines Kalenderjahrs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Jahr {
    pub teile: f64,
    pub tanken: f64,
    pub liter: f64,
}

/// Kostenübersicht.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Kosten {
    pub teile: f64,
    pub tanken: f64,
    pub gesamt: f64,
    /// Zeitraum vom ersten Eintrag mit Preis bis heute
    pub von: String,
    pub tage: i64,
    /// gefahrene km in diesem Zeitraum (soweit bekannt)
    pub strecke_km: Option<u32>,
    pub je_monat: Option<f64>,
    pub je_1000km: Option<f64>,
    pub jahre: BTreeMap<i64, Jahr>,
}

fn jahr_von(datum: &str) -> Option<i64> {
    days_of(datum)?;
    datum.trim().split(' ').next()?.rsplit('.').next()?.parse().ok()
}

pub fn kosten(v: &Vehicle, heute: &str) -> Kosten {
    let mut k = Kosten::default();
    // (Tag, km) aller Einträge mit Preis — für Zeitraum und Strecke
    let mut punkte: Vec<(i64, Option<u32>)> = Vec::new();
    for t in &v.teile {
        if let Some(p) = t.preis {
            k.teile += p;
            if let Some(j) = jahr_von(&t.datum) {
                k.jahre.entry(j).or_default().teile += p;
            }
            if let Some(d) = days_of(&t.datum) {
                punkte.push((d, t.km));
            }
        }
    }
    for t in &v.tankbuch {
        if let Some(p) = t.preis {
            k.tanken += p;
        }
        if let Some(j) = jahr_von(&t.datum) {
            let e = k.jahre.entry(j).or_default();
            e.tanken += t.preis.unwrap_or(0.0);
            e.liter += t.liter;
        }
        if let (Some(_), Some(d)) = (t.preis, days_of(&t.datum)) {
            punkte.push((d, Some(t.km)));
        }
    }
    k.gesamt = k.teile + k.tanken;
    let Some(&(erster, _)) = punkte.iter().min_by_key(|p| p.0) else { return k };
    k.von = crate::vehicle::datum_aus_tagen(erster);
    k.tage = days_of(heute).map(|h| h - erster).unwrap_or(0).max(0);
    // Strecke: kleinster bekannter km-Stand im Zeitraum bis zum aktuellen Stand
    let min_km = punkte.iter().filter_map(|p| p.1).min();
    let max_km = punkte.iter().filter_map(|p| p.1).chain(v.km).max();
    k.strecke_km = min_km.zip(max_km).map(|(a, b)| b.saturating_sub(a)).filter(|&s| s > 0);
    // erst ab einem Monat sinnvoll
    if k.tage >= 30 {
        k.je_monat = Some(k.gesamt * 30.44 / k.tage as f64);
    }
    if let Some(s) = k.strecke_km.filter(|&s| s >= 500) {
        k.je_1000km = Some(k.gesamt * 1000.0 / f64::from(s));
    }
    k
}

/// Durchschnittlich gefahrene km pro Tag (aus Tankbuch und Teilen), für
/// Schätzungen wie „Ölwechsel bei 15.000 km — ungefähr im März“.
pub fn km_pro_tag(v: &Vehicle) -> Option<f64> {
    let mut p: Vec<(i64, u32)> = v
        .tankbuch
        .iter()
        .filter_map(|t| Some((days_of(&t.datum)?, t.km)))
        .chain(v.teile.iter().filter_map(|t| Some((days_of(&t.datum)?, t.km?))))
        .chain(v.wartung.iter().filter_map(|w| Some((days_of(&w.letztes_datum)?, w.letzte_km?))))
        .chain(days_of(&v.km_datum).zip(v.km))
        .collect();
    p.sort();
    let (a, b) = (p.first()?, p.last()?);
    let tage = b.0 - a.0;
    // mindestens 30 Tage und 300 km Abstand, sonst zu ungenau
    (tage >= 30 && b.1 >= a.1 + 300).then(|| f64::from(b.1 - a.1) / tage as f64)
}

/// Euro-Betrag deutsch, z. B. „1.234,50 €“.
pub fn euro(x: f64) -> String {
    let cent = (x * 100.0).round() as i64;
    let (ganz, rest) = (cent.abs() / 100, cent.abs() % 100);
    let vz = if cent < 0 { "-" } else { "" };
    format!("{vz}{},{rest:02} €", crate::report::group_thousands(ganz as u32))
}

/// Zahl mit Komma, z. B. „6,8“.
pub fn komma(x: f64, stellen: usize) -> String {
    format!("{x:.stellen$}").replace('.', ",")
}

/// Eingabe „45,30“ / „45.30“ / „1.234,50“ → Zahl.
pub fn zahl_lesen(s: &str) -> Option<f64> {
    let s = s.trim().trim_end_matches('€').trim().replace(' ', "");
    if s.is_empty() {
        return None;
    }
    let s = if s.contains(',') { s.replace('.', "").replace(',', ".") } else { s };
    s.parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::Teil;

    fn t(d: &str, km: u32, l: f64, p: Option<f64>, voll: bool) -> Tankung {
        Tankung { datum: d.into(), km, liter: l, preis: p, voll, notiz: String::new() }
    }

    #[test]
    fn verbrauch_voll_zu_voll() {
        let tank = [
            t("01.01.2026", 100_000, 40.0, Some(70.0), true),
            t("10.01.2026", 100_300, 15.0, Some(27.0), false), // Teilfüllung
            t("20.01.2026", 100_700, 27.0, Some(48.0), true),
            t("01.02.2026", 101_200, 35.0, None, true),
        ];
        let v = verbrauch(&tank);
        assert_eq!(v.abschnitte.len(), 2);
        // (15 + 27) l auf 700 km = 6,0 l/100 km
        assert!((v.abschnitte[0].l_100km - 6.0).abs() < 1e-9);
        assert!((v.abschnitte[1].l_100km - 7.0).abs() < 1e-9);
        // 77 l auf 1200 km
        assert!((v.schnitt.unwrap() - 77.0 * 100.0 / 1200.0).abs() < 1e-9);
        assert!((v.euro_pro_liter.unwrap() - 145.0 / 82.0).abs() < 1e-9);
        // eine einzige Füllung: kein Verbrauch
        assert_eq!(verbrauch(&tank[..1]).schnitt, None);
    }

    #[test]
    fn kosten_rechnen() {
        let mut v = Vehicle { km: Some(110_000), ..Default::default() };
        v.teile.push(Teil { datum: "15.01.2025".into(), km: Some(100_000), name: "Bremsen".into(), preis: Some(300.0), ..Default::default() });
        v.teile.push(Teil { datum: "15.06.2025".into(), km: Some(105_000), name: "Ohne Preis".into(), ..Default::default() });
        v.tankbuch.push(t("01.03.2026", 108_000, 40.0, Some(70.0), true));
        let k = kosten(&v, "15.01.2026");
        assert_eq!((k.teile, k.tanken, k.gesamt), (300.0, 70.0, 370.0));
        assert_eq!(k.von, "15.01.2025");
        assert_eq!(k.tage, 365);
        assert_eq!(k.strecke_km, Some(10_000));
        assert!((k.je_1000km.unwrap() - 37.0).abs() < 1e-9);
        assert!((k.je_monat.unwrap() - 370.0 * 30.44 / 365.0).abs() < 1e-9);
        assert_eq!(k.jahre[&2025].teile, 300.0);
        assert_eq!(k.jahre[&2026].tanken, 70.0);
        // ohne Preise: nichts
        assert_eq!(kosten(&Vehicle::default(), "01.01.2026").gesamt, 0.0);
    }

    #[test]
    fn eingaben_und_ausgaben() {
        assert_eq!(zahl_lesen("45,30"), Some(45.3));
        assert_eq!(zahl_lesen("45.30 €"), Some(45.3));
        assert_eq!(zahl_lesen("1.234,50"), Some(1234.5));
        assert_eq!(zahl_lesen(""), None);
        assert_eq!(zahl_lesen("-3"), None);
        assert_eq!(euro(1234.5), "1.234,50 €");
        assert_eq!(komma(6.849, 1), "6,8");
    }

    #[test]
    fn km_je_tag() {
        let mut v = Vehicle::default();
        v.tankbuch.push(t("01.01.2026", 100_000, 40.0, None, true));
        assert_eq!(km_pro_tag(&v), None);
        v.tankbuch.push(t("31.01.2026", 101_500, 40.0, None, true));
        assert!((km_pro_tag(&v).unwrap() - 50.0).abs() < 1e-9);
    }
}
