//! GUI: Zielbereich (Fahrer-Assistent, Stellfunktion, Simulation)

use super::*;

impl App {
    pub(super) fn zl_toggle(&mut self) {
        self.zl_running = !self.zl_running;
        if self.zl_running {
            self.zl_start = std::time::Instant::now();
            self.zl_ist.clear();
            self.zl_soll.clear();
            self.zl_n = 0;
            self.zl_in = 0;
            self.zl_out_since = None;
        }
    }

    pub(super) fn poll_ziel(&mut self, ctx: &egui::Context) {
        if !self.zl_running {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        if self.zl_last.elapsed() < std::time::Duration::from_millis(200) {
            return;
        }
        self.zl_last = std::time::Instant::now();
        let t = self.zl_start.elapsed().as_secs_f64();
        let soll = self.zl_m.muster().ziel(t);
        match self.obd_open().and_then(|tp| Obd::new(&*tp).read_value(self.zl_pid)) {
            Ok(ist) => {
                use ecu_werkbank::control::{bewerten, Richtung};
                let (r, st) = bewerten(ist, soll, self.zl_tol);
                self.zl_n += 1;
                if r == Richtung::Passt {
                    self.zl_in += 1;
                    self.zl_out_since = None;
                } else {
                    let since = *self.zl_out_since.get_or_insert_with(std::time::Instant::now);
                    if self.zl_beep && since.elapsed() > std::time::Duration::from_secs(2) && self.zl_last_beep.elapsed() > std::time::Duration::from_secs(3) {
                        beep();
                        self.zl_last_beep = std::time::Instant::now();
                    }
                }
                self.zl_ist.push([t, ist]);
                self.zl_soll.push([t, soll]);
                if self.zl_ist.len() > 3000 {
                    self.zl_ist.remove(0);
                    self.zl_soll.remove(0);
                }
                self.zl_now = Some((ist, soll));
                // Handy-Anzeige mitversorgen
                if self.web_server.is_some() {
                    let (name, unit) = obd::pid_info(self.zl_pid).unwrap_or(("Wert", ""));
                    let state = match st {
                        Stufe::Gut => "gut",
                        Stufe::Hinweis => "hinweis",
                        Stufe::Warnung => "warnung",
                    };
                    let mut d = self.web_data.lock().unwrap();
                    d.tiles = vec![
                        ecu_werkbank::web::Tile { name: name.into(), unit: unit.into(), value: Some(ist), state: state.into() },
                        ecu_werkbank::web::Tile { name: tr!("Ziel"), unit: unit.into(), value: Some(soll), state: String::new() },
                    ];
                    d.alarm = match r {
                        Richtung::Mehr => tr!("MEHR GAS"),
                        Richtung::Weniger => tr!("WENIGER GAS"),
                        Richtung::Passt => String::new(),
                    };
                }
            }
            Err(e) => {
                self.zl_running = false;
                self.status = tr!("Zielbereich: {e}", e = format!("{:#}", e));
            }
        }
    }

    pub(super) fn st_functions(&self) -> Vec<ecu_werkbank::control::Stellfunktion> {
        let mut v = Vec::new();
        if matches!(self.backend(), Backend::Demo) {
            v.push(ecu_werkbank::control::demo_stellfunktion());
        }
        v.extend(self.db.stellfunktionen().iter().cloned());
        v
    }

    pub(super) fn st_start(&mut self) {
        let list = self.st_functions();
        let Some(f) = list.get(self.st_sel).cloned() else {
            self.status = tr!("Keine Stellfunktion gewählt.");
            return;
        };
        if !self.st_confirm {
            self.status = tr!("Bitte erst bestätigen: Fahrzeug steht, Handbremse angezogen, Gang raus/P.");
            return;
        }
        match ecu_werkbank::control::Regler::start(&self.backend(), f) {
            Ok(r) => {
                self.status = tr!("{a} läuft — NOT-AUS gibt die Kontrolle sofort zurück.", a = r.f.name);
                self.st_regler = Some(r);
                self.st_ist.clear();
                self.st_soll.clear();
            }
            Err(e) => self.status = tr!("Nicht gestartet: {e}", e = format!("{:#}", e)),
        }
    }

    pub(super) fn st_stop(&mut self, why: &str) {
        if let Some(mut r) = self.st_regler.take() {
            r.stop();
            self.status = tr!("{why} — Kontrolle ans Steuergerät zurückgegeben.", why = why);
        }
        self.st_confirm = false;
    }

    pub(super) fn poll_stellen(&mut self, ctx: &egui::Context) {
        let Some(r) = self.st_regler.as_mut() else { return };
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        if self.st_last.elapsed() < std::time::Duration::from_millis(250) {
            return;
        }
        self.st_last = std::time::Instant::now();
        let t = r.laufzeit();
        if t >= f64::from(self.st_seconds) {
            self.st_stop("Dauer erreicht");
            return;
        }
        let soll = self.st_m.muster().ziel(t);
        match r.schritt(soll) {
            Ok(m) => {
                self.st_soll.push([t, m.soll]);
                if let Some(i) = m.ist {
                    self.st_ist.push([t, i]);
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                if ecu_werkbank::control::ist_abbruch(&e) {
                    self.st_stop(&msg);
                } else {
                    self.status = tr!("Stellfunktion: {msg} (wird wiederholt)", msg = msg);
                }
            }
        }
    }

    pub(super) fn ui_ziel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_source("zielscroll").show(ui, |ui| {
            // ---------- 1. Fahrer-Assistent ----------
            ui.heading(tr!("Zielbereich — Fahrer-Assistent"));
            ui.label(tr!("Du hältst den Wert selbst (Gaspedal). Die Anzeige sagt, ob du im Bereich bist — z. B. für Regenerations-, Readiness- oder Messfahrten."));
            ui.horizontal_wrapped(|ui| {
                platz(ui, 230.0);
                ui.label(tr!("Wert:"));
                egui::ComboBox::from_id_source("zlpid")
                    .selected_text(obd::pid_info(self.zl_pid).map(|(n, _)| n).unwrap_or("PID"))
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for pid in [0x0C, 0x0D, 0x05, 0x04, 0x11, 0x0B, 0x10, 0x42] {
                            if let Some((n, _)) = obd::pid_info(pid) {
                                ui.selectable_value(&mut self.zl_pid, pid, n);
                            }
                        }
                    });
                ui.label(tr!("Toleranz ±"));
                let unit = obd::pid_info(self.zl_pid).map(|(_, u)| u).unwrap_or("");
                ui.add(egui::DragValue::new(&mut self.zl_tol).speed(1.0).range(0.1..=10_000.0).suffix(format!(" {unit}")));
                ui.checkbox(&mut self.zl_beep, tr!("Ton, wenn länger daneben"));
            });
            let unit = obd::pid_info(self.zl_pid).map(|(_, u)| u).unwrap_or("").to_string();
            self.zl_m.ui(ui, "zlmode", &unit);
            ui.horizontal(|ui| {
                if ui.button(if self.zl_running { tr!("⏹ Stop") } else { tr!("▶ Start") }).clicked() {
                    self.zl_toggle();
                }
                if self.zl_n > 0 {
                    ui.label(tr!("Im Bereich: {a} % der Zeit", a = format!("{:.0}", f64::from(self.zl_in) * 100.0 / f64::from(self.zl_n))));
                }
                if self.web_server.is_some() {
                    ui.weak(tr!("(wird auch auf dem Handy angezeigt)"));
                }
            });
            if let Some((ist, soll)) = self.zl_now.filter(|_| self.zl_running || !self.zl_ist.is_empty()) {
                draw_band(ui, ist, soll, self.zl_tol, &unit);
            }
            if !self.zl_ist.is_empty() {
                let tol = self.zl_tol;
                let ist = self.zl_ist.clone();
                let hi: Vec<[f64; 2]> = self.zl_soll.iter().map(|p| [p[0], p[1] + tol]).collect();
                let lo: Vec<[f64; 2]> = self.zl_soll.iter().map(|p| [p[0], p[1] - tol]).collect();
                egui_plot::Plot::new("zlplot").height(180.0).legend(egui_plot::Legend::default()).show(ui, |pui| {
                    pui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(hi)).name(tr!("Bereich")).color(egui::Color32::from_rgb(60, 150, 85)).width(1.0_f32));
                    pui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(lo)).name(tr!("Bereich")).color(egui::Color32::from_rgb(60, 150, 85)).width(1.0_f32));
                    pui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(ist)).name(tr!("Ist")).width(2.0_f32));
                });
            }

            // ---------- 2. Steuergerät stellt selbst ----------
            ui.add_space(10.0);
            ui.separator();
            ui.heading(tr!("Steuergerät stellt selbst (Werkstattfunktion)"));
            ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Nur im Stand! Abbruch automatisch bei Fahrt, Überschreitung, Fehlern oder Zeitlimit — und jederzeit mit NOT-AUS."));
            let list = self.st_functions();
            if list.is_empty() {
                ui.label(tr!("Noch keine Stellfunktion für dein Fahrzeug. Die Kennungen sind je Motorsteuergerät verschieden und kommen aus einem Mitschnitt (Reiter „Mitschnitt“): ein Werkstattgerät hebt die Leerlaufdrehzahl an, ecu-werkbank zeichnet auf, die Kennung wird in eine Label-Datei eingetragen:"));
                ui.monospace(tr!("[[stellfunktion]]\nname = \"Leerlaufdrehzahl anheben\"\nart = \"uds-io\"\ntx = 0x7E0\nrx = 0x7E8\nid = 0x....   # aus dem Mitschnitt\nmin = 800.0\nmax = 1500.0"));
                ui.weak(tr!("Zum Ausprobieren: oben „🚗 Demo-Fahrzeug“ wählen — das hat eine Demo-Stellfunktion."));
            } else {
                let running = self.st_regler.is_some();
                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr!("Funktion:"));
                        let cur = list.get(self.st_sel).map(|f| f.name.clone()).unwrap_or_default();
                        egui::ComboBox::from_id_source("stsel").selected_text(cur).width(260.0).show_ui(ui, |ui| {
                            for (i, f) in list.iter().enumerate() {
                                ui.selectable_value(&mut self.st_sel, i, &f.name);
                            }
                        });
                    });
                    if let Some(f) = list.get(self.st_sel) {
                        ui.weak(tr!("{a} {b} an {c} · erlaubt {d}–{e} {f} · max. {g} s", a = f.art, b = format!("{:#06x}", f.id), c = format!("{:#05x}", f.tx), d = f.min, e = f.max, f = f.einheit, g = f.max_dauer_s));
                        let unit = f.einheit.clone();
                        self.st_m.ui(ui, "stmode", &unit);
                    }
                    ui.horizontal(|ui| {
                        ui.label(tr!("Dauer:"));
                        ui.add(egui::DragValue::new(&mut self.st_seconds).range(5..=600).suffix(" s"));
                        ui.checkbox(&mut self.st_confirm, tr!("Fahrzeug steht, Handbremse angezogen, Gang raus / P"));
                    });
                });
                ui.horizontal(|ui| {
                    if !running {
                        if ui.add_enabled(self.st_confirm, egui::Button::new(tr!("▶ Starten"))).clicked() {
                            self.st_start();
                        }
                    } else {
                        let stop = egui::Button::new(egui::RichText::new(tr!("  NOT-AUS  ")).heading().strong().color(egui::Color32::WHITE))
                            .fill(egui::Color32::from_rgb(190, 30, 30));
                        if ui.add(stop).clicked() {
                            self.st_stop("NOT-AUS");
                        }
                        if let Some(r) = &self.st_regler {
                            ui.label(tr!("läuft seit {a} s", a = format!("{:.0}", r.laufzeit())));
                        }
                    }
                });
                if !self.st_soll.is_empty() {
                    let (soll, ist) = (self.st_soll.clone(), self.st_ist.clone());
                    if let (Some(s), Some(i)) = (soll.last(), ist.last()) {
                        ui.label(tr!("Soll {a}   Ist {b}", a = format!("{:.0}", s[1]), b = format!("{:.0}", i[1])));
                    }
                    egui_plot::Plot::new("stplot").height(180.0).legend(egui_plot::Legend::default()).show(ui, |pui| {
                        pui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(soll)).name(tr!("Soll")).width(1.5_f32));
                        pui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(ist)).name(tr!("Ist")).width(2.0_f32));
                    });
                }
            }

            // ---------- 3. Simulation ----------
            if matches!(self.backend(), Backend::Demo) {
                ui.add_space(10.0);
                ui.separator();
                ui.heading(tr!("Simulation (Demo-Fahrzeug)"));
                ui.label(tr!("Lässt einen Messwert des Demo-Fahrzeugs einem Verlauf folgen — zum Testen von Grafik, Warnungen, Assistent und Handy-Anzeige."));
                use ecu_werkbank::sim::Param;
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_source("simparam")
                        .selected_text(Param::ALL[self.sim_param].name())
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            for (i, p) in Param::ALL.iter().enumerate() {
                                ui.selectable_value(&mut self.sim_param, i, p.name());
                            }
                        });
                });
                self.sim_m.ui(ui, "simmode", "");
                ui.horizontal(|ui| {
                    if ui.button(tr!("Anwenden")).clicked() {
                        ecu_werkbank::sim::set_muster(Param::ALL[self.sim_param], Some(self.sim_m.muster()));
                    }
                    if ui.button(tr!("Zurücksetzen (normale Fahrt)")).clicked() {
                        ecu_werkbank::sim::clear_muster();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(tr!("Fehlerbild (Gemisch):"));
                    let cur = ecu_werkbank::sim::fehlerbild();
                    egui::ComboBox::from_id_source("simfehler").selected_text(cur.name()).width(220.0).show_ui(ui, |ui| {
                        for f in ecu_werkbank::sim::Fehlerbild::ALL {
                            if ui.selectable_label(f == cur, f.name()).clicked() {
                                ecu_werkbank::sim::set_fehlerbild(f);
                            }
                        }
                    });
                });
                let mut manip = ecu_werkbank::sim::manipuliert();
                if ui.checkbox(&mut manip, tr!("Gebrauchtwagen „manipuliert“ (Fehler gelöscht, Tacho zurückgedreht, fremdes Kombi)")).changed() {
                    ecu_werkbank::sim::set_manipuliert(manip);
                }
                if ui.button(tr!("Demo-Fahrzeug zurücksetzen (Fehler wieder da)")).clicked() {
                    ecu_werkbank::sim::reset();
                }
                for (p, m) in ecu_werkbank::sim::muster() {
                    ui.weak(format!("• {}: {}", p.name(), m.text()));
                }
            }
        });
    }
}
