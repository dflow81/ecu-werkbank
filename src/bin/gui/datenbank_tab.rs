//! GUI: Werkzeuge → Fahrzeug-Datenbank. Übersicht, Quellen, Import
//! (Dateiart wird erkannt), mitgelieferte Daten, Messwert-Texte lernen, Export.

use super::*;
use ecu_werkbank::db::{self, eigen::mit_db};
use ecu_werkbank::db::import::Art;
use ecu_werkbank::db::inhalt::{CanBotschaft, Inhalt};
use ecu_werkbank::db::paket::{PaketAnpassung, PaketCodierung, PaketDid, PaketFehler, PaketFehlerart, PaketSteuergeraet, FEHLERART_PROTOKOLLE};

type Vorschau = Vec<(PathBuf, Result<(Art, String, Inhalt), String>)>;

/// Was gerade bearbeitet wird.
enum Bearbeitung {
    Sg { id: i64, s: PaketSteuergeraet, adresse: String, tx: String, rx: String, kennung: String, loeschen: bool },
    /// Eigene Texte eines Bereichs (fehler, messwert, codier, kennung, wert): (Schlüssel, Text), Stand beim Öffnen
    Eigene { bereich: &'static str, liste: Vec<(String, String)>, vorher: Vec<String> },
    Fehler { quelle: i64, liste: Vec<PaketFehler> },
    Fehlerarten { quelle: i64, liste: Vec<PaketFehlerart> },
    Can(CanBotschaft),
}

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
    /// Vorschau vor dem Übernehmen
    vorschau: Option<Vorschau>,
    /// gewählte Quelle zum Ansehen/Bearbeiten und ihr Inhalt
    quelle: Option<i64>,
    inhalt: Option<Inhalt>,
    filter: String,
    bearbeitung: Option<Bearbeitung>,
    teil_suche: String,
    teil_ergebnis: Vec<String>,
    fehler_suche: String,
    fehler_ergebnis: Vec<String>,
    /// Anzahl eigener Texte je Bereich (zwischengespeichert)
    eigene_anzahl: Option<Vec<usize>>,
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
            vorschau: None,
            quelle: None,
            inhalt: None,
            filter: String::new(),
            bearbeitung: None,
            teil_suche: String::new(),
            teil_ergebnis: Vec::new(),
            fehler_suche: String::new(),
            fehler_ergebnis: Vec::new(),
            eigene_anzahl: None,
        }
    }
}

impl App {
    /// Dateien prüfen: Vorschau in einer leeren Datenbank im Speicher (eigene Daten bleiben unberührt).
    pub(super) fn db_dateien_importieren(&mut self, dateien: Vec<PathBuf>) {
        if dateien.is_empty() {
            return;
        }
        let (h, l) = (self.dbs.hersteller.clone(), self.dbs.lizenz.clone());
        self.tab = Tab::Datenbank;
        self.im_hintergrund(tr!("Vorschau erstellen"), move || {
            let v: Vorschau = dateien.into_iter().map(|p| {
                let r = db::import::vorschau(&p, &h, &l).map_err(|e| format!("{e:#}"));
                (p, r)
            }).collect();
            Box::new(move |app: &mut App| {
                app.status = tr!("Vorschau fertig — prüfen und „Übernehmen“ drücken.");
                app.dbs.vorschau = Some(v);
            })
        });
    }

