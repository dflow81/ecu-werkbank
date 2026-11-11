//! GUI: Verbindung, Gebrauchtwagen-Check, geführte Fehlersuche, Reparatur-Modus, Lernhilfe

use super::*;

impl App {
    pub(super) fn connect_window(&mut self, ctx: &egui::Context) {
        let Some(items) = self.con_result.clone() else { return };
        let mut open = true;
        egui::Window::new(tr!("Verbindung prüfen")).open(&mut open).collapsible(false).show(ctx, |ui| {
            ui.weak(self.backend().describe());
            for (st, t) in &items {
                ui.horizontal_wrapped(|ui| {
                    dot(ui, stufe_color(*st));
                    ui.label(t);
                });
            }
            let ifaces = ecu_werkbank::connect::can_interfaces();
            if !ifaces.is_empty() {
                ui.add_space(4.0);
                ui.weak(tr!("CAN-Interfaces im System:"));
                for (n, up, br) in ifaces {
                    ui.weak(format!("  {n}: {}{}", if up { "aktiv" } else { "aus" }, br.map(|b| tr!(", {a} kbit/s", a = b / 1000)).unwrap_or_default()));
                }
            }
            if ui.button(tr!("↻ erneut prüfen")).clicked() {
                self.con_result = Some(ecu_werkbank::connect::check(&self.backend()));
            }
        });
        if !open {
            self.con_result = None;
        }
    }

    pub(super) fn start_kaufcheck(&mut self) {
        if self.kc_job.is_some() {
            return;
        }
        let be = self.backend();
        let labels_dir = self.labels_dir.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
            let res = ecu_werkbank::usedcar::run(&be, &db, |p| {
                let _ = tx.send(KcMsg::Progress(p.to_string()));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(KcMsg::Done(res));
        });
        self.kc_job = Some(rx);
        self.kc_result = None;
    }

    pub(super) fn poll_kaufcheck(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.kc_job else { return };
        let mut done = None;
        while let Ok(m) = rx.try_recv() {
            match m {
                KcMsg::Progress(p) => self.kc_progress = p,
                KcMsg::Done(r) => done = Some(r),
            }
        }
        let Some(res) = done else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        self.kc_job = None;
        match res {
            Ok(mut e) => {
                e.report.note = self.kc_note.trim().to_string();
                // Kauf-Check: Fahrzeug nur über die VIN zuordnen (nicht das aktive Fahrzeug!)
                let path = e
                    .report
                    .assign_vehicle(None)
                    .ok()
                    .and_then(|v| v.sub("berichte").ok())
                    .map(|d| d.join(ecu_werkbank::vehicle::stamped("kaufcheck", "pdf")));
                let saved = path.and_then(|p| std::fs::write(&p, e.to_pdf()).ok().map(|_| p));
                let _ = ecu_werkbank::history::save(&e.report);
                if let Some(p) = &saved {
                    self.pdf_last = Some(p.clone());
                }
                self.status = tr!("Gebrauchtwagen-Check: {a}", a = e.gesamt.text());
                self.kc_result = Some((e.punkte.clone(), e.gesamt, e.urteil().to_string(), saved));
            }
            Err(err) => self.status = tr!("Gebrauchtwagen-Check: {err}", err = err),
        }
    }

