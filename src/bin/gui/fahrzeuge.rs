//! GUI: Fahrzeuge/Fahrzeugakte, Tankbuch, Kosten, Kalender, Datensicherung, Fahrzeug-Erkennung

use super::*;

impl App {
    /// Fahrzeug-Erkennung: Ergebnis abholen, bei freiem Bus neuen Versuch starten.
    pub(super) fn erkennung_tick(&mut self, ctx: &egui::Context) {
        let be = self.backend();
        let fuer = be.describe();
        if fuer != self.erk_fuer {
            // anderer Adapter gewählt: von vorn
            self.erk_fuer = fuer;
            self.erk_fund = None;
            self.erk_zaehler = None;
            self.erk_zuletzt = None;
        }
        if let Some(rx) = &self.erk_job {
            match rx.try_recv() {
                Ok(m) => {
                    self.erk_job = None;
                    self.erkennung_ergebnis(m);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.erk_job = None,
            }
        }
        if !self.erk_an {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        if self.erk_job.is_some() {
            return;
        }
        // nur wenn seit ein paar Sekunden niemand den Bus benutzt
        if !ecu_werkbank::bus::frei_seit().is_some_and(|d| d >= std::time::Duration::from_secs(5)) {
            return;
        }
        let abstand = if self.erk_fund.is_some() { 10 } else { 4 };
        let seit = self.erk_zuletzt.map(|t| t.elapsed()).unwrap_or(std::time::Duration::MAX);
        if seit < std::time::Duration::from_secs(abstand) {
            return;
        }
        self.erk_zuletzt = Some(std::time::Instant::now());
        let (tx, rx) = mpsc::channel();
        let weg = self.erk_fund.as_ref().map(|f| f.weg);
        let vorher = self.erk_zaehler;
        std::thread::spawn(move || {
            use ecu_werkbank::erkennung as e;
            let m = match weg {
                Some(w) => ErkMsg::NochDa(e::noch_da(&be, w)),
                None if !e::versuch_lohnt(&be, vorher, seit) => ErkMsg::Ausgelassen,
                None => {
                    let f = e::suchen(&be);
                    ErkMsg::Gesucht(f, e::zaehler(&be))
                }
            };
            let _ = tx.send(m);
        });
        self.erk_job = Some(rx);
    }

    pub(super) fn erkennung_ergebnis(&mut self, m: ErkMsg) {
        match m {
            ErkMsg::Ausgelassen => {}
            ErkMsg::Gesucht(None, z) => self.erk_zaehler = z,
            ErkMsg::Gesucht(Some(f), z) => {
                self.erk_zaehler = z;
                let v = f.vin.as_deref().and_then(|vin| ecu_werkbank::vehicle::resolve(Some(vin), None).ok());
                match &v {
                    Some(v) => {
                        if self.active_vehicle.as_deref() != Some(v.key.as_str()) {
                            self.set_active_vehicle(Some(v.key.clone()));
                        }
                        self.status = tr!("🚗 Fahrzeug erkannt: {a}", a = v.label());
                    }
                    None => self.status = tr!("🚗 Auto erkannt (Fahrgestellnummer nicht lesbar)."),
                }
                self.erk_fund = Some(f);
            }
            ErkMsg::NochDa(true) => {}
            ErkMsg::NochDa(false) => {
                self.erk_fund = None;
                self.erk_zaehler = None;
                self.status = tr!("Auto getrennt (oder Zündung aus).");
            }
        }
    }

    /// Anzeige in der Kopfzeile: erkanntes Auto bzw. „kein Auto“. Klick schaltet die Erkennung aus/an.
    pub(super) fn erkennung_anzeige(&mut self, ui: &mut egui::Ui) {
        let (text, farbe) = match (&self.erk_fund, self.erk_an) {
            (_, false) => (tr!("Erkennung aus"), ui.visuals().weak_text_color()),
            (Some(f), true) => {
                let name = f.vin.as_deref().and_then(ecu_werkbank::vehicle::find_by_vin).map(|v| v.label()).unwrap_or_else(|| tr!("Auto"));
                (format!("🚗 {name}"), stufe_color(Stufe::Gut))
            }
            (None, true) => (tr!("kein Auto"), ui.visuals().weak_text_color()),
        };
        let hilfe = if self.erk_an {
            tr!("ECUWerkbank schaut alle paar Sekunden (nur lesend), ob ein Auto antwortet, und wählt es automatisch aus. Klick: Erkennung ausschalten.")
        } else {
            tr!("Automatische Fahrzeug-Erkennung ist aus. Klick: einschalten.")
        };
        // extend: nicht mitten im Text umbrechen, sondern als Ganzes in die nächste Zeile
        if ui.add(egui::Label::new(egui::RichText::new(text).small().color(farbe)).extend().sense(egui::Sense::click())).on_hover_text(hilfe).clicked() {
            self.erk_an = !self.erk_an;
            let f = ecu_werkbank::history::data_dir().join("erkennung-aus");
            if self.erk_an {
                let _ = ecu_werkbank::ablage::entfernen(f);
            } else {
                let _ = ecu_werkbank::ablage::schreiben(f, "");
                self.erk_fund = None;
            }
        }
    }

    pub(super) fn set_active_vehicle(&mut self, key: Option<String>) {
        self.active_vehicle = key;
        self.code_bk = None; // Codier-Helfer lädt die Sicherung des neuen Fahrzeugs
        let f = ecu_werkbank::history::data_dir().join("aktives-fahrzeug");
        let _ = ecu_werkbank::ablage::schreiben(f, self.active_vehicle.clone().unwrap_or_default());
        // Labeldateien passen zu den Steuergeräten des Fahrzeugs
        // anderes Auto, andere Teilenummern: Datenbank-Wissen neu zuordnen
        self.mw_teile.clear();
        self.mw_texte_neu_laden();
        if let Some(k) = &self.active_vehicle {
            self.hist_vehicle = k.clone();
            self.hist_refresh();
        }
    }

    /// Unterordner des aktiven Fahrzeugs (ohne aktives: „ohne-Fahrzeug“).
    pub(super) fn vehicle_sub(&self, sub: &str) -> Option<PathBuf> {
        ecu_werkbank::vehicle::resolve(None, self.active_vehicle.as_deref()).ok()?.sub(sub).ok()
    }

    pub(super) fn scope_pdf(&mut self) {
        let cols: Vec<ecu_werkbank::csvlog::Column> = self
            .scope_series
            .iter()
            .filter(|s| s.visible && !s.points.is_empty())
            .map(|s| ecu_werkbank::csvlog::Column { name: s.name.clone(), unit: s.unit.clone(), points: s.points.iter().copied().collect(), soll: s.soll })
            .collect();
        if cols.is_empty() {
            self.status = tr!("Keine sichtbaren Kurven mit Daten.");
            return;
        }
        let mut meta = vec![(tr!("Datum").to_string(), ecu_werkbank::report::now_local())];
        if let Some(v) = self.active_vehicle.as_deref().and_then(ecu_werkbank::vehicle::load) {
            meta.insert(0, (tr!("Fahrzeug"), v.label()));
            if !v.vin.is_empty() {
                meta.insert(1, ("Fahrgestellnummer".into(), v.vin.clone()));
            }
        }
        meta.push((
            tr!("Quelle"),
            self.scope_file.clone().unwrap_or_else(|| tr!("Live über {a}", a = self.backend().describe())),
        ));
        let Some(dir) = self.vehicle_sub("scope") else {
            self.status = tr!("Kein Ordner für das PDF verfügbar.");
            return;
        };
        let path = dir.join(ecu_werkbank::vehicle::stamped("scope", "pdf"));
        match ecu_werkbank::ablage::schreiben(&path, ecu_werkbank::chart::to_pdf("Messwert-Aufzeichnung", &meta, &cols)) {
            Ok(()) => {
                self.status = tr!("Scope-PDF gespeichert: {a} ({b} Kurve(n))", a = path.display(), b = cols.len());
                self.pdf_last = Some(path);
            }
            Err(e) => self.status = tr!("PDF-Fehler: {e}", e = e),
        }
    }

    /// Automatische Sicherung beim Start und Abholen des Ergebnisses.
    pub(super) fn datensicherung_tick(&mut self, ctx: &egui::Context) {
        if !self.ds_geprueft {
            self.ds_geprueft = true;
            // nicht im Demo-/Foto-Betrieb mit fremdem Datenordner stören
            if self.foto.is_none() {
                let e = ecu_werkbank::datensicherung::laden();
                if e.faellig(ecu_werkbank::report::unix_now()) {
                    self.datensicherung_starten(PathBuf::from(e.ziel.trim()), e.behalten);
                } else if e.ziel_fehlt() && e.faellig_ohne_ziel(ecu_werkbank::report::unix_now()) {
                    self.status = tr!("Datensicherung fällig, aber der Ordner {a} ist nicht erreichbar (USB-Stick einstecken?).", a = e.ziel.trim());
                }
            }
        }
        if let Some(rx) = &self.ds_job {
            match rx.try_recv() {
                Ok(r) => {
                    self.ds_job = None;
                    self.ds = ecu_werkbank::datensicherung::laden();
                    self.status = match r {
                        Ok((p, n)) => tr!("Datensicherung: {a} Dateien gesichert in {b}", a = n, b = p.display()),
                        Err(e) => tr!("Datensicherung fehlgeschlagen: {e}", e = e),
                    };
                }
                Err(mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
                Err(mpsc::TryRecvError::Disconnected) => self.ds_job = None,
            }
        }
    }

    pub(super) fn datensicherung_starten(&mut self, ziel: PathBuf, behalten: usize) {
        if self.ds_job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(ecu_werkbank::datensicherung::sichern(&ziel, behalten).map_err(|e| format!("{e:#}")));
        });
        self.ds_job = Some(rx);
        self.status = tr!("Datensicherung läuft …");
    }

    /// Einstellungen und Knöpfe der Datensicherung.
    pub(super) fn ui_datensicherung(&mut self, ui: &mut egui::Ui) {
        use ecu_werkbank::datensicherung as ds;
        egui::CollapsingHeader::new(tr!("💾 Datensicherung (alle Fahrzeuge)")).id_source("datensicherung").show(ui, |ui| {
            ui.label(tr!("Sichert den ganzen Datenordner (Fahrzeuge, Scans, Sicherungen, eigene Texte) und deine eigenen Labels als ZIP in einen Ordner deiner Wahl — am besten USB-Stick oder Cloud-Ordner. Beim Start sichert ecu-werkbank von selbst, wenn es wieder Zeit ist und der Ordner erreichbar ist."));
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("Ordner:"));
                ui.add(egui::TextEdit::singleline(&mut self.ds.ziel).desired_width(320.0).hint_text(tr!("z. B. /media/…/USB-Stick/ecu-werkbank")));
                if ui.button(tr!("📂 wählen …")).clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_title(tr!("Ordner für die Datensicherung")).pick_folder() {
                        self.ds.ziel = p.display().to_string();
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("alle"));
                ui.add(egui::DragValue::new(&mut self.ds.tage).range(1..=90));
                ui.label(tr!("Tage, die neuesten"));
                ui.add(egui::DragValue::new(&mut self.ds.behalten).range(1..=50));
                ui.label(tr!("Sicherungen behalten"));
            });
            ui.horizontal_wrapped(|ui| {
                let gespeichert = ds::laden();
                let geaendert = gespeichert.ziel.trim() != self.ds.ziel.trim() || gespeichert.tage != self.ds.tage || gespeichert.behalten != self.ds.behalten;
                if ui.add_enabled(geaendert, egui::Button::new(tr!("✔ Einstellung speichern"))).clicked() {
                    let e = ds::Einstellung { ziel: self.ds.ziel.trim().to_string(), zuletzt: gespeichert.zuletzt, ..self.ds.clone() };
                    self.status = match ds::speichern(&e) {
                        Ok(()) if e.ziel.is_empty() => tr!("Automatische Datensicherung ausgeschaltet."),
                        Ok(()) => tr!("Datensicherung eingerichtet: alle {a} Tage nach {b}", a = e.tage, b = e.ziel),
                        Err(e) => format!("{e:#}"),
                    };
                }
                let ziel = PathBuf::from(self.ds.ziel.trim());
                let busy = self.ds_job.is_some();
                if ui.add_enabled(!busy && !self.ds.ziel.trim().is_empty(), egui::Button::new(tr!("💾 Jetzt sichern"))).clicked() {
                    let behalten = self.ds.behalten;
                    self.datensicherung_starten(ziel.clone(), behalten);
                }
                if busy {
                    ui.spinner();
                }
                if ui.button(tr!("↩ Sicherung einspielen …")).on_hover_text(tr!("Holt fehlende Dateien aus einer Sicherung zurück (z. B. nach Neuinstallation). Vorhandene Dateien bleiben unverändert.")).clicked() {
                    let mut dlg = rfd::FileDialog::new().add_filter("ZIP", &["zip"]).set_title(tr!("Datensicherung wählen"));
                    if ziel.is_dir() {
                        dlg = dlg.set_directory(&ziel);
                    }
                    if let Some(z) = dlg.pick_file() {
                        self.status = match ds::zurueckspielen(&z, false) {
                            Ok((ein, ueber)) => {
                                self.reload_labels();
                                tr!("Eingespielt: {a} Datei(en), {b} schon vorhanden (unverändert).", a = ein, b = ueber)
                            }
                            Err(e) => format!("{e:#}"),
                        };
                    }
                }
            });
            let g = ds::laden();
            if g.zuletzt > 0 {
                let tage = ecu_werkbank::report::unix_now().saturating_sub(g.zuletzt) / 86_400;
                ui.weak(match tage {
                    0 => tr!("Letzte Sicherung: heute"),
                    1 => tr!("Letzte Sicherung: gestern"),
                    t => tr!("Letzte Sicherung: vor {a} Tagen", a = t),
                });
            } else if !g.ziel.is_empty() {
                ui.weak(tr!("Noch keine Sicherung gemacht."));
            }
            if g.ziel_fehlt() {
                ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Ordner gerade nicht erreichbar — USB-Stick eingesteckt?"));
            }
        });
    }

    /// Firmenkopf (Werkstatt) mit Logo — steht oben auf jeder Seite aller PDF-Berichte.
    fn ui_firmenkopf(&mut self, ui: &mut egui::Ui) {
        let f = self.firmenkopf.get_or_insert_with(ecu_werkbank::firmenkopf::laden);
        let titel = if f.ist_leer() { tr!("🏢 Firmenkopf für Berichte (Werkstatt)") } else { tr!("🏢 Firmenkopf: {a}", a = f.name) };
        let mut speichern = false;
        let mut meldung = None;
        egui::CollapsingHeader::new(titel).id_source("firmenkopf").show(ui, |ui| {
            ui.weak(tr!("Steht oben auf jeder Seite aller PDF-Berichte — auf Seite 1 rechts im Titelbalken, mit Logo. Alles leer lassen = kein Firmenkopf."));
            egui::Grid::new("firmenkopf_grid").num_columns(2).show(ui, |ui| {
                for (name, feld) in [(trn!("Name"), &mut f.name), (trn!("Anschrift"), &mut f.adresse), (trn!("Telefon"), &mut f.telefon), (trn!("E-Mail"), &mut f.email), (trn!("Zusatz"), &mut f.zusatz)] {
                    ui.label(ecu_werkbank::i18n::t(name));
                    ui.add(egui::TextEdit::singleline(feld).desired_width(320.0));
                    ui.end_row();
                }
                ui.label(tr!("Logo"));
                ui.horizontal_wrapped(|ui| {
                    match f.logo_bild() {
                        Some(b) => {
                            ui.label(tr!("{a} × {b} Pixel", a = b.breite, b = b.hoehe));
                        }
                        None if !f.logo.is_empty() => {
                            ui.colored_label(stufe_color(Stufe::Warnung), tr!("Logo nicht lesbar"));
                        }
                        None => {
                            ui.weak(tr!("kein Logo"));
                        }
                    }
                    if ui.button(tr!("🖼 Logo wählen…")).on_hover_text(tr!("PNG oder JPG, am besten quer und mit wenig Rand, unter 300 KB")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter(tr!("Bild"), &["png", "jpg", "jpeg"]).pick_file() {
                            meldung = Some(match std::fs::read(&p).map_err(anyhow::Error::from).and_then(|roh| f.logo_setzen(&roh)) {
                                Ok(b) => tr!("Logo übernommen ({a} × {b} Pixel) — mit „Speichern“ festhalten.", a = b.breite, b = b.hoehe),
                                Err(e) => tr!("Logo: {e}", e = format!("{e:#}")),
                            });
                        }
                    }
                    if !f.logo.is_empty() && ui.button(tr!("✖ Logo entfernen")).clicked() {
                        f.logo.clear();
                    }
                });
                ui.end_row();
            });
            let z = f.zeilen();
            if !z.is_empty() {
                ui.weak(tr!("Vorschau: {a}", a = z.join("  ·  ")));
            }
            speichern = ui.button(tr!("💾 Speichern")).clicked();
        });
        if let Some(m) = meldung {
            self.status = m;
        }
        if speichern {
            let f = self.firmenkopf.clone().unwrap_or_default();
            self.status = match ecu_werkbank::firmenkopf::speichern(&f) {
                Ok(()) if f.ist_leer() => tr!("Firmenkopf entfernt."),
                Ok(()) => tr!("Firmenkopf gespeichert — erscheint oben in allen neuen Berichten."),
                Err(e) => tr!("Firmenkopf: {e}", e = format!("{e:#}")),
            };
        }
    }

    pub(super) fn ui_fahrzeuge(&mut self, ui: &mut egui::Ui) {
        use ecu_werkbank::vehicle;
        ui.heading(tr!("Fahrzeuge"));
        ui.label(tr!("Je Fahrzeug ein Ordner (nach Fahrgestellnummer) mit allen Berichten, Scans, Sicherungen und Aufzeichnungen. Das Kennzeichen dient zum leichteren Finden."));
        self.ui_datensicherung(ui);
        self.ui_firmenkopf(ui);
        ui.horizontal_wrapped(|ui| {
            ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(&mut self.veh_new_plate).hint_text(tr!("Kennzeichen")));
            ui.add_sized([170.0, 20.0], egui::TextEdit::singleline(&mut self.veh_new_vin).hint_text(tr!("Fahrgestellnr. (optional)")));
            ui.add_sized([180.0, 20.0], egui::TextEdit::singleline(&mut self.veh_new_name).hint_text(tr!("Bezeichnung, z. B. Golf 6")));
            if ui.button(tr!("+ Fahrzeug anlegen")).clicked() {
                match vehicle::create(&self.veh_new_plate, &self.veh_new_vin, &self.veh_new_name) {
                    Ok(v) => {
                        self.status = tr!("Angelegt: {a} ({b})", a = v.label(), b = v.dir().display());
                        self.veh_new_plate.clear();
                        self.veh_new_vin.clear();
                        self.veh_new_name.clear();
                        self.veh_sel = Some(v.key.clone());
                        self.veh_edit = v.clone();
                        self.set_active_vehicle(Some(v.key));
                    }
                    Err(e) => self.status = format!("{e:#}"),
                }
            }
        });
        ui.separator();
        let list = vehicle::list();
        if list.is_empty() {
            ui.weak(tr!("Noch keine Fahrzeuge. Oben anlegen — oder einfach scannen: mit lesbarer Fahrgestellnummer entsteht das Fahrzeug automatisch."));
            return;
        }
        // Breites Fenster: Liste links, Details rechts. Schmal: untereinander.
        if ui.available_width() >= 720.0 {
            let h = ui.available_height();
            let lw = (ui.available_width() * 0.28).clamp(170.0, 280.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(lw, h), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                    ui.set_width(lw);
                    egui::ScrollArea::vertical().id_source("vehlist").show(ui, |ui| self.fz_liste(ui, &list));
                });
                ui.separator();
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical().id_source("vehdetail").show(ui, |ui| self.fz_details(ui, &list));
                });
            });
        } else {
            egui::ScrollArea::vertical().id_source("vehlist").max_height(110.0).show(ui, |ui| self.fz_liste(ui, &list));
            ui.separator();
            self.fz_details(ui, &list);
        }
    }

    /// Fahrzeugliste (links bzw. oben).
    pub(super) fn fz_liste(&mut self, ui: &mut egui::Ui, list: &[ecu_werkbank::vehicle::Vehicle]) {
        for v in list {
            let sel = self.veh_sel.as_deref() == Some(v.key.as_str());
            let active = self.active_vehicle.as_deref() == Some(v.key.as_str());
            let text = format!("{}{}", if active { "▶ " } else { "" }, v.label());
            if ui.selectable_label(sel, text).clicked() {
                self.veh_sel = Some(v.key.clone());
                self.veh_edit = v.clone();
            }
        }
    }

    /// Daten, km-Stand, Wartung und Teile des gewählten Fahrzeugs.
    /// Tankbuch: Liste, Verbrauch und Eingabezeile.
    pub(super) fn fz_tankbuch(&mut self, ui: &mut egui::Ui, key: &str) {
        use ecu_werkbank::kosten::{euro, komma, zahl_lesen};
        use ecu_werkbank::vehicle;
        let Some(v) = vehicle::load(key) else { return };
        ui.add_space(6.0);
        ui.strong(tr!("Tankbuch"));
        let vb = ecu_werkbank::kosten::verbrauch(&v.tankbuch);
        if v.tankbuch.is_empty() {
            ui.weak(tr!("Noch keine Tankfüllung. Mit „voll“ getankt lässt sich der Verbrauch berechnen (ab der zweiten vollen Füllung)."));
        } else {
            let mut zeile = Vec::new();
            if let Some(s) = vb.schnitt {
                zeile.push(tr!("Verbrauch im Schnitt: {a} l/100 km", a = komma(s, 1)));
            }
            if let Some(a) = vb.abschnitte.last() {
                zeile.push(tr!("zuletzt {a} l/100 km", a = komma(a.l_100km, 1)));
            }
            if let Some(p) = vb.euro_pro_liter {
                zeile.push(tr!("im Schnitt {a} €/l", a = komma(p, 3)));
            }
            if !zeile.is_empty() {
                ui.label(zeile.join("  ·  "));
            }
            let mut del: Option<usize> = None;
            egui::CollapsingHeader::new(tr!("{a} Tankfüllungen", a = v.tankbuch.len())).id_source(("tankliste", key)).show(ui, |ui| {
                egui::Grid::new("tankbuch").striped(true).spacing([8.0, 2.0]).show(ui, |ui| {
                    for h in [tr!("Datum"), tr!("km"), tr!("Liter"), tr!("Preis"), tr!("€/l"), tr!("l/100 km"), tr!("Notiz")] {
                        ui.weak(h);
                    }
                    ui.end_row();
                    for (i, t) in v.tankbuch.iter().enumerate() {
                        ui.label(&t.datum);
                        ui.label(ecu_werkbank::report::group_thousands(t.km));
                        ui.label(if t.voll { komma(t.liter, 2) } else { tr!("{a} (teil)", a = komma(t.liter, 2)) });
                        ui.label(t.preis.map(euro).unwrap_or_default());
                        ui.label(t.preis.filter(|_| t.liter > 0.0).map(|p| komma(p / t.liter, 3)).unwrap_or_default());
                        ui.label(vb.abschnitte.iter().find(|a| a.km == t.km).map(|a| komma(a.l_100km, 1)).unwrap_or_default());
                        ui.label(&t.notiz);
                        if ui.small_button("🗑").on_hover_text(tr!("Eintrag löschen")).clicked() {
                            del = Some(i);
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some(i) = del {
                let mut v = v.clone();
                v.tankbuch.remove(i);
                if vehicle::save(&v).is_ok() {
                    self.status = tr!("Tankfüllung gelöscht.");
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(&mut self.tk_km).hint_text(v.km.map(|k| k.to_string()).unwrap_or_else(|| tr!("km"))))
                .on_hover_text(tr!("km-Stand beim Tanken (Pflicht)"));
            ui.add_sized([60.0, 20.0], egui::TextEdit::singleline(&mut self.tk_liter).hint_text(tr!("Liter")));
            ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut self.tk_preis).hint_text(tr!("Preis €"))).on_hover_text(tr!("Bezahlt gesamt, optional"));
            ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(&mut self.tk_datum).hint_text(vehicle::today())).on_hover_text(tr!("TT.MM.JJJJ, leer = heute"));
            ui.checkbox(&mut self.tk_voll, tr!("voll")).on_hover_text(tr!("Bis zum Abschalten der Zapfpistole getankt — nur dann lässt sich der Verbrauch berechnen"));
            ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(&mut self.tk_notiz).hint_text(tr!("Notiz")));
            let km = self.tk_km.trim().replace('.', "").parse::<u32>().ok();
            let liter = zahl_lesen(&self.tk_liter).filter(|&l| l > 0.0);
            if ui.add_enabled(km.is_some() && liter.is_some(), egui::Button::new(tr!("+ tanken"))).on_hover_text(tr!("km-Stand und Liter angeben")).clicked() {
                let preis_text = self.tk_preis.trim().to_string();
                let preis = zahl_lesen(&preis_text);
                let datum = self.tk_datum.trim().to_string();
                if !preis_text.is_empty() && preis.is_none() {
                    self.status = tr!("Preis bitte als Zahl eingeben, z. B. 45,90");
                } else if !datum.is_empty() && vehicle::days_of(&datum).is_none() {
                    self.status = tr!("Datum bitte als TT.MM.JJJJ eingeben.");
                } else if let (Some(km), Some(liter)) = (km, liter) {
                    let mut v = v.clone();
                    v.tanken_eintragen(vehicle::Tankung { datum, km, liter, preis, voll: self.tk_voll, notiz: self.tk_notiz.clone() });
                    match vehicle::save(&v) {
                        Ok(()) => {
                            let vb = ecu_werkbank::kosten::verbrauch(&v.tankbuch);
                            self.status = match vb.abschnitte.iter().find(|a| a.km == km) {
                                Some(a) => tr!("Getankt: {a} l — {b} l/100 km seit der letzten vollen Füllung.", a = komma(liter, 2), b = komma(a.l_100km, 1)),
                                None => tr!("Getankt: {a} l", a = komma(liter, 2)),
                            };
                            self.veh_edit.km = v.km;
                            self.veh_edit.km_datum = v.km_datum.clone();
                            for f in [&mut self.tk_km, &mut self.tk_liter, &mut self.tk_preis, &mut self.tk_datum, &mut self.tk_notiz] {
                                f.clear();
                            }
                            self.tk_voll = true;
                        }
                        Err(e) => self.status = format!("{e:#}"),
                    }
                }
            }
        });
    }

    /// Kostenübersicht aus Teilen (mit Preis) und Tankbuch.
    pub(super) fn fz_kosten(&mut self, ui: &mut egui::Ui, key: &str) {
        use ecu_werkbank::kosten::{euro, komma};
        let Some(v) = ecu_werkbank::vehicle::load(key) else { return };
        let k = ecu_werkbank::kosten::kosten(&v, &ecu_werkbank::vehicle::today());
        ui.add_space(6.0);
        ui.strong(tr!("Kosten"));
        if k.gesamt <= 0.0 {
            ui.weak(tr!("Noch keine Preise eingetragen — bei Teilen und Tankfüllungen den Preis angeben."));
            return;
        }
        ui.label(tr!("Seit {a}: {b} gesamt (Teile {c}, Tanken {d})", a = &k.von, b = euro(k.gesamt), c = euro(k.teile), d = euro(k.tanken)));
        let mut z = Vec::new();
        if let Some(m) = k.je_monat {
            z.push(tr!("{a} pro Monat", a = euro(m)));
        }
        if let Some(t) = k.je_1000km {
            z.push(tr!("{a} pro 1000 km", a = euro(t)));
        }
        if let Some(s) = k.strecke_km {
            z.push(tr!("{a} km gefahren", a = ecu_werkbank::report::group_thousands(s)));
        }
        if !z.is_empty() {
            ui.label(z.join("  ·  "));
        }
        if k.jahre.len() > 1 || k.jahre.values().any(|j| j.liter > 0.0) {
            egui::Grid::new("kostenjahre").striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
                for h in [tr!("Jahr"), tr!("Teile"), tr!("Tanken"), tr!("Liter"), tr!("gesamt")] {
                    ui.weak(h);
                }
                ui.end_row();
                for (j, w) in k.jahre.iter().rev() {
                    ui.label(j.to_string());
                    ui.label(euro(w.teile));
                    ui.label(euro(w.tanken));
                    ui.label(komma(w.liter, 0));
                    ui.strong(euro(w.teile + w.tanken));
                    ui.end_row();
                }
            });
        }
    }

    /// Wartungstermine als .ics in den Fahrzeugordner schreiben und öffnen.
    pub(super) fn kalender_export(&mut self, key: &str) {
        let Some(v) = ecu_werkbank::vehicle::load(key) else { return };
        let t = ecu_werkbank::kalender::termine(&v, &ecu_werkbank::vehicle::today());
        if t.is_empty() {
            self.status = tr!("Keine Termine: Beim Wartungsplan erst die letzte Durchführung eintragen.");
            return;
        }
        let pfad = v.sub("berichte").unwrap_or_else(|_| v.dir()).join("wartung.ics");
        match ecu_werkbank::ablage::schreiben(&pfad, ecu_werkbank::kalender::ics(&v, &t)) {
            Ok(()) => {
                self.status = tr!("{a} Termine gespeichert: {b} — Datei aufs Handy schicken oder im Kalender importieren.", a = t.len(), b = pfad.display());
                if let Some(ordner) = pfad.parent() {
                    open_path(ordner, &mut self.status);
                }
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    pub(super) fn fz_details(&mut self, ui: &mut egui::Ui, list: &[ecu_werkbank::vehicle::Vehicle]) {
        use ecu_werkbank::vehicle;
        let Some(key) = self.veh_sel.clone().filter(|k| list.iter().any(|v| v.key == *k)) else {
            ui.weak(tr!("Fahrzeug in der Liste wählen."));
            return;
        };
        if self.veh_edit.key != key {
            self.veh_edit = vehicle::load(&key).unwrap_or_default();
        }
        egui::Grid::new("vehedit").spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label(tr!("Kennzeichen:"));
            ui.add_sized([160.0, 20.0], egui::TextEdit::singleline(&mut self.veh_edit.kennzeichen));
            ui.end_row();
            ui.label(tr!("Bezeichnung:"));
            ui.add_sized([160.0, 20.0], egui::TextEdit::singleline(&mut self.veh_edit.bezeichnung));
            ui.end_row();
            ui.label("Fahrgestellnr.:");
            ui.add_sized([160.0, 20.0], egui::TextEdit::singleline(&mut self.veh_edit.vin));
            ui.end_row();
            ui.label(tr!("Hubraum:"));
            let mut h = self.veh_edit.hubraum_l.unwrap_or(0.0);
            if ui.add(egui::DragValue::new(&mut h).speed(0.1).range(0.0..=8.0).suffix(" l")).on_hover_text(tr!("für die Luftmassen-Plausibilität im Gemisch-Assistenten (0 = unbekannt)")).changed() {
                self.veh_edit.hubraum_l = (h > 0.0).then_some((h * 10.0).round() / 10.0);
            }
            ui.end_row();
            ui.label(tr!("Notiz:"));
            ui.add_sized([160.0, 50.0], egui::TextEdit::multiline(&mut self.veh_edit.notiz));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button(tr!("💾 Speichern")).clicked() {
                let was_active = self.active_vehicle.as_deref() == Some(key.as_str());
                // Wartungsplan und Teile werden direkt gespeichert — den
                // aktuellen Stand nehmen, nicht die Kopie aus dem Formular.
                let mut neu = self.veh_edit.clone();
                if let Some(cur) = vehicle::load(&key) {
                    neu.wartung = cur.wartung;
                    neu.teile = cur.teile;
                    neu.tankbuch = cur.tankbuch;
                    neu.service = cur.service;
                    neu.dpf = cur.dpf;
                }
                match vehicle::update(neu) {
                    Ok(v) => {
                        self.status = tr!("Gespeichert: {a} ({b})", a = v.label(), b = v.dir().display());
                        self.veh_sel = Some(v.key.clone());
                        if was_active {
                            self.set_active_vehicle(Some(v.key.clone()));
                        }
                        self.veh_edit = v;
                    }
                    Err(e) => self.status = format!("{e:#}"),
                }
            }
            if ui.button(tr!("▶ Als aktiv wählen")).clicked() {
                self.set_active_vehicle(Some(key.clone()));
            }
            if ui.button(tr!("📂 Ordner öffnen")).clicked() {
                open_path(&vehicle::dir(&key), &mut self.status);
            }
        });
        ui.horizontal(|ui| {
            if ui.button(tr!("📄 Fahrzeugakte (PDF)")).on_hover_text(tr!("Verlauf, Wartung, Reparaturen und Codierung als ein PDF")).clicked() {
                if let Some(v) = vehicle::load(&key) {
                    let res = v.sub("berichte").and_then(|d| {
                        let p = d.join(vehicle::stamped("fahrzeugakte", "pdf"));
                        ecu_werkbank::ablage::schreiben(&p, ecu_werkbank::akte::to_pdf(&v, &self.db))?;
                        Ok(p)
                    });
                    match res {
                        Ok(p) => {
                            self.status = tr!("Fahrzeugakte gespeichert: {a}", a = p.display());
                            self.pdf_last = Some(p);
                        }
                        Err(e) => self.status = format!("{e:#}"),
                    }
                }
            }
            if ui.button(tr!("📦 Als ZIP sichern…")).on_hover_text(tr!("Ganzen Fahrzeugordner in eine ZIP-Datei, z. B. auf USB-Stick")).clicked() {
                if let Some(v) = vehicle::load(&key) {
                    let name = tr!("ecu-werkbank-{a}-{b}.zip", a = vehicle::plate_key(&v.kennzeichen).if_empty(&v.key), b = vehicle::today().replace('.', "-"));
                    if let Some(out) = rfd::FileDialog::new().set_file_name(&name).add_filter("ZIP", &["zip"]).save_file() {
                        self.status = match ecu_werkbank::ablage::ordner_zippen(&v.dir(), &out, &v.key) {
                            Ok(n) => tr!("{n} Datei(en) gesichert: {a}", n = n, a = out.display()),
                            Err(e) => tr!("ZIP: {e}", e = format!("{:#}", e)),
                        };
                    }
                }
            }
        });

        // Kilometerstand
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let cur = self.veh_edit.km.map(|k| tr!("{a} km ({b})", a = ecu_werkbank::report::group_thousands(k), b = self.veh_edit.km_datum)).unwrap_or_else(|| "unbekannt".into());
            ui.label(format!("km-Stand: {cur}"));
            ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(&mut self.veh_km).hint_text(tr!("neu")));
            if ui.small_button(tr!("übernehmen")).clicked() {
                match self.veh_km.trim().replace('.', "").parse::<u32>() {
                    Ok(k) => {
                        if let Some(mut v) = vehicle::load(&key) {
                            v.km = Some(k);
                            v.km_datum = vehicle::today();
                            if vehicle::save(&v).is_ok() {
                                self.veh_edit.km = v.km;
                                self.veh_edit.km_datum = v.km_datum.clone();
                                self.veh_km.clear();
                                self.status = tr!("km-Stand {k} gespeichert.", k = k);
                            }
                        }
                    }
                    Err(_) => self.status = tr!("km-Stand: bitte eine Zahl eingeben."),
                }
            }
            let tacho = self.veh_km.trim().replace('.', "").parse::<u32>().ok();
            let knopf = if tacho.is_some() { tr!("🔍 Stelle im Kombi suchen") } else { tr!("⬇ Genau aus dem Kombi lesen") };
            let hilfe = tr!("Ohne Eingabe: genauen Stand lesen (wenn die Stelle bekannt ist). Mit Tachostand im Feld: einmal die Stelle im Kombi suchen und merken (nur lesen, bis 1 Minute).");
            if ui.add_enabled(self.km_job.is_none(), egui::Button::new(knopf)).on_hover_text(hilfe).clicked() {
                let be = self.backend();
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(ecu_werkbank::km::ermitteln(&be, tacho, |_, _| {}).map_err(|e| format!("{e:#}")));
                });
                self.km_job = Some(rx);
                self.km_lines = vec![if tacho.is_some() { tr!("Suche im Kombi — nur lesen, dauert bis zu einer Minute…") } else { tr!("Lese Kombi …") }];
            }
            if self.km_job.is_some() {
                ui.spinner();
            }
        });
        if let Some(rx) = &self.km_job {
            match rx.try_recv() {
                Ok(Ok(e)) => {
                    self.km_job = None;
                    self.km_lines = e.zeilen.clone();
                    if let (Some(k), Some(mut v)) = (e.km, vehicle::load(&key).filter(|v| e.passt_zu(v))) {
                        v.km = Some(k);
                        v.km_datum = vehicle::today();
                        if vehicle::save(&v).is_ok() {
                            self.veh_edit.km = v.km;
                            self.veh_edit.km_datum = v.km_datum.clone();
                            self.veh_km.clear();
                            self.status = tr!("km-Stand {k} gespeichert.", k = k);
                        }
                    }
                }
                Ok(Err(e)) => {
                    self.km_job = None;
                    self.km_lines = vec![e];
                }
                Err(mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(200)),
                Err(mpsc::TryRecvError::Disconnected) => self.km_job = None,
            }
        }
        for l in &self.km_lines {
            ui.monospace(l);
        }

        // Wartung
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.strong(tr!("Wartung"));
            if ui
                .small_button(tr!("📅 Termine in den Kalender"))
                .on_hover_text(tr!("Fällige Termine als Kalenderdatei (.ics) — öffnen oder aufs Handy schicken und in den Kalender übernehmen. Erinnerung jeweils eine Woche vorher."))
                .clicked()
            {
                self.kalender_export(&key);
            }
        });
        let heute = vehicle::today();
        let mut done: Option<usize> = None;
        let mut del: Option<usize> = None;
        if let Some(v) = vehicle::load(&key) {
            if v.wartung.is_empty() {
                ui.weak(tr!("Noch kein Wartungsplan."));
            }
            egui::Grid::new("wartung").striped(true).spacing([8.0, 2.0]).show(ui, |ui| {
                for (i, w) in v.wartung.iter().enumerate() {
                    let f = w.faellig(v.km, &heute);
                    dot(ui, stufe_color(f.stufe));
                    ui.label(&w.name).on_hover_text(tr!("Intervall: {a}{b}\nzuletzt: {c}{d}", a = w.km_intervall.map(|k| format!("{} km ", ecu_werkbank::report::group_thousands(k))).unwrap_or_default(), b = w.monate.map(|m| tr!("{m} Monate", m = m)).unwrap_or_default(), c = if w.letztes_datum.is_empty() { "—".to_string() } else { w.letztes_datum.clone() }, d = w.letzte_km.map(|k| tr!(" bei {a} km", a = ecu_werkbank::report::group_thousands(k))).unwrap_or_default()));
                    ui.colored_label(stufe_color(f.stufe), &f.text);
                    if ui.small_button(tr!("✔ heute erledigt")).clicked() {
                        done = Some(i);
                    }
                    if ui.small_button("🗑").clicked() {
                        del = Some(i);
                    }
                    ui.end_row();
                }
            });
            if done.is_some() || del.is_some() {
                let mut v = v.clone();
                if let Some(i) = done {
                    let name = v.wartung[i].name.clone();
                    v.wartung_erledigt(&name, None);
                    self.status = tr!("{name}: heute erledigt{a}.", name = name, a = v.km.map(|k| tr!(" bei {k} km", k = k)).unwrap_or_default());
                }
                if let Some(i) = del {
                    v.wartung.remove(i);
                }
                let _ = vehicle::save(&v);
            }
            ui.horizontal(|ui| {
                if ui.small_button(tr!("+ Vorlage (Öl, Filter, Kerzen …)")).clicked() {
                    let mut v = v.clone();
                    for w in vehicle::wartung_vorlage() {
                        if !v.wartung.iter().any(|x| x.name == w.name) {
                            v.wartung.push(w);
                        }
                    }
                    let _ = vehicle::save(&v);
                }
            });
            ui.horizontal(|ui| {
                ui.add_sized([130.0, 20.0], egui::TextEdit::singleline(&mut self.wt_new_name).hint_text(tr!("eigener Posten")));
                ui.add_sized([60.0, 20.0], egui::TextEdit::singleline(&mut self.wt_new_km).hint_text("km"));
                ui.add_sized([50.0, 20.0], egui::TextEdit::singleline(&mut self.wt_new_mon).hint_text(tr!("Monate")));
                if ui.small_button("+").clicked() && !self.wt_new_name.trim().is_empty() {
                    let mut v = v.clone();
                    v.wartung.push(vehicle::Wartung {
                        name: self.wt_new_name.trim().to_string(),
                        km_intervall: self.wt_new_km.trim().replace('.', "").parse().ok(),
                        monate: self.wt_new_mon.trim().parse().ok(),
                        ..Default::default()
                    });
                    let _ = vehicle::save(&v);
                    self.wt_new_name.clear();
                    self.wt_new_km.clear();
                    self.wt_new_mon.clear();
                }
            });
        }

        // Gewechselte Teile
        ui.add_space(6.0);
        ui.strong(tr!("Gewechselte Teile"));
        if let Some(v) = vehicle::load(&key) {
            let mut del: Option<usize> = None;
            if v.teile.is_empty() {
                ui.weak(tr!("Noch nichts eingetragen."));
            } else {
                egui::Grid::new("teile").striped(true).spacing([8.0, 2.0]).show(ui, |ui| {
                    for h in [tr!("Datum"), tr!("km"), tr!("Teil"), tr!("Teilenummer"), tr!("Preis"), tr!("Notiz")] {
                        ui.weak(h);
                    }
                    ui.end_row();
                    for (i, t) in v.teile.iter().enumerate() {
                        ui.label(&t.datum);
                        ui.label(t.km.map(ecu_werkbank::report::group_thousands).unwrap_or_else(|| "–".into()));
                        ui.strong(&t.name);
                        ui.monospace(&t.teilenummer);
                        ui.label(t.preis.map(ecu_werkbank::kosten::euro).unwrap_or_default());
                        ui.label(&t.notiz);
                        if ui.small_button("🗑").on_hover_text(tr!("Eintrag löschen")).clicked() {
                            del = Some(i);
                        }
                        ui.end_row();
                    }
                });
            }
            if let Some(i) = del {
                let mut v = v.clone();
                let t = v.teile.remove(i);
                if vehicle::save(&v).is_ok() {
                    self.status = tr!("Eintrag gelöscht: {a}", a = t.name);
                }
            }
            // Eingabe
            let km_hint = v.km.map(|k| k.to_string()).unwrap_or_else(|| tr!("km"));
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_source("teil_vorschlag")
                    .width(18.0)
                    .selected_text("")
                    .show_ui(ui, |ui| {
                        for w in &v.wartung {
                            if ui.selectable_label(false, &w.name).clicked() {
                                self.teil_name = w.name.clone();
                            }
                        }
                    })
                    .response
                    .on_hover_text(tr!("Posten aus dem Wartungsplan übernehmen"));
                ui.add_sized([170.0, 20.0], egui::TextEdit::singleline(&mut self.teil_name).hint_text(tr!("Teil, z. B. Bremsbeläge vorn")));
                ui.add_sized([110.0, 20.0], egui::TextEdit::singleline(&mut self.teil_nr).hint_text(tr!("Teilenummer")));
                ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut self.teil_km).hint_text(km_hint))
                    .on_hover_text(tr!("leer = aktueller km-Stand"));
                ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(&mut self.teil_datum).hint_text(vehicle::today()))
                    .on_hover_text(tr!("TT.MM.JJJJ, leer = heute"));
                ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut self.teil_preis).hint_text(tr!("Preis €")))
                    .on_hover_text(tr!("Kosten in Euro (Teil + Arbeit), optional"));
                ui.add_sized([150.0, 20.0], egui::TextEdit::singleline(&mut self.teil_notiz).hint_text(tr!("Notiz, z. B. Marke, Werkstatt")));
                let km_text = self.teil_km.trim().replace('.', "");
                let preis_text = self.teil_preis.trim().to_string();
                let preis = ecu_werkbank::kosten::zahl_lesen(&preis_text);
                let km = if km_text.is_empty() { Ok(v.km) } else { km_text.parse::<u32>().map(Some) };
                let datum = self.teil_datum.trim().to_string();
                let datum_ok = datum.is_empty() || vehicle::days_of(&datum).is_some();
                let klick = ui
                    .add_enabled(!self.teil_name.trim().is_empty(), egui::Button::new(tr!("+ eintragen")))
                    .on_hover_text(tr!("Heißt das Teil wie ein Wartungsposten, wird der Posten mit eingetragen."))
                    .clicked();
                if klick {
                    match (km, datum_ok) {
                        (Err(_), _) => self.status = tr!("km-Stand: bitte eine Zahl eingeben."),
                        (_, false) => self.status = tr!("Datum bitte als TT.MM.JJJJ eingeben."),
                        _ if !preis_text.is_empty() && preis.is_none() => self.status = tr!("Preis bitte als Zahl eingeben, z. B. 45,90"),
                        (Ok(km), true) => {
                            let mut v = v.clone();
                            let posten = v.teil_eintragen(vehicle::Teil {
                                datum,
                                km,
                                name: self.teil_name.clone(),
                                teilenummer: self.teil_nr.clone(),
                                notiz: self.teil_notiz.clone(),
                                preis,
                            });
                            match vehicle::save(&v) {
                                Ok(()) => {
                                    self.status = match posten {
                                        Some(p) => tr!("Eingetragen: {a} — Wartungsposten „{p}“ mit aktualisiert.", a = self.teil_name.trim(), p = p),
                                        None => tr!("Eingetragen: {a}", a = self.teil_name.trim()),
                                    };
                                    self.veh_edit.km = v.km;
                                    self.veh_edit.km_datum = v.km_datum.clone();
                                    for f in [&mut self.teil_name, &mut self.teil_nr, &mut self.teil_km, &mut self.teil_datum, &mut self.teil_notiz, &mut self.teil_preis] {
                                        f.clear();
                                    }
                                }
                                Err(e) => self.status = format!("{e:#}"),
                            }
                        }
                    }
                }
            });
        }

        self.fz_tankbuch(ui, &key);
        self.fz_kosten(ui, &key);

        ui.add_space(6.0);
        let v = vehicle::load(&key).unwrap_or_default();
        ui.weak(v.dir().display().to_string());
        for (sub, n) in vehicle::stats(&v) {
            ui.horizontal(|ui| {
                ui.label(format!("{sub}: {n}"));
                if n > 0 && ui.small_button(tr!("öffnen")).clicked() {
                    open_path(&v.dir().join(sub), &mut self.status);
                }
            });
        }
        // Neueste Berichte direkt öffnen
        let mut pdfs: Vec<PathBuf> = ["berichte", "scope"]
            .iter()
            .flat_map(|s| std::fs::read_dir(v.dir().join(s)).into_iter().flatten().flatten().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("pdf"))
            .collect();
        pdfs.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        if !pdfs.is_empty() {
            ui.add_space(4.0);
            ui.strong(tr!("Neueste PDFs"));
            for p in pdfs.iter().rev().take(6) {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if ui.link(format!("📄 {name}")).clicked() {
                    open_path(p, &mut self.status);
                }
            }
        }
    }
}
