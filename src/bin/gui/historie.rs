//! GUI: Fahrzeug-Historie und Trends

use super::*;

impl App {
    pub(super) fn hist_refresh(&mut self) {
        let vehicles = ecu_werkbank::history::vehicles();
        if let Some(a) = self.active_vehicle.clone().filter(|a| vehicles.iter().any(|(v, _, _)| v.key == *a)) {
            self.hist_vehicle = a;
        }
        if self.hist_vehicle.is_empty() || !vehicles.iter().any(|(v, _, _)| v.key == self.hist_vehicle) {
            self.hist_vehicle = vehicles.last().map(|(v, _, _)| v.key.clone()).unwrap_or_default();
        }
        self.hist_scans = if self.hist_vehicle.is_empty() { vec![] } else { ecu_werkbank::history::list(&self.hist_vehicle) };
        let n = self.hist_scans.len();
        self.hist_b = n.checked_sub(1);
        self.hist_a = n.checked_sub(2);
    }

    pub(super) fn ui_historie(&mut self, ui: &mut egui::Ui) {
        use ecu_werkbank::history;
        ui.heading(tr!("Fahrzeug-Historie"));
        ui.label(tr!("Jedes Fehler-PDF speichert den Scan automatisch im Fahrzeugordner. Zwei Scans wählen, darunter stehen die Unterschiede."));
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(tr!("📥 Auto-Scan-Protokolle importieren"))
                .on_hover_text(tr!("Auto-Scan-Protokolle als Textdatei (auch mehrere auf einmal): Fehler mit km-Stand und Datum kommen in den Verlauf des Fahrzeugs (erkannt an der Fahrgestellnummer), die Fehlertexte in deine eigenen Fehlertexte — nur bei Codes, für die du noch keinen Text hast."))
                .clicked()
            {
                if let Some(dateien) = rfd::FileDialog::new().add_filter(tr!("Textdateien"), &["txt", "TXT"]).set_title(tr!("Auto-Scan-Protokolle wählen")).pick_files() {
                    let (mut neu, mut texte, mut fehler) = (0, 0, Vec::new());
                    let mut zuletzt = None;
                    for d in &dateien {
                        match ecu_werkbank::vcds::importieren(d) {
                            Ok(e) => {
                                neu += usize::from(e.neu_im_verlauf);
                                texte += e.neue_texte;
                                zuletzt = Some(e.fahrzeug);
                            }
                            Err(e) => fehler.push(format!("{}: {e:#}", d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
                        }
                    }
                    // Fehlertexte sofort überall nutzen
                    for (c, t) in ecu_werkbank::fehlertexte::alle() {
                        self.db.set_dtc_text(&c, &t);
                    }
                    self.status = tr!("Import: {a} von {b} Protokoll(en) neu im Verlauf, {c} neue Fehlertexte.", a = neu, b = dateien.len(), c = texte);
                    if !fehler.is_empty() {
                        self.status += &format!("  {}", tr!("Nicht erkannt: {a}", a = fehler.join("; ")));
                    }
                    if let Some(k) = zuletzt.and_then(|l| history::vehicles().into_iter().find(|(v, _, _)| v.label() == l).map(|(v, _, _)| v.key)) {
                        self.hist_vehicle = k;
                    }
                    self.hist_refresh();
                }
            }
        });
        let vehicles = history::vehicles();
        ui.horizontal(|ui| {
            ui.label(tr!("Fahrzeug:"));
            let mut pick = None;
            let current = vehicles
                .iter()
                .find(|(v, _, _)| v.key == self.hist_vehicle)
                .map(|(v, _, _)| v.label())
                .unwrap_or_else(|| "—".into());
            egui::ComboBox::from_id_source("histveh")
                .selected_text(current)
                .width(260.0)
                .show_ui(ui, |ui| {
                    for (v, n, last) in &vehicles {
                        if ui.selectable_label(v.key == self.hist_vehicle, tr!("{a}  ({n} Scans, zuletzt {last})", a = v.label(), n = n, last = last)).clicked() {
                            pick = Some(v.key.clone());
                        }
                    }
                });
            if let Some(k) = pick {
                self.hist_vehicle = k;
                self.hist_refresh();
            }
            if ui.button(tr!("↻ Neu laden")).clicked() {
                self.hist_refresh();
            }
        });
        if self.hist_scans.is_empty() {
            ui.add_space(8.0);
            ui.weak(tr!("Noch keine Scans gespeichert. Oben „📄 Fehler-PDF“ erstellt den ersten."));
            return;
        }
        if let Some(v) = ecu_werkbank::vehicle::load(&self.hist_vehicle) {
            verlauf_diagramme(ui, &v);
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_source("histlist").max_height(180.0).show(ui, |ui| {
            egui::Grid::new("histscans").striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
                ui.weak(tr!("älter"));
                ui.weak(tr!("neuer"));
                ui.weak(tr!("Datum"));
                ui.weak(tr!("Steuergeräte"));
                ui.weak(tr!("Fehler"));
                ui.weak("km");
                ui.weak(tr!("Reparatur / Notiz"));
                ui.end_row();
                for (i, (_, r)) in self.hist_scans.iter().enumerate().rev() {
                    ui.radio_value(&mut self.hist_a, Some(i), "");
                    ui.radio_value(&mut self.hist_b, Some(i), "");
                    ui.label(&r.created);
                    ui.label(r.modules.len().to_string());
                    ui.label(r.total_dtcs().to_string());
                    ui.label(r.km.map(ecu_werkbank::report::group_thousands).unwrap_or_default());
                    let note = [r.reparatur.as_str(), r.note.as_str()].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join(" · ");
                    ui.label(note);
                    ui.end_row();
                }
            });
        });
        // Reparatur-Notiz zum „neueren“ Scan
        if let Some(b) = self.hist_b.filter(|b| *b < self.hist_scans.len()) {
            if self.hist_note_for != Some(b) {
                self.hist_note = self.hist_scans[b].1.reparatur.clone();
                self.hist_note_for = Some(b);
            }
            ui.horizontal(|ui| {
                ui.label(tr!("Reparatur zum Scan vom {a}:", a = self.hist_scans[b].1.created));
                ui.add_sized([300.0, 20.0], egui::TextEdit::singleline(&mut self.hist_note).hint_text(tr!("z. B. Zündspule Zyl. 3 getauscht, 45 €")));
                if ui.button("💾").on_hover_text(tr!("Notiz speichern")).clicked() {
                    let path = self.hist_scans[b].0.clone();
                    match ecu_werkbank::history::set_repair_note(&path, &self.hist_note) {
                        Ok(()) => {
                            self.status = tr!("Reparatur-Notiz gespeichert.");
                            self.hist_scans[b].1.reparatur = self.hist_note.trim().to_string();
                        }
                        Err(e) => self.status = format!("{e:#}"),
                    }
                }
            });
        }
        let (Some(a), Some(b)) = (self.hist_a, self.hist_b) else { return };
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a == b {
            ui.weak(tr!("Zum Vergleich zwei verschiedene Scans wählen."));
            return;
        }
        let earlier: Vec<_> = self.hist_scans[..a].iter().map(|(_, r)| r.clone()).collect();
        let (old, new) = (&self.hist_scans[a].1, &self.hist_scans[b].1);
        let cmp = history::Comparison { since: old.created.clone(), diff: history::diff(old, new, &earlier) };
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong(format!("{}  bis  {}", old.created, new.created));
            if ui.button(tr!("📄 PDF mit diesem Vergleich…")).clicked() {
                let mut r = new.clone();
                r.comparison = Some(cmp.clone());
                let target = ecu_werkbank::vehicle::load(&self.hist_vehicle)
                    .and_then(|v| v.sub("berichte").ok())
                    .map(|d| d.join(ecu_werkbank::vehicle::stamped("vergleich", "pdf")));
                if let Some(p) = target {
                    match r.save(&p) {
                        Ok(()) => {
                            self.status = tr!("PDF gespeichert: {a}", a = p.display());
                            self.pdf_last = Some(p);
                        }
                        Err(e) => self.status = tr!("PDF-Fehler: {e}", e = format!("{:#}", e)),
                    }
                }
            }
        });
        self.ui_trends(ui);
        egui::ScrollArea::vertical().id_source("histdiff").show(ui, |ui| {
            if cmp.diff.is_empty() {
                ui.label(tr!("Keine Fehler – weder im älteren noch im neueren Scan."));
            }
            let red = egui::Color32::from_rgb(230, 110, 90);
            let green = egui::Color32::from_rgb(90, 190, 110);
            for (label, items, color) in [
                (tr!("Neu"), &cmp.diff.new, red),
                (tr!("Behoben"), &cmp.diff.gone, green),
                (tr!("Weiterhin vorhanden"), &cmp.diff.still, egui::Color32::GRAY),
            ] {
                if items.is_empty() {
                    continue;
                }
                ui.add_space(4.0);
                ui.colored_label(color, egui::RichText::new(format!("{label} ({})", items.len())).strong());
                let model = ecu_werkbank::vehicle::load(&self.hist_vehicle).map(|v| v.bezeichnung).unwrap_or_default();
                for (i, it) in items.iter().enumerate() {
                    let rec = if it.recurring { "  (wiederkehrend)" } else { "" };
                    ui.horizontal(|ui| {
                        let km = it.km.as_ref().map(|k| tr!("  · gespeichert bei {a}", a = k)).unwrap_or_default();
                        ui.label(format!("   {}  {}   —  {}{rec}{km}", it.code, it.text, it.module));
                        if ui.small_button("🔎").on_hover_text(tr!("im Internet nachschlagen")).clicked() {
                            open_code_search(&it.code, &model, &mut self.status);
                        }
                    });
                    umgebung_zeigen(ui, ("hist_umg", label.as_str(), i), &it.umgebung);
                }
            }
        });
    }
}
