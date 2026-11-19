//! GUI: Batterietest

use super::*;

impl App {
    /// Fahrzeugmodell für die Websuche.
    pub(super) fn vehicle_model(&self) -> String {
        self.active_vehicle
            .as_deref()
            .and_then(ecu_werkbank::vehicle::load)
            .map(|v| v.bezeichnung)
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "VW".into())
    }

    // ----- Schnell-Check -----
    pub(super) fn start_check(&mut self) {
        if self.chk_job.is_some() {
            return;
        }
        let be = self.backend();
        let labels_dir = self.labels_dir.clone();
        let active = self.active_vehicle.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
            let res = ecu_werkbank::check::run(&be, &db, active.as_deref(), |p| {
                let _ = tx.send(ChkMsg::Progress(p.to_string()));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(ChkMsg::Done(res));
        });
        self.chk_job = Some(rx);
        self.chk_progress = tr!("Schnell-Check läuft…");
    }

    pub(super) fn poll_check(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.chk_job else { return };
        let mut done = None;
        while let Ok(m) = rx.try_recv() {
            match m {
                ChkMsg::Progress(p) => self.chk_progress = p,
                ChkMsg::Done(r) => done = Some(r),
            }
        }
        let Some(res) = done else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        self.chk_job = None;
        match res {
            Ok(mut c) => {
                if c.report.km.is_none() {
                    c.report.km = self.pdf_km.trim().replace('.', "").parse().ok();
                }
                c.report.note = self.pdf_note.trim().to_string();
                let path = ecu_werkbank::vehicle::load(&c.report.vehicle)
                    .and_then(|v| v.sub("berichte").ok())
                    .map(|d| d.join(ecu_werkbank::vehicle::stamped("schnellcheck", "pdf")));
                let saved = path.and_then(|p| c.report.save(&p).ok().map(|_| p));
                if c.report.vehicle != ecu_werkbank::vehicle::UNKNOWN && self.active_vehicle.as_deref() != Some(c.report.vehicle.as_str()) {
                    self.set_active_vehicle(Some(c.report.vehicle.clone()));
                }
                if let Some(p) = &saved {
                    self.pdf_last = Some(p.clone());
                }
                self.status = tr!("Schnell-Check: {a}", a = c.gesamt.text());
                self.chk_result = Some((c.report.check.clone(), c.gesamt, saved));
                self.hist_refresh();
            }
            Err(e) => self.status = tr!("Schnell-Check fehlgeschlagen: {e}", e = e),
        }
    }

    pub(super) fn check_window(&mut self, ctx: &egui::Context) {
        let Some((items, gesamt, path)) = self.chk_result.clone() else { return };
        let mut open = true;
        egui::Window::new(tr!("Schnell-Check")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let (title, col) = match gesamt {
                Stufe::Gut => (tr!("Alles in Ordnung"), stufe_color(Stufe::Gut)),
                Stufe::Hinweis => (tr!("Hinweise"), stufe_color(Stufe::Hinweis)),
                Stufe::Warnung => (tr!("Bitte prüfen"), stufe_color(Stufe::Warnung)),
            };
            ui.label(egui::RichText::new(title).heading().strong().color(col));
            ui.add_space(4.0);
            for (st, t) in &items {
                ui.horizontal(|ui| {
                    dot(ui, stufe_color(*st));
                    ui.label(t);
                });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if let Some(p) = &path {
                    if ui.button(tr!("📄 PDF öffnen")).clicked() {
                        open_path(p, &mut self.status);
                    }
                }
                if ui.button(tr!("Historie")).clicked() {
                    self.tab = Tab::Historie;
                }
            });
        });
        if !open {
            self.chk_result = None;
        }
    }

    // ----- Readiness live -----
    pub(super) fn poll_readiness(&mut self, ctx: &egui::Context) {
        if !self.obd_ready_live {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        if self.obd_ready_last.elapsed() < std::time::Duration::from_secs(2) {
            return;
        }
        let text = tr!("Readiness verfolgen");
        if self.bus.wartend.contains(&text) {
            return;
        }
        self.obd_ready_last = std::time::Instant::now();
        let be = self.backend();
        self.im_hintergrund_still(text, move || {
            let r = be.open(obd::DEFAULT_TX, obd::DEFAULT_RX).and_then(|tp| ecu_werkbank::readiness::lesen(&Obd::new(&*tp)));
            Box::new(move |app: &mut App| app.readiness_auswerten(r))
        });
    }

    /// Readiness-Stand übernehmen; meldet frisch fertige Abgas-Tests.
    fn readiness_auswerten(&mut self, r: anyhow::Result<ecu_werkbank::readiness::Stand>) {
        if !self.obd_ready_live {
            return;
        }
        match r {
            Ok(st) => {
                let r = st.readiness.clone();
                let was_open = self.obd_readiness.as_ref().map(|x| !x.all_complete()).unwrap_or(false);
                if was_open && r.all_complete() {
                    self.status = tr!("Readiness: alle Abgas-Tests fertig");
                    notify("ECUWerkbank", "Readiness: alle Abgas-Tests fertig");
                } else if let Some(alt) = &self.obd_readiness {
                    // einzelner Test gerade fertig geworden
                    for m in r.monitors.iter().filter(|m| m.available && m.complete) {
                        if alt.monitors.iter().any(|a| a.name == m.name && a.available && !a.complete) {
                            self.status = tr!("Readiness: {a} jetzt fertig", a = m.name);
                        }
                    }
                }
                self.obd_readiness = Some(r);
                self.obd_stand = Some(st);
            }
            Err(e) => {
                self.obd_ready_live = false;
                self.status = tr!("Readiness: {e}", e = format!("{:#}", e));
            }
        }
    }

    // ----- Batterietest -----
    pub(super) fn start_battery(&mut self) {
        if self.bat_job.is_some() {
            return;
        }
        let be = self.backend();
        let secs = f64::from(self.bat_seconds);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let res = ecu_werkbank::battery::measure(&be, secs, move |t, v| {
                let _ = tx2.send(BatMsg::Sample(t, v));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(BatMsg::Done(res));
        });
        self.bat_points.clear();
        self.bat_result = None;
        self.bat_job = Some(rx);
        self.status = tr!("Batterietest läuft — nach 2 Sekunden den Motor starten.");
    }

    pub(super) fn poll_battery(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.bat_job else { return };
        let mut done = None;
        while let Ok(m) = rx.try_recv() {
            match m {
                BatMsg::Sample(t, v) => self.bat_points.push([t, v]),
                BatMsg::Done(r) => done = Some(r),
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        let Some(res) = done else { return };
        self.bat_job = None;
        match res {
            Ok(pts) => {
                let r = ecu_werkbank::battery::analyze(&pts);
                self.status = tr!("Batterietest: {a}", a = r.gesamt().text());
                self.bat_points = pts;
                if let Some(v) = self.active_vehicle.as_deref().and_then(ecu_werkbank::vehicle::load) {
                    let mut tr = Vec::new();
                    if let Some(x) = r.ruhe { tr.push(("Ruhespannung".to_string(), x, "V".to_string())); }
                    if let Some(x) = r.minimum { tr.push((tr!("Startspannung min").to_string(), x, "V".to_string())); }
                    if let Some(x) = r.laden { tr.push((tr!("Ladespannung").to_string(), x, "V".to_string())); }
                    let _ = ecu_werkbank::trend::add(&v.key, v.km, &tr);
                }
                self.bat_result = Some(r);
            }
            Err(e) => self.status = tr!("Batterietest: {e}", e = e),
        }
    }

    pub(super) fn battery_pdf(&mut self) {
        let Some(r) = &self.bat_result else { return };
        let mut meta = vec![(tr!("Datum").to_string(), ecu_werkbank::report::now_local()), (tr!("Ergebnis").to_string(), r.gesamt().text().to_string())];
        if let Some(v) = self.active_vehicle.as_deref().and_then(ecu_werkbank::vehicle::load) {
            meta.insert(0, (tr!("Fahrzeug"), v.label()));
        }
        for (i, (_, t)) in r.bewertung.iter().enumerate() {
            meta.push((tr!("Bewertung {a}", a = i + 1), t.clone()));
        }
        let col = ecu_werkbank::csvlog::Column {
            name: tr!("Bordspannung"),
            unit: "V".into(),
            points: self.bat_points.clone(),
            soll: Some(ecu_werkbank::limits::Range { min: Some(9.6), max: Some(14.9) }),
        };
        let Some(dir) = self.vehicle_sub("berichte") else { return };
        let p = dir.join(ecu_werkbank::vehicle::stamped("batterietest", "pdf"));
        match ecu_werkbank::ablage::schreiben(&p, ecu_werkbank::chart::to_pdf(ecu_werkbank::trs!("Batterie- und Starttest"), &meta, &[col])) {
            Ok(()) => {
                self.status = tr!("PDF gespeichert: {a}", a = p.display());
                self.pdf_last = Some(p);
            }
            Err(e) => self.status = tr!("PDF: {e}", e = e),
        }
    }

    pub(super) fn ui_batterie(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Batterie- und Starttest"));
        ui.label(tr!("1. Zündung AN, Motor AUS.  2. Test starten.  3. Nach 2 Sekunden den Motor starten und laufen lassen."));
        ui.weak(tr!("Gemessen wird die Spannung am Motorsteuergerät (OBD-II). Ruhestrom (Entladung über Nacht) braucht ein Strommessgerät."));
        ui.horizontal(|ui| {
            ui.label(tr!("Dauer:"));
            ui.add(egui::DragValue::new(&mut self.bat_seconds).range(8..=60).suffix(" s"));
            if ui.add_enabled(self.bat_job.is_none(), egui::Button::new(tr!("▶ Test starten"))).clicked() {
                self.start_battery();
            }
            if self.bat_result.is_some() && ui.button(tr!("📄 Als PDF")).clicked() {
                self.battery_pdf();
            }
        });
        if self.bat_job.is_some() {
            let t = self.bat_points.last().map(|p| p[0]).unwrap_or(0.0);
            let msg = if t < 2.0 { tr!("Messung läuft … gleich starten") } else { tr!(">>> JETZT MOTOR STARTEN <<<") };
            ui.label(egui::RichText::new(msg).heading().strong().color(stufe_color(if t < 2.0 { Stufe::Hinweis } else { Stufe::Warnung })));
            if let Some(p) = self.bat_points.last() {
                ui.label(egui::RichText::new(format!("{:.2} V", p[1])).heading());
            }
        }
        if let Some(r) = &self.bat_result {
            ui.add_space(4.0);
            for (st, t) in &r.bewertung {
                ui.horizontal(|ui| {
                    dot(ui, stufe_color(*st));
                    ui.label(t);
                });
            }
        }
        if !self.bat_points.is_empty() {
            let pts: Vec<[f64; 2]> = self.bat_points.clone();
            egui_plot::Plot::new("battery")
                .height(260.0)
                .x_axis_label("t [s]")
                .y_axis_label("U [V]")
                .include_y(9.0)
                .include_y(15.0)
                .show(ui, |pui| {
                    pui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(pts)).name(tr!("Bordspannung")).width(2.0_f32));
                    for (y, name) in [(9.6, tr!("min. beim Start")), (12.4, tr!("Ruhe gut")), (13.6, tr!("Laden"))] {
                        pui.hline(egui_plot::HLine::new(y).name(name).width(1.0_f32));
                    }
                });
        }
    }
}
