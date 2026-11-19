//! GUI: Werkzeuge → Fahrzeug-Datenbank. Übersicht, Quellen, Import
//! (Dateiart wird erkannt), mitgelieferte Daten, Messwert-Texte lernen, Export.

use super::*;
use ecu_werkbank::db::{self, eigen::mit_db};

pub(super) struct DatenbankSeite {
    hersteller: String,
    lizenz: String,
    export_name: String,
    export_lizenz: String,
    entfernen: Option<i64>,
    blockmap: Option<PathBuf>,
    adresse: String,
    gelernt: Option<ecu_werkbank::wertetexte::Gelernt>,
    meldungen: Vec<String>,
}

impl Default for DatenbankSeite {
    fn default() -> Self {
        Self {
            hersteller: "Volkswagen".into(),
            lizenz: String::new(),
            export_name: tr!("Eigene Fahrzeugdaten"),
            export_lizenz: "CC0-1.0".into(),
            entfernen: None,
            blockmap: None,
            adresse: "20".into(),
            gelernt: None,
            meldungen: Vec::new(),
        }
    }
}

impl App {
    /// Dateien übernehmen (im Hintergrund, große DBC/ODX brauchen etwas).
    pub(super) fn db_dateien_importieren(&mut self, dateien: Vec<PathBuf>) {
        if dateien.is_empty() {
            return;
        }
        let (h, l) = (self.dbs.hersteller.clone(), self.dbs.lizenz.clone());
        self.im_hintergrund(tr!("Dateien in die Datenbank übernehmen"), move || {
            let zeilen: Vec<String> = dateien
                .iter()
                .map(|p| {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    match mit_db(|d| db::import::datei(d, p, &h, &l)) {
                        Ok((art, text)) => tr!("✓ {a} — {b}: {c}", a = name, b = art.text(), c = text),
                        Err(e) => tr!("✗ {a}: {e}", a = name, e = format!("{e:#}")),
                    }
                })
                .collect();
            Box::new(move |app: &mut App| {
                app.status = zeilen.last().cloned().unwrap_or_default();
                app.dbs.meldungen = zeilen;
                app.reload_labels();
                app.tab = Tab::Datenbank;
            })
        });
    }