    /// Dateien wirklich übernehmen (nach der Vorschau).
    fn db_dateien_uebernehmen(&mut self, dateien: Vec<PathBuf>) {
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
                app.dbs.inhalt = None;
                app.reload_labels();
                app.tab = Tab::Datenbank;
            })
        });
    }

    pub(super) fn ui_datenbank(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Fahrzeug-Datenbank"));
        if self.dbs.vorschau.is_some() {
            self.ui_db_vorschau(ui);
            return;
        }
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
                if q.ist_teilbar() {
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

        // --- Inhalt ansehen und bearbeiten
        ui.add_space(8.0);
        ui.separator();
        self.ui_db_inhalt(ui, &quellen);

        // --- Eigene Texte (früher „Eigene Namen“ und „Eigene Fehlertexte“)
        ui.add_space(8.0);
        ui.separator();
        self.ui_db_eigene_texte(ui);

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
            if ui.button(tr!("🧹 Aufräumen")).on_hover_text(tr!("Anpassungskanäle ohne echten Namen („Kanal 4“) löschen — die Nummer zeigt das Programm ohnehin")).clicked() {
                self.status = match mit_db(|d| d.platzhalter_entfernen()) {
                    Ok(n) => tr!("{a} Anpassungskanäle ohne echten Namen gelöscht.", a = n),
                    Err(e) => tr!("Fehler: {e}", e = format!("{e:#}")),
                };
                self.dbs.inhalt = None;
                self.reload_labels();
            }
        });
        // --- Nachschlagen nach Teilenummer (so findet das Programm die Namen am Auto)
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("🔎 Teilenummer:"));
            let r = ui.add(egui::TextEdit::singleline(&mut self.dbs.teil_suche).desired_width(140.0).hint_text("5K0937086"));
            if ui.button(tr!("Nachschlagen")).clicked() || r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let t = self.dbs.teil_suche.clone();
                self.dbs.teil_ergebnis = match mit_db(|d| d.wissen_zur_teilenummer(&t)) {
                    Ok(None) => vec![tr!("Zu {a} steht nichts in der Datenbank.", a = t)],
                    Ok(Some(w)) => {
                        let mut z = vec![format!("{}  {}", w.zusammenfassung(), w.name)];
                        z.extend(w.anpassung.iter().map(|(k, n)| tr!("  Anpassung Kanal {a}: {b}", a = format!("{k:03}"), b = n)));
                        z.extend(w.codierung.iter().map(|c| tr!("  Codierung {a}: {b}", a = c.bit.map(|b| format!("{}.{}", c.byte, b)).unwrap_or_else(|| c.byte.to_string()), b = c.name)));
                        z.extend(w.dids.iter().map(|d| tr!("  Messwert {a}: {b} {c}", a = format!("{:#06x}", d.did), b = d.name, c = d.einheit)));
                        z.extend(w.diagnose.iter().flat_map(|d| d.zeilen()));
                        z.extend(w.fehler.iter().map(|f| tr!("  Fehler {a}: {b}", a = f.code, b = f.text)));
                        z
                    }
                    Err(e) => vec![format!("{e:#}")],
                };
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Fehlercode und optionale Fehlerart:"));
            let suchfeld = ui.add(egui::TextEdit::singleline(&mut self.dbs.fehler_suche).desired_width(140.0).hint_text("B1054 13"));
            if ui.button(tr!("Fehler suchen")).clicked() || suchfeld.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let mut teile = self.dbs.fehler_suche.split_whitespace();
                let code = teile.next().unwrap_or_default();
                let art = teile.next().map(|a| u8::from_str_radix(a.trim_start_matches("0x").trim_start_matches("0X"), 16)).transpose();
                self.dbs.fehler_ergebnis = match art {
                    Ok(art) if !code.is_empty() && teile.next().is_none() => match mit_db(|d| d.fehlerreferenz(code, art)) {
                        Ok(treffer) => treffer.zeilen(),
                        Err(e) => vec![format!("{e:#}")],
                    },
                    _ => vec![tr!("Bitte einen Fehlercode und optional eine hexadezimale Fehlerart eingeben.")],
                };
            }
        });
        if !self.dbs.fehler_ergebnis.is_empty() {
            egui::ScrollArea::vertical().id_source("fehlerreferenz").max_height(160.0).show(ui, |ui| {
                for zeile in &self.dbs.fehler_ergebnis {
                    ui.monospace(zeile);
                }
            });
        }
        if !self.dbs.teil_ergebnis.is_empty() {
            egui::ScrollArea::vertical().id_source("teil_ergebnis").max_height(160.0).show(ui, |ui| {
                for z in &self.dbs.teil_ergebnis {
                    ui.monospace(z);
                }
            });
        }
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

fn ui_diagnosereferenz(ui: &mut egui::Ui, s: &PaketSteuergeraet) {
    if !s.diagnosereferenz.ist_leer() {
        ui.collapsing(tr!("Diagnosereferenzen ({a})", a = s.diagnosereferenz.anzahl()), |ui| {
            for zeile in s.diagnosereferenz.zeilen() {
                ui.label(zeile);
            }
        });
    }
}

/// Inhalt einer Quelle als Baum anzeigen (nur lesen) — für die Vorschau.
fn inhalt_baum(ui: &mut egui::Ui, i: &Inhalt, schluessel: &str) {
    let lokal = !i.quelle.ist_teilbar();
    ui.label(tr!(
        "Wird eingetragen als Quelle „{a}“ (Art {b}, Lizenz {c}){d}",
        a = i.quelle.name,
        b = i.quelle.art.text(),
        c = if i.quelle.lizenz.is_empty() { "–".to_string() } else { i.quelle.lizenz.clone() },
        d = if lokal { tr!(" — bleibt nur bei dir") } else { String::new() }
    ));
    for (id, h, s) in &i.steuergeraete {
        let a = s.adresse.map(|a| format!("{a:02X}")).unwrap_or_else(|| "—".into());
        egui::CollapsingHeader::new(tr!("{h} › Adresse {a} › {n}  ({f} Fehler, {d} Kennungen, {c} Codier-Bits, {p} Anpassungen)", h = h, a = a, n = s.name, f = s.fehler.len(), d = s.dids.len(), c = s.codierung.len(), p = s.anpassung.len()))
            .id_source((schluessel, "sg", id))
            .show(ui, |ui| {
                if !s.protokoll.is_empty() || s.tx.is_some() {
                    ui.weak(tr!("Protokoll {p}, Anfrage {t}, Antwort {r}, Gruppe {g}", p = s.protokoll, t = s.tx.map(|x| format!("{x:#05x}")).unwrap_or_else(|| "–".into()), r = s.rx.map(|x| format!("{x:#05x}")).unwrap_or_else(|| "–".into()), g = s.gruppe));
                }
                for f in &s.fehler {
                    ui.monospace(tr!("Fehler  {c}  {t}", c = f.code, t = f.text));
                }
                for d in &s.dids {
                    ui.monospace(tr!("Kennung {a}  {n} [{e}] ×{f} +{o}", a = format!("{:#06X}", d.did), n = d.name, e = d.einheit, f = d.faktor, o = d.offset));
                }
                ui_diagnosereferenz(ui, s);
                for c in &s.codierung {
                    let bit = c.bit.map(|b| format!(" Bit {b}")).unwrap_or_default();
                    ui.monospace(tr!("Codierung Byte {b}{bit}  {n}", b = c.byte, bit = bit, n = c.name));
                }
                for a in &s.anpassung {
                    ui.monospace(tr!("Anpassung Kanal {k}  {n}", k = a.kanal, n = a.name));
                }
            });
    }
    if !i.fehler.is_empty() {
        egui::CollapsingHeader::new(tr!("Allgemeine Fehlertexte ({a})", a = i.fehler.len())).id_source((schluessel, "fehler")).show(ui, |ui| {
            for f in i.fehler.iter().take(500) {
                ui.monospace(format!("{}  {}", f.code, f.text));
            }
        });
    }
    if !i.fehlerarten.is_empty() {
        egui::CollapsingHeader::new(tr!("Fehlerarten ({a})", a = i.fehlerarten.len())).id_source((schluessel, "fehlerart")).show(ui, |ui| {
            for f in &i.fehlerarten {
                ui.monospace(format!("{:<6} {:>3}  {}", f.protokoll, f.nr, f.text));
            }
        });
    }
    if !i.can.is_empty() {
        egui::CollapsingHeader::new(tr!("CAN-Botschaften ({a})", a = i.can.len())).id_source((schluessel, "can")).show(ui, |ui| {
            for b in i.can.iter().take(300) {
                ui.collapsing(format!("{:03X}  {}  ({} Signale)", b.can_id, b.name, b.signale.len()), |ui| {
                    for g in &b.signale {
                        ui.monospace(tr!("{n}: Bit {s}, Länge {l}, ×{f} +{o} {e}", n = g.name, s = g.start, l = g.laenge, f = g.faktor, o = g.offset, e = g.einheit));
                    }
                });
            }
        });
    }
}

