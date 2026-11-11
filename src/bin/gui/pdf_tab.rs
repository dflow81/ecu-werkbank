//! GUI: Fehlerspeicher-Protokoll als PDF

use super::*;

impl App {
    pub(super) fn start_pdf(&mut self) {
        if self.pdf_job.is_some() {
            return;
        }
        self.start_pdf_to(None);
    }

    /// Scan starten; `path` = None » PDF automatisch in den Fahrzeugordner.
    pub(super) fn start_pdf_to(&mut self, path: Option<PathBuf>) {
        let be = self.backend();
        let labels_dir = self.labels_dir.clone();
        let voll = self.voll_scan;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
            let res = ecu_werkbank::report::scan_opts(&be, &db, true, voll, |p| {
                let _ = tx.send(PdfMsg::Progress(p.to_string()));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(PdfMsg::Done(res));
        });
        self.pdf_job = Some((path, rx));
        self.pdf_progress = tr!("Steuergeräte werden abgefragt…");
        self.status = tr!("Fehlerspeicher-Protokoll über {a} …", a = self.backend().describe());
    }

    pub(super) fn poll_pdf(&mut self, ctx: &egui::Context) {
        let Some((path, rx)) = &self.pdf_job else { return };
        let path = path.clone();
        let mut done = None;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                PdfMsg::Progress(p) => self.pdf_progress = p,
                PdfMsg::Done(r) => done = Some(r),
            }
        }
        match done {
            None => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Some(res) => {
                self.pdf_job = None;
                self.pdf_progress.clear();
                match res {
                    Ok(mut report) => {
                        report.note = self.pdf_note.trim().to_string();
                        if report.km.is_none() {
                            report.km = self.pdf_km.trim().replace('.', "").parse().ok();
                        }
                        let veh = match report.assign_vehicle(self.active_vehicle.as_deref()) {
                            Ok(v) => v,
                            Err(e) => {
                                self.status = tr!("Fahrzeugordner: {e}", e = format!("{:#}", e));
                                return;
                            }
                        };
                        if self.active_vehicle.as_deref() != Some(veh.key.as_str()) && veh.key != ecu_werkbank::vehicle::UNKNOWN {
                            self.set_active_vehicle(Some(veh.key.clone()));
                        }
                        let path = match path {
                            Some(p) => p,
                            None => match veh.sub("berichte") {
                                Ok(d) => d.join(ecu_werkbank::vehicle::stamped("fehlerspeicher", "pdf")),
                                Err(e) => {
                                    self.status = tr!("Fahrzeugordner: {e}", e = format!("{:#}", e));
                                    return;
                                }
                            },
                        };
                        report.comparison = ecu_werkbank::history::compare_with_previous(&report);
                        if !report.modules.is_empty() {
                            if let Err(e) = ecu_werkbank::history::save(&report) {
                                self.status = tr!("Historie: {e}", e = format!("{:#}", e));
                            }
                        }
                        match report.save(&path) {
                            Ok(()) => {
                                let cmp = match &report.comparison {
                                    Some(c) => tr!(", seit {a}: {b} neu, {c} behoben", a = c.since, b = c.diff.new.len(), c = c.diff.gone.len()),
                                    None => String::new(),
                                };
                                self.status = tr!("PDF für {a} gespeichert: {b} ({c} Steuergerät(e), {d} Fehler{cmp})", a = veh.label(), b = path.display(), c = report.modules.len(), d = report.total_dtcs(), cmp = cmp);
                                self.pdf_last = Some(path);
                                self.hist_refresh();
                            }
                            Err(e) => self.status = tr!("PDF-Fehler: {e}", e = format!("{:#}", e)),
                        }
                    }
                    Err(e) => self.status = tr!("Scan fehlgeschlagen: {e}", e = e),
                }
            }
        }
    }
}
