//! GUI: Label-Editor

use super::*;

impl App {
    /// Eigene Labels aus der Datenbank: (id, Anzeige „09  Bordnetz“).
    pub(super) fn label_liste(&self) -> Vec<(i64, String)> {
        ecu_werkbank::db::eigen::mit_db(|db| db.eigene_labels())
            .unwrap_or_default()
            .into_iter()
            .map(|(id, l)| {
                let a = l.ecu.address.map(|a| format!("{a:02X}")).unwrap_or_else(|| "—".into());
                let n = if l.ecu.name.is_empty() { tr!("(ohne Namen)") } else { l.ecu.name.clone() };
                (id, format!("{a}  {n}"))
            })
            .collect()
    }

    fn ed_anzeigen(&mut self, id: Option<i64>, f: LabelFile) {
        let h = |v: Option<u16>| v.map(|x| format!("{x:#04x}")).unwrap_or_default();
        self.ed_addr = h(f.ecu.address);
        self.ed_tx = h(f.ecu.tx);
        self.ed_rx = h(f.ecu.rx);
        self.ed_file = f;
        self.ed_id = Some(id);
        self.ed_loeschen = false;
    }

    /// Eintrag aus der Datenbank öffnen.
    pub(super) fn ed_oeffnen(&mut self, id: i64) {
        match ecu_werkbank::db::eigen::mit_db(|db| db.eigene_labels()).map(|l| l.into_iter().find(|(i, _)| *i == id)) {
            Ok(Some((id, f))) => self.ed_anzeigen(Some(id), f),
            Ok(None) => self.status = tr!("Eintrag nicht mehr vorhanden."),
            Err(e) => self.status = tr!("Fehler: {e}", e = format!("{:#}", e)),
        }
    }

    /// Label-Datei (z. B. hineingezogen) als neuen, noch nicht gespeicherten Eintrag öffnen.
    pub(super) fn ed_open(&mut self, path: PathBuf) {
        match LabelFile::load(&path) {
            Ok(f) => {
                self.ed_anzeigen(None, f);
                self.status = tr!("{a} geöffnet — mit „Speichern“ in die Datenbank übernehmen.", a = path.display());
            }
            Err(e) => self.status = tr!("Fehler: {e}", e = format!("{:#}", e)),
        }
    }

    pub(super) fn ed_save(&mut self) {
        let Some(id) = self.ed_id else { return };
        let opt = |s: &str| -> Result<Option<u16>, ()> {
            if s.trim().is_empty() { Ok(None) } else { parse_hex(s).map(Some).ok_or(()) }
        };
        let (Ok(a), Ok(t), Ok(r)) = (opt(&self.ed_addr), opt(&self.ed_tx), opt(&self.ed_rx)) else {
            self.status = tr!("Adresse/TX/RX ungültig (z. B. 0x01, 0x7E0).");
            return;
        };
        self.ed_file.ecu.address = a;
        self.ed_file.ecu.tx = t;
        self.ed_file.ecu.rx = r;
        let f = self.ed_file.clone();
        match ecu_werkbank::db::eigen::mit_db(|db| db.label_speichern(id, &f)) {
            Ok(neu) => {
                self.ed_id = Some(Some(neu));
                self.reload_labels();
                self.status = tr!("Gespeichert (Fahrzeug-Datenbank).");
            }
            Err(e) => self.status = tr!("Speichern fehlgeschlagen: {e}", e = format!("{:#}", e)),
        }
    }

