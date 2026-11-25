//! GUI: Diagnose (Mode 06), Gemisch-Assistent, Trends, Reparaturen

use super::*;

impl App {
    /// Frühere Reparaturen zu einem Code (gecacht je aktivem Fahrzeug).
    pub(super) fn repairs_for(&mut self, code: &str) -> Vec<String> {
        let Some(key) = self.active_vehicle.clone() else { return vec![] };
        if self.rep_cache_for.as_deref() != Some(key.as_str()) {
            self.rep_cache.clear();
            self.rep_cache_for = Some(key.clone());
        }
        self.rep_cache
            .entry(code.to_string())
            .or_insert_with(|| {
                let mut v = ecu_werkbank::history::earlier_repairs(&key, code);
                v.extend(ecu_werkbank::history::other_vehicle_repairs(&key, code));
                v.extend(ecu_werkbank::history::erfahrung(code).kurz());
                v
            })
            .clone()
    }

    pub(super) fn run_mode06(&mut self) {
        let be = self.backend();
        self.im_hintergrund(tr!("Prüfergebnisse lesen"), move || {
            let r = be.open(obd::DEFAULT_TX, obd::DEFAULT_RX).and_then(|tp| Obd::new(&*tp).test_results());
            Box::new(move |app: &mut App| match r {
                Ok(t) => {
                    app.m6_summary = obd::test_summary(&t);
                    app.status = tr!("{a} Prüfergebnisse gelesen.", a = t.len());
                    app.m6_results = t;
                }
                Err(e) => app.status = tr!("Prüfergebnisse: {e}", e = format!("{:#}", e)),
            })
        });
    }

    pub(super) fn gm_begin(&mut self) {
        self.gm_phase = 1;
        self.gm_start = std::time::Instant::now();
        self.gm_idle.clear();
        self.gm_high.clear();
        self.gm_result.clear();
        if matches!(self.backend(), Backend::Demo) {
            ecu_werkbank::sim::set_muster(ecu_werkbank::sim::Param::Drehzahl, Some(ecu_werkbank::control::Muster::Halten(800.0)));
        }
    }