    pub(super) fn ui_datenbank(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Fahrzeug-Datenbank"));
        ui.label(tr!("Hier liegt dein ganzes Fahrzeugwissen: eigene Labels und Texte, übernommene Datenpakete, CAN-Datenbanken. Jede Quelle mit Herkunft und Lizenz — und lässt sich als Ganzes wieder entfernen."));
        let statistik = mit_db(|d| d.statistik()).unwrap_or_default();
        let quellen = mit_db(|d| d.quellen()).unwrap_or_default();
        let zahl = |n: &str| statistik.iter().find(|(t, _)| *t == n).map(|(_, v)| *v).unwrap_or(0);
        ui.weak(tr!(
            "{a} Steuergeräte · {b} Fehlertexte · {c} Messwerte · {d} Codier-Einträge · {e} Anpassungen · {f} CAN-Botschaften · {g} CAN-Signale",
            a = zahl("ecu"),
            b = zahl("dtc"),
            c = zahl("measurement") + zahl("did"),
            d = zahl("coding_item"),
            e = zahl("adaptation"),
            f = zahl("can_message"),
            g = zahl("can_signal")
        ));

        // --- Quellen
        ui.add_space(6.0);
        ui.strong(tr!("Quellen"));
        let mut entfernen = None;
        egui::Grid::new("db_quellen").striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
            for h in [trn!("Art"), trn!("Name"), trn!("Lizenz"), trn!("Einträge"), trn!("übernommen"), ""] {
                ui.strong(ecu_werkbank::i18n::t(h));
            }
            ui.end_row();
            for q in &quellen {
                ui.monospace(q.art.text());
                ui.label(&q.name);
                if db::darf_weitergeben(q.art, &q.lizenz) {
                    ui.label(if q.lizenz.is_empty() { "–".to_string() } else { q.lizenz.clone() });
                } else {
                    ui.weak(tr!("bleibt lokal")).on_hover_text(tr!("Ohne Lizenz oder selbst gekaufte Herstellerdaten: wird nie exportiert oder über die Schnittstelle ausgegeben."));
                }
                ui.label(q.zeilen.to_string());
                ui.weak(&q.importiert);
                if self.dbs.entfernen == Some(q.id) {
                    ui.horizontal(|ui| {
                        if ui.button(tr!("Ja, entfernen")).clicked() {
                            entfernen = Some(q.id);
                        }
                        if ui.button(tr!("Abbrechen")).clicked() {
                            self.dbs.entfernen = None;
                        }
                    });
                } else if ui.small_button("🗑").on_hover_text(tr!("Diese Quelle mit allen ihren Einträgen entfernen")).clicked() {
                    self.dbs.entfernen = Some(q.id);
                }
                ui.end_row();
            }
        });
        if quellen.is_empty() {
            ui.weak(tr!("Noch keine Quellen — unten „Mitgelieferte Daten aktualisieren“ oder eine Datei importieren."));
        }
        if let Some(id) = entfernen {
            self.dbs.entfernen = None;
            self.status = match mit_db(|d| d.quelle_entfernen(id)) {
                Ok(_) => tr!("Quelle entfernt."),
                Err(e) => tr!("Entfernen: {e}", e = format!("{e:#}")),
            };
            self.reload_labels();
        }

        // --- Import
        ui.add_space(8.0);
        ui.separator();
        ui.strong(tr!("Importieren"));
        ui.weak(tr!("Die Art wird erkannt: DBC, Datenpaket (TOML/JSON), Label-Datei, CSV (code;text oder did;name;…), ODX/PDX. Dateien lassen sich auch einfach ins Fenster ziehen."));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Hersteller:"));
            ui.add(egui::TextEdit::singleline(&mut self.dbs.hersteller).desired_width(120.0)).on_hover_text(tr!("Für DBC und ODX/PDX"));
            ui.label(tr!("Lizenz:"));
            ui.add(egui::TextEdit::singleline(&mut self.dbs.lizenz).desired_width(100.0).hint_text(tr!("z. B. MIT"))).on_hover_text(tr!("Für DBC und CSV. Ohne Angabe bleiben die Daten nur bei dir."));
            if ui.button(tr!("📂 Datei importieren…")).clicked() {
                if let Some(v) = rfd::FileDialog::new().add_filter(tr!("Fahrzeugdaten"), &["dbc", "toml", "json", "csv", "odx", "pdx"]).pick_files() {
                    self.db_dateien_importieren(v);
                }
            }
            if ui.button(tr!("🔄 Mitgelieferte Daten aktualisieren")).on_hover_text(tr!("Normdaten und die mit ECUWerkbank gelieferten offenen Daten (z. B. VW-Adressliste) übernehmen bzw. auffrischen")).clicked() {
                let r = mit_db(|d| {
                    d.normdaten_eintragen()?;
                    d.community_uebernehmen()
                });
                self.status = match r {
                    Ok(l) => tr!("Mitgelieferte Daten aktualisiert: {a}", a = l.iter().map(|(n, e)| format!("{n} ({})", e.text())).collect::<Vec<_>>().join(", ")),
                    Err(e) => tr!("Fehler: {e}", e = format!("{e:#}")),
                };
            }
        });
        for z in &self.dbs.meldungen {
            ui.monospace(z);
        }

        // --- Messwert-Texte lernen
        ui.add_space(8.0);
        ui.separator();
        ui.strong(tr!("Messwert-Texte lernen"));
        ui.weak(tr!("Aus dem Block-Export deines Werkstattprogramms (blockmap-….CSV) und den Rohwerten, die ECUWerkbank vom selben Steuergerät liest. Übernommen wird nur, was überall gleich angezeigt wurde — die Texte bleiben nur auf deinem Rechner."));
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("1. Block-Export wählen…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv", "CSV"]).pick_file() {
                    self.dbs.blockmap = Some(p);
                    self.dbs.gelernt = None;
                }
            }
            if let Some(p) = &self.dbs.blockmap {
                ui.weak(p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            }
        });
        if let Some(bm) = self.dbs.blockmap.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("2. Rohwerte:"));
                ui.label(tr!("TP-2.0-Adresse"));
                ui.add(egui::TextEdit::singleline(&mut self.dbs.adresse).desired_width(40.0));
                if ui.add_enabled(self.backend().supports_tp20(), egui::Button::new(tr!("am Auto lesen"))).on_hover_text(tr!("Liest alle Messwertblöcke dieses Steuergeräts (nur lesen, dauert etwa eine Minute)")).clicked() {
                    match parse_hex(&self.dbs.adresse).filter(|a| *a <= 0xFF) {
                        Some(a) => {
                            let be = self.backend();
                            let bm = bm.clone();
                            self.im_hintergrund(tr!("Messwertblöcke lesen"), move || {
                                let r = ecu_werkbank::kwp::blockscan(&be, a as u8, 1, 255, |_| {}).map(|v| {
                                    v.iter()
                                        .flat_map(|(b, w)| w.iter().enumerate().map(move |(i, x)| (*b, i as u8 + 1, x.formula, x.a, x.b)).collect::<Vec<_>>())
                                        .collect::<Vec<_>>()
                                });
                                Box::new(move |app: &mut App| app.dbs_lernen(&bm, r))
                            });
                        }
                        None => self.status = tr!("Adresse ungültig (hex, z. B. 20)."),
                    }
                }
                if ui.button(tr!("aus gespeicherter Lesung…")).on_hover_text(tr!("Ausgabe von „kwp-blockscan“ als Textdatei")).clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Text", &["txt"]).pick_file() {
                        let r = std::fs::read_to_string(&p).map(|t| ecu_werkbank::wertetexte::roh_aus_blockscan(&t)).map_err(anyhow::Error::from);
                        self.dbs_lernen(&bm, r);
                    }
                }
            });
        }
        if let Some(g) = &self.dbs.gelernt {
            ui.label(tr!("{a} Texte eindeutig, {b} widersprüchlich (werden nicht übernommen).", a = g.eindeutig.len(), b = g.widerspruch.len()));
            egui::ScrollArea::vertical().id_source("db_gelernt").max_height(140.0).show(ui, |ui| {
                for (k, t) in &g.eindeutig {
                    ui.monospace(format!("{k}  →  {t}"));
                }
            });
            if !g.eindeutig.is_empty() && ui.button(tr!("3. Übernehmen")).clicked() {
                let liste: Vec<(String, String)> = g.eindeutig.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                self.status = match ecu_werkbank::wertetexte::mehrere_setzen(&liste) {
                    Ok(()) => tr!("{a} Messwert-Texte übernommen.", a = liste.len()),
                    Err(e) => tr!("Fehler: {e}", e = format!("{e:#}")),
                };
                self.dbs.gelernt = None;
            }
        }

        // --- Export
        ui.add_space(8.0);
        ui.separator();
        ui.strong(tr!("Exportieren"));
        ui.weak(tr!("Alles Weitergebbare als Datenpaket — z. B. als Beitrag für andere. Daten ohne Lizenz und gekaufte Herstellerdaten bleiben draußen."));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Name:"));
            ui.add(egui::TextEdit::singleline(&mut self.dbs.export_name).desired_width(170.0));
            ui.label(tr!("Lizenz:"));
            ui.add(egui::TextEdit::singleline(&mut self.dbs.export_lizenz).desired_width(90.0));
            if ui.add_enabled(!self.dbs.export_lizenz.trim().is_empty(), egui::Button::new(tr!("💾 Exportieren…"))).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("TOML", &["toml"]).add_filter("JSON", &["json"]).set_file_name("fahrzeugdaten.toml").save_file() {
                    let (n, l) = (self.dbs.export_name.clone(), self.dbs.export_lizenz.clone());
                    self.status = match mit_db(|d| d.paket_exportieren(&n, &l)).and_then(|(paket, weg)| paket.schreiben(&p).map(|_| weg)) {
                        Ok(weg) if weg.is_empty() => tr!("Exportiert: {a}", a = p.display()),
                        Ok(weg) => tr!("Exportiert: {a} (nicht enthalten: {b})", a = p.display(), b = weg.join(", ")),
                        Err(e) => tr!("Export: {e}", e = format!("{e:#}")),
                    };
                }
            }
        });
    }

    fn dbs_lernen(&mut self, blockmap: &std::path::Path, roh: anyhow::Result<Vec<ecu_werkbank::wertetexte::Roh>>) {
        match (std::fs::read(blockmap), roh) {
            (Ok(csv), Ok(roh)) => {
                let g = ecu_werkbank::wertetexte::lernen(&roh, &ecu_werkbank::wertetexte::anzeige_aus_csv(&csv));
                self.status = tr!("{a} Texte gefunden — prüfen und übernehmen.", a = g.eindeutig.len());
                self.dbs.gelernt = Some(g);
            }
            (Err(e), _) => self.status = tr!("Block-Export lesen: {e}", e = e),
            (_, Err(e)) => self.status = tr!("Rohwerte: {e}", e = format!("{e:#}")),
        }
    }
}