    /// Für jedes Steuergerät aus dem letzten Scan des aktiven Fahrzeugs eine
    /// vorbefüllte Label-Datei anlegen — nur dort, wo es noch keine gibt.
    /// Überschreibt nie etwas; Inhalt stammt allein aus dem eigenen Scan.
    pub(super) fn ed_aus_scan(&mut self) {
        let Some(key) = self.active_vehicle.clone() else {
            self.status = tr!("Kein Fahrzeug gewählt — oben ein Fahrzeug wählen und einen Scan machen.");
            return;
        };
        let Some((_, last)) = ecu_werkbank::history::list(&key).pop() else {
            self.status = tr!("Noch kein Scan für dieses Fahrzeug gespeichert.");
            return;
        };
        // Adressen, zu denen es schon eine Label-Datei gibt
        let vorhanden: std::collections::HashSet<u16> = self.db.ecus().into_iter().map(|(a, _)| a).collect();
        let mut angelegt = Vec::new();
        let mut ohne_adresse = 0;
        for m in &last.modules {
            let Some(addr) = m.address else {
                ohne_adresse += 1;
                continue;
            };
            if vorhanden.contains(&addr) {
                continue;
            }
            // Nur bei UDS sind das feste Diagnose-IDs. Bei TP 2.0 steht im Scan die
            // dynamisch vergebene Kanal-ID — als tx/rx würde der Scan sie als UDS-Ziel abfragen.
            let (tx, rx) = match m.protocol.as_str() {
                "UDS" => ecu_werkbank::profil::parse_can(&m.can).map(|(t, r)| (Some(t), Some(r))).unwrap_or((None, None)),
                _ => (None, None),
            };
            let lf = ecu_werkbank::labels::skelett(addr, &m.name, m.ident.first().map(String::as_str), tx, rx);
            if ecu_werkbank::db::eigen::mit_db(|db| db.label_speichern(None, &lf)).is_ok() {
                angelegt.push(format!("{addr:02X} {}", m.name));
            }
        }
        if angelegt.is_empty() {
            self.status = if ohne_adresse > 0 {
                tr!("Nichts angelegt — alle Steuergeräte haben schon einen Eintrag ({a} ohne Adresse übersprungen).", a = ohne_adresse)
            } else {
                tr!("Nichts angelegt — alle Steuergeräte aus dem Scan haben schon einen Eintrag.")
            };
        } else {
            self.reload_labels();
            self.status = tr!("{a} Einträge aus dem Scan angelegt: {b}", a = angelegt.len(), b = angelegt.join(", "));
        }
    }

