//! GUI: Label-Editor

use super::*;

impl App {
    pub(super) fn label_files(&self) -> Vec<PathBuf> {
        let mut v = Vec::new();
        let mut stack = vec![self.labels_dir.clone()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|x| x.to_str()) == Some("toml") {
                    v.push(p);
                }
            }
        }
        v.sort();
        v
    }

    pub(super) fn ed_open(&mut self, path: PathBuf) {
        match LabelFile::load(&path) {
            Ok(f) => {
                let h = |v: Option<u16>| v.map(|x| format!("{x:#04x}")).unwrap_or_default();
                self.ed_addr = h(f.ecu.address);
                self.ed_tx = h(f.ecu.tx);
                self.ed_rx = h(f.ecu.rx);
                self.ed_file = f;
                self.status = tr!("Geöffnet: {a}", a = path.display());
                self.ed_path = Some(path);
            }
            Err(e) => self.status = tr!("Fehler: {e}", e = format!("{:#}", e)),
        }
    }

    pub(super) fn ed_save(&mut self) {
        let Some(path) = self.ed_path.clone() else { return };
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
        let text = self.ed_file.to_toml();
        if let Err(e) = ecu_werkbank::labels::LabelDb::check_file(&text) {
            self.status = tr!("Nicht gespeichert, Datei wäre ungültig: {e}", e = e);
            return;
        }
        let res = (|| -> anyhow::Result<()> {
            if path.exists() {
                std::fs::copy(&path, path.with_extension("toml.bak"))?;
            }
            std::fs::write(&path, text)?;
            Ok(())
        })();
        match res {
            Ok(()) => {
                self.reload_labels();
                self.status = tr!("Gespeichert: {a} (Sicherung: .toml.bak)", a = path.display());
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
            let basis = ecu_werkbank::labels::dateiname(addr, &m.name);
            let mut p = self.labels_dir.join(format!("{basis}.toml"));
            let mut n = 2;
            while p.exists() {
                p = self.labels_dir.join(format!("{basis}-{n}.toml"));
                n += 1;
            }
            if std::fs::write(&p, lf.to_toml()).is_ok() {
                angelegt.push(p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
            }
        }
        if angelegt.is_empty() {
            self.status = if ohne_adresse > 0 {
                tr!("Nichts angelegt — alle Steuergeräte haben schon eine Datei ({a} ohne Adresse übersprungen).", a = ohne_adresse)
            } else {
                tr!("Nichts angelegt — alle Steuergeräte aus dem Scan haben schon eine Label-Datei.")
            };
        } else {
            self.reload_labels();
            self.status = tr!("{a} Label-Datei(en) aus dem Scan angelegt: {b}", a = angelegt.len(), b = angelegt.join(", "));
        }
    }

    pub(super) fn ui_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Label-Editor"));
        ui.label(tr!("Eigene Klartexte für Steuergeräte, Fehlercodes und Messwerte pflegen. Beim Speichern wird eine Sicherung (.toml.bak) angelegt; Kommentare in der Datei gehen dabei verloren."));
        ui.horizontal(|ui| {
            let current = self
                .ed_path
                .as_ref()
                .and_then(|p| p.strip_prefix(&self.labels_dir).ok().map(|r| r.display().to_string()))
                .unwrap_or_else(|| tr!("— Datei wählen —"));
            let mut pick = None;
            egui::ComboBox::from_id_source("edfile").selected_text(current).width(260.0).show_ui(ui, |ui| {
                for p in self.label_files() {
                    let name = p.strip_prefix(&self.labels_dir).unwrap_or(&p).display().to_string();
                    if ui.selectable_label(self.ed_path.as_ref() == Some(&p), name).clicked() {
                        pick = Some(p);
                    }
                }
            });
            if let Some(p) = pick {
                self.ed_open(p);
            }
            ui.separator();
            ui.add_sized([110.0, 20.0], egui::TextEdit::singleline(&mut self.ed_new_name).hint_text(tr!("neue-datei")));
            if ui.button(tr!("+ Neu")).clicked() {
                let name: String = self.ed_new_name.trim().chars().filter(|c| c.is_alphanumeric() || "-_".contains(*c)).collect();
                if name.is_empty() {
                    self.status = tr!("Dateinamen eingeben (Buchstaben, Ziffern, - _).");
                } else {
                    let p = self.labels_dir.join(format!("{name}.toml"));
                    if p.exists() {
                        self.status = tr!("{a} gibt es schon.", a = p.display());
                    } else {
                        self.ed_file = LabelFile::default();
                        self.ed_addr.clear();
                        self.ed_tx.clear();
                        self.ed_rx.clear();
                        self.ed_path = Some(p);
                        self.status = tr!("Neue Datei — ausfüllen und speichern.");
                    }
                }
            }
            ui.separator();
            if ui
                .button(tr!("⬇ Aus letztem Scan anlegen"))
                .on_hover_text(tr!("Legt für jedes Steuergerät aus dem letzten Scan des aktiven Fahrzeugs eine vorbefüllte Datei an (Adresse, Name, Teilenummer, CAN-IDs) — nur wo noch keine existiert. Überschreibt nichts."))
                .clicked()
            {
                self.ed_aus_scan();
            }
        });
        if self.ed_path.is_none() {
            ui.add_space(8.0);
            ui.weak(tr!("Datei wählen oder neue anlegen."));
            return;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button(tr!("💾 Speichern")).clicked() {
                self.ed_save();
            }
            if ui.button(tr!("↺ Verwerfen")).clicked() {
                if let Some(p) = self.ed_path.clone() {
                    if p.exists() {
                        self.ed_open(p);
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