/// Überschrift und Hinweis zu einem Bereich der eigenen Texte.
fn eigene_bereich_text(bereich: &str) -> (String, String) {
    match bereich {
        "fehler" => (tr!("Eigene Fehlertexte"), tr!("Code, z. B. 01627 oder P0420 — gilt vor allen anderen Texten, auch im PDF.")),
        "messwert" => (tr!("Eigene Messwert-Namen (TP 2.0)"), tr!("Schlüssel Adresse-Block oder Adresse-Block-Feld, z. B. 01-002 oder 01-002-3. Am schnellsten mit ✏ am Messwertblock im TP-2.0-Reiter.")),
        "codier" => (tr!("Eigene Codier-Texte"), tr!("Schlüssel CAN-ID-Byte.Bit, z. B. 220-04.3 (TP 2.0: 200 + Adresse). Am schnellsten auf der Seite Codierung.")),
        "kennung" => (tr!("Eigene Kennungs-Namen (UDS)"), tr!("Schlüssel Gerät-Kennung, z. B. 714-2203.")),
        _ => (tr!("Eigene Wert-Texte"), tr!("Texte zu Textnummern bei Messwerten (Formel F25), meist gelernt.")),
    }
}

/// Bearbeitung eines Steuergeräts vorbereiten (Hex-Felder als Text).
fn sg_bearbeitung(id: i64, s: PaketSteuergeraet) -> Bearbeitung {
    let h = |v: Option<u16>| v.map(|x| format!("{x:#04x}")).unwrap_or_default();
    Bearbeitung::Sg { id, adresse: h(s.adresse), tx: h(s.tx), rx: h(s.rx), kennung: s.codier_kennung.map(|x| format!("{x:#06x}")).unwrap_or_default(), s, loeschen: false }
}

fn hex_opt(s: &str) -> Result<Option<u16>, ()> {
    if s.trim().is_empty() {
        Ok(None)
    } else {
        parse_hex(s).map(Some).ok_or(())
    }
}

impl App {
    /// Abschnitt „Eigene Texte“: alles, was du selbst benannt hast, an einer Stelle.
    fn ui_db_eigene_texte(&mut self, ui: &mut egui::Ui) {
        ui.strong(tr!("Eigene Texte"));
        ui.weak(tr!("Was du selbst benannt hast: Fehlertexte, Namen von Messwertblöcken, Codier-Bits und Kennungen. Gilt vor allen anderen Quellen."));
        const BEREICHE: [&str; 5] = ["fehler", "messwert", "codier", "kennung", "wert"];
        let anzahl = self.dbs.eigene_anzahl.get_or_insert_with(|| BEREICHE.iter().map(|b| ecu_werkbank::db::eigen::alle(b).len()).collect()).clone();
        ui.horizontal_wrapped(|ui| {
            for (b, n) in BEREICHE.into_iter().zip(anzahl) {
                let aktiv = matches!(&self.dbs.bearbeitung, Some(Bearbeitung::Eigene { bereich, .. }) if *bereich == b);
                if ui.selectable_label(aktiv, format!("{} ({n})", eigene_bereich_text(b).0)).clicked() {
                    self.dbs_eigene_texte_oeffnen(b);
                }
            }
        });
        if matches!(self.dbs.bearbeitung, Some(Bearbeitung::Eigene { .. })) {
            ui.add_space(4.0);
            self.ui_db_bearbeiten(ui);
        }
    }