    pub(super) fn poll_gemisch(&mut self, ctx: &egui::Context) {
        if !(1..=2).contains(&self.gm_phase) {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if self.gm_last.elapsed() < std::time::Duration::from_millis(300) {
            return;
        }
        let text = tr!("Gemisch messen");
        if self.bus.wartend.contains(&text) {
            return;
        }
        self.gm_last = std::time::Instant::now();
        let be = self.backend();
        self.im_hintergrund_still(text, move || {
            let r = be.open(obd::DEFAULT_TX, obd::DEFAULT_RX).and_then(|tp| ecu_werkbank::mixture::sample(&Obd::new(&*tp)));
            Box::new(move |app: &mut App| app.gemisch_auswerten(r))
        });
    }

    /// Eine Gemisch-Messung auswerten (im Zeichen-Thread).
    fn gemisch_auswerten(&mut self, r: anyhow::Result<ecu_werkbank::mixture::Sample>) {
        if !(1..=2).contains(&self.gm_phase) {
            return; // inzwischen abgebrochen
        }
        let s = match r {
            Ok(s) => s,
            Err(e) => {
                self.gm_phase = 0;
                self.status = tr!("Gemisch-Assistent: {e}", e = format!("{:#}", e));
                return;
            }
        };
        self.gm_now = Some(s);
        let done = self.gm_start.elapsed() >= std::time::Duration::from_secs(u64::from(self.gm_seconds));
        if self.gm_phase == 1 {
            self.gm_idle.push(s);
            if done {
                self.gm_phase = 2;
                self.gm_start = std::time::Instant::now();
                if matches!(self.backend(), Backend::Demo) {
                    ecu_werkbank::sim::set_muster(ecu_werkbank::sim::Param::Drehzahl, Some(ecu_werkbank::control::Muster::Halten(2500.0)));
                }
                beep();
            }
        } else {
            if (s.rpm - 2500.0).abs() < 400.0 {
                self.gm_high.push(s);
            }
            if done {
                self.gm_phase = 3;
                if matches!(self.backend(), Backend::Demo) {
                    ecu_werkbank::sim::clear_muster();
                }
                beep();
                if self.gm_high.is_empty() {
                    self.gm_result = vec![(Stufe::Hinweis, tr!("Phase 2 nicht bei ca. 2500 1/min gemessen — bitte wiederholen."))];
                    return;
                }
                let veh = self.active_vehicle.as_deref().and_then(ecu_werkbank::vehicle::load);
                let (a, b) = (ecu_werkbank::mixture::summarize(&self.gm_idle), ecu_werkbank::mixture::summarize(&self.gm_high));
                self.gm_result = ecu_werkbank::mixture::analyze(&a, &b, veh.as_ref().and_then(|v| v.hubraum_l));
                if let Some(v) = veh {
                    let mut tr = vec![(tr!("Gemisch Leerlauf").to_string(), a.total(), "%".to_string()), (tr!("Gemisch 2500").to_string(), b.total(), "%".to_string())];
                    if let Some(m) = a.maf {
                        tr.push((tr!("Luftmasse Leerlauf"), m, "g/s".into()));
                    }
                    let _ = ecu_werkbank::trend::add(&v.key, v.km, &tr);
                }
            }
        }
    }

    pub(super) fn ui_diagnose(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_source("diagscroll").show(ui, |ui| {
            self.ui_diag_extra(ui);
            // ---------- Mode 06 ----------
            ui.heading(tr!("Prüfergebnisse der Abgas-Überwachung (OBD-II Mode 06)"));
            ui.label(tr!("Zeigt, wie knapp Katalysator, Lambdasonden & Co. an ihrer Fehlergrenze sind — bevor die Motorleuchte angeht. Aussetzer-Zähler je Zylinder werden verglichen."));
            if ui.button(tr!("ℹ Prüfergebnisse lesen")).clicked() {
                self.run_mode06();
            }
            for (st, t) in self.m6_summary.clone() {
                ui.horizontal_wrapped(|ui| {
                    dot(ui, stufe_color(st));
                    ui.label(t);
                });
            }
            if !self.m6_results.is_empty() {
                egui::Grid::new("m6").striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
                    for h in ["", trn!("Test"), trn!("Wert"), trn!("Grenzen"), trn!("Reserve")] {
                        ui.weak(ecu_werkbank::i18n::t(h));
                    }
                    ui.end_row();
                    for t in &self.m6_results {
                        dot(ui, stufe_color(t.stufe()));
                        ui.label(t.name());
                        ui.label(format!("{:.1} {}", t.value, t.unit));
                        ui.weak(if t.has_limits() { format!("{:.1} – {:.1}", t.min, t.max) } else { "—".into() });
                        match t.reserve().filter(|_| t.has_limits()) {
                            Some(r) => {
                                ui.add(egui::ProgressBar::new((r / 100.0).clamp(0.0, 1.0) as f32).desired_width(120.0).text(format!("{r:.0} %")));
                            }
                            None => {
                                ui.weak("");
                            }
                        }
                        ui.end_row();
                    }
                });
            }

            // ---------- Gemisch ----------
            ui.add_space(10.0);
            ui.separator();
            ui.heading(tr!("Gemisch-Assistent (Falschluft, Kraftstoffdruck …)"));
            ui.label(tr!("Motor betriebswarm, Verbraucher aus, Fahrzeug steht. Phase 1: Leerlauf. Phase 2: ca. 2500 1/min halten. Danach zeigt der Assistent die wahrscheinliche Ursache."));
            ui.horizontal(|ui| {
                ui.label(tr!("Dauer je Phase:"));
                ui.add(egui::DragValue::new(&mut self.gm_seconds).range(3..=120).suffix(" s"));
                let running = (1..=2).contains(&self.gm_phase);
                if ui.add_enabled(!running, egui::Button::new(tr!("▶ Starten"))).clicked() {
                    self.gm_begin();
                }
                if running && ui.button(tr!("⏹ Abbrechen")).clicked() {
                    self.gm_phase = 0;
                    if matches!(self.backend(), Backend::Demo) {
                        ecu_werkbank::sim::clear_muster();
                    }
                }
            });
            if (1..=2).contains(&self.gm_phase) {
                let left = u64::from(self.gm_seconds).saturating_sub(self.gm_start.elapsed().as_secs());
                let title = if self.gm_phase == 1 { tr!("Phase 1: Leerlauf — nichts tun ({left} s)", left = left) } else { tr!("Phase 2: ca. 2500 1/min halten ({left} s)", left = left) };
                ui.label(egui::RichText::new(title).heading().strong());
                if let Some(s) = self.gm_now {
                    if self.gm_phase == 2 {
                        draw_band(ui, s.rpm, 2500.0, 250.0, "1/min");
                    }
                    ui.label(tr!("Drehzahl {a} 1/min · Korrektur kurz {b} % · lang {c} % · gesamt {d} %", a = format!("{:.0}", s.rpm), b = format!("{:+.1}", s.stft), c = format!("{:+.1}", s.ltft), d = format!("{:+.1}", s.stft + s.ltft)));
                }
            }
            for (st, t) in self.gm_result.clone() {
                ui.horizontal_wrapped(|ui| {
                    dot(ui, stufe_color(st));
                    ui.label(t);
                });
            }
            if self.gm_phase == 3 && self.active_vehicle.is_some() {
                ui.weak(tr!("Ergebnis in den Trends des Fahrzeugs gespeichert (Reiter Historie, Abschnitt Trends)."));
            }
        });
    }

    /// Trends im Historie-Reiter.
    pub(super) fn ui_trends(&mut self, ui: &mut egui::Ui) {
        let series = ecu_werkbank::trend::series(&self.hist_vehicle);
        if series.is_empty() {
            return;
        }
        egui::CollapsingHeader::new(tr!("Trends ({a} Messwerte)", a = series.len())).default_open(false).show(ui, |ui| {
            let hints = ecu_werkbank::trend::analyze(&series);
            if hints.is_empty() {
                ui.weak(tr!("Keine schleichenden Veränderungen erkannt."));
            }
            for (st, t) in hints {
                ui.horizontal_wrapped(|ui| {
                    dot(ui, stufe_color(st));
                    ui.label(t);
                });
            }
            if !series.contains_key(&self.tr_sel) {
                self.tr_sel = series.keys().next().cloned().unwrap_or_default();
            }
            ui.horizontal(|ui| {
                ui.label(tr!("Verlauf:"));
                egui::ComboBox::from_id_source("trsel").selected_text(self.tr_sel.clone()).width(260.0).show_ui(ui, |ui| {
                    for k in series.keys() {
                        ui.selectable_value(&mut self.tr_sel, k.clone(), k);
                    }
                });
            });
            if let Some(pts) = series.get(&self.tr_sel) {
                let t0 = pts.first().map(|p| p.ts).unwrap_or(0);
                let line: Vec<[f64; 2]> = pts.iter().map(|p| [(p.ts.saturating_sub(t0)) as f64 / 86_400.0, p.wert]).collect();
                let unit = pts.last().map(|p| p.einheit.clone()).unwrap_or_default();
                egui_plot::Plot::new("trendplot").height(160.0).x_axis_label(tr!("Tage seit erster Messung")).y_axis_label(unit).show(ui, |pui| {
                    pui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(line.clone())).width(2.0_f32));
                    pui.points(egui_plot::Points::new(egui_plot::PlotPoints::from(line)).radius(3.0_f32));
                });
            }
        });
    }
}
