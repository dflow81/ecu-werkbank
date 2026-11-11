//! Komplett-Sicherung für den Notfall: alles, was man braucht, wenn ein
//! Steuergerät kaputtgeht — Teilenummern und Softwarestände, Codierung und
//! Anpassungen aller Steuergeräte, dazu ein PDF „Ersatzteil-Pass“.
//! Nur lesen. Die Software (der Programmspeicher) selbst ist über die
//! Diagnose nicht lesbar und wird nicht gebraucht: ein Ersatzteil mit gleicher
//! Teilenummer bringt sie mit.

use crate::anpassung;
use crate::backup::{self, CodingBackup};
use crate::labels::LabelDb;
use crate::pdf::{self, Font};
use crate::report::{clip, Layout, MARGIN};
use crate::swinfo::{self, ModulSw, Stand};
use crate::tr;
use crate::transport::Backend;
use crate::vehicle::Vehicle;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Ergebnis mit den Ablageorten.
pub struct Ergebnis {
    pub software: Stand,
    pub codierung: CodingBackup,
    pub anpassung: anpassung::Sicherung,
    pub dateien: Vec<PathBuf>,
    pub pdf: PathBuf,
}

/// Alles sichern und den Ersatzteil-Pass schreiben. `fortschritt` bekommt kurze Texte.
pub fn sichern(be: &Backend, db: &LabelDb, v: &Vehicle, mut fortschritt: impl FnMut(&str)) -> Result<Ergebnis> {
    let mut dateien = Vec::new();

    fortschritt(&tr!("1/3 Teilenummern und Softwarestände …"));
    let software = swinfo::read(be, db, true, |_| {})?;
    dateien.push(swinfo::save(&v.key, &software)?);

    fortschritt(&tr!("2/3 Codierung …"));
    let mut codierung = backup::backup(be, db, true, |_| {})?;
    codierung.vehicle = v.key.clone();
    dateien.push(backup::save(&codierung)?);

    fortschritt(&tr!("3/3 Anpassungen (dauert einige Minuten) …"));
    let mut zuletzt = String::new();
    let mut anp = anpassung::sichern(be, db, true, |g, _| {
        if g != zuletzt {
            fortschritt(&tr!("3/3 Anpassungen: {a}", a = g));
            zuletzt = g.to_string();
        }
    })?;
    anp.vehicle = v.key.clone();
    dateien.push(anpassung::speichern(&anp)?);

    let pdf_bytes = pass_pdf(v, &software, &codierung, &anp);
    let pdf = v.sub("berichte")?.join(crate::vehicle::stamped("ersatzteilpass", "pdf"));
    std::fs::write(&pdf, pdf_bytes)?;
    Ok(Ergebnis { software, codierung, anpassung: anp, dateien, pdf })
}

/// Ersatzteil-Pass aus den zuletzt gesicherten Daten neu erzeugen (liest das
/// Auto NICHT). Nützlich nach einem Fehler in der Darstellung oder um einen
/// frischen Ausdruck zu bekommen.
pub fn pass_neu(v: &Vehicle) -> Result<PathBuf> {
    let software = swinfo::list(&v.key).pop().context(tr!("Keine Software-Sicherung vorhanden (erst: notfall-sicherung)."))?;
    let codierung = backup::list_all()
        .into_iter()
        .filter(|(_, b)| b.vehicle == v.key)
        .max_by_key(|(_, b)| b.timestamp)
        .map(|(_, b)| b)
        .context(tr!("Keine Codierungs-Sicherung vorhanden."))?;
    let anp = anpassung::liste(&v.key).into_iter().map(|(_, s)| s).find(|s| s.vollstaendig()).context(tr!("Keine Anpassungs-Sicherung vorhanden."))?;
    let pdf_bytes = pass_pdf(v, &software, &codierung, &anp);
    let pdf = v.sub("berichte")?.join(crate::vehicle::stamped("ersatzteilpass", "pdf"));
    std::fs::write(&pdf, pdf_bytes)?;
    Ok(pdf)
}

