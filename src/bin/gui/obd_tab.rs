//! GUI: OBD-II (Mode 01/03/04/09, Readiness, Freeze-Frame). Logik in
//! ecu_werkbank::obd und ecu_werkbank::readiness; hier nur die Bedienung.

use super::*;

impl App {
    pub(super) fn obd_open(&self) -> anyhow::Result<Box<dyn ecu_werkbank::transport::Transport>> {
        self.backend().open(obd::DEFAULT_TX, obd::DEFAULT_RX)
    }

    pub(super) fn run_obd_info(&mut self) {
        let res = (|| -> anyhow::Result<(String, ecu_werkbank::readiness::Stand, Vec<u8>)> {
            let tp = self.obd_open()?;
            let o = Obd::new(&*tp);
            let vin = o.vin().unwrap_or_else(|_| tr!("nicht lesbar"));
            let r = ecu_werkbank::readiness::lesen(&o)?;
            let sup = o.supported_pids().unwrap_or_default();
            Ok((vin, r, sup))
        })();
        match res {
            Ok((vin, r, sup)) => {
                self.obd_vin = vin;
                self.obd_readiness = Some(r.readiness.clone());
                self.obd_stand = Some(r);
                self.obd_supported = sup.into_iter().filter(|p| obd::pid_info(*p).is_some()).collect();
                self.status = tr!("OBD-II gelesen.");
            }
            Err(e) => self.status = tr!("OBD-Fehler: {e}", e = format!("{:#}", e)),
        }
    }

    pub(super) fn run_obd_dtc(&mut self) {
        let res = (|| -> anyhow::Result<Vec<String>> {
            let tp = self.obd_open()?;
            let o = Obd::new(&*tp);
            let mut lines = Vec::new();
            for (svc, what) in [(obd::DTC_STORED, tr!("Gespeichert")), (obd::DTC_PENDING, tr!("Sporadisch")), (obd::DTC_PERMANENT, tr!("Permanent"))] {
                match o.read_dtcs(svc) {
                    Ok(c) if c.is_empty() => lines.push(tr!("{what}: keine", what = what)),
                    Ok(c) => {
                        for code in c {
                            let t = self.db.dtc_description(&code).unwrap_or("");
                            lines.push(format!("{what}: {code}  {t}"));
                        }
                    }
                    Err(_) => lines.push(tr!("{what}: nicht unterstützt", what = what)),
                }
            }
            Ok(lines)
        })();
        match res {
            Ok(l) => {
                self.obd_dtcs = l;
                self.status = tr!("OBD-Fehlercodes gelesen.");
            }
            Err(e) => self.status = tr!("OBD-Fehler: {e}", e = format!("{:#}", e)),
        }
    }

    fn run_obd_clear(&mut self) {
        let res = self.obd_open().and_then(|tp| Obd::new(&*tp).clear());
        self.status = match res {
            Ok(()) => {
                self.obd_dtcs.clear();
                self.obd_readiness = None;
                tr!("OBD-Fehler und Readiness gelöscht.")
            }
            Err(e) => tr!("OBD-Fehler: {e}", e = format!("{:#}", e)),
        };
    }