    pub(super) fn ui_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Label-Editor"));
        ui.label(tr!("Eigene Klartexte für Steuergeräte, Fehlercodes und Messwerte pflegen. Gespeichert wird in der Fahrzeug-Datenbank; „Als Datei“ schreibt einen Eintrag zum Weitergeben oder Sichern."));
        ui.horizontal_wrapped(|ui| {
            let liste = self.label_liste();
            let current = match self.ed_id {
                Some(Some(id)) => liste.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone()).unwrap_or_default(),
                Some(None) => tr!("(neu, noch nicht gespeichert)"),
                None => tr!("— Steuergerät wählen —"),
            };
            let mut pick = None;
            egui::ComboBox::from_id_source("edfile").selected_text(current).width(260.0).show_ui(ui, |ui| {
                for (id, name) in &liste {
                    if ui.selectable_label(self.ed_id == Some(Some(*id)), name).clicked() {
                        pick = Some(*id);
                    }
                }
            });
            if let Some(id) = pick {
                self.ed_oeffnen(id);
            }
            if ui.button(tr!("+ Neu")).clicked() {
                self.ed_anzeigen(None, LabelFile::default());
                self.status = tr!("Neuer Eintrag — ausfüllen und speichern.");
            }
            if ui
                .button(tr!("⬇ Aus letztem Scan anlegen"))
                .on_hover_text(tr!("Legt für jedes Steuergerät aus dem letzten Scan des aktiven Fahrzeugs einen vorbefüllten Eintrag an (Adresse, Name, Teilenummer, CAN-IDs) — nur wo noch keiner existiert. Überschreibt nichts."))
                .clicked()
            {
                self.ed_aus_scan();
            }
        });
        let Some(id) = self.ed_id else {
            ui.add_space(8.0);
            ui.weak(tr!("Steuergerät wählen oder neu anlegen. Eine Label-Datei kann auch ins Fenster gezogen werden."));
            return;
        };
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("💾 Speichern")).clicked() {
                self.ed_save();
            }
            if ui.button(tr!("↺ Verwerfen")).clicked() {
                match id {
                    Some(id) => self.ed_oeffnen(id),
                    None => self.ed_id = None,
                }
            }
            if ui.button(tr!("📄 Als Datei…")).on_hover_text(tr!("Diesen Eintrag als Label-Datei (TOML) speichern — zum Weitergeben oder als Sicherung")).clicked() {
                let name = format!("{}.toml", ecu_werkbank::labels::dateiname(self.ed_file.ecu.address.unwrap_or(0), &self.ed_file.ecu.name));
                if let Some(p) = rfd::FileDialog::new().add_filter("TOML", &["toml"]).set_file_name(&name).save_file() {
                    self.status = match ecu_werkbank::ablage::schreiben(&p, self.ed_file.to_toml()) {
                        Ok(()) => tr!("Gespeichert: {a}", a = p.display()),
                        Err(e) => tr!("Speichern fehlgeschlagen: {e}", e = e),
                    };
                }
            }
            if let Some(id) = id {
                if !self.ed_loeschen {
                    if ui.button(tr!("🗑 Löschen…")).clicked() {
                        self.ed_loeschen = true;
                    }
                } else {
                    ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Diesen Eintrag wirklich löschen?"));
                    if ui.button(tr!("Ja, löschen")).clicked() {
                        self.status = match ecu_werkbank::db::eigen::mit_db(|db| db.label_entfernen(id)) {
                            Ok(_) => tr!("Eintrag gelöscht."),
                            Err(e) => tr!("Löschen fehlgeschlagen: {e}", e = format!("{:#}", e)),
                        };
                        self.ed_id = None;
                        self.ed_loeschen = false;
                        self.reload_labels();
                    }
                    if ui.button(tr!("Abbrechen")).clicked() {
                        self.ed_loeschen = false;
                    }
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.strong(tr!("Steuergerät"));
            egui::Grid::new("edecu").spacing([10.0, 4.0]).show(ui, |ui| {
                ui.label(tr!("Adresse:"));
                ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut self.ed_addr).hint_text("0x01"));
                ui.label(tr!("Name:"));
                ui.add_sized([220.0, 20.0], egui::TextEdit::singleline(&mut self.ed_file.ecu.name));
                ui.end_row();
                ui.label(tr!("TX (optional):"));
                ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut self.ed_tx).hint_text("0x7E0"));
                ui.label(tr!("RX (optional):"));
                ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut self.ed_rx).hint_text("0x7E8"));
                ui.end_row();
            });

            ui.add_space(8.0);
            ui.strong(tr!("Fehlercodes ({a})", a = self.ed_file.dtc.len()));
            let mut del = None;
            egui::Grid::new("eddtc").striped(true).spacing([8.0, 2.0]).show(ui, |ui| {
                for (i, d) in self.ed_file.dtc.iter_mut().enumerate() {
                    ui.add_sized([90.0, 20.0], egui::TextEdit::singleline(&mut d.code).hint_text("P0300 / 16684"));
                    ui.add_sized([380.0, 20.0], egui::TextEdit::singleline(&mut d.description).hint_text(tr!("eigene Beschreibung")));
                    if ui.small_button("🗑").on_hover_text(tr!("Eintrag löschen")).clicked() {
                        del = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = del {
                self.ed_file.dtc.remove(i);
            }
            if ui.button(tr!("+ Fehlercode")).clicked() {
                self.ed_file.dtc.push(ecu_werkbank::labels::DtcLabel { code: String::new(), description: String::new() });
            }

            ui.add_space(8.0);
            ui.strong(tr!("Messwerte ({a})", a = self.ed_file.measuring.len()));
            ui.weak(tr!("ID: KWP-Block (1–255) oder UDS-DID (z. B. F40C). Anzeige = Rohwert × Faktor + Offset."));
            let mut del = None;
            egui::Grid::new("edmeas").striped(true).spacing([8.0, 2.0]).show(ui, |ui| {
                ui.weak(tr!("ID (hex)"));
                ui.weak(tr!("Name"));
                ui.weak(tr!("Einheit"));
                ui.weak(tr!("Faktor"));
                ui.weak(tr!("Offset"));
                ui.end_row();
                for (i, m) in self.ed_file.measuring.iter_mut().enumerate() {
                    ui.add(egui::DragValue::new(&mut m.id).hexadecimal(4, false, true));
                    ui.add_sized([200.0, 20.0], egui::TextEdit::singleline(&mut m.name));
                    ui.add_sized([60.0, 20.0], egui::TextEdit::singleline(&mut m.unit));
                    ui.add(egui::DragValue::new(&mut m.factor).speed(0.01));
                    ui.add(egui::DragValue::new(&mut m.offset).speed(0.1));
                    if ui.small_button("🗑").on_hover_text(tr!("Eintrag löschen")).clicked() {
                        del = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = del {
                self.ed_file.measuring.remove(i);
            }
            if ui.button(tr!("+ Messwert")).clicked() {
                self.ed_file.measuring.push(ecu_werkbank::labels::Measuring {
                    id: 1,
                    name: String::new(),
                    unit: String::new(),
                    factor: 1.0,
                    offset: 0.0,
                });
            }
            if !self.ed_file.coding.is_empty() || !self.ed_file.adaptation.is_empty() {
                ui.add_space(8.0);
                ui.weak(tr!("{a} Codier- und {b} Anpassungs-Einträge bleiben beim Speichern erhalten (hier nicht bearbeitbar).", a = self.ed_file.coding.len(), b = self.ed_file.adaptation.len()));
            }
        });
    }
}
