//! GUI: Codierungs-Sicherung, Vergleich, Zurückschreiben

use super::*;

impl App {
    pub(super) fn start_backup(&mut self) {
        if self.bk_job.is_some() {
            return;
        }
        let be = self.backend();
        let labels_dir = self.labels_dir.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
            let res = ecu_werkbank::backup::backup(&be, &db, true, |p| {
                let _ = tx.send(BackupMsg::Progress(p.to_string()));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(BackupMsg::Done(res));
        });
        self.bk_job = Some(rx);
        self.bk_progress = tr!("Steuergeräte werden gelesen…");
    }

    pub(super) fn poll_backup(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.bk_job else { return };
        let mut done = None;
        while let Ok(m) = rx.try_recv() {
            match m {
                BackupMsg::Progress(p) => self.bk_progress = p,
                BackupMsg::Done(r) => done = Some(r),
            }
        }
        let Some(res) = done else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        self.bk_job = None;
        self.bk_progress.clear();
        match res {
            Ok(b) if b.entries.is_empty() => self.status = tr!("Keine Steuergeräte erreichbar — nichts gesichert."),
            Ok(mut b) => match ecu_werkbank::vehicle::resolve(b.vin.as_deref(), self.active_vehicle.as_deref())
                .and_then(|v| {
                    b.vehicle = v.key;
                    ecu_werkbank::backup::save(&b)
                }) {
                Ok(p) => {
                    self.status = tr!("{a} Steuergerät(e) gesichert: {b}", a = b.entries.len(), b = p.display());
                    // gesichert wurde das angesteckte Auto: das ist jetzt das gewählte
                    if self.active_vehicle.as_deref() != Some(b.vehicle.as_str()) {
                        self.set_active_vehicle(Some(b.vehicle.clone()));
                    }
                    self.bk_laden();
                }
                Err(e) => self.status = tr!("Sicherung: {e}", e = format!("{:#}", e)),
            },
            Err(e) => self.status = tr!("Sicherung fehlgeschlagen: {e}", e = e),
        }
    }

    /// Komplett-Sicherung für den Notfall: Knopf, Fortschritt, Ergebnis.
    pub(super) fn ui_notfall(&mut self, ui: &mut egui::Ui) {
        if let Some(rx) = &self.nf_job {
            let mut fertig = None;
            while let Ok(m) = rx.try_recv() {
                match m {
                    NfMsg::Progress(p) => self.nf_progress = p,
                    NfMsg::Done(r) => fertig = Some(r),
                }
            }
            match fertig {
                Some(Ok((key, pdf, text))) => {
                    self.nf_job = None;
                    self.status = text;
                    self.nf_pdf = Some(pdf);
                    if self.active_vehicle.as_deref() != Some(key.as_str()) {
                        self.set_active_vehicle(Some(key));
                    }
                    self.bk_laden();
                }
                Some(Err(e)) => {
                    self.nf_job = None;
                    self.status = tr!("Komplett-Sicherung fehlgeschlagen: {e}", e = e);
                }
                None => ui.ctx().request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        ui.heading(tr!("🛟 Komplett-Sicherung für den Notfall"));
        ui.label(tr!("Sichert von allen Steuergeräten Teilenummer, Softwarestand, Codierung und Anpassungen und erstellt den „Ersatzteil-Pass“ (PDF): welches Teil du kaufen musst und was nach dem Einbau zu tun ist. Nur lesen, dauert etwa 10–20 Minuten. Zündung an, am besten Ladegerät an die Batterie."));
        ui.horizontal(|ui| {
            let busy = self.nf_job.is_some();
            if ui.add_enabled(!busy, egui::Button::new(tr!("🛟 Komplett-Sicherung starten"))).clicked() {
                let be = self.backend();
                let labels_dir = self.labels_dir.clone();
                let aktiv = self.active_vehicle.clone();
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
                    let vin = ecu_werkbank::profil::quick_vin(&be);
                    let r = ecu_werkbank::vehicle::resolve(vin.as_deref(), aktiv.as_deref()).and_then(|v| {
                        let t2 = tx.clone();
                        let e = ecu_werkbank::notfall::sichern(&be, &db, &v, |p| {
                            let _ = t2.send(NfMsg::Progress(p.to_string()));
                        })?;
                        let n: usize = e.anpassung.geraete.iter().map(|g| g.kennungen.len()).sum();
                        let text = tr!("Komplett-Sicherung fertig: {a} Steuergeräte, {b} Codierungen, {c} Anpassungswerte.", a = e.software.module.len(), b = e.codierung.entries.len(), c = n);
                        Ok((v.key, e.pdf, text))
                    });
                    let _ = tx.send(NfMsg::Done(r.map_err(|e| format!("{e:#}"))));
                });
                self.nf_job = Some(rx);
                self.nf_pdf = None;
                self.nf_progress = tr!("Steuergeräte werden gelesen…");
            }
            if busy {
                ui.spinner();
                ui.weak(&self.nf_progress);
            }
            if let Some(p) = self.nf_pdf.clone() {
                if ui.button(tr!("📄 Ersatzteil-Pass öffnen")).clicked() {
                    open_path(&p, &mut self.status);
                }
            }
        });
    }