    pub(super) fn poll_obd(&mut self, ctx: &egui::Context) {
        if !self.obd_live {
            if let Some(log) = self.obd_csv.take() {
                self.status = tr!("CSV gespeichert: {a} Zeile(n) in {b}", a = log.rows(), b = log.path().display());
            }
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if self.obd_last.elapsed() < std::time::Duration::from_millis(500) {
            return;
        }
        self.obd_last = std::time::Instant::now();
        let pids: Vec<u8> = self.obd_selected.iter().copied().collect();
        let res = (|| -> anyhow::Result<Vec<(String, String, String, Option<Stufe>)>> {
            let tp = self.obd_open()?;
            let o = Obd::new(&*tp);
            let mut out = Vec::new();
            for pid in pids {
                let (name, unit) = obd::pid_info(pid).unwrap_or(("PID", ""));
                let v = o.read_value(pid).ok();
                let st = v.and_then(|x| self.db.sollwert("obd", None, u16::from(pid), None).map(|r| r.check(x)));
                out.push((
                    format!("{name} [{unit}]"),
                    v.map(|v| format!("{v:.1} {unit}")).unwrap_or_else(|| "—".into()),
                    v.map(ecu_werkbank::csvlog::num).unwrap_or_default(),
                    st,
                ));
            }
            Ok(out)
        })();
        match res {
            Ok(rows) => {
                if let Some(log) = self.obd_csv.as_mut() {
                    let cols: Vec<(String, String)> = rows.iter().map(|(n, _, c, _)| (n.clone(), c.clone())).collect();
                    if let Err(e) = log.write(&cols) {
                        self.status = tr!("CSV-Fehler: {e}", e = format!("{:#}", e));
                    }
                }
                self.obd_values = rows.into_iter().map(|(n, v, _, st)| (n, v, st)).collect();
            }
            Err(e) => {
                self.obd_live = false;
                self.status = tr!("Live-Daten gestoppt: {e}", e = format!("{:#}", e));
            }
        }
    }

    pub(super) fn ui_obd(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("OBD-II / EOBD — herstellerunabhängig"));
        ui.label(tr!("Genormte Abgas-Diagnose (SAE J1979) über das Motorsteuergerät 0x7E0/0x7E8. Funktioniert mit jedem Adapter."));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("ℹ Fahrzeug & Readiness lesen")).clicked() {
                self.run_obd_info();
            }
            if ui.button(tr!("🛠 Fehlercodes lesen")).clicked() {
                self.run_obd_dtc();
            }
            if !self.obd_confirm_clear {
                if ui.button(tr!("✖ Fehler löschen…")).clicked() {
                    self.obd_confirm_clear = true;
                }
            } else {
                ui.colored_label(egui::Color32::from_rgb(220, 160, 40), tr!("Löscht auch die Readiness (AU/HU erst nach Fahrt wieder möglich)!"));
                if ui.button(tr!("Ja, löschen")).clicked() {
                    self.run_obd_clear();
                    self.obd_confirm_clear = false;
                }
                if ui.button(tr!("Abbrechen")).clicked() {
                    self.obd_confirm_clear = false;
                }
            }
        });

        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(r) = &self.obd_readiness {
                ui.add_space(6.0);
                ui.separator();
                egui::Grid::new("obdinfo").spacing([16.0, 4.0]).show(ui, |ui| {
                    ui.label(tr!("VIN:"));
                    ui.monospace(&self.obd_vin);
                    ui.end_row();
                    ui.label(tr!("Motorkontrollleuchte:"));
                    if r.mil {
                        ui.colored_label(egui::Color32::from_rgb(230, 120, 40), "AN");
                    } else {
                        ui.label(tr!("aus"));
                    }
                    ui.end_row();
                    ui.label(tr!("Gespeicherte Fehler:"));
                    ui.label(r.dtc_count.to_string());
                    ui.end_row();
                    ui.label(tr!("Motor:"));
                    ui.label(if r.compression { tr!("Diesel") } else { tr!("Benziner") });
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.strong(if r.all_complete() { tr!("Readiness: alle Tests fertig") } else { tr!("Readiness: noch nicht alle Tests fertig") });
                let stand = self.obd_stand.as_ref();
                // Fortschritt seit dem Löschen
                if let Some(st) = stand {
                    let mut z = Vec::new();
                    if let Some(k) = st.km_seit_loeschen {
                        z.push(tr!("{a} km", a = ecu_werkbank::report::group_thousands(k as u32)));
                    }
                    if let Some(w) = st.warmlaeufe {
                        z.push(tr!("{a} Warmläufe", a = w as u32));
                    }
                    if let Some(m) = st.min_seit_loeschen {
                        z.push(if m >= 120.0 { tr!("{a} Std. Motorlauf", a = (m / 60.0).round() as u32) } else { tr!("{a} min Motorlauf", a = m as u32) });
                    }
                    if !z.is_empty() {
                        ui.label(tr!("Seit dem letzten Löschen: {a}", a = z.join(", ")));
                    }
                }
                let mit_fahrt = stand.is_some_and(|s| s.fahrt.is_some());
                egui::Grid::new("readiness").striped(true).spacing([16.0, 2.0]).show(ui, |ui| {
                    ui.weak(tr!("Test"));
                    ui.weak(tr!("Stand"));
                    if mit_fahrt {
                        ui.weak(tr!("diese Fahrt"));
                    }
                    ui.end_row();
                    for m in r.monitors.iter().filter(|m| m.available) {
                        ui.label(m.name).on_hover_text(ecu_werkbank::readiness::anleitung(m.name));
                        if m.complete {
                            ui.colored_label(egui::Color32::from_rgb(80, 180, 100), tr!("fertig"));
                        } else {
                            ui.colored_label(egui::Color32::from_rgb(220, 160, 40), tr!("nicht fertig"));
                        }
                        if mit_fahrt {
                            match stand.and_then(|s| s.in_fahrt(m.name)) {
                                Some(true) => ui.colored_label(egui::Color32::from_rgb(80, 180, 100), tr!("geprüft")),
                                Some(false) => ui.colored_label(egui::Color32::from_rgb(120, 160, 230), tr!("läuft …")),
                                None => ui.weak("—"),
                            };
                        }
                        ui.end_row();
                    }
                });
            }
            ui.weak(tr!("OBD-II zeigt nur abgasrelevante, bestätigte Fehler. Alle Fehler (auch sporadische) liefert der Fehlerspeicher des Steuergeräts: Start » Fehler-PDF oder Diagnose » Steuergeräte."));
            if !self.obd_dtcs.is_empty() {
                ui.add_space(6.0);
                ui.separator();
                let model = self.vehicle_model();
                for l in self.obd_dtcs.clone() {
                    let notes = code_in(&l).map(|c| self.repairs_for(&c)).unwrap_or_default();
                    self.dtc_row(ui, &l, &model, &notes);
                }
            }

            // Readiness-Assistent + Freeze-Frame
            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.strong(tr!("Readiness-Assistent"));
                let cap = if self.obd_ready_live { tr!("⏸ stoppen") } else { tr!("▶ live verfolgen") };
                ui.toggle_value(&mut self.obd_ready_live, cap)
                    .on_hover_text(tr!("Fragt alle 2 s den Stand der Abgas-Tests ab — so siehst du während der Fahrt, wann alles fertig ist."));
                if ui.button(tr!("Umgebungsbedingungen (Freeze-Frame)")).clicked() {
                    self.obd_freeze = Some(
                        self.obd_open()
                            .and_then(|tp| Obd::new(&*tp).freeze_frame())
                            .unwrap_or_else(|e| {
                                self.status = tr!("Freeze-Frame: {e}", e = format!("{:#}", e));
                                None
                            }),
                    );
                }
            });
            if let Some(r) = &self.obd_readiness {
                let open: Vec<&obd::Monitor> = r.monitors.iter().filter(|m| m.available && !m.complete).collect();
                let total = r.monitors.iter().filter(|m| m.available).count();
                ui.label(tr!("{a} von {total} Tests fertig.", a = total - open.len(), total = total));
                let offen = !open.is_empty();
                for m in open {
                    ui.horizontal_wrapped(|ui| {
                        dot(ui, stufe_color(Stufe::Hinweis));
                        ui.colored_label(stufe_color(Stufe::Hinweis), format!("{}:", m.name));
                        ui.add(egui::Label::new(ecu_werkbank::readiness::anleitung(m.name)).wrap());
                    });
                }
                if offen {
                    for t in ecu_werkbank::readiness::tipps() {
                        ui.weak(format!("• {t}"));
                    }
                    ui.weak(tr!("Richtwerte — VW veröffentlicht die genauen Fahrzyklen nicht, sie unterscheiden sich je Motor."));
                }
            }
            match &self.obd_freeze {
                Some(Some(ff)) => {
                    ui.add_space(4.0);
                    ui.strong(tr!("Freeze-Frame — ausgelöst durch {a}", a = ff.code));
                    egui::Grid::new("freeze").spacing([16.0, 2.0]).show(ui, |ui| {
                        for (i, (k, v)) in ff.values.iter().enumerate() {
                            ui.weak(k);
                            ui.label(v);
                            if i % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
                }
                Some(None) => {
                    ui.weak(tr!("Kein Freeze-Frame gespeichert."));
                }
                None => {}
            }

            ui.add_space(6.0);
            ui.separator();
            ui.strong(tr!("Live-Daten"));
            let choices: Vec<u8> = if self.obd_supported.is_empty() {
                vec![0x0C, 0x0D, 0x05, 0x04, 0x0F, 0x10, 0x11, 0x0B, 0x42, 0x2F, 0x1F, 0x46]
            } else {
                self.obd_supported.clone()
            };
            if self.obd_supported.is_empty() {
                ui.weak(tr!("Tipp: erst „Fahrzeug & Readiness lesen“, dann stehen nur die unterstützten Werte zur Wahl."));
            }
            ui.horizontal_wrapped(|ui| {
                for pid in choices {
                    let Some((name, _)) = obd::pid_info(pid) else { continue };
                    let mut on = self.obd_selected.contains(&pid);
                    if ui.checkbox(&mut on, name).changed() {
                        if on {
                            self.obd_selected.insert(pid);
                        } else {
                            self.obd_selected.remove(&pid);
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                let caption = if self.obd_live { tr!("⏸ Live stoppen") } else { tr!("▶ Live (2×/s)") };
                ui.toggle_value(&mut self.obd_live, caption);
                let mut rec = self.obd_csv.is_some();
                if ui.add_enabled(self.obd_live, egui::Checkbox::new(&mut rec, tr!("⏺ CSV aufzeichnen"))).changed() {
                    self.obd_csv = if rec { auto_csv(self.vehicle_sub("aufzeichnungen"), "obd-fahrt", &mut self.status) } else { None };
                }
            });
            if !self.obd_values.is_empty() {
                egui::Grid::new("obdlive").spacing([28.0, 8.0]).show(ui, |ui| {
                    for (i, (name, v, st)) in self.obd_values.iter().enumerate() {
                        ui.vertical(|ui| {
                            // feste Breite, sonst werden lange Namen Buchstabe für Buchstabe umbrochen
                            ui.set_min_width(170.0);
                            ui.weak(name.split(" [").next().unwrap_or(name));
                            match st {
                                Some(s) => ui.label(egui::RichText::new(v).heading().color(stufe_color(*s))),
                                None => ui.heading(v),
                            };
                        });
                        if (i + 1) % 4 == 0 {
                            ui.end_row();
                        }
                    }
                });
            }
        });
    }
}
