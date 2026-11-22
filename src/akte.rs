//! Fahrzeugakte als PDF: Fahrzeugdaten, Wartung, Verlauf aller Scans
//! (km, Fehler, Reparaturen), aktuelle Fehler und Codierungsänderungen.

use crate::tr;
use crate::backup;
use crate::history;
use crate::labels::LabelDb;
use crate::limits::Stufe;
use crate::pdf::{self, Font};
use crate::report::{clip, group_thousands, Layout, MARGIN};
use crate::vehicle::{self, Vehicle};

pub fn to_pdf(v: &Vehicle, db: &LabelDb) -> Vec<u8> {
    let mut l = Layout::new();
    let accent = (0.10, 0.32, 0.55);
    l.doc.banner(accent);
    l.doc.text_rgb(MARGIN, pdf::A4_H - 42.0, 20.0, Font::Bold, crate::trs!("Fahrzeugakte"), (1.0, 1.0, 1.0));
    l.doc.text_rgb(MARGIN, pdf::A4_H - 58.0, 9.0, Font::Regular, &tr!("erstellt am {a} mit ECUWerkbank", a = crate::report::now_local()), (0.85, 0.9, 1.0));
    l.y = pdf::A4_H - 100.0;

    let info = [
        (crate::trs!("Kennzeichen"), v.kennzeichen.clone()),
        (crate::trs!("Fahrzeug"), v.bezeichnung.clone()),
        (crate::trs!("Fahrgestellnummer"), v.vin.clone()),
        (crate::trs!("Kilometerstand"), v.km.map(|k| tr!("{a} km (Stand {b})", a = group_thousands(k), b = v.km_datum)).unwrap_or_default()),
        (crate::trs!("Notiz"), v.notiz.clone()),
    ];
    for (k, val) in info.iter().filter(|(_, x)| !x.is_empty()) {
        l.doc.text_rgb(MARGIN, l.y, 10.0, Font::Regular, k, (0.4, 0.4, 0.4));
        for (i, line) in pdf::wrap(val, 380.0, 10.0, Font::Bold).iter().enumerate() {
            l.doc.text(MARGIN + 110.0, l.y - 13.0 * i as f32, 10.0, Font::Bold, line);
            if i > 0 {
                l.y -= 13.0;
            }
        }
        l.y -= 15.0;
    }

    // Wartung
    if !v.wartung.is_empty() {
        l.y -= 10.0;
        l.heading(crate::trs!("Wartung"));
        let heute = vehicle::today();
        for w in &v.wartung {
            l.ensure(15.0);
            let f = w.faellig(v.km, &heute);
            l.doc.rect(MARGIN, l.y - 1.0, 40.0, 11.0, f.stufe.rgb());
            l.doc.text_rgb(MARGIN + 4.0, l.y + 1.0, 7.5, Font::Bold, f.stufe.text(), (1.0, 1.0, 1.0));
            l.doc.text(MARGIN + 50.0, l.y, 9.5, Font::Bold, &clip(&w.name, 150.0, 9.5));
            let last = match (w.letzte_km, w.letztes_datum.as_str()) {
                (Some(k), d) if !d.is_empty() => tr!("zuletzt {d} bei {a} km", d = d, a = group_thousands(k)),
                (None, d) if !d.is_empty() => format!("zuletzt {d}"),
                (Some(k), _) => tr!("zuletzt bei {a} km", a = group_thousands(k)),
                _ => crate::tr!("noch nicht eingetragen"),
            };
            l.doc.text(MARGIN + 210.0, l.y, 9.0, Font::Regular, &clip(&last, 150.0, 9.0));
            l.doc.text_rgb(MARGIN + 365.0, l.y, 9.0, Font::Regular, &clip(&f.text, 130.0, 9.0), f.stufe.rgb());
            l.y -= 15.0;
        }
    }

    // Gewechselte Teile
    if !v.teile.is_empty() {
        l.y -= 10.0;
        l.heading(crate::trs!("Gewechselte Teile"));
        let cx = [MARGIN, MARGIN + 65.0, MARGIN + 125.0, MARGIN + 280.0, MARGIN + 365.0, MARGIN + 425.0];
        for (x, h) in cx.iter().zip([crate::trs!("Datum"), "km", crate::trs!("Teil"), crate::trs!("Teilenummer"), crate::trs!("Preis"), crate::trs!("Notiz")]) {
            l.doc.text_rgb(*x, l.y, 9.0, Font::Bold, h, (0.4, 0.4, 0.4));
        }
        l.y -= 5.0;
        l.doc.line(MARGIN, l.y, pdf::A4_W - MARGIN, l.y, 0.6, 0.6);
        l.y -= 13.0;
        for t in &v.teile {
            let notiz = pdf::wrap(&t.notiz, pdf::A4_W - MARGIN - cx[5], 9.0, Font::Regular);
            l.ensure(13.0 * notiz.len().max(1) as f32 + 2.0);
            l.doc.text(cx[0], l.y, 9.0, Font::Regular, &t.datum);
            l.doc.text(cx[1], l.y, 9.0, Font::Regular, &t.km.map(group_thousands).unwrap_or_else(|| "–".into()));
            l.doc.text(cx[2], l.y, 9.0, Font::Bold, &clip(&t.name, 150.0, 9.0));
            l.doc.text(cx[3], l.y, 9.0, Font::Regular, &clip(&t.teilenummer, 80.0, 9.0));
            l.doc.text(cx[4], l.y, 9.0, Font::Regular, &t.preis.map(crate::kosten::euro).unwrap_or_default());
            for (k, line) in notiz.iter().enumerate() {
                l.doc.text(cx[5], l.y - 13.0 * k as f32, 9.0, Font::Regular, line);
            }
            l.y -= 13.0 * notiz.len().max(1) as f32 + 2.0;
        }
    }

    // Kosten und Verbrauch
    let k = crate::kosten::kosten(v, &crate::vehicle::today());
    let vb = crate::kosten::verbrauch(&v.tankbuch);
    if k.gesamt > 0.0 || !v.tankbuch.is_empty() {
        use crate::kosten::{euro, komma};
        l.y -= 10.0;
        l.heading(crate::trs!("Kosten und Verbrauch"));
        let mut zeilen = Vec::new();
        if k.gesamt > 0.0 {
            zeilen.push(tr!("Seit {a}: {b} gesamt (Teile {c}, Tanken {d})", a = &k.von, b = euro(k.gesamt), c = euro(k.teile), d = euro(k.tanken)));
            let mut z = Vec::new();
            if let Some(m) = k.je_monat {
                z.push(tr!("{a} pro Monat", a = euro(m)));
            }
            if let Some(t) = k.je_1000km {
                z.push(tr!("{a} pro 1000 km", a = euro(t)));
            }
            if !z.is_empty() {
                zeilen.push(z.join("  ·  "));
            }
            for (j, w) in k.jahre.iter().rev() {
                zeilen.push(format!("{j}:  {}", tr!("Teile {a}, Tanken {b} ({c} l), gesamt {d}", a = euro(w.teile), b = euro(w.tanken), c = komma(w.liter, 0), d = euro(w.teile + w.tanken))));
            }
        }
        if !v.tankbuch.is_empty() {
            let mut z = vec![tr!("{a} Tankfüllungen", a = v.tankbuch.len())];
            if let Some(s) = vb.schnitt {
                z.push(tr!("Verbrauch im Schnitt: {a} l/100 km", a = komma(s, 1)));
            }
            if let Some(p) = vb.euro_pro_liter {
                z.push(tr!("im Schnitt {a} €/l", a = komma(p, 3)));
            }
            zeilen.push(z.join("  ·  "));
        }
        for z in zeilen {
            l.ensure(14.0);
            l.doc.text(MARGIN, l.y, 10.0, Font::Regular, &z);
            l.y -= 14.0;
        }
    }

    // Verlauf
    let scans = history::list(&v.key);
    l.y -= 12.0;
    l.heading(&tr!("Verlauf ({a} Scan(s))", a = scans.len()));
    if scans.is_empty() {
        l.doc.text(MARGIN, l.y, 10.0, Font::Regular, crate::trs!("Noch keine Scans gespeichert."));
        l.y -= 14.0;
    } else {
        let cx = [MARGIN, MARGIN + 95.0, MARGIN + 160.0, MARGIN + 205.0, MARGIN + 245.0, MARGIN + 290.0];
        for (x, h) in cx.iter().zip([crate::trs!("Datum"), "km", crate::trs!("Fehler"), crate::trs!("neu"), crate::trs!("behoben"), crate::trs!("Reparatur / Notiz")]) {
            l.doc.text_rgb(*x, l.y, 9.0, Font::Bold, h, (0.4, 0.4, 0.4));
        }
        l.y -= 5.0;
        l.doc.line(MARGIN, l.y, pdf::A4_W - MARGIN, l.y, 0.6, 0.6);
        l.y -= 13.0;
        let reports: Vec<_> = scans.iter().map(|(_, r)| r).collect();
        for (i, r) in reports.iter().enumerate() {
            let (new, gone) = if i == 0 {
                (r.total_dtcs(), 0)
            } else {
                let earlier: Vec<_> = reports[..i - 1].iter().map(|x| (*x).clone()).collect();
                let d = history::diff(reports[i - 1], r, &earlier);
                (d.new.len(), d.gone.len())
            };
            let note = [r.reparatur.as_str(), r.note.as_str()].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join(" · ");
            let lines = pdf::wrap(&note, pdf::A4_W - MARGIN - cx[5], 9.0, Font::Regular);
            l.ensure(13.0 * lines.len().max(1) as f32 + 2.0);
            l.doc.text(cx[0], l.y, 9.0, Font::Regular, &r.created);
            l.doc.text(cx[1], l.y, 9.0, Font::Regular, &r.km.map(group_thousands).unwrap_or_else(|| "–".into()));
            l.doc.text(cx[2], l.y, 9.0, Font::Bold, &r.total_dtcs().to_string());
            let red = Stufe::Warnung.rgb();
            let green = Stufe::Gut.rgb();
            l.doc.text_rgb(cx[3], l.y, 9.0, Font::Regular, &new.to_string(), if new > 0 { red } else { (0.4, 0.4, 0.4) });
            l.doc.text_rgb(cx[4], l.y, 9.0, Font::Regular, &gone.to_string(), if gone > 0 { green } else { (0.4, 0.4, 0.4) });
            for (k, line) in lines.iter().enumerate() {
                l.doc.text(cx[5], l.y - 13.0 * k as f32, 9.0, Font::Regular, line);
            }
            l.y -= 13.0 * lines.len().max(1) as f32 + 2.0;
        }
        // Aktuelle Fehler (letzter Scan)
        if let Some((_, last)) = scans.last() {
            l.y -= 12.0;
            l.heading(&tr!("Fehler beim letzten Scan ({a})", a = last.created));
            let mut any = false;
            for m in &last.modules {
                for e in m.dtcs.iter().flatten() {
                    any = true;
                    l.ensure(13.0);
                    l.doc.text(MARGIN + 6.0, l.y, 9.5, Font::Bold, &e.code);
                    let text = if e.text.is_empty() { crate::tr!("(kein Klartext)") } else { e.text.clone() };
                    l.doc.text(MARGIN + 70.0, l.y, 9.0, Font::Regular, &clip(&format!("{text}  –  {}", m.title()), 420.0, 9.0));
                    l.y -= 13.0;
                }
            }
            if !any {
                l.doc.text_rgb(MARGIN + 6.0, l.y, 9.5, Font::Bold, crate::trs!("keine Fehler"), Stufe::Gut.rgb());
                l.y -= 13.0;
            }
        }
    }

    // Trends
    let series = crate::trend::series(&v.key);
    if !series.is_empty() {
        l.y -= 12.0;
        l.heading(crate::trs!("Trends"));
        for (st, t) in crate::trend::analyze(&series) {
            l.ensure(13.0);
            l.doc.rect(MARGIN, l.y - 1.0, 40.0, 11.0, st.rgb());
            l.doc.text_rgb(MARGIN + 4.0, l.y + 1.0, 7.5, Font::Bold, st.text(), (1.0, 1.0, 1.0));
            l.doc.text(MARGIN + 50.0, l.y, 9.0, Font::Regular, &clip(&t, 440.0, 9.0));
            l.y -= 14.0;
        }
        let cx = [MARGIN, MARGIN + 220.0, MARGIN + 300.0, MARGIN + 380.0];
        for (x, h) in cx.iter().zip(["Messwert", "erster", "letzter", "Messungen"]) {
            l.doc.text_rgb(*x, l.y, 9.0, Font::Bold, h, (0.4, 0.4, 0.4));
        }
        l.y -= 13.0;
        for (name, pts) in &series {
            l.ensure(12.0);
            let (a, b) = (pts.first().expect("nicht leer"), pts.last().expect("nicht leer"));
            let n = crate::csvlog::num;
            l.doc.text(cx[0], l.y, 9.0, Font::Regular, &clip(name, 210.0, 9.0));
            l.doc.text(cx[1], l.y, 9.0, Font::Regular, &format!("{} {}", n(a.wert), a.einheit));
            l.doc.text(cx[2], l.y, 9.0, Font::Bold, &format!("{} {}", n(b.wert), b.einheit));
            l.doc.text(cx[3], l.y, 9.0, Font::Regular, &pts.len().to_string());
            l.y -= 12.0;
        }
    }

    // Codierungsänderungen
    let mut backups: Vec<_> = backup::list_all().into_iter().filter(|(_, b)| b.vehicle == v.key).map(|(_, b)| b).collect();
    backups.sort_by_key(|b| b.timestamp);
    if !backups.is_empty() {
        l.y -= 12.0;
        l.heading(&tr!("Codierung ({a} Sicherung(en))", a = backups.len()));
        let mut changes = 0;
        for w in backups.windows(2) {
            for c in backup::compare(&w[0], &w[1], db) {
                changes += 1;
                l.ensure(26.0);
                l.doc.text(MARGIN + 6.0, l.y, 9.0, Font::Bold, &format!("{} → {}", w[0].created, w[1].created));
                l.doc.text(MARGIN + 200.0, l.y, 9.0, Font::Regular, &clip(&c.module, 290.0, 9.0));
                l.y -= 12.0;
                l.doc.text_rgb(MARGIN + 20.0, l.y, 8.5, Font::Regular, &clip(&format!("{}  →  {}", c.old_hex, c.new_hex), 470.0, 8.5), (0.35, 0.35, 0.35));
                l.y -= 14.0;
            }
        }
        if changes == 0 {
            l.doc.text(MARGIN + 6.0, l.y, 9.5, Font::Regular, crate::trs!("Keine Codierung zwischen den Sicherungen geändert."));
            l.y -= 13.0;
        }
    }

    // Softwarestände
    let sw = crate::swinfo::list(&v.key);
    if let Some(last) = sw.last() {
        l.y -= 12.0;
        l.heading(&tr!("Softwarestände ({a})", a = last.created));
        for m in &last.module {
            l.ensure(12.0);
            l.doc.text(MARGIN + 6.0, l.y, 8.5, Font::Regular, &clip(&format!("{}  {}", m.adresse, if m.system.is_empty() { &m.name } else { &m.system }), 190.0, 8.5));
            l.doc.text(MARGIN + 200.0, l.y, 8.5, Font::Regular, &clip(&tr!("{a}  SW {b}  HW {c}", a = m.teilenummer, b = m.software, c = m.hardware_ver), 290.0, 8.5));
            l.y -= 12.0;
        }
        for w in sw.windows(2) {
            for (m, t) in crate::swinfo::compare(&w[0], &w[1]) {
                l.ensure(12.0);
                l.doc.text_rgb(MARGIN + 6.0, l.y, 8.5, Font::Regular, &clip(&format!("{}: {m}: {t}", w[1].created), 480.0, 8.5), (0.55, 0.3, 0.05));
                l.y -= 12.0;
            }
        }
    }

    // Dateien
    l.y -= 12.0;
    l.heading(crate::trs!("Inhalt des Fahrzeugordners"));
    let st: Vec<String> = vehicle::stats(v).into_iter().map(|(s, n)| format!("{s}: {n}")).collect();
    l.ensure(13.0);
    l.doc.text(MARGIN + 6.0, l.y, 9.0, Font::Regular, &st.join("   "));

    let n = l.doc.page_count();
    for p in 0..n {
        l.doc.text_on_page(p, MARGIN, 28.0, 8.0, Font::Regular, &tr!("ECUWerkbank · Fahrzeugakte · {a}", a = v.label()));
        l.doc.text_on_page(p, pdf::A4_W - MARGIN - 50.0, 28.0, 8.0, Font::Regular, &tr!("Seite {a} von {n}", a = p + 1, n = n));
    }
    l.doc.to_bytes(crate::trs!("Fahrzeugakte"))
}
