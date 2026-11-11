//! GUI: Scope — Live-Grafik mit mehreren Kurven

use super::*;

impl App {
    pub(super) fn scope_start_stop(&mut self) {
        self.scope_running = !self.scope_running;
        if self.scope_running {
            self.scope_file = None;
            for s in &mut self.scope_series {
                s.points.clear();
                s.error = None;
            }
            self.scope_series.retain(|s| s.kind != ChanKind::File);
            self.scope_start = std::time::Instant::now();
            self.scope_last = std::time::Instant::now() - std::time::Duration::from_secs(10);
        } else {
            self.scope_kwp_ch.clear();
            if let Some(l) = self.scope_csv.take() {
                self.status = tr!("CSV gespeichert: {a} Zeile(n) in {b}", a = l.rows(), b = l.path().display());
            }
        }
    }

    /// Messwertblock als Kurven hinzufügen (Felder werden durch eine erste Abfrage erkannt).
    pub(super) fn scope_add_kwp_block_series(&mut self) {
        let res = (|| -> anyhow::Result<Vec<Series>> {
            let addr = parse_hex(&self.scope_add_kwp_addr)
                .filter(|a| (1..=0x7F).contains(a))
                .ok_or_else(|| anyhow::anyhow!(tr!("Adresse ungültig")))? as u8;
            let block: u8 = self.scope_add_kwp_block.trim().parse().map_err(|_| anyhow::anyhow!(tr!("Block ungültig")))?;
            if self.scope_series.iter().any(|s| matches!(s.kind, ChanKind::Kwp { addr: a, block: b, .. } if a == addr && b == block)) {
                anyhow::bail!(tr!("Block {a} ist schon in der Grafik.", a = format!("{:03}", block)));
            }
            // offenen Kanal der laufenden Aufzeichnung nutzen; sonst neu öffnen
            // (wartet bis ~5 s, falls der Golf-6-Motor TP 2.0 nach OBD kurz sperrt)
            let values = match self.scope_kwp_ch.get_mut(&addr) {
                Some(ch) => Kwp::new(ch).read_block(block),
                None => Err(anyhow::anyhow!(tr!("kein Kanal"))),
            };
            let values = match values {
                Ok(v) => v,
                Err(_) => {
                    self.scope_kwp_ch.remove(&addr);
                    let mut ch = kwp::open_session(&self.backend(), addr)?;
                    let v = Kwp::new(&mut ch).read_block(block)?;
                    if self.scope_running {
                        self.scope_kwp_ch.insert(addr, ch);
                    }
                    v
                }
            };
            let label = self.mw_texte.block(addr, block).map(str::to_string).or_else(|| self.db.measuring(u16::from(addr), u16::from(block)).map(|m| m.name.clone())).unwrap_or_default();
            Ok(values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let unit = v.decode().map(|(_, u)| u).unwrap_or("");
                    let feld = self.mw_texte.name(addr, block, i as u8 + 1).map(|t| format!(" {t}")).unwrap_or_default();
                    let name = if label.is_empty() {
                        format!("{addr:#04x} B{block:03}.{}{feld}", i + 1)
                    } else {
                        format!("{label} .{}{feld}", i + 1)
                    };
                    Series::new(&name, unit, ChanKind::Kwp { addr, block, field: i })
                })
                .collect())
        })();
        match res {
            Ok(v) => {
                self.status = tr!("{a} Kurve(n) hinzugefügt.", a = v.len());
                self.scope_series.extend(v);
            }
            Err(e) => self.status = tr!("Messwertblock: {e}", e = format!("{:#}", e)),
        }
    }

    pub(super) fn poll_scope(&mut self, ctx: &egui::Context) {
        self.trip_check(ctx);
        if !self.scope_running {
            return;
        }
        // Sollwerte einmalig zuordnen
        for s in self.scope_series.iter_mut().filter(|s| !s.soll_checked) {
            s.soll_checked = true;
            s.soll = match s.kind {
                ChanKind::Obd(pid) => self.db.sollwert("obd", None, u16::from(pid), None),
                ChanKind::Kwp { addr, block, field } => self.db.sollwert("kwp", Some(u16::from(addr)), u16::from(block), Some(field as u8 + 1)),
                ChanKind::Did { did, .. } => self.db.sollwert("uds", None, did, None),
                ChanKind::File => None,
            };
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        if self.scope_last.elapsed() < std::time::Duration::from_millis(self.scope_interval_ms) {
            return;
        }
        self.scope_last = std::time::Instant::now();
        let t = self.scope_start.elapsed().as_secs_f64();
        let be = self.backend();

        // KWP: je (Adresse, Block) eine Abfrage; Kanäle bleiben offen
        let mut blocks: Vec<(u8, u8)> = self
            .scope_series
            .iter()
            .filter_map(|s| match s.kind {
                ChanKind::Kwp { addr, block, .. } => Some((addr, block)),
                _ => None,
            })
            .collect();
        blocks.sort();
        blocks.dedup();
        for (addr, block) in blocks {
            let read = (|| -> anyhow::Result<Vec<kwp::BlockValue>> {
                if let std::collections::hash_map::Entry::Vacant(e) = self.scope_kwp_ch.entry(addr) {
                    e.insert(kwp::open_session(&be, addr)?);
                }
                let ch = self.scope_kwp_ch.get_mut(&addr).expect("gerade geöffnet");
                Kwp::new(ch).read_block(block)
            })();
            if read.is_err() {
                self.scope_kwp_ch.remove(&addr);
            }
            for s in self.scope_series.iter_mut() {
                if let ChanKind::Kwp { addr: a, block: b, field } = s.kind {
                    if (a, b) != (addr, block) {
                        continue;
                    }
                    match &read {
                        Ok(vals) => match vals.get(field).and_then(|v| v.decode()) {
                            Some((v, _)) => {
                                s.push(t, v);
                                s.error = None;
                            }
                            None => s.error = Some(tr!("Feld nicht dekodierbar")),
                        },
                        Err(e) => s.error = Some(format!("{e:#}")),
                    }
                }
            }
        }

        // OBD: eine Verbindung für alle PIDs — erst nach den Messwertblöcken, weil der
        // Golf-6-Motor nach einer OBD-Abfrage ~3 s keinen neuen TP-2.0-Kanal annimmt
        if self.scope_series.iter().any(|s| matches!(s.kind, ChanKind::Obd(_))) {
            match be.open(obd::DEFAULT_TX, obd::DEFAULT_RX) {
                Ok(tp) => {
                    let o = Obd::new(&*tp);
                    for s in self.scope_series.iter_mut() {
                        if let ChanKind::Obd(pid) = s.kind {
                            match o.read_value(pid) {
                                Ok(v) => {
                                    s.push(t, v);
                                    s.error = None;
                                }
                                Err(e) => s.error = Some(format!("{e:#}")),
                            }
                        }
                    }
                }
                Err(e) => {
                    for s in self.scope_series.iter_mut().filter(|s| matches!(s.kind, ChanKind::Obd(_))) {
                        s.error = Some(format!("{e:#}"));
                    }
                }
            }
        }

        // UDS-DIDs (mit Label-Skalierung)
        for s in self.scope_series.iter_mut() {
            if let ChanKind::Did { tx, rx, did } = s.kind {
                let r = (|| -> anyhow::Result<f64> {
                    let tp = be.open(tx, rx)?;
                    let uds = Uds::new(&*tp);
                    uds.diagnostic_session(0x01)?;
                    let raw = ecu_werkbank::raw_to_f64(&uds.read_data_by_id(did)?);
                    Ok(match self.db.measuring_any(did) {
                        Some(m) => raw * m.factor + m.offset,
                        None => raw,
                    })
                })();
                match r {
                    Ok(v) => {
                        s.push(t, v);
                        s.error = None;
                    }
                    Err(e) => s.error = Some(format!("{e:#}")),
                }
            }
        }

        if let Some(log) = self.scope_csv.as_mut() {
            let cols: Vec<(String, String)> = self
                .scope_series
                .iter()
                .map(|s| {
                    let v = s.points.back().filter(|p| (p[0] - t).abs() < 1e-9).map(|p| ecu_werkbank::csvlog::num(p[1]));
                    (s.label(), v.unwrap_or_default())
                })
                .collect();
            if let Err(e) = log.write_at(t, &cols) {
                self.status = tr!("CSV-Fehler: {e}", e = format!("{:#}", e));
            }
        }
        self.scope_alarms(t);
        self.trip_running_check();
        self.web_update();
    }

    /// Grenzwert-Warnungen: Ton, Meldung, Aufzeichnung inkl. der 30 s davor.
    pub(super) fn scope_alarms(&mut self, t: f64) {
        if !self.scope_alarm_on {
            return;
        }
        let mut hits = Vec::new();
        for s in self.scope_series.iter_mut() {
            let (Some(range), Some(p)) = (s.soll, s.points.back()) else { continue };
            if (p[0] - t).abs() > 1e-9 {
                continue;
            }
            match range.check(p[1]) {
                Stufe::Warnung if !s.alarmed => {
                    s.alarmed = true;
                    hits.push(tr!("{a} {b} {c} (Soll {d})", a = s.name, b = ecu_werkbank::csvlog::num(p[1]), c = s.unit, d = range.text()));
                }
                Stufe::Gut => s.alarmed = false,
                _ => {}
            }
        }
        if !hits.is_empty() {
            self.scope_alarm_text = format!("⚠ {}  —  {}", hits.join(";  "), ecu_werkbank::report::now_local());
            notify("ECUWerkbank: Grenzwert", &hits.join("\n"));
            if self.scope_csv.is_none() {
                if let Some(mut log) = auto_csv(self.vehicle_sub("aufzeichnungen"), "alarm", &mut self.status) {
                    // Vorlauf: die letzten 30 s aus dem Puffer
                    let mut times: Vec<f64> = self
                        .scope_series
                        .iter()
                        .flat_map(|s| s.points.iter().map(|p| p[0]))
                        .filter(|x| *x >= t - 30.0 && *x < t)
                        .collect();
                    times.sort_by(f64::total_cmp);
                    times.dedup();
                    for ts in times {
                        let cols: Vec<(String, String)> = self
                            .scope_series
                            .iter()
                            .map(|s| {
                                let v = s.points.iter().rev().find(|p| (p[0] - ts).abs() < 1e-9).map(|p| ecu_werkbank::csvlog::num(p[1]));
                                (s.label(), v.unwrap_or_default())
                            })
                            .collect();
                        let _ = log.write_at(ts, &cols);
                    }
                    self.scope_csv = Some(log);
                    self.scope_alarm_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
                }
            } else if self.scope_alarm_until.is_some() {
                self.scope_alarm_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
            }
        }
        if let Some(until) = self.scope_alarm_until {
            if std::time::Instant::now() > until && !self.trip_active {
                self.scope_alarm_until = None;
                if let Some(l) = self.scope_csv.take() {
                    self.status = tr!("Warn-Aufzeichnung gespeichert: {a} ({b} Zeilen)", a = l.path().display(), b = l.rows());
                }
            }
        }
    }

    /// Fahrtenschreiber: Start bei laufendem Motor.
    pub(super) fn trip_check(&mut self, ctx: &egui::Context) {
        if !self.trip_on || self.scope_running {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        if self.trip_last_check.elapsed() < std::time::Duration::from_secs(2) {
            return;
        }
        self.trip_last_check = std::time::Instant::now();
        let rpm = self.obd_open().and_then(|tp| Obd::new(&*tp).read_value(0x0C)).unwrap_or(0.0);
        if rpm < 300.0 {
            return;
        }
        if self.scope_series.iter().all(|s| s.kind == ChanKind::File) {
            self.scope_series.clear();
            for pid in [0x0C, 0x0D, 0x05, 0x04, 0x42] {
                if let Some((n, u)) = obd::pid_info(pid) {
                    self.scope_series.push(Series::new(n, u, ChanKind::Obd(pid)));
                }
            }
        }
        self.scope_start_stop();
        self.scope_csv = auto_csv(self.vehicle_sub("aufzeichnungen"), "fahrt", &mut self.status);
        self.trip_active = true;
        self.trip_low_since = None;
        self.status = tr!("Fahrtenschreiber: Motor läuft — Aufzeichnung gestartet.");
    }

    /// Fahrtenschreiber: Ende, wenn der Motor > 10 s aus ist.
    pub(super) fn trip_running_check(&mut self) {
        if !self.trip_active {
            return;
        }
        let rpm = self
            .scope_series
            .iter()
            .find(|s| s.kind == ChanKind::Obd(0x0C) || s.unit == "1/min")
            .and_then(|s| if s.error.is_some() { Some(0.0) } else { s.points.back().map(|p| p[1]) })
            .unwrap_or(0.0);
        if rpm >= 100.0 {
            self.trip_low_since = None;
            return;
        }
        let since = *self.trip_low_since.get_or_insert_with(std::time::Instant::now);
        if since.elapsed() < std::time::Duration::from_secs(10) {
            return;
        }
        self.trip_active = false;
        self.scope_pdf();
        let pdf = self.status.clone();
        self.scope_start_stop(); // stoppt + speichert CSV
        self.status = tr!("Fahrt beendet. {pdf} · {a}", pdf = pdf, a = self.status);
    }

    /// Werte für die Handy-Anzeige bereitstellen.
    pub(super) fn web_update(&mut self) {
        if self.web_server.is_none() {
            return;
        }
        let title = self.active_vehicle.as_deref().and_then(ecu_werkbank::vehicle::load).map(|v| v.label()).unwrap_or_else(|| "ECUWerkbank".into());
        let tiles = self
            .scope_series
            .iter()
            .filter(|s| s.visible)
            .map(|s| ecu_werkbank::web::Tile {
                name: s.name.clone(),
                unit: s.unit.clone(),
                value: s.points.back().map(|p| p[1]),
                state: match s.state() {
                    Some(Stufe::Gut) => "gut",
                    Some(Stufe::Hinweis) => "hinweis",
                    Some(Stufe::Warnung) => "warnung",
                    None => "",
                }
                .into(),
            })
            .collect();
        let mut d = self.web_data.lock().unwrap();
        d.title = title;
        d.tiles = tiles;
        d.alarm = self.scope_alarm_text.clone();
    }

    pub(super) fn ui_scope(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Scope — Live-Grafik"));
        ui.horizontal_wrapped(|ui| {
            let btn = if self.scope_running { tr!("⏹ Stop") } else { tr!("▶ Start") };
            if ui.add_enabled(!self.scope_series.is_empty(), egui::Button::new(btn)).clicked() {
                self.scope_start_stop();
            }
            let mut rec = self.scope_csv.is_some();
            if ui.add_enabled(self.scope_running, egui::Checkbox::new(&mut rec, tr!("⏺ CSV aufzeichnen"))).changed() {
                self.scope_csv = if rec { auto_csv(self.vehicle_sub("aufzeichnungen"), "scope", &mut self.status) } else { None };
            }
            if ui.add_enabled(!self.scope_running, egui::Button::new(tr!("📂 CSV laden…"))).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).pick_file() {
                    match read_csv_series(&p) {
                        Ok(v) => {
                            self.status = tr!("{a} Kurve(n) aus {b} geladen.", a = v.len(), b = p.display());
                            self.scope_series = v;
                            self.scope_file = Some(p.display().to_string());
                            self.scope_window = 0.0;
                        }
                        Err(e) => self.status = tr!("CSV laden: {e}", e = format!("{:#}", e)),
                    }
                }
            }
            let has_data = self.scope_series.iter().any(|s| !s.points.is_empty());
            if ui.add_enabled(has_data, egui::Button::new(tr!("📄 Als PDF"))).on_hover_text(tr!("Alle sichtbaren Kurven als PDF in den Fahrzeugordner (scope/)")).clicked() {
                self.scope_pdf();
            }
            if ui.add_enabled(!self.scope_running, egui::Button::new(tr!("Alle entfernen"))).clicked() {
                self.scope_series.clear();
                self.scope_file = None;
            }
            ui.separator();
            platz(ui, 140.0);
            ui.label(tr!("Takt:"));
            egui::ComboBox::from_id_source("scopeint")
                .selected_text(format!("{} ms", self.scope_interval_ms))
                .width(70.0)
                .show_ui(ui, |ui| {
                    for ms in [100u64, 250, 500, 1000] {
                        ui.selectable_value(&mut self.scope_interval_ms, ms, format!("{ms} ms"));
                    }
                });
            platz(ui, 150.0);
            ui.label(tr!("Fenster:"));
            egui::ComboBox::from_id_source("scopewin")
                .selected_text(if self.scope_window == 0.0 { "alles".into() } else { format!("{} s", self.scope_window) })
                .width(70.0)
                .show_ui(ui, |ui| {
                    for w in [30.0, 60.0, 300.0, 0.0] {
                        let txt = if w == 0.0 { "alles".to_string() } else { format!("{w} s") };
                        ui.selectable_value(&mut self.scope_window, w, txt);
                    }
                });
            ui.checkbox(&mut self.scope_normalize, tr!("normiert (0–100 %)"))
                .on_hover_text(tr!("Jede Kurve auf ihren eigenen Bereich skalieren — so lassen sich Werte mit verschiedenen Einheiten vergleichen."));
        });
        ui.horizontal_wrapped(|ui| {
            ui.toggle_value(&mut self.scope_gauges, tr!("🕓 Armaturen")).on_hover_text(tr!("Große Rundinstrumente statt Grafik"));
            ui.checkbox(&mut self.scope_alarm_on, tr!("⚠ Warnen bei Grenzwert"))
                .on_hover_text(tr!("Ton + Meldung bei Überschreitung des Sollwerts; zeichnet automatisch die 30 s davor und danach auf."));
            let was = self.trip_on;
            ui.checkbox(&mut self.trip_on, tr!("🚗 Fahrtenschreiber"))
                .on_hover_text(tr!("Startet die Aufzeichnung von selbst, sobald der Motor läuft, und speichert beim Abstellen CSV + PDF."));
            if was && !self.trip_on && self.trip_active {
                self.trip_active = false;
            }
            let mut web = self.web_server.is_some();
            if ui.checkbox(&mut web, tr!("📱 Handy-Anzeige")).on_hover_text(tr!("Live-Werte im Browser des Handys (gleiches WLAN)")).changed() {
                if web {
                    match ecu_werkbank::web::serve(8088, self.web_data.clone()).or_else(|_| ecu_werkbank::web::serve(0, self.web_data.clone())) {
                        Ok(srv) => {
                            self.status = tr!("Handy-Anzeige: im Browser {a} öffnen", a = srv.url);
                            self.web_server = Some(srv);
                            self.web_update();
                        }
                        Err(e) => self.status = tr!("Handy-Anzeige: {e}", e = format!("{:#}", e)),
                    }
                } else {
                    self.web_server = None;
                }
            }
            if let Some(srv) = &self.web_server {
                ui.label(egui::RichText::new(&srv.url).monospace().strong());
            }
            if ui.add_enabled(!self.scope_running, egui::Button::new(tr!("📂 CSV dazuladen…"))).on_hover_text(tr!("Zweite Aufzeichnung zum Vergleich darüberlegen")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).pick_file() {
                    match read_csv_series(&p) {
                        Ok(v) => {
                            let tag = p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            let n = v.len();
                            for mut s in v {
                                s.name = format!("{tag}: {}", s.name);
                                self.scope_series.push(s);
                            }
                            self.scope_file = Some(tr!("mehrere Dateien"));
                            self.scope_window = 0.0;
                            self.status = tr!("{n} Kurve(n) aus {a} dazugeladen.", n = n, a = p.display());
                        }
                        Err(e) => self.status = tr!("CSV laden: {e}", e = format!("{:#}", e)),
                    }
                }
            }
        });
        if self.trip_on {
            ui.weak(if self.trip_active { tr!("Fahrtenschreiber: Aufzeichnung läuft.") } else { tr!("Fahrtenschreiber: wartet auf laufenden Motor…") });
        }
        if !self.scope_alarm_text.is_empty() {
            ui.horizontal(|ui| {
                ui.colored_label(stufe_color(Stufe::Warnung), egui::RichText::new(&self.scope_alarm_text).strong());
                if ui.small_button("✖").clicked() {
                    self.scope_alarm_text.clear();
                }
            });
        }
        if let Some(f) = &self.scope_file {
            ui.weak(tr!("Datei: {f}", f = f));
        }

        // Kurven hinzufügen (auch während der Aufzeichnung)
        ui.scope(|ui| {
            ui.horizontal_wrapped(|ui| {
                // je Gruppe prüfen, ob sie noch in die Zeile passt (Klappmenü bricht nicht um)
                platz(ui, 300.0);
                ui.label(tr!("+ OBD-II:"));
                egui::ComboBox::from_id_source("scopeobd")
                    .selected_text(obd::pid_info(self.scope_add_obd).map(|(n, _)| n).unwrap_or("PID"))
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for pid in [0x0C, 0x0D, 0x05, 0x04, 0x11, 0x10, 0x0B, 0x0F, 0x42, 0x2F, 0x06, 0x07, 0x0E, 0x46] {
                            if let Some((n, _)) = obd::pid_info(pid) {
                                ui.selectable_value(&mut self.scope_add_obd, pid, n);
                            }
                        }
                    });
                if ui.button(tr!("hinzufügen")).clicked() {
                    let pid = self.scope_add_obd;
                    if !self.scope_series.iter().any(|s| s.kind == ChanKind::Obd(pid)) {
                        let (n, u) = obd::pid_info(pid).unwrap_or(("PID", ""));
                        self.scope_series.push(Series::new(n, u, ChanKind::Obd(pid)));
                    }
                }
                ui.separator();
                platz(ui, 250.0);
                ui.label(tr!("+ Messwertblock:"));
                ui.add_sized([46.0, 20.0], egui::TextEdit::singleline(&mut self.scope_add_kwp_addr));
                ui.label(tr!("Block"));
                ui.add_sized([34.0, 20.0], egui::TextEdit::singleline(&mut self.scope_add_kwp_block));
                if ui.button(tr!("hinzufügen")).clicked() {
                    self.scope_add_kwp_block_series();
                }
                ui.separator();
                platz(ui, 320.0);
                ui.label(tr!("+ UDS-DID:"));
                ui.add_sized([56.0, 20.0], egui::TextEdit::singleline(&mut self.scope_did));
                ui.add_sized([52.0, 20.0], egui::TextEdit::singleline(&mut self.scope_tx));
                ui.add_sized([52.0, 20.0], egui::TextEdit::singleline(&mut self.scope_rx));
                if ui.button(tr!("hinzufügen")).clicked() {
                    match (parse_hex(&self.scope_did), parse_hex(&self.scope_tx), parse_hex(&self.scope_rx)) {
                        (Some(did), Some(tx), Some(rx)) => {
                            let (n, u) = self
                                .db
                                .measuring_any(did)
                                .map(|m| (m.name.clone(), m.unit.clone()))
                                .unwrap_or_else(|| (format!("DID {did:#06x}"), String::new()));
                            self.scope_series.push(Series::new(&n, &u, ChanKind::Did { tx, rx, did }));
                        }
                        _ => self.status = tr!("DID/TX/RX ungültig (hex, z. B. 0xF40C 0x7E0 0x7E8)."),
                    }
                }
            });
        });

        if self.scope_series.is_empty() {
            ui.add_space(10.0);
            ui.weak(tr!("Noch keine Kurven. Oben Werte hinzufügen und ▶ Start — oder eine CSV-Aufzeichnung laden."));
            return;
        }

        // Tabelle: aktuell / min / max
        let mut remove = None;
        egui::Grid::new("scopetab").striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
            ui.weak("");
            ui.weak(tr!("Kurve"));
            ui.weak(tr!("aktuell"));
            ui.weak("min");
            ui.weak("max");
            ui.weak(tr!("Soll"));
            ui.end_row();
            for (i, s) in self.scope_series.iter_mut().enumerate() {
                ui.checkbox(&mut s.visible, "");
                ui.label(s.label());
                let (cur, min, max) = if s.points.is_empty() {
                    (None, None, None)
                } else {
                    let min = s.points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
                    let max = s.points.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
                    (s.points.back().map(|p| p[1]), Some(min), Some(max))
                };
                let f = |v: Option<f64>| v.map(|v| format!("{v:.1}")).unwrap_or_else(|| "—".into());
                match s.state() {
                    Some(st) => ui.label(egui::RichText::new(f(cur)).strong().color(stufe_color(st))),
                    None => ui.strong(f(cur)),
                };
                ui.label(f(min));
                ui.label(f(max));
                ui.weak(s.soll.map(|r| r.text()).unwrap_or_default());
                if let Some(e) = &s.error {
                    ui.colored_label(egui::Color32::from_rgb(220, 160, 40), "⚠").on_hover_text(e);
                } else {
                    ui.label("");
                }
                if !self.scope_running && ui.small_button("🗑").on_hover_text(tr!("Kurve entfernen")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.scope_series.remove(i);
        }

        if self.scope_gauges {
            egui::ScrollArea::vertical().id_source("gauges").show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for s in self.scope_series.iter().filter(|s| s.visible) {
                        draw_gauge(ui, s);
                    }
                });
            });
            return;
        }

        // Grafik
        let t_end = self.scope_series.iter().filter_map(|s| s.points.back().map(|p| p[0])).fold(0.0, f64::max);
        let t_start = if self.scope_window > 0.0 { t_end - self.scope_window } else { f64::NEG_INFINITY };
        let normalize = self.scope_normalize;
        let lines: Vec<egui_plot::Line> = self
            .scope_series
            .iter()
            .filter(|s| s.visible && !s.points.is_empty())
            .map(|s| {
                let (min, max) = s.points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
                let span = if (max - min).abs() < 1e-9 { 1.0 } else { max - min };
                let pts: Vec<[f64; 2]> = s
                    .points
                    .iter()
                    .filter(|p| p[0] >= t_start)
                    .map(|p| if normalize { [p[0], (p[1] - min) / span * 100.0] } else { *p })
                    .collect();
                egui_plot::Line::new(egui_plot::PlotPoints::from(pts)).name(s.label()).width(2.0_f32)
            })
            .collect();
        egui_plot::Plot::new("scope")
            .legend(egui_plot::Legend::default())
            .x_axis_label("t [s]")
            .y_axis_label(if normalize { tr!("% des Bereichs") } else { tr!("Wert") })
            .auto_bounds([true, true].into())
            .show(ui, |pui| {
                for l in lines {
                    pui.line(l);
                }
            });
    }
}