    /// Sicherungen nur des gewählten Fahrzeugs laden (neueste zuerst).
    pub(super) fn bk_laden(&mut self) {
        let key = self.active_vehicle.clone();
        self.bk_list = match &key {
            Some(k) => {
                let dir = ecu_werkbank::backup::backup_dir(k);
                ecu_werkbank::backup::list_all().into_iter().filter(|(p, _)| p.starts_with(&dir)).collect()
            }
            None => Vec::new(),
        };
        self.bk_sel = (!self.bk_list.is_empty()).then_some(0);
        self.bk_cmp = self.bk_list.get(1).map(|_| 1);
        self.bk_confirm = None;
        self.bk_fuer = Some(key);
    }

    pub(super) fn ui_sicherung(&mut self, ui: &mut egui::Ui) {
        self.ui_notfall(ui);
        ui.separator();
        ui.scope(|ui| self.ui_sicherung_codierung(ui));
        self.ui_anpassungen(ui);
    }

    pub(super) fn ui_sicherung_codierung(&mut self, ui: &mut egui::Ui) {
        use ecu_werkbank::backup;
        ui.heading(tr!("Codierungs-Sicherung"));
        ui.label(tr!("Vor jeder Änderung: Codierung aller Steuergeräte sichern. Später vergleichen und bei Bedarf einzeln zurückschreiben."));
        ui.horizontal(|ui| {
            if ui.add_enabled(self.bk_job.is_none(), egui::Button::new(tr!("💾 Jetzt sichern"))).clicked() {
                self.start_backup();
            }
            if self.bk_job.is_some() {
                ui.spinner();
                ui.weak(&self.bk_progress);
            }
            if ui.button(tr!("↻ Liste laden")).clicked() || self.bk_fuer.as_ref() != Some(&self.active_vehicle) {
                self.bk_laden();
            }
        });
        let Some(key) = self.active_vehicle.clone() else {
            ui.add_space(8.0);
            ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Bitte oben ein Fahrzeug wählen — angezeigt werden nur die Sicherungen dieses Fahrzeugs."));
            return;
        };
        let name = ecu_werkbank::vehicle::load(&key).map(|v| v.label()).unwrap_or(key);
        if self.bk_list.is_empty() {
            ui.add_space(8.0);
            ui.weak(tr!("Noch keine Sicherungen für {a}. „💾 Jetzt sichern“ legt die erste an.", a = name));
            return;
        }
        ui.weak(tr!("Sicherungen von {a}", a = name));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let name = |i: Option<usize>, l: &[(PathBuf, backup::CodingBackup)]| {
                i.and_then(|i| l.get(i)).map(|(_, b)| backup_label(b)).unwrap_or_else(|| "—".into())
            };
            ui.label(tr!("Sicherung:"));
            let sel_txt = name(self.bk_sel, &self.bk_list);
            let cmp_txt = name(self.bk_cmp, &self.bk_list);
            egui::ComboBox::from_id_source("bksel").selected_text(sel_txt).width(240.0).show_ui(ui, |ui| {
                for (i, (_, b)) in self.bk_list.iter().enumerate() {
                    ui.selectable_value(&mut self.bk_sel, Some(i), backup_label(b));
                }
            });
            ui.label(tr!("vergleichen mit:"));
            egui::ComboBox::from_id_source("bkcmp").selected_text(cmp_txt).width(240.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.bk_cmp, None, "—");
                for (i, (_, b)) in self.bk_list.iter().enumerate() {
                    ui.selectable_value(&mut self.bk_cmp, Some(i), backup_label(b));
                }
            });
        });
        let Some(sel) = self.bk_sel.filter(|i| *i < self.bk_list.len()) else { return };
        let current = self.bk_list[sel].1.clone();

        if let Some(c) = self.bk_cmp.filter(|c| *c != sel && *c < self.bk_list.len()) {
            let other = &self.bk_list[c].1;
            let (old, new) = if other.timestamp <= current.timestamp { (other, &current) } else { (&current, other) };
            let changes = backup::compare(old, new, &self.db);
            ui.separator();
            ui.strong(tr!("Änderungen vom {a} bis {b}", a = old.created, b = new.created));
            if changes.is_empty() {
                ui.label(tr!("Keine Codierung geändert."));
            }
            for ch in &changes {
                ui.label(egui::RichText::new(&ch.module).strong());
                ui.monospace(tr!("  vorher: {a}\n  danach: {b}", a = ch.old_hex, b = ch.new_hex));
                for (byte, bit, x, y, name) in &ch.bits {
                    ui.monospace(tr!("    Byte {byte} Bit {bit}: {a} » {b}  {name}", byte = byte, bit = bit, a = if *x { "ein" } else { "aus" }, b = if *y { "ein" } else { "aus" }, name = name));
                }
            }
        }

        ui.separator();
        ui.strong(tr!("Inhalt der Sicherung vom {a}", a = current.created));
        let mut do_restore = None;
        egui::ScrollArea::vertical().id_source("bkentries").show(ui, |ui| {
            egui::Grid::new("bkgrid").striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                for (i, e) in current.entries.iter().enumerate() {
                    ui.label(e.title());
                    if e.data_hex.is_empty() {
                        ui.weak(&e.note);
                    } else {
                        let short: String = e.data_hex.chars().take(36).collect();
                        let more = if e.data_hex.len() > 36 { " …" } else { "" };
                        ui.monospace(format!("{short}{more}")).on_hover_text(if e.note.is_empty() { e.data_hex.clone() } else { format!("{}\n{}", e.data_hex, e.note) });
                    }
                    if e.can_restore() {
                        if self.bk_confirm == Some(i) {
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::from_rgb(220, 160, 40), tr!("Wirklich schreiben?"));
                                if ui.button(tr!("Ja")).clicked() {
                                    do_restore = Some(i);
                                }
                                if ui.button(tr!("Nein")).clicked() {
                                    self.bk_confirm = None;
                                }
                            });
                        } else if ui.button(tr!("↩ zurückschreiben")).clicked() {
                            self.bk_confirm = Some(i);
                        }
                    } else {
                        ui.weak("—");
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(i) = do_restore {
            self.bk_confirm = None;
            let e = current.entries[i].clone();
            let login = ecu_werkbank::login::fuer(&current.vehicle, e.tx);
            let be = self.backend();
            self.im_hintergrund(tr!("{a} zurückschreiben", a = e.title()), move || {
                let t = match backup::restore(&be, &e, login.as_ref()) {
                    Ok(before) if before == e.bytes() => tr!("{a}: war bereits so codiert.", a = e.title()),
                    Ok(before) => tr!("{a}: zurückgeschrieben ({b} zu {c}) und kontrolliert.", a = e.title(), b = backup::hex(&before), c = e.data_hex),
                    Err(err) => format!("{}: {err:#}", e.title()),
                };
                Box::new(move |app: &mut App| app.status = t)
            });
        }
    }
}
