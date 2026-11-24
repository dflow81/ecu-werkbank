//! CLI: Fahrzeug-Datenbank.
//! Aus main.rs ausgelagert (Roadmap 27); Aufruf über `enum Command` in main.rs.

use super::*;

// ---------------------------------------------------------------------------
// Fahrzeug-Datenbank
// ---------------------------------------------------------------------------

pub(super) fn db_cmd(aktion: DbAktion, datei: Option<PathBuf>, labels_dir: &std::path::Path) -> Result<()> {
    use ecu_werkbank::db::{standard_pfad, Db};
    let pfad = datei.unwrap_or_else(standard_pfad);
    let db = Db::oeffnen(&pfad)?;
    match aktion {
        DbAktion::Status => {
            println!("{}", tr!("Datenbank: {a}", a = pfad.display()));
            println!("{}", tr!("Schema-Version: {a}", a = db.schema_version()?));
            for (t, n) in db.statistik()? {
                println!("  {t:<20} {n:>9}");
            }
            if db.quellen()?.is_empty() {
                println!("{}", tr!("\nNoch leer. Start:  ecu-werkbank db eingebaut   (dann z. B. db labels, db dbc --datei …)"));
            }
        }
        DbAktion::Aufraeumen => {
            let n = db.platzhalter_entfernen()?;
            println!("{}", tr!("{a} Anpassungskanäle ohne echten Namen gelöscht.", a = n));
        }
        DbAktion::Teil { teilenummer } => match db.wissen_zur_teilenummer(&teilenummer)? {
            None => println!("{}", tr!("Zu {a} steht nichts in der Datenbank.", a = teilenummer)),
            Some(w) => {
                println!("{}  {}", w.zusammenfassung(), w.name);
                for (k, n) in &w.anpassung {
                    println!("{}", tr!("  Anpassung Kanal {a}: {b}", a = format!("{k:03}"), b = n));
                }
                for c in &w.codierung {
                    let wo = c.bit.map(|b| format!("{}.{}", c.byte, b)).unwrap_or_else(|| c.byte.to_string());
                    println!("{}", tr!("  Codierung {a}: {b}", a = wo, b = c.name));
                }
                for d in &w.dids {
                    println!("{}", tr!("  Messwert {a}: {b} {c}", a = format!("{:#06x}", d.did), b = d.name, c = d.einheit));
                }
                for zeile in w.diagnose.iter().flat_map(|d| d.zeilen()) {
                    println!("  {zeile}");
                }
                for f in &w.fehler {
                    println!("{}", tr!("  Fehler {a}: {b}", a = f.code, b = f.text));
                }
            }
        },
        DbAktion::Fehler { code, art } => {
            let art = art.as_deref().map(|a| {
                u8::from_str_radix(a.trim_start_matches("0x").trim_start_matches("0X"), 16)
            }).transpose().context(tr!("Fehlerart als Hexzahl angeben, z. B. 13."))?;
            for zeile in db.fehlerreferenz(&code, art)?.zeilen() {
                println!("{zeile}");
            }
        }
        DbAktion::Quellen => {
            let q = db.quellen()?;
            if q.is_empty() {
                println!("{}", tr!("Keine Quellen."));
            }
            for q in q {
                let teilen = if q.ist_teilbar() { "" } else { " [bleibt lokal]" };
                println!("{:>3}  {:<9} {}  {}  Lizenz: {}  {} Einträge  ({}){teilen}", q.id, q.art.text(), q.name, q.version, if q.lizenz.is_empty() { "–" } else { &q.lizenz }, q.zeilen, q.importiert);
                if !q.herkunft.is_empty() {
                    println!("       {}", q.herkunft);
                }
            }
        }
        DbAktion::Eingebaut => {
            db.normdaten_eintragen()?;
            println!("{}", tr!("Normdaten (UDS-Dienste, Sitzungen) eingetragen."));
            for (name, e) in db.community_uebernehmen()? {
                println!("{}", tr!("Community „{a}“: {b}", a = name, b = e.text()));
            }
        }
        DbAktion::Import { datei, hersteller, lizenz } => {
            for p in datei {
                match ecu_werkbank::db::import::datei(&db, &p, &hersteller, &lizenz) {
                    Ok((art, text)) => println!("{}", tr!("✓ {a} — {b}: {c}", a = p.display(), b = art.text(), c = text)),
                    Err(e) => println!("{}", tr!("✗ {a}: {e}", a = p.display(), e = format!("{e:#}"))),
                }
            }
        }
        DbAktion::Paket { datei, community } => {
            use ecu_werkbank::db::{paket::Datenpaket, QuellArt};
            let p = Datenpaket::lesen(&datei)?;
            if community && p.quelle.lizenz.trim().is_empty() {
                anyhow::bail!(tr!("Community-Daten brauchen eine Lizenz ([quelle] lizenz = …)."));
            }
            let e = db.paket_importieren(&p, if community { QuellArt::Community } else { QuellArt::User })?;
            println!("{}", tr!("„{a}“ übernommen: {b} (Quelle {c}).", a = p.quelle.name, b = e.text(), c = e.quelle_id));
        }
        DbAktion::Odx { datei, hersteller } => {
            let (p, art) = ecu_werkbank::db::odx::lesen(&datei, &hersteller)?;
            let e = db.paket_importieren_lokal(&p, art)?;
            println!("{}", tr!("{a} übernommen: {b} (Quelle {c}).", a = art.text(), b = e.text(), c = e.quelle_id));
            println!("{}", tr!("Diese Daten bleiben nur in deiner Datenbank: kein Export, keine Schnittstelle."));
        }
        DbAktion::Csv { datei, lizenz } => {
            let e = db.csv_importieren(&datei, &lizenz)?;
            println!("{}", tr!("Übernommen: {a} (Quelle {b}).", a = e.text(), b = e.quelle_id));
        }
        DbAktion::Export { ziel, lizenz, name } => {
            let (p, weg) = db.paket_exportieren(&name, &lizenz)?;
            p.schreiben(&ziel)?;
            let n: usize = p.hersteller.iter().map(|h| h.steuergeraete.len()).sum();
            println!("{}", tr!("Geschrieben: {a} ({n} Steuergeräte, {f} allgemeine Fehlertexte).", a = ziel.display(), n = n, f = p.fehler.len()));
            for w in weg {
                println!("{}", tr!("  nicht enthalten (bleibt lokal): {a}", a = w));
            }
        }
        DbAktion::Labels { ordner } => {
            let ordner = ordner.unwrap_or_else(|| labels_dir.to_path_buf());
            let vw = db.hersteller("Volkswagen", &["WVW", "WV1", "WV2", "WV3"], None)?;
            let e = db.labels_importieren(&ordner, vw)?;
            println!("{}", tr!("Aus {a} übernommen: {b} (Quelle {c}).", a = ordner.display(), b = e.text(), c = e.quelle_id));
        }
        DbAktion::Dbc { datei, lizenz, hersteller } => {
            let text = std::fs::read(&datei).with_context(|| tr!("DBC lesen: {a}", a = datei.display()))?;
            let dbc = ecu_werkbank::dbc::Dbc::lesen(&String::from_utf8_lossy(&text));
            if dbc.botschaften.is_empty() {
                anyhow::bail!(tr!("Keine Botschaften in {a} gefunden.", a = datei.display()));
            }
            let h = hersteller.map(|h| db.hersteller(&h, &[], None)).transpose()?;
            let name = datei.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let e = db.dbc_importieren(&dbc, &name, &lizenz, &datei.display().to_string(), h)?;
            println!("{}", tr!("Übernommen: {a} (Quelle {b}).", a = e.text(), b = e.quelle_id));
            if lizenz.is_empty() {
                println!("{}", tr!("Ohne Lizenzangabe bleiben die Daten nur in deiner Datenbank."));
            }
        }
        DbAktion::Entfernen { id, ja } => {
            let Some(q) = db.quellen()?.into_iter().find(|q| q.id == id) else {
                anyhow::bail!(tr!("Keine Quelle mit der Nummer {a}.", a = id));
            };
            if !ja {
                println!("{}", tr!("Würde entfernen: {a} „{b}“ mit {c} Einträgen. Wirklich: --ja", a = q.art.text(), b = q.name, c = q.zeilen));
                return Ok(());
            }
            db.quelle_entfernen(id)?;
            println!("{}", tr!("Entfernt: {a} „{b}“.", a = q.art.text(), b = q.name));
        }
        DbAktion::Sichern { ziel } => {
            db.sichern(&ziel)?;
            println!("{}", tr!("Kopie gespeichert: {a}", a = ziel.display()));
        }
    }
    Ok(())
}