    /// Eigene Texte eines Bereichs zum Bearbeiten öffnen.
    pub(super) fn dbs_eigene_texte_oeffnen(&mut self, bereich: &'static str) {
        let liste: Vec<(String, String)> = ecu_werkbank::db::eigen::alle(bereich).into_iter().collect();
        let vorher = liste.iter().map(|(k, _)| k.clone()).collect();
        self.dbs.bearbeitung = Some(Bearbeitung::Eigene { bereich, liste, vorher });
    }

    /// Zähler der eigenen Texte neu bestimmen lassen (nach Änderungen anderswo).
    pub(super) fn dbs_zaehler_neu(&mut self) {
        self.dbs.eigene_anzahl = None;
    }

    /// Quelle für „Inhalt ansehen und bearbeiten“ wählen.
    pub(super) fn dbs_quelle_waehlen(&mut self, id: Option<i64>) {
        self.dbs.quelle = id;
        self.dbs.inhalt = None;
        self.dbs.bearbeitung = None;
    }

    /// Bildschirmfoto: erste Quelle außer den eigenen Labels öffnen, erstes Steuergerät bearbeiten.
    pub(super) fn db_demo_inhalt(&mut self) {
        let q = mit_db(|d| d.quellen()).unwrap_or_default().into_iter().find(|q| q.name != db::labels::QUELLE);
        if let Some(q) = q {
            self.dbs.quelle = Some(q.id);
            if let Ok(i) = mit_db(|d| d.inhalt(q.id)) {
                if let Some((id, _, s)) = i.steuergeraete.first().cloned() {
                    self.dbs.bearbeitung = Some(sg_bearbeitung(id, s));
                }
                self.dbs.inhalt = Some(i);
            }
        }
    }