    /// Andere Marken (z. B. Mercedes): Steuergeräte suchen, Kennung + Fehler lesen.
    pub(super) fn ui_fremd(&mut self, ui: &mut egui::Ui) {
        // Ergebnis der Suche abholen
        if let Some(rx) = &self.fm_job {
            let mut fertig = None;
            while let Ok(m) = rx.try_recv() {
                match m {
                    FmMsg::Progress(p) => self.fm_progress = p,
                    FmMsg::Done(r) => fertig = Some(r),
                }
            }
            match fertig {
                Some(Ok((mut r, s))) => {
                    self.fm_job = None;
                    // Fahrzeugordner (nach Fahrgestellnummer), Historie, PDF
                    self.fm_pdf = r.assign_vehicle(self.active_vehicle.as_deref()).ok().and_then(|v| {
                        if self.active_vehicle.as_deref() != Some(v.key.as_str()) {
                            self.set_active_vehicle(Some(v.key.clone()));
                        }
                        let p = v.sub("berichte").ok()?.join(ecu_werkbank::vehicle::stamped("fehlerspeicher", "pdf"));
                        std::fs::write(&p, r.to_pdf()).ok().map(|_| p)
                    });
                    let _ = ecu_werkbank::history::save(&r);
                    if let Some(p) = &self.fm_pdf {
                        self.pdf_last = Some(p.clone());
                    }
                    self.status = tr!("{a} Steuergerät(e) gefunden, {b} Fehler.", a = r.modules.len(), b = r.total_dtcs());
                    self.fm_result = Some((r, s));
                }
                Some(Err(e)) => {
                    self.fm_job = None;
                    self.status = tr!("Suche fehlgeschlagen: {e}", e = e);
                }
                None => ui.ctx().request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        ui.heading(tr!("Andere Marken (z. B. Mercedes, BMW, Opel)"));
        ui.label(tr!("Sucht die Steuergeräte an der OBD-Buchse und liest Kennung und Fehlerspeicher mit den genormten Diensten (UDS / KWP2000 auf CAN). Fehlercodes erscheinen genormt (P/C/B/U); Klartext gibt es nur für genormte Codes."));
        ui.weak(tr!("Sicher: erst wird nur zugehört, benutzte CAN-IDs werden nie angesprochen, gefragt wird nur „Tester Present“. Ist die Buchse an einem Fahrzeug-Bus mit viel Verkehr, werden nur die OBD-Adressen gefragt. Nicht möglich: Codierung, Anpassungen, Herstellertexte, ältere Autos mit K-Leitung."));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let laeuft = self.fm_job.is_some();
            if ui.add_enabled(!laeuft, egui::Button::new(tr!("🔍 Steuergeräte suchen und Fehler lesen"))).on_hover_text(tr!("Dauert mit SocketCAN etwa 30 Sekunden. Zündung an, Motor aus.")).clicked() {
                let be = self.backend();
                let labels_dir = self.labels_dir.clone();
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
                    let res = ecu_werkbank::fremd::scan(&be, &db, |p| {
                        let _ = tx.send(FmMsg::Progress(p.to_string()));
                    })
                    .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(FmMsg::Done(res));
                });
                self.fm_job = Some(rx);
                self.fm_result = None;
                self.fm_confirm = None;
                self.fm_progress = tr!("Starte…");
            }
            if laeuft {
                ui.spinner();
                ui.weak(&self.fm_progress);
            }
            if let Some(p) = self.fm_pdf.clone() {
                if ui.button(tr!("📄 PDF öffnen")).clicked() {
                    let _ = std::process::Command::new(OEFFNER).arg(&p).spawn();
                }
            }
        });
        let Some((r, s)) = self.fm_result.clone() else { return };
        ui.separator();
        if s.nur_obd {
            ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Nur die OBD-Adressen wurden gefragt ({a} CAN-IDs mit Verkehr gesehen bzw. kein SocketCAN). Weitere Steuergeräte sind so nicht erreichbar.", a = s.verkehr));
        }
        if let Some(v) = &r.vin {
            ui.label(tr!("Fahrgestellnummer: {a}", a = v));
        }
        if r.modules.is_empty() {
            ui.label(tr!("Kein Steuergerät hat geantwortet. Zündung an? Bitrate 500 kbit/s? Ältere Autos (K-Leitung) gehen mit diesem Adapter nicht."));
            return;
        }
        let mut loeschen = None;
        egui::ScrollArea::vertical().id_source("fremd").show(ui, |ui| {
            for (i, m) in r.modules.iter().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        dot(ui, stufe_color(if m.dtc_count() > 0 { Stufe::Warnung } else { Stufe::Gut }));
                        let name = if m.name.is_empty() { tr!("Steuergerät") } else { m.name.clone() };
                        ui.strong(format!("{name}  ·  {}  ·  {}", m.can, m.protocol));
                    });
                    for id in &m.ident {
                        ui.weak(id);
                    }
                    match &m.dtcs {
                        None => {
                            ui.weak(tr!("Fehlerspeicher nicht lesbar"));
                        }
                        Some(l) if l.is_empty() => {
                            ui.label(tr!("Keine Fehler gespeichert"));
                        }
                        Some(l) => {
                            for d in l {
                                let mut t = format!("{}  {}", d.code, if d.text.is_empty() { tr!("(kein Klartext)") } else { d.text.clone() });
                                if let Some(a) = &d.alt {
                                    t.push_str(&format!("  ·  {a}"));
                                }
                                ui.label(egui::RichText::new(t).monospace());
                                ui.weak(format!("    {}", d.status));
                            }
                            let (tx, rx) = m.can.split_once('/').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
                            let hex = |x: &str| u16::from_str_radix(x.trim_start_matches("0x"), 16).ok();
                            if let (Some(tx), Some(rx)) = (hex(&tx), hex(&rx)) {
                                if self.fm_confirm == Some(i) {
                                    ui.horizontal(|ui| {
                                        ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Fehler in diesem Steuergerät wirklich löschen?"));
                                        if ui.button(tr!("Ja, löschen")).clicked() {
                                            loeschen = Some((i, tx, rx, m.protocol.clone()));
                                        }
                                        if ui.button(tr!("Nein")).clicked() {
                                            self.fm_confirm = None;
                                        }
                                    });
                                } else if ui.button(tr!("✖ Fehler löschen…")).clicked() {
                                    self.fm_confirm = Some(i);
                                }
                            }
                        }
                    }
                });
            }
        });
        if let Some((i, tx, rx, prot)) = loeschen {
            self.fm_confirm = None;
            let be = self.backend();
            match ecu_werkbank::fremd::loeschen(&be, tx, rx, &prot) {
                Ok(()) => {
                    // nachlesen
                    if let (Ok(neu), Some((r, _))) = (ecu_werkbank::fremd::lesen(&be, &self.db, tx, rx), self.fm_result.as_mut()) {
                        r.modules[i] = neu;
                    }
                    self.status = tr!("Fehler gelöscht und nachgelesen.");
                }
                Err(e) => self.status = tr!("Löschen nicht möglich: {e}", e = format!("{e:#}")),
            }
        }
    }

    /// Gebrauchtwagen-Check aus dem letzten gespeicherten Scan (ohne Auto).
    pub(super) fn kaufcheck_aus_scan(&mut self) {
        let Some(key) = self.active_vehicle.clone() else { return };
        let Some(r) = ecu_werkbank::usedcar::letzter_scan(&key) else {
            self.status = tr!("Für dieses Fahrzeug ist noch kein Scan gespeichert");
            return;
        };
        let mut e = ecu_werkbank::usedcar::aus_scan(r);
        if !self.kc_note.trim().is_empty() {
            e.report.note = self.kc_note.trim().to_string();
        }
        let saved = ecu_werkbank::vehicle::load(&key)
            .and_then(|v| v.sub("berichte").ok())
            .map(|d| d.join(ecu_werkbank::vehicle::stamped("kaufcheck", "pdf")))
            .and_then(|p| std::fs::write(&p, e.to_pdf()).ok().map(|_| p));
        if let Some(p) = &saved {
            self.pdf_last = Some(p.clone());
        }
        self.status = tr!("Gebrauchtwagen-Check aus dem Scan vom {a}: {b}", a = e.aus_scan.clone().unwrap_or_default(), b = e.gesamt.text());
        self.kc_result = Some((e.punkte.clone(), e.gesamt, e.urteil().to_string(), saved));
    }

    /// Codes aus dem letzten Scan des aktiven Fahrzeugs (für die Fehlersuche).
    pub(super) fn recent_codes(&self) -> Vec<(String, String)> {
        let Some(k) = &self.active_vehicle else { return vec![] };
        ecu_werkbank::history::list(k)
            .pop()
            .map(|(_, r)| {
                r.modules
                    .iter()
                    .flat_map(|m| m.dtcs.iter().flatten().map(|d| (d.alt.clone().unwrap_or(d.code.clone()), d.text.clone())))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn open_tool(&mut self, w: ecu_werkbank::guide::Werkzeug) {
        use ecu_werkbank::guide::Werkzeug::*;
        match w {
            Gemisch => self.status = tr!("Gemisch-Assistent: weiter unten in diesem Reiter starten."),
            Pruefergebnisse => {
                self.run_mode06();
                self.status = tr!("Prüfergebnisse gelesen — weiter unten in diesem Reiter.");
            }
            Batterie => self.tab = Tab::Batterie,
            Readiness => {
                self.tab = Tab::Obd;
                self.obd_ready_live = true;
            }
            FreezeFrame => {
                self.tab = Tab::Obd;
                self.obd_freeze = Some(self.obd_open().and_then(|tp| Obd::new(&*tp).freeze_frame()).unwrap_or(None));
            }
            LiveWerte => self.tab = Tab::Scope,
            Websuche => {
                let model = self.vehicle_model();
                open_code_search(&self.gf_code.clone(), &model, &mut self.status);
            }
            FehlerLoeschen => self.status = tr!("Reparatur-Modus: in diesem Reiter oben (Schritt 3 löscht die Fehler)."),
        }
    }

    pub(super) fn rp_scan(&mut self, step: u8) {
        let be = self.backend();
        let labels_dir = self.labels_dir.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
            let _ = tx.send(ecu_werkbank::report::scan(&be, &db, true, |_| {}).map_err(|e| format!("{e:#}")));
        });
        self.rp_job = Some((step, rx));
    }

    pub(super) fn poll_repair(&mut self, ctx: &egui::Context) {
        let Some((step, rx)) = &self.rp_job else { return };
        let step = *step;
        let Ok(res) = rx.try_recv() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        self.rp_job = None;
        let mut r = match res {
            Ok(r) => r,
            Err(e) => {
                self.status = tr!("Reparatur-Modus: {e}", e = e);
                return;
            }
        };
        if r.assign_vehicle(self.active_vehicle.as_deref()).is_err() {
            self.status = tr!("Fahrzeugordner nicht verfügbar.");
            return;
        }
        if step == 1 {
            r.note = tr!("vor Reparatur");
            let _ = ecu_werkbank::history::save(&r);
            self.status = tr!("Vorher-Stand gesichert: {a} Fehler.", a = r.total_dtcs());
            self.rp_before = Some(r);
            self.rp_cleared.clear();
        } else if let Some(before) = self.rp_before.clone() {
            r.reparatur = self.rp_arbeiten.trim().to_string();
            r.comparison = ecu_werkbank::history::compare_with_previous(&r);
            let _ = ecu_werkbank::history::save(&r);
            let offen: Vec<String> = self
                .obd_open()
                .ok()
                .and_then(|tp| Obd::new(&*tp).readiness().ok())
                .map(|rd| rd.monitors.iter().filter(|m| m.available && !m.complete).map(|m| m.name.to_string()).collect())
                .unwrap_or_default();
            let path = ecu_werkbank::vehicle::load(&r.vehicle)
                .and_then(|v| v.sub("berichte").ok())
                .map(|d| d.join(ecu_werkbank::vehicle::stamped("reparaturbericht", "pdf")));
            if let Some(p) = path {
                if std::fs::write(&p, ecu_werkbank::repair::bericht_pdf(&before, &r, &self.rp_arbeiten, &offen)).is_ok() {
                    let d = ecu_werkbank::history::diff(&before, &r, &[]);
                    self.status = tr!("Reparaturbericht gespeichert ({a} behoben, {b} noch da): {c}", a = d.gone.len(), b = d.still.len(), c = p.display());
                    self.pdf_last = Some(p);
                }
            }
            self.rp_before = None;
            self.rp_arbeiten.clear();
            self.hist_refresh();
        }
    }

    pub(super) fn ui_diag_extra(&mut self, ui: &mut egui::Ui) {
        // ---------- Gebrauchtwagen-Check ----------
        ui.heading(tr!("Gebrauchtwagen-Check"));
        ui.label(tr!("Vor dem Autokauf: gelöschte Fehler, Tacho-Plausibilität, Fahrgestellnummern aller Steuergeräte, Readiness, Abgas-Tests, Batterie — mit PDF „Kaufberatung“."));
        ui.horizontal(|ui| {
            ui.add_sized([260.0, 20.0], egui::TextEdit::singleline(&mut self.kc_note).hint_text(tr!("Notiz, z. B. Verkäufer / Inserat")));
            if ui.add_enabled(self.kc_job.is_none(), egui::Button::new(tr!("🔍 Check starten"))).clicked() {
                self.start_kaufcheck();
            }
            if ui
                .add_enabled(self.kc_job.is_none() && self.active_vehicle.is_some(), egui::Button::new(tr!("📄 Aus letztem Scan (ohne Auto)")))
                .on_hover_text(tr!("Erstellt den Check aus dem letzten gespeicherten Scan des gewählten Fahrzeugs — am besten einem Schnell-Check, der enthält Readiness und Abgas-Prüfwerte."))
                .clicked()
            {
                self.kaufcheck_aus_scan();
            }
            if self.kc_job.is_some() {
                ui.spinner();
                ui.weak(&self.kc_progress);
            }
        });
        if let Some((pts, gesamt, urteil, path)) = self.kc_result.clone() {
            ui.label(egui::RichText::new(urteil).strong().color(stufe_color(gesamt)));
            for x in &pts {
                ui.horizontal_wrapped(|ui| {
                    dot(ui, stufe_color(x.stufe));
                    ui.strong(format!("{}:", x.bereich));
                    ui.label(&x.text);
                });
            }
            if let Some(p) = path {
                if ui.button(tr!("📄 Kaufberatung (PDF) öffnen")).clicked() {
                    open_path(&p, &mut self.status);
                }
            }
        }

        // ---------- Geführte Fehlersuche ----------
        ui.add_space(10.0);
        ui.separator();
        ui.heading(tr!("Geführte Fehlersuche"));
        ui.horizontal(|ui| {
            ui.label(tr!("Fehlercode:"));
            ui.add_sized([90.0, 20.0], egui::TextEdit::singleline(&mut self.gf_code).hint_text("P0300"));
            let codes = self.recent_codes();
            if !codes.is_empty() {
                egui::ComboBox::from_id_source("gfcodes").selected_text(tr!("aus letztem Scan …")).width(240.0).show_ui(ui, |ui| {
                    for (c, t) in &codes {
                        if ui.selectable_label(false, format!("{c}  {t}")).clicked() {
                            self.gf_code = c.clone();
                        }
                    }
                });
            }
        });
        let code = self.gf_code.trim().to_uppercase();
        if !code.is_empty() {
            let plan = ecu_werkbank::guide::plan_for(&code);
            if self.gf_for != code {
                self.gf_for = code.clone();
                self.gf_done = vec![false; plan.schritte.len()];
            }
            let text = self.db.dtc_description(&code).or_else(|| self.db.dtc_description(&ecu_werkbank::guide::normalize(&code))).unwrap_or("").to_string();
            ui.strong(format!("{} — {}", plan.titel, text));
            for (q, t) in ecu_werkbank::wissen::suchen(&code, &self.vehicle_model()).into_iter().filter(|(q, _)| q.starts_with("eigene")) {
                ui.colored_label(egui::Color32::from_rgb(120, 180, 240), tr!("Fehlerbild [{q}]: {t}", q = q, t = t));
            }
            for n in self.repairs_for(&code) {
                ui.colored_label(egui::Color32::from_rgb(225, 150, 50), tr!("Früher: {n}", n = n));
            }
            let mut open: Option<ecu_werkbank::guide::Werkzeug> = None;
            for (i, st) in plan.schritte.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.gf_done[i], "");
                    ui.add(egui::Label::new(format!("{}. {}", i + 1, st.text)).wrap());
                    if let Some(w) = st.werkzeug {
                        if ui.small_button(format!("▶ {}", w.name())).clicked() {
                            open = Some(w);
                        }
                    }
                });
            }
            if let Some(w) = open {
                self.open_tool(w);
            }
            ui.horizontal(|ui| {
                ui.add_sized([300.0, 20.0], egui::TextEdit::singleline(&mut self.gf_note).hint_text(tr!("Ergebnis / Ursache, z. B. Zündspule Zyl. 3 defekt")));
                if ui.button(tr!("💾 Als Reparatur-Notiz speichern")).on_hover_text(tr!("Erledigte Schritte + Notiz zum letzten Scan des aktiven Fahrzeugs")).clicked() {
                    let done: Vec<String> = plan.schritte.iter().zip(&self.gf_done).filter(|(_, d)| **d).map(|(s, _)| s.text.split(['.', ':', '—']).next().unwrap_or(s.text).to_string()).collect();
                    let note = format!("{code}: {}{}", self.gf_note.trim(), if done.is_empty() { String::new() } else { tr!(" (geprüft: {a})", a = done.join("; ")) });
                    match self.active_vehicle.clone().and_then(|k| ecu_werkbank::history::list(&k).pop()) {
                        Some((p, _)) => match ecu_werkbank::history::set_repair_note(&p, &note) {
                            Ok(()) => {
                                self.status = tr!("Notiz gespeichert.");
                                self.rep_cache.clear();
                            }
                            Err(e) => self.status = format!("{e:#}"),
                        },
                        None => self.status = tr!("Kein Scan des aktiven Fahrzeugs — erst Fehler-PDF oder Schnell-Check."),
                    }
                }
            });
        }

        // ---------- Reparatur-Modus ----------
        ui.add_space(10.0);
        ui.separator();
        ui.heading(tr!("Reparatur-Modus (vorher / nachher)"));
        let busy = self.rp_job.is_some();
        ui.horizontal(|ui| {
            let label = if self.rp_before.is_some() { tr!("1. Vorher-Stand gesichert ✔").replace('✔', "(OK)") } else { tr!("1. Vorher-Stand sichern") };
            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                self.rp_scan(1);
            }
            if busy {
                ui.spinner();
            }
            if let Some(b) = &self.rp_before {
                ui.weak(tr!("{a} · {b} Fehler", a = b.created, b = b.total_dtcs()));
            }
        });
        if self.rp_before.is_some() {
            ui.horizontal(|ui| {
                ui.label(tr!("2. Durchgeführte Arbeiten:"));
                ui.add_sized([320.0, 20.0], egui::TextEdit::singleline(&mut self.rp_arbeiten).hint_text(tr!("z. B. Zündkerzen + Zündspule Zyl. 3 erneuert")));
            });
            ui.horizontal(|ui| {
                if !self.rp_confirm_clear {
                    if ui.button(tr!("3. Alle Fehler löschen…")).clicked() {
                        self.rp_confirm_clear = true;
                    }
                } else {
                    ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Löscht alle Fehler und die Readiness."));
                    if ui.button(tr!("Ja, löschen")).clicked() {
                        if let Some(b) = self.rp_before.clone() {
                            self.rp_cleared = ecu_werkbank::repair::clear_all(&self.backend(), &b);
                        }
                        self.rp_confirm_clear = false;
                    }
                    if ui.button(tr!("Abbrechen")).clicked() {
                        self.rp_confirm_clear = false;
                    }
                }
                if ui.button(tr!("4. Probefahrt: Readiness-Assistent")).clicked() {
                    self.tab = Tab::Obd;
                    self.obd_ready_live = true;
                }
                if ui.add_enabled(!busy, egui::Button::new(tr!("5. Nachher prüfen » Reparaturbericht"))).clicked() {
                    self.rp_scan(2);
                }
            });
            for (m, r) in &self.rp_cleared {
                ui.weak(format!("  {m}: {}", match r { Ok(()) => tr!("gelöscht").to_string(), Err(e) => e.clone() }));
            }
        } else {
            ui.weak(tr!("Ablauf: vorher sichern » reparieren » Fehler löschen » Probefahrt » nachher prüfen. Ergebnis: PDF mit Behoben / Noch vorhanden."));
        }
        ui.add_space(10.0);
        ui.separator();
    }

    /// Mitschnitt-Lernhilfe (im Reiter Mitschnitt).
    pub(super) fn ui_learn(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(tr!("Lernhilfe: Label-Einträge aus einem Werkstatt-Mitschnitt")).default_open(!self.learn.is_empty()).show(ui, |ui| {
            ui.label(tr!("Mitschneiden, während ein Werkstattgerät z. B. die Leerlaufdrehzahl anhebt oder codiert. Optional einen zweiten Mitschnitt desselben Ablaufs OHNE die Aktion laden — dann werden nur die neuen Befehle vorgeschlagen."));
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr!("📂 Vergleich (ohne Aktion) laden…")).clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Log", &["log", "txt"]).pick_file() {
                        if let Ok(t) = std::fs::read_to_string(&p) {
                            self.log_cmp = Some((p.display().to_string(), t));
                        }
                    }
                }
                if let Some((n, _)) = &self.log_cmp {
                    ui.weak(tr!("Vergleich: {n}", n = n));
                    if ui.small_button("✖").clicked() {
                        self.log_cmp = None;
                    }
                }
                if ui.button(tr!("🚗 Beispiel mit Werkstatt-Aktion")).clicked() {
                    self.log_text = ecu_werkbank::canlog::demo_log_aktion();
                    self.log_cmp = Some((tr!("(Beispiel ohne Aktion)"), ecu_werkbank::canlog::demo_log()));
                    let d = ecu_werkbank::canlog::Decoder::decode_text(&self.log_text);
                    self.log_lines = d.lines().iter().map(|l| gui_text(l)).collect();
                    self.log_report = gui_text(&d.report.text());
                    self.log_path = tr!("(Beispiel mit Aktion)");
                }
                if ui.add_enabled(!self.log_text.is_empty(), egui::Button::new(tr!("💡 Vorschläge erzeugen"))).clicked() {
                    self.learn = ecu_werkbank::learn::lernen(&self.log_text, self.log_cmp.as_ref().map(|c| c.1.as_str()));
                    if self.learn.is_empty() {
                        self.status = tr!("Keine Stellfunktionen, Schreibbefehle oder Messwerte im Mitschnitt gefunden.");
                    }
                }
            });
            if self.learn.is_empty() {
                return;
            }
            for v in &self.learn {
                ui.label(format!("• {}", v.titel));
            }
            ui.horizontal(|ui| {
                ui.label(tr!("Steuergerät-Adresse:"));
                ui.add_sized([50.0, 20.0], egui::TextEdit::singleline(&mut self.learn_addr));
                ui.label(tr!("Name:"));
                ui.add_sized([180.0, 20.0], egui::TextEdit::singleline(&mut self.learn_name));
                if ui.button(tr!("💾 Als Label-Datei speichern")).clicked() {
                    let addr = parse_hex(&self.learn_addr).unwrap_or(0x01);
                    let text = ecu_werkbank::learn::als_label_datei(&self.learn, addr, &self.learn_name);
                    match ecu_werkbank::labels::LabelDb::check_file(&text) {
                        Ok(()) => {
                            let p = self.labels_dir.join(ecu_werkbank::vehicle::stamped("gelernt", "toml"));
                            match std::fs::write(&p, text) {
                                Ok(()) => {
                                    self.reload_labels();
                                    self.status = tr!("Gespeichert: {a} — im Label-Editor Namen/Einheiten/Grenzen prüfen.", a = p.display());
                                }
                                Err(e) => self.status = format!("{e}"),
                            }
                        }
                        Err(e) => self.status = tr!("Vorschlag ungültig: {e}", e = e),
                    }
                }
            });
            let preview = ecu_werkbank::learn::als_label_datei(&self.learn, parse_hex(&self.learn_addr).unwrap_or(1), &self.learn_name);
            egui::ScrollArea::vertical().id_source("learnprev").max_height(220.0).show(ui, |ui| {
                ui.monospace(preview);
            });
        });
        ui.add_space(4.0);
    }
}
