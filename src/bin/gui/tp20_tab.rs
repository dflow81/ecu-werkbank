//! GUI: TP 2.0 / KWP2000 (Blockscan, Namen, Grundeinstellung, Favoriten)

use super::*;

impl App {
    pub(super) fn tp_address(&self) -> anyhow::Result<u8> {
        parse_hex(&self.tp_addr)
            .filter(|a| (0x01..=0x7F).contains(a))
            .map(|a| a as u8)
            .ok_or_else(|| anyhow::anyhow!(tr!("Adresse ungültig (0x01..0x7F)")))
    }

    /// Hintergrund-Suche und Live-Messwerte; wird in jedem Frame aufgerufen.
    pub(super) fn poll_tp20(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.tp_autostart) {
            self.start_sweep();
        }
        if let Some(rx) = &self.tp_sweep {
            let mut done = None;
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SweepMsg::Progress(a) => self.tp_sweep_at = a,
                    SweepMsg::Done(found) => done = Some(found),
                }
            }
            if let Some(result) = done {
                match result {
                    Ok(found) => {
                        self.status = tr!("TP-2.0-Suche: {a} Steuergerät(e) gefunden.", a = found.len());
                        self.tp_found = found;
                    }
                    Err(e) => self.status = tr!("TP-2.0-Suche fehlgeschlagen: {e}", e = e),
                }
                self.tp_sweep = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }

        if self.tp_live {
            if self.tp_last_read.elapsed() >= std::time::Duration::from_millis(500) {
                self.tp_last_read = std::time::Instant::now();
                // nur ein Lese-Auftrag zur Zeit; wartet einer noch, diesen Takt auslassen
                if !self.bus.wartend.iter().any(|t| t == &tr!("Messwertblock lesen")) {
                    self.block_lesen();
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            if self.tp_channel.is_some() {
                self.tp_channel = None; // Kanal schließen, sobald nicht mehr live
            }
            if let Some(log) = self.tp_csv.take() {
                self.status = tr!("CSV gespeichert: {a} Zeile(n) in {b}", a = log.rows(), b = log.path().display());
            }
        }
    }

    pub(super) fn start_sweep(&mut self) {
        if self.tp_sweep.is_some() {
            return;
        }
        let be = self.backend();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let found = kwp::sweep(&be, |a| {
                let _ = tx.send(SweepMsg::Progress(a));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(SweepMsg::Done(found));
        });
        self.tp_sweep = Some(rx);
        self.tp_sweep_at = 1;
        self.status = tr!("TP-2.0-Suche über {a} läuft…", a = self.backend().describe());
    }

    pub(super) fn tp_dtc_line(&self, d: &kwp::KwpDtc, address: u8) -> String {
        let obd = d.obd_code(Some(address));
        let text = obd
            .as_deref()
            .and_then(|c| self.db.dtc_description(c))
            .or_else(|| self.db.dtc_description(&d.vag_code()))
            .unwrap_or("");
        let obd_s = obd.map(|c| format!(" ({c})")).unwrap_or_default();
        format!("{}{obd_s}  {text}  ({})", d.vag_code(), d.status_text())
    }

    /// Fehlerzeilen, darunter (eingerückt) die Umgebungsdaten, falls vorhanden.
    pub(super) fn tp_dtc_lines(&self, dtcs: &[(kwp::KwpDtc, Option<Vec<u8>>)], address: u8) -> Vec<String> {
        let mut out = Vec::new();
        for (d, env) in dtcs {
            out.push(self.tp_dtc_line(d, address));
            if let Some(e) = env {
                out.push(tr!("      Umgebungsdaten (roh): {a}", a = kwp::umgebung_hex(e)));
            }
        }
        out
    }

    /// Adresse aus dem Eingabefeld; bei Fehler Meldung in der Statuszeile.
    fn tp_address_oder_meldung(&mut self) -> Option<u8> {
        match self.tp_address() {
            Ok(a) => Some(a),
            Err(e) => {
                self.status = tr!("Fehler: {e}", e = format!("{:#}", e));
                None
            }
        }
    }

    pub(super) fn run_tp_ident(&mut self) {
        let Some(addr) = self.tp_address_oder_meldung() else { return };
        let be = self.backend();
        self.im_hintergrund(tr!("Identifikation lesen"), move || {
            let res = (|| -> anyhow::Result<Vec<String>> {
                let mut ch = kwp::open_session(&be, addr)?;
                let data = Kwp::new(&mut ch).read_ecu_id(kwp::ECU_ID_VAG)?;
                let mut lines = vec![tr!("Kanal tx={a} rx={b}", a = format!("{:#05x}", ch.tx_id()), b = format!("{:#05x}", ch.rx_id()))];
                lines.extend(kwp::ident_strings(&data));
                lines.push(tr!("Rohdaten: {a}", a = data.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")));
                Ok(lines)
            })();
            Box::new(move |app: &mut App| match res {
                Ok(lines) => {
                    app.tp_lines = lines;
                    app.status = tr!("Identifikation gelesen.");
                }
                Err(e) => app.status = tr!("Fehler: {e}", e = format!("{:#}", e)),
            })
        });
    }

    pub(super) fn run_tp_dtc(&mut self) {
        let Some(addr) = self.tp_address_oder_meldung() else { return };
        let be = self.backend();
        self.im_hintergrund(tr!("Fehlerspeicher lesen"), move || {
            let res = kwp::open_session(&be, addr).and_then(|mut ch| Kwp::new(&mut ch).read_dtcs_mit_umgebung());
            Box::new(move |app: &mut App| match res {
                Ok(dtcs) if dtcs.is_empty() => {
                    app.tp_lines = vec![tr!("Keine Fehlercodes gespeichert.")];
                    app.status = tr!("Fehlerspeicher gelesen.");
                }
                Ok(dtcs) => {
                    let mut lines = vec![tr!("{a} Fehlercode(s):", a = dtcs.len())];
                    lines.extend(app.tp_dtc_lines(&dtcs, addr));
                    app.tp_lines = lines;
                    app.status = tr!("Fehlerspeicher gelesen.");
                }
                Err(e) => app.status = tr!("Fehler: {e}", e = format!("{:#}", e)),
            })
        });
    }

    pub(super) fn run_tp_clear(&mut self) {
        let Some(addr) = self.tp_address_oder_meldung() else { return };
        let be = self.backend();
        self.im_hintergrund(tr!("Fehlerspeicher löschen"), move || {
            let res = kwp::open_session(&be, addr).and_then(|mut ch| Kwp::new(&mut ch).clear_dtcs());
            Box::new(move |app: &mut App| {
                app.status = match res {
                    Ok(()) => {
                        app.tp_lines = vec![tr!("Fehlerspeicher gelöscht.")];
                        tr!("Fehlerspeicher gelöscht.")
                    }
                    Err(e) => tr!("Fehler: {e}", e = format!("{:#}", e)),
                };
            })
        });
    }

    /// Einen Messwertblock lesen (im Hintergrund). Im Live-Betrieb bleibt
    /// der Kanal danach offen und wird beim nächsten Lesen wiederverwendet.
    pub(super) fn block_lesen(&mut self) {
        let addr = match self.tp_address() {
            Ok(a) => a,
            Err(e) => {
                self.tp_live = false;
                self.status = tr!("Fehler: {e}", e = format!("{:#}", e));
                return;
            }
        };
        let Ok(block) = self.tp_block.trim().parse::<u8>() else {
            self.tp_live = false;
            self.status = tr!("Blocknummer ungültig (1..255)");
            return;
        };
        let kanal = self.tp_channel.take().filter(|(a, _)| *a == addr).map(|(_, c)| c);
        let be = self.backend();
        let live = self.tp_live;
        let arbeit = move || -> hintergrund::Ergebnis {
            let r = kanal.map_or_else(|| kwp::open_session(&be, addr), Ok).and_then(|mut ch| {
                let v = Kwp::new(&mut ch).read_block(block)?;
                Ok((ch, v))
            });
            Box::new(move |app: &mut App| {
                let r = r.and_then(|(ch, values)| {
                    if app.tp_live {
                        app.tp_channel = Some((addr, ch));
                    }
                    app.block_uebernehmen(addr, block, values)
                });
                match r {
                    Ok(()) if !app.tp_live => app.status = tr!("Messwertblock gelesen."),
                    Ok(()) => {}
                    Err(e) if app.tp_live => {
                        app.tp_live = false;
                        app.tp_channel = None;
                        app.status = tr!("Live-Messung gestoppt: {e}", e = e);
                    }
                    Err(e) => app.status = tr!("Fehler: {e}", e = format!("{:#}", e)),
                }
            })
        };
        if live {
            // alle 0,5 s: nicht jedes Mal die Statuszeile überschreiben
            self.im_hintergrund_still(tr!("Messwertblock lesen"), arbeit);
        } else {
            self.im_hintergrund(tr!("Messwertblock lesen"), arbeit);
        }
    }

    /// Gelesene Werte anzeigen (mit Sollwert-Prüfung) und ggf. in die CSV schreiben.
    fn block_uebernehmen(&mut self, addr: u8, block: u8, values: Vec<kwp::BlockValue>) -> anyhow::Result<()> {
        if self.tp_block_key != (addr, block) {
            self.mw_edit = None;
        }
        self.tp_block_key = (addr, block);
        self.tp_block_raw = values.clone();
        self.tp_block_values = values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let st = v
                    .decode()
                    .and_then(|(x, _)| self.db.sollwert("kwp", Some(u16::from(addr)), u16::from(block), Some(i as u8 + 1)).map(|r| r.check(x)));
                (v.display(), st)
            })
            .collect();
        if let Some(log) = self.tp_csv.as_mut() {
            let cols: Vec<(String, String)> = values
                .iter()
                .enumerate()
                .map(|(i, v)| match v.decode() {
                    Some((x, unit)) => (format!("B{block:03}.{} [{unit}]", i + 1), ecu_werkbank::csvlog::num(x)),
                    None => (tr!("B{block}.{a} [roh]", block = format!("{:03}", block), a = i + 1), format!("{:02X}{:02X}{:02X}", v.formula, v.a, v.b)),
                })
                .collect();
            log.write(&cols)?;
        }
        Ok(())
    }

    /// Laufenden Blockscan abholen (Fortschritt, Ergebnis).
    pub(super) fn bs_poll(&mut self, ui: &egui::Ui) {
        if let Some(rx) = &self.bs_job {
            let mut fertig = None;
            while let Ok(m) = rx.try_recv() {
                match m {
                    ScanMsg::Block(b) => self.bs_at = b,
                    ScanMsg::Fertig(r) => fertig = Some(r),
                }
            }
            match fertig {
                Some(Ok(v)) => {
                    self.status = tr!("Blockscan fertig: {a} Block/Blöcke mit Werten.", a = v.len());
                    let addr = self.bs_result.as_ref().map(|r| r.0).unwrap_or(0);
                    self.bs_result = Some((addr, v));
                    self.bs_job = None;
                }
                Some(Err(e)) => {
                    self.status = tr!("Blockscan: {e}", e = e);
                    self.bs_job = None;
                }
                None => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }

    }

    /// Blockscan für ein Steuergerät starten (alle Blöcke 1–255, nur lesen).
    pub(super) fn bs_start(&mut self, addr: u8) {
        self.tp_live = false;
        self.tp_channel = None;
        let be = self.backend();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = kwp::blockscan(&be, addr, 1, 255, |b| {
                let _ = tx.send(ScanMsg::Block(b));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(ScanMsg::Fertig(r));
        });
        self.bs_job = Some(rx);
        self.bs_at = 1;
        self.bs_result = Some((addr, Vec::new()));
    }

    /// Blockscan: alle Messwertblöcke des gewählten Steuergeräts (nur lesen).
    pub(super) fn ui_blockscan(&mut self, ui: &mut egui::Ui) {
        self.bs_poll(ui);

        ui.add_space(8.0);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.strong(tr!("Blockscan"));
            let busy = self.bs_job.is_some();
            let addr = parse_hex(&self.tp_addr).and_then(|a| u8::try_from(a).ok());
            if ui
                .add_enabled(!busy && addr.is_some(), egui::Button::new(tr!("🔍 Alle Blöcke lesen (1–255)")))
                .on_hover_text(tr!("Liest jeden Messwertblock einmal (nur lesen) und zeigt, welche es gibt."))
                .clicked()
            {
                self.bs_start(addr.expect("geprüft"));
            }
            if busy {
                ui.add(egui::ProgressBar::new(f32::from(self.bs_at) / 255.0).desired_width(160.0).text(tr!("Block {a}", a = self.bs_at)));
            }
        });
        let Some((addr, bloecke)) = &self.bs_result else { return };
        if bloecke.is_empty() {
            return;
        }
        let addr = *addr;
        ui.weak(tr!("Steuergerät {a}: {b} Block/Blöcke. Klick öffnet den Block unten — dort mit ✏ benennen.", a = format!("{addr:02X}"), b = bloecke.len()));
        let texte = &self.mw_texte;
        let mut oeffnen = None;
        egui::ScrollArea::vertical().id_source("bs_list").max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
            for (block, werte) in bloecke {
                let name = texte.block(addr, *block).map(|n| format!("  [{n}]")).unwrap_or_default();
                let zeile = format!("{block:03}{name}: {}", texte.zeile(addr, *block, werte));
                if ui.selectable_label(false, egui::RichText::new(zeile).monospace()).clicked() {
                    oeffnen = Some(*block);
                }
            }
        });
        let bloecke = bloecke.clone();
        egui::CollapsingHeader::new(tr!("✏ Namen für alle Blöcke und Felder eintragen"))
            .id_source(("bs_namen", addr))
            .show(ui, |ui| self.bs_namen_ui(ui, addr, &bloecke));
        if let Some(b) = oeffnen {
            self.tp_addr = format!("{addr:#04x}");
            self.tp_block = b.to_string();
            self.tp_live = false;
            self.tp_channel = None;
            self.block_lesen();
        }
    }

    /// Alle gefundenen Blöcke eines Steuergeräts als Liste zum Benennen.
    pub(super) fn bs_namen_ui(&mut self, ui: &mut egui::Ui, addr: u8, bloecke: &[(u8, Vec<kwp::BlockValue>)]) {
        use ecu_werkbank::messwerttexte::{art, schluessel};
        let eigene = ecu_werkbank::messwerttexte::alle();
        // Eingabefelder mit den gespeicherten Namen vorbelegen
        for (b, werte) in bloecke {
            let keys = std::iter::once(schluessel(addr, *b, None)).chain((1..=werte.len() as u8).map(|f| schluessel(addr, *b, Some(f))));
            for k in keys {
                self.bs_namen.entry(k).or_insert_with_key(|k| eigene.get(k).cloned().unwrap_or_default());
            }
        }
        let pre = format!("{addr:02X}-");
        let geaendert: Vec<(String, String)> = self
            .bs_namen
            .iter()
            .filter(|(k, t)| k.starts_with(&pre) && eigene.get(*k).map(String::as_str).unwrap_or("") != t.trim())
            .map(|(k, t)| (k.clone(), t.clone()))
            .collect();
        ui.label(tr!("Trag die Namen ein, die du kennst (z. B. aus einem anderen Diagnoseprogramm oder dem Reparaturleitfaden). Leer lassen geht auch. Grau steht, was ECUWerkbank schon weiß."));
        // Fortschritt: benannte Felder (gespeichert oder gerade eingetippt)
        let (mut gesamt, mut benannt) = (0, 0);
        for (b, werte) in bloecke {
            for (i, v) in werte.iter().enumerate() {
                if v.is_leer() {
                    continue;
                }
                gesamt += 1;
                if self.bs_namen.get(&schluessel(addr, *b, Some(i as u8 + 1))).is_some_and(|t| !t.trim().is_empty()) {
                    benannt += 1;
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::ProgressBar::new(if gesamt == 0 { 0.0 } else { benannt as f32 / gesamt as f32 }).desired_width(160.0).text(tr!("{a} von {b} Feldern benannt", a = benannt, b = gesamt)));
            ui.checkbox(&mut self.bn_nur_offen, tr!("nur Felder ohne Namen"));
            ui.checkbox(&mut self.bn_roh_aus, tr!("Rohwerte ohne Umrechnung ausblenden"));
            ui.label(tr!("Suche:"));
            ui.add(egui::TextEdit::singleline(&mut self.bn_suche).desired_width(140.0).hint_text(tr!("Wert oder Name")));
        });
        let (nur_offen, roh_aus, suche) = (self.bn_nur_offen, self.bn_roh_aus, self.bn_suche.trim().to_lowercase());
        if ui.add_enabled(!geaendert.is_empty(), egui::Button::new(tr!("💾 Namen speichern ({a})", a = geaendert.len()))).clicked() {
            self.status = match ecu_werkbank::messwerttexte::mehrere_setzen(&geaendert) {
                Ok(()) => {
                    self.mw_texte = ecu_werkbank::messwerttexte::Texte::laden();
                    tr!("{a} Name(n) gespeichert.", a = geaendert.len())
                }
                Err(e) => format!("{e:#}"),
            };
        }
        let texte = &self.mw_texte;
        let namen = &mut self.bs_namen;
        egui::ScrollArea::vertical().id_source(("bs_namen_scroll", addr)).max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new(("bs_namen_grid", addr)).striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                for (b, werte) in bloecke {
                    ui.strong(tr!("Block {a}", a = format!("{b:03}")));
                    ui.weak(tr!("Name des Blocks"));
                    let hint = texte.block(addr, *b).unwrap_or("").to_string();
                    if let Some(f) = namen.get_mut(&schluessel(addr, *b, None)) {
                        ui.add_sized([300.0, 20.0], egui::TextEdit::singleline(f).hint_text(hint));
                    }
                    ui.end_row();
                    for (i, v) in werte.iter().enumerate() {
                        if v.is_leer() {
                            continue;
                        }
                        let feld = i as u8 + 1;
                        let wert = match art(v.formula) {
                            Some(a) => format!("{}  (~{a})", v.display()),
                            None => v.display(),
                        };
                        let name_jetzt = namen.get(&schluessel(addr, *b, Some(feld))).map(|t| t.trim().to_string()).unwrap_or_default();
                        if (nur_offen && !name_jetzt.is_empty())
                            || (roh_aus && v.decode().is_none() && v.text.is_none() && !matches!(v.formula, 0x10 | 0x11 | 0x6B | 0xA1))
                            || (!suche.is_empty() && !wert.to_lowercase().contains(&suche) && !name_jetzt.to_lowercase().contains(&suche))
                        {
                            continue;
                        }
                        ui.label(tr!("  Feld {a}", a = feld));
                        ui.allocate_ui(egui::vec2(210.0, 18.0), |ui| {
                            ui.set_min_width(210.0);
                            ui.add(egui::Label::new(egui::RichText::new(&wert).monospace()).truncate()).on_hover_text(&wert);
                        });
                        let hint = tr!("Name eintragen …");
                        if let Some(f) = namen.get_mut(&schluessel(addr, *b, Some(feld))) {
                            ui.add_sized([300.0, 20.0], egui::TextEdit::singleline(f).hint_text(hint));
                        }
                        ui.end_row();
                    }
                }
            });
        });
    }

    /// Stellglieddiagnose (Funktion 03) am gewählten TP-2.0-Steuergerät.
    /// Steuert Bauteile wirklich an — nur nach Bestätigung, Schritt für Schritt.
    pub(super) fn ui_stellglied(&mut self, ui: &mut egui::Ui) {
        // Meldungen abholen
        if let Some((_, rx)) = &self.sg_job {
            let mut fertig = None;
            while let Ok(m) = rx.try_recv() {
                match m {
                    SgMsg::Stand(t) => self.sg_log.push(t),
                    SgMsg::Fertig(r) => fertig = Some(r),
                }
            }
            match fertig {
                Some(Ok(())) => {
                    self.sg_log.push(tr!("Stellglieddiagnose beendet."));
                    self.sg_job = None;
                }
                Some(Err(e)) => {
                    self.sg_log.push(tr!("Abgebrochen: {e}", e = e));
                    self.sg_job = None;
                }
                None => ui.ctx().request_repaint_after(std::time::Duration::from_millis(150)),
            }
        }
        ui.add_space(8.0);
        ui.separator();
        let laeuft = self.sg_job.is_some();
        let offen = laeuft || std::mem::take(&mut self.sg_zeigen);
        egui::CollapsingHeader::new(tr!("🔧 Stellglieddiagnose (Funktion 03)")).id_source("stellglied-tp20").open(offen.then_some(true)).show(ui, |ui| {
            ui.colored_label(stufe_color(Stufe::Warnung), tr!("Achtung: Die Bauteile werden wirklich angesteuert — Fensterheber fahren, Verriegelung schaltet, Lampen gehen an. Hände weg von Fenstern und Türen."));
            let Some(addr) = parse_hex(&self.tp_addr).filter(|a| *a <= 0xFF).map(|a| a as u8) else {
                ui.weak(tr!("Oben eine Steuergerät-Adresse eingeben."));
                return;
            };
            if !laeuft {
                ui.checkbox(&mut self.sg_bestaetigt, tr!("Ich habe das Fahrzeug gesichert und weiß, dass sich Bauteile bewegen."));
                if ui.add_enabled(self.sg_bestaetigt && self.backend().supports_tp20(), egui::Button::new(tr!("▶ Stellglieddiagnose starten ({a})", a = format!("{addr:02X}")))).clicked() {
                    let (cmd_tx, cmd_rx) = mpsc::channel();
                    let (msg_tx, msg_rx) = mpsc::channel();
                    let be = self.backend();
                    std::thread::spawn(move || {
                        let r = kwp::stellglieddiagnose(&be, addr, |s| {
                            if let Some(s) = s {
                                let _ = msg_tx.send(SgMsg::Stand(s.text()));
                            }
                            cmd_rx.try_recv().unwrap_or(kwp::StellBefehl::Bleiben)
                        });
                        let _ = msg_tx.send(SgMsg::Fertig(r.map_err(|e| format!("{e:#}"))));
                    });
                    self.sg_log = vec![tr!("Gestartet an Steuergerät {a}.", a = format!("{addr:02X}"))];
                    self.sg_job = Some((cmd_tx, msg_rx));
                    self.sg_bestaetigt = false;
                }
            } else {
                ui.horizontal(|ui| {
                    if ui.button(tr!("⏭ Nächstes Bauteil")).clicked() {
                        if let Some((tx, _)) = &self.sg_job {
                            let _ = tx.send(kwp::StellBefehl::Weiter);
                        }
                    }
                    if ui.button(tr!("⏹ Beenden")).clicked() {
                        if let Some((tx, _)) = &self.sg_job {
                            let _ = tx.send(kwp::StellBefehl::Ende);
                        }
                    }
                    ui.spinner();
                });
            }
            for z in self.sg_log.iter().rev().take(12) {
                ui.monospace(z);
            }
        });
    }

    /// Anpassung (Funktion 10) am gewählten TP-2.0-Steuergerät lesen (nur lesen).
    pub(super) fn ui_anpassung_tp20(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.separator();
        let offen = std::mem::take(&mut self.an_zeigen);
        egui::CollapsingHeader::new(tr!("🔧 Anpassung (Funktion 10)")).id_source("anpassung-tp20").open(offen.then_some(true)).show(ui, |ui| {
            ui.weak(tr!("Liest gespeicherte Anpassungswerte wie ein Werkstatt-Diagnosegerät (am Golf 6: Bordnetz, Einparkhilfe). Ältere Steuergeräte wie die Türen kennen das nicht."));
            let Some(addr) = parse_hex(&self.tp_addr).filter(|a| *a <= 0xFF).map(|a| a as u8) else {
                ui.weak(tr!("Oben eine Steuergerät-Adresse eingeben."));
                return;
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("Kanäle (dezimal, mit Komma):"));
                ui.add(egui::TextEdit::singleline(&mut self.an_kanaele).desired_width(160.0).hint_text("1,3,17"));
                if ui.add_enabled(self.backend().supports_tp20(), egui::Button::new(tr!("📖 Lesen ({a})", a = format!("{addr:02X}")))).clicked() {
                    let kanaele: Vec<u8> = self.an_kanaele.split(',').filter_map(|t| t.trim().parse().ok()).collect();
                    let be = self.backend();
                    self.im_hintergrund(tr!("Anpassung lesen"), move || {
                        let zeilen = match kwp::anpassung_lesen(&be, addr, &kanaele) {
                            Ok(l) => l
                                .into_iter()
                                .map(|r| match r {
                                    Ok(k) => {
                                        let f: Vec<String> = k.felder.iter().filter(|v| !v.is_leer()).map(|v| v.display()).collect();
                                        tr!("Kanal {a}: Wert {b}   [{c}]", a = format!("{:03}", k.kanal), b = k.wert, c = f.join("  |  "))
                                    }
                                    Err(e) => e,
                                })
                                .collect(),
                            Err(e) => vec![format!("{e:#}")],
                        };
                        Box::new(move |app: &mut App| {
                            app.an_result = zeilen;
                            app.status = tr!("Anpassung gelesen.");
                        })
                    });
                }
            });
            for z in &self.an_result {
                ui.monospace(z);
            }
            ui.add_space(6.0);
            ui.strong(tr!("Wert ändern"));
            if !kwp::darf_senden(&self.backend(), kwp::ANPASSUNG_SCHREIBEN_GEPRUEFT) {
                ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Am Auto noch nicht bestätigt: es läuft erst ein Probelauf (liest den alten Wert, zeigt, was gesendet würde, sendet nichts)."));
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("Kanal:"));
                ui.add(egui::TextEdit::singleline(&mut self.an_kanal).desired_width(40.0).hint_text("23"));
                ui.label(tr!("neuer Wert:"));
                ui.add(egui::TextEdit::singleline(&mut self.an_wert).desired_width(70.0));
            });
            let kanal = self.an_kanal.trim().parse::<u8>().ok();
            let wert = self.an_wert.trim().parse::<u16>().ok();
            ui.checkbox(&mut self.an_ok, tr!("Alten Wert gelesen und notiert — neuen Wert speichern"));
            if ui.add_enabled(self.an_ok && kanal.is_some() && wert.is_some() && self.backend().supports_tp20(), egui::Button::new(tr!("✍ Anpassung speichern"))).clicked() {
                self.an_ok = false;
                let (kanal, wert) = (kanal.unwrap_or_default(), wert.unwrap_or_default());
                let w = |b: &[u8]| if b.len() == 2 { u16::from_be_bytes([b[0], b[1]]).to_string() } else { ecu_werkbank::backup::hex(b) };
                let be = self.backend();
                self.im_hintergrund(tr!("Anpassung speichern"), move || {
                    let zeile = match kwp::anpassung_schreiben(&be, addr, kanal, wert) {
                        Ok(kwp::Schreiben::Probelauf { anfrage, alt }) => tr!("Kanal {a}: Probelauf — nichts gesendet. Jetzt {b}, würde senden: {c}", a = format!("{kanal:03}"), b = w(&alt), c = ecu_werkbank::backup::hex(&anfrage)),
                        Ok(kwp::Schreiben::Geschrieben { alt, danach }) if danach == wert.to_be_bytes() => tr!("Kanal {a}: gespeichert und kontrolliert, {b} → {c}", a = format!("{kanal:03}"), b = w(&alt), c = w(&danach)),
                        Ok(kwp::Schreiben::Geschrieben { alt, danach }) => tr!("Kanal {a}: Achtung, danach gelesen {b} (alter Wert {c})", a = format!("{kanal:03}"), b = w(&danach), c = w(&alt)),
                        Err(e) => format!("{e:#}"),
                    };
                    Box::new(move |app: &mut App| {
                        app.status = zeile.clone();
                        app.an_result.push(zeile);
                    })
                });
            }
            if !self.an_wert.trim().is_empty() && wert.is_none() {
                ui.weak(tr!("Bitte eine ganze Zahl von 0 bis 65535 eingeben."));
            }
        });
    }

    /// VAG_FOTO: nach der Wartezeit ein Bild des Fensters als PPM speichern und
    /// das Programm schließen (für die Bilder im Handbuch, auch bei verdecktem Fenster).
    pub(super) fn foto_tick(&mut self, ctx: &egui::Context) {
        let Some((pfad, warten, start, angefordert)) = &mut self.foto else { return };
        if !*angefordert && start.elapsed() >= *warten {
            *angefordert = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        }
        let bild = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = bild {
            let [w, h] = img.size;
            let mut daten = format!("P6\n{w} {h}\n255\n").into_bytes();
            daten.reserve(w * h * 3);
            for px in &img.pixels {
                daten.extend_from_slice(&[px.r(), px.g(), px.b()]);
            }
            if let Err(e) = ecu_werkbank::ablage::schreiben(&*pfad, daten) {
                eprintln!("Foto: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// VAG_FILM: eine Bildfolge aufnehmen, während die Demo die Werte bewegt
    /// (für bewegte Ausschnitte im Werbevideo). Nur Demo/Screenshots.
    pub(super) fn film_tick(&mut self, ctx: &egui::Context) {
        let Some(f) = &mut self.film else { return };
        if f.start.elapsed() < f.warten {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
            return;
        }
        if f.wartet_bild {
            let bild = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(img) = bild {
                let [w, h] = img.size;
                let mut daten = format!("P6\n{w} {h}\n255\n").into_bytes();
                daten.reserve(w * h * 3);
                for px in &img.pixels {
                    daten.extend_from_slice(&[px.r(), px.g(), px.b()]);
                }
                let _ = std::fs::create_dir_all(&f.dir);
                let pfad = f.dir.join(format!("frame_{:04}.ppm", f.zaehler));
                if let Err(e) = ecu_werkbank::ablage::schreiben(&pfad, daten) {
                    eprintln!("Film: {e}");
                }
                f.zaehler += 1;
                f.wartet_bild = false;
                f.naechste = std::time::Instant::now();
                if f.zaehler >= f.total {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
        }
        if !f.wartet_bild && f.naechste.elapsed() >= f.interval {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            f.wartet_bild = true;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(25));
    }

    /// Meldungen der laufenden Grundeinstellung abholen (jedes Bild, auch in anderen Reitern).
    pub(super) fn poll_grund(&mut self, ctx: &egui::Context) {
        use ecu_werkbank::kwp_grund::Meldung;
        let Some((_, rx)) = &self.ge_job else { return };
        let mut msgs = Vec::new();
        let mut getrennt = false;
        loop {
            match rx.try_recv() {
                Ok(m) => msgs.push(m),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    getrennt = true;
                    break;
                }
            }
        }
        let mut ende = getrennt;
        for m in msgs {
            match m {
                Meldung::Laeuft => {
                    self.ge_laeuft = true;
                    self.ge_status = tr!("Grundeinstellung läuft.");
                    self.status = tr!("⚙ Grundeinstellung läuft — ausschalten im Reiter „TP 2.0“.");
                }
                Meldung::Werte(w) => self.ge_werte = w,
                Meldung::Fertig(t) => {
                    self.ge_status = t.clone();
                    self.status = t;
                    ende = true;
                }
                Meldung::Fehler(t) => {
                    self.ge_status = tr!("Fehler: {e}", e = t);
                    self.status = self.ge_status.clone();
                    ende = true;
                }
            }
        }
        if ende {
            self.ge_job = None;
            self.ge_laeuft = false;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    /// Grundeinstellung im Hintergrund starten (nur nach der Rückfrage).
    pub(super) fn ge_starten(&mut self, a: ecu_werkbank::kwp_grund::Ablauf, gruppe: u8) {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        // eigenen Live-Kanal zum Steuergerät vorher freigeben
        self.tp_live = false;
        self.tp_channel = None;
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let be = self.backend();
        let s2 = stop.clone();
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            if let Err(e) = ecu_werkbank::kwp_grund::ausfuehren(&be, &a, gruppe, &s2, |m| {
                let _ = tx2.send(m);
            }) {
                let _ = tx.send(ecu_werkbank::kwp_grund::Meldung::Fehler(format!("{e:#}")));
            }
        });
        self.ge_job = Some((stop, rx));
        self.ge_laeuft = false;
        self.ge_werte.clear();
        self.ge_status = tr!("Starte …");
    }

    /// Abschnitt „Grundeinstellung“ im TP-2.0-Reiter: gesperrt bis ein eigener
    /// Mitschnitt den Ablauf für dieses Steuergerät dieses Fahrzeugs belegt.
    pub(super) fn ui_grund(&mut self, ui: &mut egui::Ui) {
        use ecu_werkbank::kwp_grund as ge;
        enum Akt {
            Auswerten(u8, u8, String),
            Uebernehmen(String),
            Verwerfen,
            Vergessen(String, u8),
            Fragen,
            Starten(ge::Ablauf, u8),
            Abbrechen,
            Aus,
        }
        let mut akt = None;
        ui.add_space(8.0);
        ui.separator();
        let offen = self.ge_job.is_some() || self.ge_neu.is_some() || self.ge_frage || self.ge_zeigen;
        egui::CollapsingHeader::new(tr!("⚙ Grundeinstellung (Funktion 04)")).id_source("grund-tp20").default_open(offen).show(ui, |ui| {
            ui.weak(tr!("Den genauen Ablauf hat der Hersteller nicht veröffentlicht. ECUWerkbank rät ihn nicht, sondern lernt ihn aus deinem eigenen Mitschnitt — so lange ist das Senden gesperrt."));
            let Some(addr) = parse_hex(&self.tp_addr).filter(|a| *a <= 0xFF).map(|a| a as u8) else {
                ui.weak(tr!("Oben eine Steuergerät-Adresse eingeben."));
                return;
            };
            let Some(fz) = self.active_vehicle.clone() else {
                ui.weak(tr!("Oben ein Fahrzeug wählen — der Ablauf wird je Fahrzeug gelernt."));
                return;
            };
            let ablauf = ge::ablauf(&fz, addr);
            let gruppe = self.ge_gruppe.trim().parse::<u8>().ok().filter(|g| *g >= 1);
            match &ablauf {
                None => {
                    ui.colored_label(stufe_color(Stufe::Hinweis), tr!("🔒 Gesperrt für Steuergerät {a}: noch kein Ablauf gelernt.", a = format!("{addr:02X}")));
                    ui.label(tr!("So wird es freigeschaltet:"));
                    ui.label(tr!("1. Reiter „Mitschnitt & Lernhilfe“: Aufnahme starten (CAN-Interface und das Kabel des Diagnosegeräts über ein OBD-Y-Kabel gleichzeitig dran)."));
                    ui.label(tr!("2. Im Diagnoseprogramm an diesem Steuergerät „Grundeinstellung – 04“ wählen, eine Gruppe eingeben, einschalten, ein paar Sekunden laufen lassen, ausschalten."));
                    ui.label(tr!("3. Aufnahme stoppen, unten dieselbe Gruppe eintragen und den Mitschnitt auswerten."));
                }
                Some(a) => {
                    ui.colored_label(stufe_color(Stufe::Gut), tr!("🔓 Ablauf gelernt aus {q} am {d} (Gruppe {g}).", q = a.quelle, d = a.datum, g = a.gruppe_gelernt));
                    egui::CollapsingHeader::new(tr!("Gesendete Anfragen")).id_source("grund-ablauf").show(ui, |ui| {
                        for z in a.beschreibung() {
                            ui.monospace(z);
                        }
                    });
                }
            }
            ui.horizontal(|ui| {
                ui.label(tr!("Gruppe:"));
                ui.add_enabled(self.ge_job.is_none(), egui::TextEdit::singleline(&mut self.ge_gruppe).desired_width(50.0).hint_text("060"));
                if ui.add_enabled(self.ge_job.is_none(), egui::Button::new(tr!("📂 Mitschnitt auswerten…"))).on_hover_text(tr!("Ablauf für dieses Steuergerät aus einem Mitschnitt lernen (die Gruppe muss die aus dem Mitschnitt sein)")).clicked() {
                    match gruppe {
                        Some(g) => {
                            if let Some(p) = rfd::FileDialog::new().set_title(tr!("Mitschnitt wählen")).pick_file() {
                                akt = Some(Akt::Auswerten(addr, g, p.display().to_string()));
                            }
                        }
                        None => self.ge_status = tr!("Erst die Gruppe eintragen (1–255)."),
                    }
                }
                if let Some(a) = &ablauf {
                    if self.ge_job.is_none() && !self.ge_frage {
                        if ui.add_enabled(gruppe.is_some(), egui::Button::new(tr!("▶ Grundeinstellung starten…"))).clicked() {
                            akt = Some(Akt::Fragen);
                        }
                        if ui.small_button(tr!("🗑 Ablauf vergessen")).on_hover_text(tr!("Steuergerät wieder sperren")).clicked() {
                            akt = Some(Akt::Vergessen(fz.clone(), a.adresse));
                        }
                    }
                }
            });
            // frisch gelernt: erst nach „Übernehmen“ wird freigeschaltet
            if let Some(n) = &self.ge_neu {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.strong(tr!("Gelernt — bitte prüfen:"));
                    for z in n.beschreibung() {
                        ui.monospace(z);
                    }
                    ui.weak(tr!("Übernommen wird nur, was das Steuergerät im Mitschnitt bestätigt hat. Beim Starten wird nur die Gruppe ausgetauscht."));
                    ui.horizontal(|ui| {
                        if ui.button(tr!("💾 Übernehmen (schaltet das Senden frei)")).clicked() {
                            akt = Some(Akt::Uebernehmen(fz.clone()));
                        }
                        if ui.button(tr!("Verwerfen")).clicked() {
                            akt = Some(Akt::Verwerfen);
                        }
                    });
                });
            }
            // Rückfrage
            if let (true, Some(a), Some(g)) = (self.ge_frage, &ablauf, gruppe) {
                egui::Frame::group(ui.style()).stroke(egui::Stroke::new(1.5_f32, stufe_color(Stufe::Warnung))).show(ui, |ui| {
                    ui.colored_label(stufe_color(Stufe::Warnung), tr!("⚠ Grundeinstellung Gruppe {g} an Steuergerät {a} starten?", g = g, a = format!("{addr:02X}")));
                    ui.label(tr!("Das Steuergerät führt die Grundeinstellung dieser Gruppe sofort aus (z. B. Drosselklappe anlernen). Was die Gruppe bewirkt und welche Bedingungen gelten, vorher nachlesen (Lexikon, Reparaturleitfaden). Auto steht, Zündung an."));
                    ui.horizontal(|ui| {
                        if ui.button(tr!("✅ Ja, starten")).clicked() {
                            akt = Some(Akt::Starten(a.clone(), g));
                        }
                        if ui.button(tr!("Abbrechen")).clicked() {
                            akt = Some(Akt::Abbrechen);
                        }
                    });
                });
            }
            // läuft
            if self.ge_job.is_some() {
                ui.horizontal(|ui| {
                    if ui.button(egui::RichText::new(tr!("⏹ Aus")).strong()).clicked() {
                        akt = Some(Akt::Aus);
                    }
                    ui.spinner();
                });
                egui::Grid::new("grund-werte").spacing([24.0, 4.0]).show(ui, |ui| {
                    for (i, w) in self.ge_werte.iter().enumerate() {
                        ui.vertical(|ui| {
                            ui.weak(tr!("Feld {a}", a = i + 1));
                            ui.heading(w);
                        });
                    }
                    ui.end_row();
                });
            }
            if !self.ge_status.is_empty() {
                ui.label(&self.ge_status);
            }
        });
        match akt {
            Some(Akt::Auswerten(addr, g, pfad)) => {
                self.ge_status = match ecu_werkbank::ablage::lesen(&pfad) {
                    Err(e) => tr!("Lesen: {e}", e = e),
                    Ok(text) => {
                        let d = ecu_werkbank::canlog::Decoder::decode_text(&text);
                        let name = std::path::Path::new(&pfad).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(pfad.clone());
                        match ge::lernen(&d.messages, addr, g, &name) {
                            Ok(a) => {
                                self.ge_neu = Some(a);
                                tr!("Ablauf erkannt — bitte prüfen und übernehmen.")
                            }
                            Err(e) => tr!("Nicht gelernt: {e}", e = format!("{e:#}")),
                        }
                    }
                };
            }
            Some(Akt::Uebernehmen(fz)) => {
                if let Some(a) = self.ge_neu.take() {
                    self.ge_status = match ge::merken(&fz, &a) {
                        Ok(()) => tr!("Übernommen — Grundeinstellung für Steuergerät {a} freigeschaltet.", a = format!("{:02X}", a.adresse)),
                        Err(e) => tr!("Speichern: {e}", e = format!("{e:#}")),
                    };
                }
            }
            Some(Akt::Verwerfen) => {
                self.ge_neu = None;
                self.ge_status = tr!("Verworfen.");
            }
            Some(Akt::Vergessen(fz, addr)) => {
                self.ge_status = match ge::vergessen(&fz, addr) {
                    Ok(()) => tr!("Ablauf vergessen — Steuergerät {a} wieder gesperrt.", a = format!("{addr:02X}")),
                    Err(e) => tr!("Speichern: {e}", e = format!("{e:#}")),
                };
            }
            Some(Akt::Fragen) => self.ge_frage = true,
            Some(Akt::Abbrechen) => {
                self.ge_frage = false;
                self.ge_status = tr!("Abgebrochen — nichts gesendet.");
            }
            Some(Akt::Starten(a, g)) => {
                self.ge_frage = false;
                self.ge_starten(a, g);
            }
            Some(Akt::Aus) => {
                if let Some((stop, _)) = &self.ge_job {
                    stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    self.ge_status = tr!("Schalte aus …");
                }
            }
            None => {}
        }
    }

    /// Leiste mit gemerkten Messwertblöcken: Klick öffnet und liest den Block.
    pub(super) fn ui_favoriten_leiste(&mut self, ui: &mut egui::Ui) {
        if self.favoriten.is_empty() {
            return;
        }
        enum FavAkt {
            Oeffnen(u8, u8),
            Entfernen(u8, u8),
        }
        let mut akt = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("⭐ Favoriten:"));
            for f in &self.favoriten {
                let name = if f.name.is_empty() { self.mw_texte.block(f.adresse, f.block).unwrap_or("").to_string() } else { f.name.clone() };
                let label = if name.is_empty() { format!("{:02X}·{:03}", f.adresse, f.block) } else { format!("{:02X}·{:03} {name}", f.adresse, f.block) };
                if ui.button(label).on_hover_text(tr!("Block öffnen und lesen")).clicked() {
                    akt = Some(FavAkt::Oeffnen(f.adresse, f.block));
                }
                if ui.small_button("✖").on_hover_text(tr!("Favorit entfernen")).clicked() {
                    akt = Some(FavAkt::Entfernen(f.adresse, f.block));
                }
                ui.add_space(4.0);
            }
        });
        match akt {
            Some(FavAkt::Oeffnen(a, b)) => {
                self.tp_addr = format!("{a:#04x}");
                self.tp_block = b.to_string();
                self.tp_live = false;
                self.tp_channel = None;
                self.block_lesen();
            }
            Some(FavAkt::Entfernen(a, b)) => {
                if let Err(e) = ecu_werkbank::favoriten::entfernen(a, b) {
                    self.status = tr!("Favorit: {e}", e = format!("{:#}", e));
                }
                self.favoriten = ecu_werkbank::favoriten::alle();
            }
            None => {}
        }
    }

    pub(super) fn ui_tp20(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Ältere Steuergeräte (TP 2.0)"));
        ui.label(tr!("Für Fahrzeuge etwa 2004–2012 (z. B. Golf 6). Erst „Steuergeräte suchen“, dann ein Steuergerät anklicken — darunter stehen Fehler, Messwerte, Grundeinstellung, Stellglieddiagnose und Anpassung. Geht nur mit dem CAN-Interface, nicht mit dem ELM327."));
        if !self.backend().supports_tp20() {
            ui.colored_label(egui::Color32::from_rgb(220, 160, 40), tr!("⚠ TP 2.0 geht nur mit SocketCAN oder Demo — oben umstellen."));
        }
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let busy = self.tp_sweep.is_some();
            if ui.add_enabled(!busy, egui::Button::new(tr!("🔎 Steuergeräte suchen (0x01–0x7F)"))).clicked() {
                self.start_sweep();
            }
            if busy {
                ui.add(egui::ProgressBar::new(f32::from(self.tp_sweep_at) / 127.0)
                    .desired_width(160.0)
                    .text(format!("{:#04x}", self.tp_sweep_at)));
            }
        });
        if !self.tp_found.is_empty() {
            egui::Grid::new("tpfound").striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.strong(tr!("Adresse"));
                ui.strong(tr!("Kennung"));
                ui.strong(tr!("Fehler"));
                ui.end_row();
                let mut pick = None;
                for f in &self.tp_found {
                    if ui.selectable_label(self.tp_addr == format!("{:#04x}", f.address), format!("{:#04x}", f.address)).clicked() {
                        pick = Some(f.address);
                    }
                    ui.label(if f.name.is_empty() { f.ident.join(" | ") } else { format!("{}  —  {}", f.name, f.ident.join(" | ")) });
                    ui.label(match &f.dtcs {
                        None => "?".to_string(),
                        Some(d) => d.len().to_string(),
                    });
                    ui.end_row();
                }
                if let Some(a) = pick {
                    self.tp_addr = format!("{a:#04x}");
                }
            });
        }

        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(tr!("Adresse:"));
            ui.add(egui::TextEdit::singleline(&mut self.tp_addr).desired_width(60.0));
            // TP-2.0-Adressen sind keine Diagnoseadressen: Name nur aus dem Auto-Scan
            let name = parse_hex(&self.tp_addr)
                .and_then(|a| self.tp_found.iter().find(|f| u16::from(f.address) == a))
                .map(|f| f.name.clone())
                .unwrap_or_default();
            ui.weak(name);
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("ℹ Identifikation")).clicked() {
                self.run_tp_ident();
            }
            if ui.button(tr!("🛠 Fehlerspeicher lesen")).clicked() {
                self.run_tp_dtc();
            }
            if ui.button(tr!("✖ Fehlerspeicher löschen")).clicked() {
                self.run_tp_clear();
            }
        });
        if !self.tp_lines.is_empty() {
            ui.add_space(4.0);
            let model = self.vehicle_model();
            for line in self.tp_lines.clone() {
                let notes = code_in(&line).map(|c| self.repairs_for(&c)).unwrap_or_default();
                self.dtc_row(ui, &line, &model, &notes);
            }
        }

        self.ui_blockscan(ui);
        self.ui_grund(ui);
        self.ui_stellglied(ui);
        self.ui_anpassung_tp20(ui);

        ui.add_space(8.0);
        ui.separator();
        ui.strong(tr!("Messwertblock"));
        self.ui_favoriten_leiste(ui);
        ui.horizontal(|ui| {
            ui.label(tr!("Block:"));
            ui.add(egui::TextEdit::singleline(&mut self.tp_block).desired_width(50.0));
            if ui.button(tr!("Lesen")).clicked() {
                self.tp_live = false;
                self.tp_channel = None;
                self.block_lesen();
            }
            let caption = if self.tp_live { tr!("⏸ Live stoppen") } else { tr!("▶ Live (2×/s)") };
            let was_live = self.tp_live;
            ui.toggle_value(&mut self.tp_live, caption);
            if !was_live && self.tp_live {
                self.tp_csv = None;
            }
            let mut rec = self.tp_csv.is_some();
            if ui.add_enabled(self.tp_live, egui::Checkbox::new(&mut rec, tr!("⏺ CSV aufzeichnen"))).changed() {
                self.tp_csv = if rec { auto_csv(self.vehicle_sub("aufzeichnungen"), "messwertblock", &mut self.status) } else { None };
            }
        });
        if let Ok(block) = self.tp_block.trim().parse::<u16>() {
            if let Some(m) = parse_hex(&self.tp_addr).and_then(|a| self.db.measuring(a, block)) {
                if !m.name.is_empty() {
                    ui.weak(tr!("Label: {a}", a = m.name));
                }
            }
        }
        if !self.tp_block_values.is_empty() {
            let (addr, block) = self.tp_block_key;
            let mut speichern: Option<(Option<u8>, String)> = None;
            ui.horizontal(|ui| {
                match self.mw_texte.block(addr, block) {
                    Some(n) => ui.strong(n),
                    None => ui.weak(tr!("Block {a} (ohne Namen)", a = format!("{:03}", block))),
                };
                if ui.small_button("✏").on_hover_text(tr!("Name des Blocks eintragen")).clicked() {
                    self.mw_edit = Some((None, self.mw_texte.block(addr, block).unwrap_or_default().to_string()));
                }
                let ist_fav = self.favoriten.iter().any(|f| f.adresse == addr && f.block == block);
                let (stern, tip) = if ist_fav { ("★", tr!("Favorit entfernen")) } else { ("☆", tr!("Als Favorit merken (Schnellzugriff oben)")) };
                if ui.small_button(stern).on_hover_text(tip).clicked() {
                    let name = self.mw_texte.block(addr, block).unwrap_or_default().to_string();
                    if let Err(e) = ecu_werkbank::favoriten::umschalten(addr, block, &name) {
                        self.status = tr!("Favorit: {e}", e = format!("{:#}", e));
                    }
                    self.favoriten = ecu_werkbank::favoriten::alle();
                }
            });
            egui::Grid::new("tpblock").spacing([24.0, 4.0]).show(ui, |ui| {
                for (i, (v, st)) in self.tp_block_values.iter().enumerate() {
                    let feld = i as u8 + 1;
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let raw = self.tp_block_raw.get(i);
                            let name = raw.map(|r| self.mw_texte.feld(addr, block, feld, r)).unwrap_or_else(|| tr!("Feld {a}", a = feld));
                            if self.mw_texte.eigen(addr, block, feld).is_some() {
                                ui.label(name);
                            } else {
                                ui.weak(name).on_hover_text(tr!("Feld {a} — „~“ = nur die Art aus der Umrechnung. Mit ✏ eigenen Text eintragen.", a = feld));
                            }
                            if ui.small_button("✏").on_hover_text(tr!("Text für dieses Feld eintragen")).clicked() {
                                self.mw_edit = Some((Some(feld), self.mw_texte.eigen(addr, block, feld).unwrap_or_default().to_string()));
                            }
                        });
                        match st {
                            Some(s) => ui.label(egui::RichText::new(v).heading().color(stufe_color(*s))),
                            None => ui.heading(v),
                        };
                    });
                    if feld.is_multiple_of(4) {
                        ui.end_row();
                    }
                }
                ui.end_row();
            });
            if let Some((feld, text)) = &mut self.mw_edit {
                ui.horizontal(|ui| {
                    let wo = match feld {
                        Some(f) => tr!("Feld {a}", a = *f),
                        None => tr!("Blockname"),
                    };
                    ui.label(tr!("{a}:", a = wo));
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(260.0).hint_text(tr!("z. B. Kühlmitteltemperatur")));
                    if ui.button(tr!("💾 Speichern")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        speichern = Some((*feld, text.clone()));
                    }
                    if ui.button(tr!("Abbrechen")).clicked() {
                        speichern = Some((Some(0), String::new())); // Marker: schließen
                    }
                });
                ui.weak(tr!("Leer speichern löscht den Text. Gilt für Steuergerät {a}, Block {b}.", a = format!("{:02X}", addr), b = format!("{:03}", block)));
            }
            match speichern {
                Some((Some(0), _)) => self.mw_edit = None,
                Some((feld, text)) => {
                    self.mw_edit = None;
                    self.status = match ecu_werkbank::messwerttexte::setzen(addr, block, feld, &text) {
                        Ok(()) => {
                            self.mw_texte = ecu_werkbank::messwerttexte::Texte::laden();
                            tr!("Text gespeichert.")
                        }
                        Err(e) => tr!("Speichern: {e}", e = format!("{:#}", e)),
                    };
                }
                None => {}
            }
        }
    }
}
