//! Messwert-Diagramme als PDF (Scope, CSV-Aufzeichnungen).
//!
//! Seite 1: Kopf, Tabelle (min/max/Mittel) und — bei mehreren Kurven — eine
//! normierte Übersicht. Danach je Messwert ein eigenes Diagramm mit echter Skala.
//! Alles als Vektorgrafik, bleibt beim Zoomen scharf.

use crate::tr;
use crate::csvlog::Column;
use crate::pdf::{self, Doc, Font};

const MARGIN: f32 = 50.0;
const PALETTE: [(f32, f32, f32); 8] = [
    (0.12, 0.42, 0.75),
    (0.85, 0.37, 0.05),
    (0.17, 0.60, 0.25),
    (0.75, 0.15, 0.20),
    (0.50, 0.30, 0.70),
    (0.55, 0.35, 0.20),
    (0.85, 0.30, 0.60),
    (0.30, 0.30, 0.30),
];

/// Kopfangaben (Bezeichnung, Wert), z. B. (crate::trs!("Fahrzeug"), "B-XY-123 · Golf 6").
pub type Meta = Vec<(String, String)>;

fn color(i: usize) -> (f32, f32, f32) {
    PALETTE[i % PALETTE.len()]
}

/// „Schöne“ Achsenteilung: Schrittweite 1/2/5·10ⁿ für etwa `n` Abschnitte.
pub fn nice_ticks(min: f64, max: f64, n: usize) -> Vec<f64> {
    let (min, max) = if (max - min).abs() < 1e-12 { (min - 1.0, max + 1.0) } else { (min, max) };
    let raw = (max - min) / n.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let start = (min / step).floor() * step;
    // Von unterhalb des Minimums bis zur ersten Marke ≥ Maximum (Kurve passt ganz hinein).
    let mut v = Vec::new();
    let mut t = start;
    loop {
        v.push((t / step).round() * step);
        if t >= max - step * 1e-9 || v.len() >= 50 {
            break;
        }
        t += step;
    }
    v
}

fn fmt(v: f64) -> String {
    let a = v.abs();
    let s = if a >= 1000.0 || (a - a.round()).abs() < 1e-9 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    };
    s.replace('.', ",")
}

/// Viele Punkte auf ≤ `max` reduzieren (je Abschnitt min und max behalten).
fn decimate(p: &[[f64; 2]], max: usize) -> Vec<[f64; 2]> {
    if p.len() <= max {
        return p.to_vec();
    }
    let bucket = p.len().div_ceil(max / 2);
    let mut out = Vec::with_capacity(max + 2);
    for chunk in p.chunks(bucket) {
        let lo = chunk.iter().min_by(|a, b| a[1].total_cmp(&b[1])).expect("nicht leer");
        let hi = chunk.iter().max_by(|a, b| a[1].total_cmp(&b[1])).expect("nicht leer");
        if lo[0] <= hi[0] {
            out.push(*lo);
            out.push(*hi);
        } else {
            out.push(*hi);
            out.push(*lo);
        }
    }
    out
}

fn stats(c: &Column) -> (f64, f64, f64) {
    let min = c.points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let max = c.points.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let avg = c.points.iter().map(|p| p[1]).sum::<f64>() / c.points.len().max(1) as f64;
    (min, max, avg)
}

fn label(c: &Column) -> String {
    if c.unit.is_empty() { c.name.clone() } else { format!("{} [{}]", c.name, c.unit) }
}

