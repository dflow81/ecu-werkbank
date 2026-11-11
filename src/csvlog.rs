//! Messwerte als CSV protokollieren (z. B. Probefahrt).
//!
//! Format für deutsche Tabellenprogramme: Trennzeichen `;`, Dezimalkomma.
//! Erste Spalte: Zeit in Sekunden seit Aufzeichnungsbeginn.

use crate::tr;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct CsvLog {
    w: BufWriter<File>,
    path: PathBuf,
    start: Instant,
    header: Option<Vec<String>>,
    rows: usize,
}

impl CsvLog {
    pub fn create(path: &Path) -> Result<Self> {
        let f = File::create(path).with_context(|| tr!("CSV anlegen: {a}", a = path.display()))?;
        Ok(Self { w: BufWriter::new(f), path: path.to_path_buf(), start: Instant::now(), header: None, rows: 0 })
    }

    /// Eine Zeile schreiben. `cols` = (Spaltenname, Wert). Die Kopfzeile wird
    /// aus der ersten Zeile gebildet.
    pub fn write(&mut self, cols: &[(String, String)]) -> Result<()> {
        let t = self.start.elapsed().as_secs_f64();
        self.write_at(t, cols)
    }

    /// Zeile mit vorgegebener Zeit `t` schreiben (z. B. Scope-Zeitachse).
    pub fn write_at(&mut self, t: f64, cols: &[(String, String)]) -> Result<()> {
        if self.header.is_none() {
            let names: Vec<String> = cols.iter().map(|(n, _)| n.clone()).collect();
            let mut line = String::from("Zeit [s]");
            for n in &names {
                line.push(';');
                line.push_str(&escape(n));
            }
            writeln!(self.w, "{line}")?;
            self.header = Some(names);
        }
        let mut line = num(t);
        for (_, v) in cols {
            line.push(';');
            line.push_str(&escape(v));
        }
        writeln!(self.w, "{line}")?;
        // Sofort schreiben: bei Abbruch (Strg-C, Stecker gezogen) bleiben die Daten.
        self.w.flush()?;
        self.rows += 1;
        Ok(())
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Eine Spalte einer CSV-Aufzeichnung.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Column {
    pub name: String,
    pub unit: String,
    /// (Zeit [s], Wert)
    pub points: Vec<[f64; 2]>,
    /// Sollwert-Bereich (für Grenzlinien im PDF)
    pub soll: Option<crate::limits::Range>,
}

/// CSV im ecu-werkbank-Format lesen (erste Spalte Zeit; `;` + Dezimalkomma oder `,` + Punkt).
pub fn read(path: &Path) -> Result<Vec<Column>> {
    let text = std::fs::read_to_string(path).with_context(|| tr!("CSV lesen: {a}", a = path.display()))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Vec<Column>> {
    let mut lines = text.lines();
    let header = lines.next().ok_or_else(|| anyhow::anyhow!(tr!("leere Datei")))?;
    let sep = if header.contains(';') { ';' } else { ',' };
    let names: Vec<&str> = header.split(sep).collect();
    if names.len() < 2 {
        anyhow::bail!(tr!("keine Messwert-Spalten gefunden"));
    }
    let mut cols: Vec<Column> = names[1..]
        .iter()
        .map(|c| {
            let c = c.trim().trim_matches('"');
            let (name, unit) = match c.rsplit_once(" [") {
                Some((n, u)) => (n.to_string(), u.trim_end_matches(']').to_string()),
                None => (c.to_string(), String::new()),
            };
            Column { name, unit, points: Vec::new(), soll: None }
        })
        .collect();
    let num = |s: &str| -> Option<f64> {
        let s = s.trim();
        if sep == ';' { s.replace(',', ".").parse().ok() } else { s.parse().ok() }
    };
    for line in lines {
        let f: Vec<&str> = line.split(sep).collect();
        let Some(t) = f.first().and_then(|x| num(x)) else { continue };
        for (i, c) in cols.iter_mut().enumerate() {
            if let Some(v) = f.get(i + 1).and_then(|x| num(x)) {
                c.points.push([t, v]);
            }
        }
    }
    cols.retain(|c| !c.points.is_empty());
    if cols.is_empty() {
        anyhow::bail!(tr!("keine Zahlenwerte gefunden"));
    }
    Ok(cols)
}

/// Zahl mit Dezimalkomma, max. 3 Nachkommastellen.
pub fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.replace('.', ",")
}

fn escape(s: &str) -> String {
    if s.contains(';') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_number_format() {
        assert_eq!(num(14.1), "14,1");
        assert_eq!(num(800.0), "800");
        assert_eq!(num(-3.25), "-3,25");
    }

    #[test]
    fn writes_header_once() {
        let p = std::env::temp_dir().join(format!("ecu-werkbank-test-{}.csv", std::process::id()));
        let mut log = CsvLog::create(&p).unwrap();
        let row = vec![("Drehzahl [1/min]".to_string(), num(800.0)), ("Text".into(), "a;b".into())];
        log.write(&row).unwrap();
        log.write(&row).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "Zeit [s];Drehzahl [1/min];Text");
        assert!(lines[1].ends_with(";800;\"a;b\""));
        let cols = read(&p).unwrap();
        assert_eq!(cols.len(), 1); // Textspalte hat keine Zahlen
        assert_eq!((cols[0].name.as_str(), cols[0].unit.as_str(), cols[0].points.len()), ("Drehzahl", "1/min", 2));
        let _ = std::fs::remove_file(p);
    }
}