/// Was nach dem Tausch dieses Steuergeräts zu tun ist.
pub fn nach_dem_tausch(m: &ModulSw) -> String {
    let n = format!("{} {} {}", m.name, m.system, m.adresse).to_lowercase();
    let hat = |w: &[&str]| w.iter().any(|x| n.contains(x));
    // Motor: TP 2.0 Adresse 0x01, UDS 0x7E0 bzw. Diagnoseadresse 0x01
    let motor = m.adresse == "0x01" || m.adresse == "ID 0x7e0" || hat(&["motor", "tsi", "tdi", "fsi", "med17", "edc"]);
    let immo = motor || hat(&["kombi", "immo", "zugang", "kessy", "schlüssel"]);
    if immo {
        tr!("Wegfahrsperre: Das Teil muss online bei VW (bzw. in einer Werkstatt mit Online-Zugang) angelernt werden, sonst springt das Auto nicht an. Danach Codierung und Anpassungen wie gesichert.")
    } else if hat(&["airbag"]) {
        tr!("Codierung wie gesichert. Kein gebrauchtes Teil mit gespeicherten Crashdaten einbauen. Fehler danach löschen.")
    } else if hat(&["abs", "esp", "brems"]) {
        tr!("Codierung wie gesichert, danach Grundeinstellung des Lenkwinkelsensors (G85). Fehler löschen.")
    } else if hat(&["lenk", "eps"]) {
        tr!("Codierung wie gesichert, danach Lenkanschläge/Lenkwinkel anlernen. Fehler löschen.")
    } else {
        tr!("Teil mit gleicher Teilenummer einbauen (Index möglichst gleich), Codierung und Anpassungen wie gesichert eintragen, Fehler löschen.")
    }
}

/// Codierung eines Steuergeräts aus der Sicherung als Text.
fn codierung_text(c: &CodingBackup, m: &ModulSw) -> String {
    let e = c.entries.iter().find(|e| e.protocol == m.protocol && (e.address.map(|a| format!("{a:#04x}")).as_deref() == Some(m.adresse.as_str()) || format!("ID {:#05x}", e.tx) == m.adresse));
    let Some(e) = e else { return "—".into() };
    if e.hat_codierung() {
        return e.data_hex.replace(' ', "");
    }
    crate::kwp::kurzcodierung_9b(&e.bytes()).map(|k| k.text()).unwrap_or_else(|| "—".into())
}

/// Anzahl gesicherter Anpassungswerte eines Steuergeräts.
fn anpassungen(a: &anpassung::Sicherung, m: &ModulSw) -> usize {
    a.geraete
        .iter()
        .filter(|g| match g.tp20 {
            Some(t) => m.protocol == "TP 2.0" && format!("{t:#04x}") == m.adresse,
            // Adresse ist eindeutig; Name nur als Rückfall und nur, wenn er gesetzt ist
            // (sonst passt „leer == leer" auf alle Geräte, siehe RNS310 ohne Namen).
            None => m.protocol == "UDS" && (m.adresse == format!("ID {:#05x}", g.tx) || (!g.name.is_empty() && g.name == m.name)),
        })
        .map(|g| g.kennungen.len())
        .sum()
}