/// Ein Diagramm in den Rahmen (x, y unten links, w, h) zeichnen.
#[allow(clippy::too_many_arguments)]
fn draw_chart(doc: &mut Doc, x: f32, y: f32, w: f32, h: f32, series: &[(&Column, (f32, f32, f32))], normalize: bool, t_range: (f64, f64), y_label: &str) {
    let (t0, t1) = t_range;
    let (vmin, vmax) = if normalize {
        (0.0, 100.0)
    } else {
        series.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (c, _)| {
            let (mi, ma, _) = stats(c);
            // Sollwert-Grenzen mit anzeigen, wenn sie nicht weit außerhalb liegen
            let span = (ma - mi).abs().max(1.0);
            let lo = c.soll.and_then(|r| r.min).filter(|m| *m > mi - span).map_or(mi, |m| mi.min(m));
            let hi = c.soll.and_then(|r| r.max).filter(|m| *m < ma + span).map_or(ma, |m| ma.max(m));
            (a.min(lo), b.max(hi))
        })
    };
    let yt = nice_ticks(vmin, vmax, 5);
    let (ylo, yhi) = (*yt.first().unwrap_or(&vmin), *yt.last().unwrap_or(&vmax));
    let xt = nice_ticks(t0, t1, 8);
    let sx = |t: f64| x + ((t - t0) / (t1 - t0).max(1e-9)) as f32 * w;
    let sy = |v: f64| y + ((v - ylo) / (yhi - ylo).max(1e-9)) as f32 * h;

    // Gitter + Beschriftung
    for v in &yt {
        let py = sy(*v);
        doc.line(x, py, x + w, py, 0.3, 0.85);
        let s = if normalize { format!("{} %", fmt(*v)) } else { fmt(*v) };
        doc.text_rgb(x - 6.0 - pdf::text_width(&s, 7.5, Font::Regular), py - 2.5, 7.5, Font::Regular, &s, (0.35, 0.35, 0.35));
    }
    for t in xt.iter().filter(|t| **t >= t0 - 1e-9 && **t <= t1 + 1e-9) {
        let px = sx(*t);
        doc.line(px, y, px, y + h, 0.3, 0.9);
        let s = fmt(*t);
        doc.text_rgb(px - pdf::text_width(&s, 7.5, Font::Regular) / 2.0, y - 11.0, 7.5, Font::Regular, &s, (0.35, 0.35, 0.35));
    }
    doc.stroke_rect(x, y, w, h, 0.6, 0.5);
    doc.text_rgb(x + w - 24.0, y - 21.0, 7.5, Font::Regular, crate::trs!("t [s]"), (0.35, 0.35, 0.35));
    if !y_label.is_empty() {
        doc.text_rgb(x, y + h + 5.0, 8.0, Font::Bold, y_label, (0.3, 0.3, 0.3));
    }

    // Sollwert-Grenzen (rot)
    if !normalize {
        for (c, _) in series {
            for lim in [c.soll.and_then(|r| r.min), c.soll.and_then(|r| r.max)].into_iter().flatten() {
                if lim >= ylo && lim <= yhi {
                    let py = sy(lim);
                    doc.polyline(&[(x, py), (x + w, py)], 0.8, (0.85, 0.2, 0.15));
                    let s = tr!("Soll {a}", a = fmt(lim));
                    doc.text_rgb(x + w - pdf::text_width(&s, 7.0, Font::Regular) - 3.0, py + 2.5, 7.0, Font::Regular, &s, (0.8, 0.2, 0.15));
                }
            }
        }
    }

    // Kurven
    for (c, rgb) in series {
        let (mi, ma, _) = stats(c);
        let span = if (ma - mi).abs() < 1e-12 { 1.0 } else { ma - mi };
        let pts: Vec<(f32, f32)> = decimate(&c.points, 1500)
            .iter()
            .map(|p| {
                let v = if normalize { (p[1] - mi) / span * 100.0 } else { p[1] };
                (sx(p[0]), sy(v).clamp(y, y + h))
            })
            .collect();
        doc.polyline(&pts, 1.1, *rgb);
    }
}