    /// Vorschau vor dem Übernehmen.
    fn ui_db_vorschau(&mut self, ui: &mut egui::Ui) {
        let Some(v) = &self.dbs.vorschau else { return };
        ui.strong(tr!("Vorschau — so würden die Daten eingetragen. Noch ist nichts übernommen."));
        ui.weak(tr!("Bitte prüfen: Hersteller, Adresse und Name der Steuergeräte, Zuordnung der Fehler und Kennungen."));
        let mut gut = Vec::new();
        egui::ScrollArea::vertical().id_source("db_vorschau").max_height(420.0).show(ui, |ui| {
            for (n, (p, r)) in v.iter().enumerate() {
                ui.add_space(4.0);
                let name = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                match r {
                    Ok((art, text, inhalt)) => {
                        ui.strong(format!("📄 {name} — {}: {text}", art.text()));
                        inhalt_baum(ui, inhalt, &format!("v{n}"));
                        gut.push(p.clone());
                    }
                    Err(e) => {
                        ui.colored_label(stufe_color(Stufe::Warnung), format!("✗ {name}: {e}"));
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!gut.is_empty(), egui::Button::new(tr!("✅ Übernehmen ({a} Datei(en))", a = gut.len()))).clicked() {
                self.dbs.vorschau = None;
                self.db_dateien_uebernehmen(gut.clone());
            }
            if ui.button(tr!("✖ Verwerfen")).clicked() {
                self.dbs.vorschau = None;
                self.status = tr!("Nichts übernommen.");
            }
        });
    }

    /// Inhalt einer Quelle ansehen und bearbeiten.
    fn ui_db_inhalt(&mut self, ui: &mut egui::Ui, quellen: &[db::Quelle]) {
        ui.strong(tr!("Inhalt ansehen und bearbeiten"));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Quelle:"));
            let aktuell = self.dbs.quelle.and_then(|id| quellen.iter().find(|q| q.id == id)).map(|q| format!("{} ({})", q.name, q.art.text())).unwrap_or_else(|| tr!("— wählen —"));
            let mut wahl = None;
            egui::ComboBox::from_id_source("db_quelle").selected_text(aktuell).width(260.0).show_ui(ui, |ui| {
                for q in quellen {
                    if ui.selectable_label(self.dbs.quelle == Some(q.id), format!("{} ({})", q.name, q.art.text())).clicked() {
                        wahl = Some(q.id);
                    }
                }
            });
            if let Some(id) = wahl {
                self.dbs.quelle = Some(id);
                self.dbs.inhalt = None;
                self.dbs.bearbeitung = None;
            }
            ui.label(tr!("Suchen:"));
            ui.add(egui::TextEdit::singleline(&mut self.dbs.filter).desired_width(120.0).hint_text(tr!("Name, Code, ID")));
        });
        let Some(qid) = self.dbs.quelle else { return };
        if self.dbs.inhalt.as_ref().map(|i| i.quelle.id) != Some(qid) {
            match mit_db(|d| d.inhalt(qid)) {
                Ok(i) => self.dbs.inhalt = Some(i),
                Err(e) => {
                    self.status = format!("{e:#}");
                    self.dbs.quelle = None;
                    return;
                }
            }
        }
        let Some(inhalt) = self.dbs.inhalt.clone() else { return };
        let ist_labels = inhalt.quelle.name == db::labels::QUELLE && inhalt.quelle.art == db::QuellArt::User;
        ui.weak(inhalt.zusammenfassung());
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("+ Steuergerät")).on_hover_text(tr!("Neues, leeres Steuergerät in dieser Quelle anlegen")).clicked() {
                match mit_db(|d| d.sg_neu(qid)) {
                    Ok(id) => {
                        self.dbs.inhalt = None;
                        if let Ok((_, s)) = mit_db(|d| d.sg_laden(id)) {
                            self.dbs.bearbeitung = Some(sg_bearbeitung(id, s));
                        }
                        self.status = tr!("Neues Steuergerät angelegt — ausfüllen und speichern.");
                    }
                    Err(e) => self.status = format!("{e:#}"),
                }
            }
            if ist_labels
                && ui
                    .button(tr!("⬇ Aus letztem Scan anlegen"))
                    .on_hover_text(tr!("Legt für jedes Steuergerät aus dem letzten Scan des aktiven Fahrzeugs einen vorbefüllten Eintrag an (Adresse, Name, Teilenummer, CAN-IDs) — nur wo noch keiner existiert. Überschreibt nichts."))
                    .clicked()
            {
                self.labels_aus_scan();
                self.dbs.inhalt = None;
            }
        });
        let f = self.dbs.filter.to_lowercase();
        let passt = |t: &str| f.is_empty() || t.to_lowercase().contains(&f);
        ui.columns(2, |sp| {
            // links: Liste
            egui::ScrollArea::vertical().id_source("db_liste").max_height(380.0).show(&mut sp[0], |ui| {
                for (id, _, s) in &inhalt.steuergeraete {
                    let t = format!("{}  {}", s.adresse.map(|a| format!("{a:02X}")).unwrap_or_else(|| "—".into()), s.name);
                    if !passt(&t) && !s.fehler.iter().any(|x| passt(&x.code) || passt(&x.text)) {
                        continue;
                    }
                    let aktiv = matches!(&self.dbs.bearbeitung, Some(Bearbeitung::Sg { id: i, .. }) if i == id);
                    if ui.selectable_label(aktiv, format!("🔧 {t}")).clicked() {
                        self.dbs.bearbeitung = Some(sg_bearbeitung(*id, s.clone()));
                    }
                }
                if !inhalt.fehler.is_empty() || !ist_labels && inhalt.steuergeraete.is_empty() && inhalt.can.is_empty() {
                    let aktiv = matches!(&self.dbs.bearbeitung, Some(Bearbeitung::Fehler { .. }));
                    if ui.selectable_label(aktiv, tr!("⚠ Allgemeine Fehlertexte ({a})", a = inhalt.fehler.len())).clicked() {
                        self.dbs.bearbeitung = Some(Bearbeitung::Fehler { quelle: qid, liste: inhalt.fehler.clone() });
                    }
                }
                if !ist_labels && (!inhalt.fehlerarten.is_empty() || inhalt.steuergeraete.is_empty() && inhalt.can.is_empty()) {
                    let aktiv = matches!(&self.dbs.bearbeitung, Some(Bearbeitung::Fehlerarten { .. }));
                    if ui.selectable_label(aktiv, tr!("🔧 Fehlerarten ({a})", a = inhalt.fehlerarten.len())).clicked() {
                        self.dbs.bearbeitung = Some(Bearbeitung::Fehlerarten { quelle: qid, liste: inhalt.fehlerarten.clone() });
                    }
                }
                for b in &inhalt.can {
                    let t = format!("{:03X}  {}", b.can_id, b.name);
                    if !passt(&t) && !b.signale.iter().any(|g| passt(&g.name)) {
                        continue;
                    }
                    let aktiv = matches!(&self.dbs.bearbeitung, Some(Bearbeitung::Can(c)) if c.id == b.id);
                    if ui.selectable_label(aktiv, format!("📶 {t}")).clicked() {
                        self.dbs.bearbeitung = Some(Bearbeitung::Can(b.clone()));
                    }
                }
            });
            // rechts: Bearbeiten
            egui::ScrollArea::vertical().id_source("db_bearbeiten").max_height(380.0).show(&mut sp[1], |ui| {
                if matches!(self.dbs.bearbeitung, Some(Bearbeitung::Eigene { .. })) {
                    ui.weak(tr!("Links einen Eintrag wählen."));
                } else {
                    self.ui_db_bearbeiten(ui);
                }
            });
        });
    }

    fn ui_db_bearbeiten(&mut self, ui: &mut egui::Ui) {
        let mut speichern = false;
        let mut verwerfen = false;
        let mut loeschen = false;
        match &mut self.dbs.bearbeitung {
            None => {
                ui.weak(tr!("Links einen Eintrag wählen."));
            }
            Some(Bearbeitung::Sg { s, adresse, tx, rx, kennung, loeschen: frage, .. }) => {
                egui::Grid::new("db_sg_kopf").num_columns(2).show(ui, |ui| {
                    ui.label(tr!("Name"));
                    ui.text_edit_singleline(&mut s.name);
                    ui.end_row();
                    ui.label(tr!("Adresse"));
                    ui.add(egui::TextEdit::singleline(adresse).desired_width(60.0).hint_text("0x09"));
                    ui.end_row();
                    ui.label(tr!("Protokoll"));
                    ui.add(egui::TextEdit::singleline(&mut s.protokoll).desired_width(80.0).hint_text("UDS / TP20"));
                    ui.end_row();
                    ui.label(tr!("Anfrage / Antwort"));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(tx).desired_width(60.0).hint_text("0x70E"));
                        ui.add(egui::TextEdit::singleline(rx).desired_width(60.0).hint_text("0x778"));
                    });
                    ui.end_row();
                    ui.label(tr!("Gruppe"));
                    ui.text_edit_singleline(&mut s.gruppe);
                    ui.end_row();
                    ui.label(tr!("Teilenummer"));
                    ui.text_edit_singleline(&mut s.teilenummer);
                    ui.end_row();
                    ui.label(tr!("Codier-Kennung"));
                    ui.add(egui::TextEdit::singleline(kennung).desired_width(60.0).hint_text("0x0600")).on_hover_text(tr!("Nur nötig, wenn die Codierung nicht unter 0x0600 steht"));
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui_diagnosereferenz(ui, s);
                ui.strong(tr!("Fehler ({a})", a = s.fehler.len()));
                let mut weg = None;
                for (i, f) in s.fehler.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut f.code).desired_width(70.0));
                        ui.add(egui::TextEdit::singleline(&mut f.text).desired_width(220.0));
                        if ui.small_button("🗑").clicked() {
                            weg = Some(i);
                        }
                    });
                }
                if let Some(i) = weg {
                    s.fehler.remove(i);
                }
                if ui.small_button(tr!("+ Fehler")).clicked() {
                    s.fehler.push(PaketFehler::default());
                }
                ui.add_space(4.0);
                ui.strong(tr!("Kennungen / Messwerte ({a})", a = s.dids.len()));
                let mut weg = None;
                for (i, d) in s.dids.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut d.did).hexadecimal(4, false, true));
                        ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(140.0));
                        ui.add(egui::TextEdit::singleline(&mut d.einheit).desired_width(50.0));
                        ui.add(egui::DragValue::new(&mut d.faktor).speed(0.01).prefix("×"));
                        ui.add(egui::DragValue::new(&mut d.offset).speed(0.1).prefix("+"));
                        if ui.small_button("🗑").clicked() {
                            weg = Some(i);
                        }
                    });
                }
                if let Some(i) = weg {
                    s.dids.remove(i);
                }
                if ui.small_button(tr!("+ Kennung")).clicked() {
                    s.dids.push(PaketDid { did: 0, name: String::new(), einheit: String::new(), faktor: 1.0, offset: 0.0 });
                }
                ui.add_space(4.0);
                ui.strong(tr!("Codier-Bits ({a})", a = s.codierung.len()));
                let mut weg = None;
                for (i, c) in s.codierung.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut c.byte).prefix(tr!("Byte ")));
                        let mut b = c.bit.map(|x| x.to_string()).unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut b).desired_width(24.0).hint_text("Bit")).changed() {
                            c.bit = b.trim().parse().ok().filter(|x: &u8| *x < 8);
                        }
                        ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(150.0));
                        let mut ben = c.benoetigt.join(",");
                        if ui.add(egui::TextEdit::singleline(&mut ben).desired_width(60.0).hint_text(tr!("braucht"))).changed() {
                            c.benoetigt = ben.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect();
                        }
                        let mut aus = c.schliesst_aus.join(",");
                        if ui.add(egui::TextEdit::singleline(&mut aus).desired_width(60.0).hint_text(tr!("nicht mit"))).changed() {
                            c.schliesst_aus = aus.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect();
                        }
                        if ui.small_button("🗑").clicked() {
                            weg = Some(i);
                        }
                    });
                }
                if let Some(i) = weg {
                    s.codierung.remove(i);
                }
                if ui.small_button(tr!("+ Codier-Bit")).clicked() {
                    s.codierung.push(PaketCodierung { byte: 0, bit: Some(0), name: String::new(), benoetigt: Vec::new(), schliesst_aus: Vec::new() });
                }
                ui.add_space(4.0);
                ui.strong(tr!("Anpassungskanäle ({a})", a = s.anpassung.len()));
                let mut weg = None;
                for (i, a) in s.anpassung.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut a.kanal).prefix(tr!("Kanal ")));
                        ui.add(egui::TextEdit::singleline(&mut a.name).desired_width(200.0));
                        if ui.small_button("🗑").clicked() {
                            weg = Some(i);
                        }
                    });
                }
                if let Some(i) = weg {
                    s.anpassung.remove(i);
                }
                if ui.small_button(tr!("+ Anpassungskanal")).clicked() {
                    s.anpassung.push(PaketAnpassung { kanal: 1, name: String::new() });
                }
                ui.add_space(4.0);
                ui.strong(tr!("Sollwerte ({a})", a = s.sollwert.len())).on_hover_text(tr!("Grenzen für die Ampel bei Messwerten: Quelle kwp (Block), uds (Kennung) oder obd (PID); Feld nur bei kwp"));
                let mut weg = None;
                for (i, w) in s.sollwert.iter_mut().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        egui::ComboBox::from_id_source(("sw_quelle", i)).width(56.0).selected_text(w.quelle.clone()).show_ui(ui, |ui| {
                            for q in ["kwp", "uds", "obd"] {
                                ui.selectable_value(&mut w.quelle, q.to_string(), q);
                            }
                        });
                        ui.add(egui::DragValue::new(&mut w.id).prefix(tr!("Nr. ")));
                        let mut feld = w.feld.map(|f| f.to_string()).unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut feld).desired_width(24.0).hint_text(tr!("Feld"))).changed() {
                            w.feld = feld.trim().parse().ok();
                        }
                        let mut min = w.min.map(|x| x.to_string()).unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut min).desired_width(50.0).hint_text("min")).changed() {
                            w.min = min.trim().replace(',', ".").parse().ok();
                        }
                        let mut max = w.max.map(|x| x.to_string()).unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut max).desired_width(50.0).hint_text("max")).changed() {
                            w.max = max.trim().replace(',', ".").parse().ok();
                        }
                        ui.add(egui::TextEdit::singleline(&mut w.name).desired_width(120.0).hint_text(tr!("Name")));
                        if ui.small_button("🗑").clicked() {
                            weg = Some(i);
                        }
                    });
                }
                if let Some(i) = weg {
                    s.sollwert.remove(i);
                }
                if ui.small_button(tr!("+ Sollwert")).clicked() {
                    s.sollwert.push(ecu_werkbank::labels::Sollwert { quelle: "kwp".into(), id: 1, feld: Some(1), min: None, max: None, name: String::new() });
                }
                if !s.stellfunktion.is_empty() {
                    ui.add_space(4.0);
                    ui.strong(tr!("Stellfunktionen ({a})", a = s.stellfunktion.len()));
                    for f in &s.stellfunktion {
                        ui.weak(tr!("{a} — {b}, Kennung {c}, {d}–{e} {f} (bleibt beim Speichern erhalten; ändern über ein Datenpaket)", a = f.name, b = f.art, c = format!("{:#06x}", f.id), d = f.min, e = f.max, f = f.einheit));
                    }
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    speichern = ui.button(tr!("💾 Speichern")).clicked();
                    verwerfen = ui.button(tr!("↺ Verwerfen")).clicked();
                    if !*frage {
                        if ui.button(tr!("🗑 Steuergerät löschen…")).clicked() {
                            *frage = true;
                        }
                    } else {
                        ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Wirklich löschen?"));
                        loeschen = ui.button(tr!("Ja, löschen")).clicked();
                        if ui.button(tr!("Abbrechen")).clicked() {
                            *frage = false;
                        }
                    }
                });
            }
            Some(Bearbeitung::Eigene { bereich, liste, .. }) => {
                let (titel, hinweis) = eigene_bereich_text(bereich);
                ui.strong(tr!("{a} ({b})", a = titel, b = liste.len()));
                ui.weak(hinweis);
                let mut weg = None;
                egui::ScrollArea::vertical().id_source("eigene_texte").max_height(320.0).show(ui, |ui| {
                    for (i, (k, t)) in liste.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(k).desired_width(90.0).font(egui::TextStyle::Monospace));
                            ui.add(egui::TextEdit::singleline(t).desired_width(320.0));
                            if ui.small_button("🗑").clicked() {
                                weg = Some(i);
                            }
                        });
                    }
                });
                if let Some(i) = weg {
                    liste.remove(i);
                }
                if ui.small_button(tr!("+ Eintrag")).clicked() {
                    liste.push((String::new(), String::new()));
                }
                ui.horizontal(|ui| {
                    speichern = ui.button(tr!("💾 Speichern")).clicked();
                    verwerfen = ui.button(tr!("↺ Verwerfen")).clicked();
                });
            }
            Some(Bearbeitung::Fehler { liste, .. }) => {
                ui.strong(tr!("Allgemeine Fehlertexte ({a})", a = liste.len()));
                let mut weg = None;
                for (i, f) in liste.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut f.code).desired_width(70.0));
                        ui.add(egui::TextEdit::singleline(&mut f.text).desired_width(240.0));
                        if ui.small_button("🗑").clicked() {
                            weg = Some(i);
                        }
                    });
                }
                if let Some(i) = weg {
                    liste.remove(i);
                }
                if ui.small_button(tr!("+ Fehlertext")).clicked() {
                    liste.push(PaketFehler::default());
                }
                ui.horizontal(|ui| {
                    speichern = ui.button(tr!("💾 Speichern")).clicked();
                    verwerfen = ui.button(tr!("↺ Verwerfen")).clicked();
                });
            }
            Some(Bearbeitung::Fehlerarten { liste, .. }) => {
                ui.strong(tr!("Fehlerarten ({a})", a = liste.len()));
                ui.weak(tr!("TP20: Nummer 0–15 aus dem Statusbyte (ältere Steuergeräte); UDS: 3. Byte des Fehlers; KW1281: alte K-Line-Steuergeräte. Eigene Texte haben Vorrang vor den eingebauten."));
                let mut weg = None;
                for (i, f) in liste.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_source(("fa_prot", i)).width(70.0).selected_text(f.protokoll.clone()).show_ui(ui, |ui| {
                            for p in FEHLERART_PROTOKOLLE {
                                ui.selectable_value(&mut f.protokoll, p.to_string(), *p);
                            }
                        });
                        ui.add(egui::DragValue::new(&mut f.nr).range(0..=255).prefix(tr!("Nr. ")));
                        ui.add(egui::TextEdit::singleline(&mut f.text).desired_width(240.0));
                        if ui.small_button("🗑").clicked() {
                            weg = Some(i);
                        }
                    });
                }
                if let Some(i) = weg {
                    liste.remove(i);
                }
                if ui.small_button(tr!("+ Fehlerart")).clicked() {
                    liste.push(PaketFehlerart { protokoll: "TP20".into(), ..Default::default() });
                }
                ui.horizontal(|ui| {
                    speichern = ui.button(tr!("💾 Speichern")).clicked();
                    verwerfen = ui.button(tr!("↺ Verwerfen")).clicked();
                });
            }
            Some(Bearbeitung::Can(b)) => {
                ui.horizontal(|ui| {
                    ui.label(format!("{:03X}", b.can_id));
                    ui.text_edit_singleline(&mut b.name);
                    ui.add(egui::DragValue::new(&mut b.dlc).range(0..=64).prefix("DLC "));
                });
                ui.strong(tr!("Signale ({a})", a = b.signale.len()));
                let mut weg = None;
                for (i, g) in b.signale.iter_mut().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut g.name).desired_width(130.0));
                        ui.add(egui::DragValue::new(&mut g.start).prefix(tr!("Bit ")));
                        ui.add(egui::DragValue::new(&mut g.laenge).range(1..=64).prefix(tr!("Länge ")));
                        ui.checkbox(&mut g.intel, "Intel");
                        ui.checkbox(&mut g.vorzeichen, "±");
                        ui.add(egui::DragValue::new(&mut g.faktor).speed(0.001).prefix("×"));
                        ui.add(egui::DragValue::new(&mut g.offset).speed(0.1).prefix("+"));
                        ui.add(egui::TextEdit::singleline(&mut g.einheit).desired_width(50.0));
                        if ui.small_button("🗑").clicked() {
                            weg = Some(i);
                        }
                    });
                }
                if let Some(i) = weg {
                    b.signale.remove(i);
                }
                ui.horizontal(|ui| {
                    speichern = ui.button(tr!("💾 Speichern")).clicked();
                    verwerfen = ui.button(tr!("↺ Verwerfen")).clicked();
                });
            }
        }
        if verwerfen {
            self.dbs.bearbeitung = None;
        }
        if loeschen {
            if let Some(Bearbeitung::Sg { id, .. }) = &self.dbs.bearbeitung {
                let id = *id;
                self.status = match mit_db(|d| d.sg_entfernen(id)) {
                    Ok(_) => tr!("Steuergerät gelöscht."),
                    Err(e) => format!("{e:#}"),
                };
                self.dbs.bearbeitung = None;
                self.dbs.inhalt = None;
                self.reload_labels();
            }
        }
        if speichern {
            let r = match &mut self.dbs.bearbeitung {
                Some(Bearbeitung::Sg { id, s, adresse, tx, rx, kennung, .. }) => match (hex_opt(adresse), hex_opt(tx), hex_opt(rx), hex_opt(kennung)) {
                    (Ok(a), Ok(t), Ok(r), Ok(k)) => {
                        s.adresse = a;
                        s.tx = t;
                        s.rx = r;
                        s.codier_kennung = k;
                        let (id, s) = (*id, s.clone());
                        mit_db(|d| d.sg_speichern(id, &s))
                    }
                    _ => Err(anyhow::anyhow!(tr!("Adresse/Anfrage/Antwort/Codier-Kennung ungültig (hex, z. B. 0x09)."))),
                },
                Some(Bearbeitung::Eigene { bereich, liste, vorher }) => {
                    let bereich: &'static str = bereich;
                    let norm = |k: &str| if bereich == "fehler" { ecu_werkbank::fehlertexte::norm(k) } else { k.trim().to_string() };
                    let neu: Vec<(String, String)> = liste.iter().map(|(k, t)| (norm(k), t.trim().to_string())).filter(|(k, t)| !k.is_empty() && !t.is_empty()).collect();
                    // weggefallene Schlüssel löschen (leerer Text = löschen)
                    let mut aenderung: Vec<(String, String)> = vorher.iter().filter(|k| !neu.iter().any(|(n, _)| n == *k)).map(|k| (k.clone(), String::new())).collect();
                    aenderung.extend(neu);
                    let r = ecu_werkbank::db::eigen::setzen(bereich, &aenderung);
                    if r.is_ok() {
                        let l = ecu_werkbank::db::eigen::alle(bereich).into_iter().collect::<Vec<_>>();
                        *vorher = l.iter().map(|(k, _)| k.clone()).collect();
                        *liste = l;
                        self.dbs.eigene_anzahl = None;
                        self.mw_texte_neu_laden();
                        self.code_texte = ecu_werkbank::codiertexte::Texte::laden();
                    }
                    r
                }
                Some(Bearbeitung::Fehler { quelle, liste }) => {
                    let (q, l) = (*quelle, liste.clone());
                    mit_db(|d| d.fehler_allgemein_speichern(q, &l))
                }
                Some(Bearbeitung::Fehlerarten { quelle, liste }) => {
                    let (q, l) = (*quelle, liste.clone());
                    mit_db(|d| d.fehlerarten_speichern(q, &l))
                }
                Some(Bearbeitung::Can(b)) => {
                    let b = b.clone();
                    mit_db(|d| d.can_speichern(&b))
                }
                None => Ok(()),
            };
            self.status = match r {
                Ok(()) => tr!("Gespeichert (Fahrzeug-Datenbank)."),
                Err(e) => tr!("Speichern fehlgeschlagen: {e}", e = format!("{e:#}")),
            };
            self.dbs.inhalt = None;
            self.reload_labels();
        }
    }
}