/// PDF „Ersatzteil-Pass“.
pub fn pass_pdf(v: &Vehicle, s: &Stand, c: &CodingBackup, a: &anpassung::Sicherung) -> Vec<u8> {
    let mut l = Layout::new();
    let accent = (0.10, 0.32, 0.55);
    let grau = (0.4, 0.4, 0.4);
    l.doc.rect(0.0, pdf::A4_H - 70.0, pdf::A4_W, 70.0, accent);
    l.doc.text_rgb(MARGIN, pdf::A4_H - 42.0, 20.0, Font::Bold, &tr!("Ersatzteil-Pass"), (1.0, 1.0, 1.0));
    l.doc.text_rgb(MARGIN, pdf::A4_H - 58.0, 9.0, Font::Regular, &tr!("{a} · erstellt mit ECUWerkbank", a = s.created), (0.85, 0.9, 1.0));
    l.y = pdf::A4_H - 100.0;
    let vin = s.vin.clone().filter(|x| !x.is_empty()).unwrap_or_else(|| v.vin.clone());
    for (k, w) in [(tr!("Fahrzeug"), v.bezeichnung.clone()), (tr!("Kennzeichen"), v.kennzeichen.clone()), (tr!("Fahrgestellnummer"), vin.clone())] {
        if w.is_empty() {
            continue;
        }
        l.doc.text_rgb(MARGIN, l.y, 10.0, Font::Regular, &k, grau);
        l.doc.text(MARGIN + 120.0, l.y, 10.0, Font::Bold, &w);
        l.y -= 15.0;
    }
    l.y -= 4.0;
    for line in pdf::wrap(
        &tr!("Geht ein Steuergerät kaputt: ein Teil mit derselben Teilenummer besorgen (die Software ist darauf schon drauf), einbauen und Codierung bzw. Anpassungen wie hier gesichert eintragen. Die vollständigen Daten liegen im Fahrzeugordner von ECUWerkbank (codierung/, anpassungen/, software/). Dieses Blatt zu den Fahrzeugpapieren legen."),
        pdf::A4_W - 2.0 * MARGIN,
        9.0,
        Font::Regular,
    ) {
        l.doc.text_rgb(MARGIN, l.y, 9.0, Font::Regular, &line, grau);
        l.y -= 12.0;
    }
    l.y -= 6.0;
    l.heading(&tr!("Steuergeräte ({a})", a = s.module.len()));
    for m in &s.module {
        let tipp = pdf::wrap(&nach_dem_tausch(m), pdf::A4_W - 2.0 * MARGIN - 12.0, 8.5, Font::Regular);
        l.ensure(60.0 + 11.0 * tipp.len() as f32);
        let name = if m.name.is_empty() { m.system.clone() } else { m.name.clone() };
        l.doc.rect(MARGIN, l.y - 3.0, pdf::A4_W - 2.0 * MARGIN, 14.0, (0.92, 0.94, 0.97));
        l.doc.text(MARGIN + 4.0, l.y, 10.0, Font::Bold, &clip(&format!("{name}  ({} {})", m.protocol, m.adresse), 470.0, 10.0));
        l.y -= 15.0;
        let sp = [MARGIN + 6.0, MARGIN + 130.0, MARGIN + 220.0, MARGIN + 330.0];
        for (i, k) in [tr!("Teilenummer"), tr!("Software"), tr!("Hardware"), tr!("Anpassungen")].iter().enumerate() {
            l.doc.text_rgb(sp[i], l.y, 7.5, Font::Regular, k, grau);
        }
        l.y -= 11.0;
        let hw = [m.hardware_nr.as_str(), m.hardware_ver.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" ");
        let n = anpassungen(a, m);
        let anp = if n == 0 { "—".to_string() } else { tr!("{a} Werte gesichert", a = n) };
        for (i, w) in [m.teilenummer.as_str(), m.software.as_str(), hw.as_str(), anp.as_str()].iter().enumerate() {
            l.doc.text(sp[i], l.y, 9.5, Font::Bold, &clip(if w.is_empty() { "—" } else { w }, 105.0, 9.5));
        }
        l.y -= 13.0;
        let cod = codierung_text(c, m);
        l.doc.text_rgb(sp[0], l.y, 7.5, Font::Regular, &tr!("Codierung"), grau);
        l.doc.text(sp[1], l.y, 8.5, Font::Regular, &clip(&cod, pdf::A4_W - MARGIN - sp[1], 8.5));
        l.y -= 12.0;
        for t in &tipp {
            l.doc.text_rgb(sp[0], l.y, 8.5, Font::Regular, t, (0.25, 0.25, 0.25));
            l.y -= 11.0;
        }
        l.y -= 6.0;
    }
    l.y -= 6.0;
    l.ensure(40.0);
    for line in pdf::wrap(
        &tr!("Hinweis: Der Programmspeicher (die Software) eines Steuergeräts lässt sich über die Diagnose nicht auslesen und wird für einen Tausch nicht gebraucht. Bauteile der Wegfahrsperre (Motor, Kombiinstrument, Schlüssel) müssen nach dem Tausch online angelernt werden."),
        pdf::A4_W - 2.0 * MARGIN,
        8.5,
        Font::Regular,
    ) {
        l.doc.text_rgb(MARGIN, l.y, 8.5, Font::Regular, &line, grau);
        l.y -= 11.0;
    }
    let n = l.doc.page_count();
    for pg in 0..n {
        l.doc.text_on_page(pg, MARGIN, 28.0, 8.0, Font::Regular, &tr!("ECUWerkbank · Ersatzteil-Pass · {a}", a = vin));
        l.doc.text_on_page(pg, pdf::A4_W - MARGIN - 50.0, 28.0, 8.0, Font::Regular, &tr!("Seite {a} von {n}", a = pg + 1, n = n));
    }
    l.doc.to_bytes(crate::trs!("Ersatzteil-Pass"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anpassungen_zaehlen_pro_geraet() {
        use crate::anpassung::{Geraet, Sicherung};
        use crate::swinfo::ModulSw;
        let mk_g = |tx: u16, n: usize| Geraet {
            name: String::new(),
            tx,
            rx: tx + 8,
            kennungen: (0..n).map(|i| (format!("{i:04X}"), "00".to_string())).collect(),
            tp20: None,
        };
        let a = Sicherung {
            created: String::new(),
            timestamp: 0,
            vehicle: "TEST".into(),
            geraete: vec![mk_g(0x714, 103), mk_g(0x76c, 36)],
            notiz: String::new(),
        };
        let mk_m = |adresse: &str, name: &str| ModulSw {
            protocol: "UDS".into(),
            adresse: adresse.into(),
            name: name.into(),
            ..Default::default()
        };
        // Namen leer (wie am Golf): jedes Modul zählt nur seine eigene Adresse
        assert_eq!(anpassungen(&a, &mk_m("ID 0x714", "")), 103);
        assert_eq!(anpassungen(&a, &mk_m("ID 0x76c", "")), 36);
        // auch mit gesetztem, aber anderem Namen: nicht über die Adresse hinaus
        assert_eq!(anpassungen(&a, &mk_m("ID 0x714", "Kombi")), 103);
    }

    #[test]
    fn demo_komplett_sicherung() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _s = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::reset();
        let dir = std::env::temp_dir().join(format!("vag-notfall-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        let v = Vehicle { key: "TEST".into(), kennzeichen: "B XY 1".into(), ..Default::default() };
        let e = sichern(&Backend::Demo, &LabelDb::default(), &v, |_| {}).unwrap();
        assert!(!e.software.module.is_empty());
        assert!(e.pdf.exists());
        assert_eq!(e.dateien.len(), 3);
        assert!(std::fs::read(&e.pdf).unwrap().starts_with(b"%PDF"));
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hinweise_je_geraet() {
        let m = |name: &str| ModulSw { protocol: "UDS".into(), adresse: "0x17".into(), name: name.into(), ..Default::default() };
        assert!(nach_dem_tausch(&m("Kombiinstrument")).contains("Wegfahrsperre"));
        assert!(nach_dem_tausch(&m("Airbag")).contains("Crashdaten"));
        assert!(nach_dem_tausch(&m("Klimaanlage")).contains("gleicher Teilenummer"));
    }
}