/// PDF mit allen Kurven erstellen.
pub fn to_pdf(title: &str, meta: &Meta, cols: &[Column]) -> Vec<u8> {
    let mut doc = Doc::new();
    doc.new_page();
    let accent = (0.10, 0.32, 0.55);
    doc.banner(accent);
    doc.text_rgb(MARGIN, pdf::A4_H - 42.0, 20.0, Font::Bold, title, (1.0, 1.0, 1.0));
    doc.text_rgb(MARGIN, pdf::A4_H - 58.0, 9.0, Font::Regular, crate::trs!("erstellt mit ECUWerkbank"), (0.85, 0.9, 1.0));
    let mut y = pdf::A4_H - 100.0;

    let t0 = cols.iter().filter_map(|c| c.points.first().map(|p| p[0])).fold(f64::INFINITY, f64::min);
    let t1 = cols.iter().filter_map(|c| c.points.last().map(|p| p[0])).fold(f64::NEG_INFINITY, f64::max);
    let (t0, t1) = if t0.is_finite() && t1 > t0 { (t0, t1) } else { (0.0, 1.0) };
    let mut info = meta.clone();
    info.push(("Dauer".into(), format!("{} s", fmt(t1 - t0))));
    info.push(("Messwerte".into(), tr!("{a} Kurve(n), {b} Punkte", a = cols.len(), b = cols.iter().map(|c| c.points.len()).sum::<usize>())));
    for (k, v) in info.iter().filter(|(_, v)| !v.is_empty()) {
        doc.text_rgb(MARGIN, y, 10.0, Font::Regular, k, (0.4, 0.4, 0.4));
        doc.text(MARGIN + 110.0, y, 10.0, Font::Bold, v);
        y -= 15.0;
    }

    // Tabelle
    y -= 10.0;
    let cx = [MARGIN, MARGIN + 18.0, MARGIN + 230.0, MARGIN + 290.0, MARGIN + 350.0, MARGIN + 410.0];
    for (x, h) in cx[1..].iter().zip(["Messwert", "min", "max", "Mittel", "Sollwert"]) {
        doc.text_rgb(*x, y, 9.0, Font::Bold, h, (0.4, 0.4, 0.4));
    }
    y -= 5.0;
    doc.line(MARGIN, y, pdf::A4_W - MARGIN, y, 0.6, 0.6);
    y -= 13.0;
    for (i, c) in cols.iter().enumerate() {
        if y < 80.0 {
            doc.new_page();
            y = pdf::A4_H - MARGIN;
        }
        let (mi, ma, av) = stats(c);
        doc.rect(cx[0], y - 1.0, 10.0, 6.0, color(i));
        doc.text(cx[1], y, 9.5, Font::Regular, &label(c));
        doc.text(cx[2], y, 9.5, Font::Regular, &fmt(mi));
        doc.text(cx[3], y, 9.5, Font::Regular, &fmt(ma));
        doc.text(cx[4], y, 9.5, Font::Bold, &fmt(av));
        if let Some(r) = c.soll {
            let (lo, hi) = r.as_pair();
            let ok = mi >= lo && ma <= hi;
            let col = if ok { crate::limits::Stufe::Gut.rgb() } else { crate::limits::Stufe::Warnung.rgb() };
            doc.text_rgb(cx[5], y, 9.0, Font::Regular, &format!("{}  {}", r.text(), if ok { "OK" } else { crate::trs!("überschritten") }), col);
        }
        y -= 14.0;
    }

    let chart_w = pdf::A4_W - 2.0 * MARGIN - 30.0;
    let chart_x = MARGIN + 30.0;
    // Übersicht (normiert), wenn mehrere Kurven
    if cols.len() > 1 {
        let h = 200.0;
        if y - h - 60.0 < 50.0 {
            doc.new_page();
            y = pdf::A4_H - MARGIN;
        }
        y -= 22.0;
        doc.text_rgb(MARGIN, y, 13.0, Font::Bold, crate::trs!("Übersicht (normiert, je Kurve 0–100 %)"), accent);
        y -= 20.0 + h;
        let series: Vec<(&Column, (f32, f32, f32))> = cols.iter().enumerate().map(|(i, c)| (c, color(i))).collect();
        draw_chart(&mut doc, chart_x, y, chart_w, h, &series, true, (t0, t1), "");
        y -= 34.0;
    }

    // Einzeldiagramme
    let h = 150.0;
    for (i, c) in cols.iter().enumerate() {
        if y - h - 40.0 < 50.0 {
            doc.new_page();
            y = pdf::A4_H - MARGIN - 10.0;
        }
        y -= h + 14.0;
        draw_chart(&mut doc, chart_x, y, chart_w, h, &[(c, color(i))], false, (t0, t1), &label(c));
        y -= 32.0;
    }

    let n = doc.page_count();
    let foot = meta.iter().map(|(_, v)| v.as_str()).filter(|v| !v.is_empty()).take(2).collect::<Vec<_>>().join(" · ");
    for p in 0..n {
        doc.text_on_page(p, MARGIN, 28.0, 8.0, Font::Regular, &tr!("ECUWerkbank · {title} · {foot}", title = title, foot = foot));
        doc.text_on_page(p, pdf::A4_W - MARGIN - 50.0, 28.0, 8.0, Font::Regular, &tr!("Seite {a} von {n}", a = p + 1, n = n));
    }
    doc.to_bytes(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_nice() {
        assert_eq!(nice_ticks(0.0, 100.0, 5), vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        let t = nice_ticks(812.0, 3409.0, 5);
        assert_eq!(t, vec![0.0, 1000.0, 2000.0, 3000.0, 4000.0]);
    }

    #[test]
    fn decimate_keeps_extremes() {
        let p: Vec<[f64; 2]> = (0..10_000).map(|i| [i as f64, if i == 5000 { 99.0 } else { 1.0 }]).collect();
        let d = decimate(&p, 500);
        assert!(d.len() <= 520);
        assert!(d.iter().any(|x| x[1] == 99.0));
    }

    #[test]
    fn pdf_renders() {
        let cols: Vec<Column> = (0..5)
            .map(|k| Column {
                name: format!("Kurve {k}"),
                unit: "1/min".into(),
                points: (0..600).map(|i| [i as f64 * 0.25, (i as f64 * 0.05 + k as f64).sin() * 1000.0 + 2000.0]).collect(),
                soll: Some(crate::limits::Range { min: None, max: Some(2800.0) }),
            })
            .collect();
        let pdf = to_pdf("Messwerte", &vec![("Fahrzeug".into(), "B-XY-123".into())], &cols);
        assert!(pdf.starts_with(b"%PDF"));
        assert!(String::from_utf8_lossy(&pdf).matches("/Type /Page ").count() >= 2);
    }
}
