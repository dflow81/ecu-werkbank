//! GUI: Werkzeuge-Reiter (Software, Nachrüstung, DPF, Fehlerbilder, Service-Anzeige, Testlauf, Sicherheit, Ident, Codier-/Grund-Hilfe)

use super::*;

impl App {
    pub(super) fn ui_software(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Softwarestände"));
        ui.label(tr!("Teilenummer, Software- und Hardwarestand jedes Steuergeräts. Der Vergleich mit dem letzten Mal zeigt getauschte oder neu programmierte Steuergeräte."));
        // Immer nur der Stand des gewählten Fahrzeugs: bei einem Wechsel neu laden
        let akt = self.active_vehicle.clone().unwrap_or_default();
        if self.sw_last.as_ref().map(|(k, _)| k) != Some(&akt) {
            let mut l = if akt.is_empty() { Vec::new() } else { ecu_werkbank::swinfo::list(&akt) };
            // Unterschiede zum vorigen gespeicherten Stand desselben Fahrzeugs
            self.sw_diff = match l.len() {
                n if n >= 2 => ecu_werkbank::swinfo::compare(&l[n - 2], &l[n - 1]),
                _ => Vec::new(),
            };
            self.sw_last = Some((akt.clone(), l.pop()));
            self.sw_fahrzeug = ecu_werkbank::vehicle::load(&akt).map(|v| v.label());
        }
        let fahrzeug = self.sw_fahrzeug.clone();
        match &fahrzeug {
            Some(n) => ui.strong(tr!("Fahrzeug: {a}", a = n)),
            None => ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Kein Fahrzeug gewählt — oben ein Fahrzeug wählen. Gezeigt wird immer nur der Stand dieses Fahrzeugs.")),
        };
        if ui.button(tr!("⬇ Auslesen und speichern")).clicked() {
            let be = self.backend();
            let dir = self.labels_dir.clone();
            self.im_hintergrund(tr!("Softwarestände auslesen"), move || {
                let db = LabelDb::load_dir(&dir).unwrap_or_default();
                let r = ecu_werkbank::swinfo::read(&be, &db, true, |_| {});
                Box::new(move |app: &mut App| match r {
                    Ok(st) => match ecu_werkbank::vehicle::resolve(st.vin.as_deref(), app.active_vehicle.as_deref()) {
                        Ok(v) => {
                            let _ = ecu_werkbank::swinfo::save(&v.key, &st);
                            // neu laden lassen: zeigt den Stand des gewählten Fahrzeugs samt Vergleich
                            app.sw_last = None;
                            app.status = if app.active_vehicle.as_deref() == Some(v.key.as_str()) {
                                tr!("{a} Steuergerät(e) ausgelesen.", a = st.module.len())
                            } else {
                                tr!("Ausgelesen und bei {a} gespeichert (Fahrgestellnummer). Zum Ansehen oben dieses Fahrzeug wählen.", a = v.label())
                            };
                        }
                        Err(e) => app.status = tr!("Softwarestände: {e}", e = format!("{:#}", e)),
                    },
                    Err(e) => app.status = tr!("Softwarestände: {e}", e = format!("{:#}", e)),
                })
            });
        }
        let Some((_, Some(st))) = &self.sw_last else {
            if fahrzeug.is_some() {
                ui.weak(tr!("Für dieses Fahrzeug ist noch kein Softwarestand gespeichert."));
            }
            return;
        };
        ui.weak(tr!("Stand vom {a}", a = st.created));
        if !self.sw_diff.is_empty() {
            ui.strong(tr!("Geändert seit dem vorigen Stand:"));
        }
        for (m, t) in &self.sw_diff {
            ui.horizontal_wrapped(|ui| {
                dot(ui, stufe_color(Stufe::Hinweis));
                ui.label(format!("{m}: {t}"));
            });
        }
        egui::ScrollArea::both().id_source("swtab").show(ui, |ui| {
            egui::Grid::new("sw").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                for h in [trn!("Adresse"), trn!("System"), trn!("Teilenummer"), trn!("Software"), trn!("HW-Nr."), trn!("HW"), trn!("Seriennr.")] {
                    ui.strong(ecu_werkbank::i18n::t(h));
                }
                ui.end_row();
                for m in &st.module {
                    ui.label(&m.adresse);
                    ui.label(if m.system.is_empty() { &m.name } else { &m.system });
                    ui.monospace(&m.teilenummer);
                    ui.monospace(&m.software);
                    ui.monospace(&m.hardware_nr);
                    ui.monospace(&m.hardware_ver);
                    ui.monospace(&m.seriennummer);
                    ui.end_row();
                }
            });
        });
    }

    /// Erweiterte Identifikation (wie VCDS „Advanced ID“): alle Standard-
    /// Kennfelder eines UDS-Steuergeräts lesen und benennen. Nur lesen.
    pub(super) fn ui_ident(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Erweiterte Identifikation"));
        ui.label(tr!("Alle Standard-Kennfelder eines Steuergeräts lesen und benennen (Teilenummern, Software-/Hardwarestand, Seriennummer, Programmierdatum …). Nur lesen — es wird nichts verändert."));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(tr!("Steuergerät (CAN-ID, z. B. 0x7E0):"));
            ui.add_sized([110.0, 20.0], egui::TextEdit::singleline(&mut self.ident_tx));
            if ui.button(tr!("⬇ Lesen")).clicked() {
                self.ident_lesen();
            }
        });
        ui.weak(tr!("Nur für UDS-Steuergeräte (CAN). Ältere TP-2.0-Geräte: Reiter „TP 2.0“."));
        if let Some((tx, zeilen)) = &self.ident_result {
            ui.add_space(6.0);
            if zeilen.is_empty() {
                ui.label(tr!("Keine Kennfelder gelesen — spricht das Gerät {a} UDS? (Zündung an?)", a = tx));
            } else {
                ui.strong(tr!("Identifikation {a}:", a = tx));
                egui::ScrollArea::vertical().id_source("ident_tab").show(ui, |ui| {
                    egui::Grid::new("ident").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                        for (name, wert) in zeilen {
                            ui.label(name);
                            ui.monospace(wert);
                            ui.end_row();
                        }
                    });
                });
            }
        }
    }

    pub(super) fn ident_lesen(&mut self) {
        use ecu_werkbank::swinfo;
        let Some(tx) = parse_hex(&self.ident_tx) else {
            self.status = tr!("CAN-ID nicht verstanden — bitte z. B. 0x7E0.");
            return;
        };
        let rx = ecu_werkbank::uds::antwort_id(tx);
        let be = self.backend();
        self.im_hintergrund(tr!("Identifikation {a} lesen", a = format!("{tx:#05x}")), move || {
            let mut zeilen: Vec<(String, String)> = Vec::new();
            let status = match be.open(tx, rx) {
                Ok(tp) => {
                    let uds = Uds::new(&*tp);
                    let stumm = uds.diagnostic_session(ecu_werkbank::uds::SESSION_DEFAULT).is_err();
                    let mut teil = String::new();
                    for (did, name) in swinfo::STD_DIDS {
                        if let Ok(d) = uds.read_data_by_id(*did) {
                            if !d.is_empty() {
                                let wert = swinfo::ident_wert(*did, &d);
                                if *did == 0xF187 {
                                    teil = wert.clone();
                                }
                                zeilen.push((ecu_werkbank::i18n::t(name), wert));
                            }
                        }
                    }
                    if let Ok(d) = uds.read_data_by_id(ecu_werkbank::backup::VAG_CODING_DID) {
                        if !d.is_empty() {
                            zeilen.push((tr!("Codierung (lang)"), ecu_werkbank::backup::hex(&d)));
                        }
                    }
                    if !teil.is_empty() {
                        zeilen.extend(swinfo::datenbank_zeilen(&teil));
                    }
                    if stumm && zeilen.is_empty() {
                        tr!("Steuergerät {a} antwortet nicht (Zündung an?).", a = format!("{tx:#05x}"))
                    } else {
                        tr!("{a} Kennfeld(er) von {b} gelesen.", a = zeilen.len(), b = format!("{tx:#05x}"))
                    }
                }
                Err(e) => tr!("Verbindung: {e}", e = format!("{:#}", e)),
            };
            Box::new(move |app: &mut App| {
                app.status = status;
                app.ident_result = Some((format!("{tx:#05x}"), zeilen));
            })
        });
    }

    /// Grundeinstellung-Katalog: Nachschlage-Referenz der üblichen
    /// Werkstattfunktionen je System. Greift nie ins Auto ein.
    pub(super) fn ui_grund_lexikon(&mut self, ui: &mut egui::Ui) {
        use ecu_werkbank::grundeinstellung as ge;
        ui.heading(tr!("Grundeinstellungen & Werkstattfunktionen (Referenz)"));
        ui.label(tr!("Übersicht, was es gibt, wozu es dient und worauf zu achten ist. Reines Nachschlagen — ausgeführt wird hier nichts."));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            platz(ui, 180.0);
            ui.label(tr!("System:"));
            egui::ComboBox::from_id_source("gek_sys").selected_text(if self.gek_system.is_empty() { tr!("alle") } else { self.gek_system.clone() }).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.gek_system, String::new(), tr!("alle"));
                for s in ge::systeme() {
                    ui.selectable_value(&mut self.gek_system, s.to_string(), s);
                }
            });
            ui.label(tr!("Suche:"));
            ui.add_sized([200.0, 20.0], egui::TextEdit::singleline(&mut self.gek_suche).hint_text(tr!("z. B. Drosselklappe, G85, DPF")));
        });
        ui.separator();
        let sys = (!self.gek_system.is_empty()).then_some(self.gek_system.as_str());
        let suche = (!self.gek_suche.trim().is_empty()).then_some(self.gek_suche.trim());
        let treffer = ge::filtern(sys, suche);
        if treffer.is_empty() {
            ui.weak(tr!("Nichts gefunden."));
            return;
        }
        egui::ScrollArea::vertical().id_source("gek_tab").show(ui, |ui| {
            let mut letztes = "";
            for e in treffer {
                if e.system != letztes {
                    ui.add_space(6.0);
                    ui.strong(e.system);
                    letztes = e.system;
                }
                ui.horizontal(|ui| {
                    ui.label(e.stufe.ampel());
                    ui.label(egui::RichText::new(e.name).strong());
                    ui.weak(format!("[{}]", e.stufe.text()));
                });
                ui.label(e.zweck);
                ui.horizontal_wrapped(|ui| {
                    ui.weak("⚠");
                    ui.weak(e.hinweis);
                });
            }
        });
        ui.add_space(6.0);
        ui.weak(tr!("Service-Rückstellung: Reiter „Service-Anzeige“. Stellglieder/Routinen: Reiter „Grundeinstellung / Stellglied“ (nur im Stand)."));
    }

    pub(super) fn nr_take(&mut self, phase: u8) {
        let be = self.backend();
        let labels_dir = self.labels_dir.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
            let _ = tx.send(ecu_werkbank::retrofit::aufnehmen(&be, &db, |_| {}).map_err(|e| format!("{e:#}")));
        });
        self.nr_job = Some((phase, rx));
    }

    pub(super) fn poll_nachruestung(&mut self, ctx: &egui::Context) {
        let Some((phase, rx)) = &self.nr_job else { return };
        let phase = *phase;
        let Ok(res) = rx.try_recv() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        self.nr_job = None;
        match res {
            Ok(mut st) if phase == 1 => {
                let _ = st.scan.assign_vehicle(self.active_vehicle.as_deref());
                self.status = tr!("Vorher-Stand: {a} Steuergeräte, {b} Fehler. Jetzt einbauen.", a = st.scan.modules.len(), b = st.scan.total_dtcs());
                self.nr_vorher = Some(st);
                self.nr_result.clear();
            }
            Ok(mut st) => {
                let Some(vorher) = self.nr_vorher.clone() else { return };
                let veh = st.scan.assign_vehicle(self.active_vehicle.as_deref()).ok();
                self.nr_result = ecu_werkbank::retrofit::vergleich(&vorher, &st, &self.db);
                st.scan.reparatur = tr!("Nachrüstung {a}{b}", a = self.nr_name.trim(), b = if self.nr_notiz.trim().is_empty() { String::new() } else { format!(": {}", self.nr_notiz.trim()) });
                let _ = ecu_werkbank::history::save(&st.scan);
                if let Some(v) = veh {
                    st.codierung.vehicle = v.key.clone();
                    let _ = ecu_werkbank::backup::save(&st.codierung);
                    if let Ok(d) = v.sub("berichte") {
                        let p = d.join(ecu_werkbank::vehicle::stamped("nachruestung", "pdf"));
                        if ecu_werkbank::ablage::schreiben(&p, ecu_werkbank::retrofit::pdf(self.nr_name.trim(), &vorher, &st, &self.nr_result, &self.nr_notiz)).is_ok() {
                            self.status = tr!("Nachrüstprotokoll gespeichert: {a}", a = p.display());
                            self.pdf_last = Some(p);
                        }
                    }
                }
                self.nr_vorher = None;
            }
            Err(e) => self.status = tr!("Nachrüstung: {e}", e = e),
        }
    }

    pub(super) fn ui_nachruestung(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Nachrüst-Assistent"));
        ui.label(tr!("Für Anhängerkupplung, Tempomat, Radio & Co.: Stand vorher sichern (Scan + Codierung), einbauen/codieren, nachher prüfen. Ergebnis: neue Steuergeräte, neue Fehler, geänderte Codierung — als Protokoll-PDF."));
        let busy = self.nr_job.is_some();
        ui.horizontal(|ui| {
            ui.label(tr!("Nachrüstung:"));
            ui.add_sized([220.0, 20.0], egui::TextEdit::singleline(&mut self.nr_name).hint_text(tr!("z. B. Anhängerkupplung")));
        });
        ui.horizontal(|ui| {
            let lbl = if self.nr_vorher.is_some() { tr!("1. Vorher-Stand (gesichert)") } else { tr!("1. Vorher-Stand sichern") };
            if ui.add_enabled(!busy && !self.nr_name.trim().is_empty(), egui::Button::new(lbl)).clicked() {
                self.nr_take(1);
            }
            if busy {
                ui.spinner();
            }
        });
        if self.nr_vorher.is_some() {
            ui.label(tr!("2. Teil einbauen, codieren/anlernen (Codierung-Seite, Lernhilfe oder Werkstattgerät)."));
            ui.horizontal(|ui| {
                ui.add_sized([300.0, 20.0], egui::TextEdit::singleline(&mut self.nr_notiz).hint_text(tr!("Notiz, z. B. Teilenummer, Codierung")));
                if ui.add_enabled(!busy, egui::Button::new(tr!("3. Nachher prüfen » Protokoll"))).clicked() {
                    self.nr_take(2);
                }
            });
        }
        for (st, t) in self.nr_result.clone() {
            ui.horizontal_wrapped(|ui| {
                dot(ui, stufe_color(st));
                ui.label(t);
            });
        }
    }

    /// Codezeile mit 🔎-Knopf; ohne bekannten Text zusätzlich ✏ zum Eintragen.
    pub(super) fn dtc_row(&mut self, ui: &mut egui::Ui, line: &str, model: &str, notes: &[String]) {
        let codes = codes_in(line);
        let ohne_text = !codes.is_empty() && codes.iter().all(|c| self.db.dtc_description(c).is_none());
        ui.horizontal(|ui| {
            ui.monospace(line);
            if let Some(c) = codes.first() {
                if ui.small_button("🔎").on_hover_text(tr!("{c} im Internet nachschlagen", c = c)).clicked() {
                    open_code_search(c, model, &mut self.status);
                }
                if ohne_text && ui.small_button("✏").on_hover_text(tr!("Eigenen Text zu diesem Code eintragen (gilt danach überall, auch im PDF)")).clicked() {
                    self.ft_inline = Some((c.clone(), String::new()));
                }
            }
        });
        let offen = matches!(&self.ft_inline, Some((c, _)) if codes.first() == Some(c));
        if offen {
            let mut speichern = false;
            let mut weg = false;
            ui.horizontal(|ui| {
                let (c, t) = self.ft_inline.as_mut().expect("offen");
                ui.label(tr!("      Text zu {c}:", c = c));
                let r = ui.add_sized([360.0, 20.0], egui::TextEdit::singleline(t).hint_text(tr!("z. B. Einparkhilfe: Sensor hinten links")));
                if t.is_empty() && !r.has_focus() {
                    r.request_focus();
                }
                speichern = ui.button(tr!("Speichern")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                weg = ui.button(tr!("Abbrechen")).clicked();
            });
            if speichern {
                if let Some((c, t)) = self.ft_inline.take() {
                    if !t.trim().is_empty() {
                        self.status = match ecu_werkbank::fehlertexte::setzen(&c, &t) {
                            Ok(()) => {
                                self.db.set_dtc_text(&c, &t);
                                tr!("Text zu {c} gespeichert — gilt ab jetzt überall (neu lesen zeigt ihn in der Zeile).", c = c)
                            }
                            Err(e) => format!("{e:#}"),
                        };
                    }
                }
            } else if weg {
                self.ft_inline = None;
            }
        }
        for n in notes {
            ui.colored_label(egui::Color32::from_rgb(225, 150, 50), tr!("      Früher: {n}", n = n));
        }
    }

    /// Gemerkten Stand im Hintergrund lesen (dauert am Auto ~1 Minute).
    pub(super) fn svz_merken(&mut self, name: &str) {
        if self.svz_job.is_some() {
            return;
        }
        let be = self.backend();
        let veh = self.active_vehicle.as_deref().and_then(ecu_werkbank::vehicle::load);
        let cfg = ecu_werkbank::service::config_fuer(&be, veh.as_ref());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut alle = ecu_werkbank::service::Schnappschuss::new();
            for (a, b) in ecu_werkbank::service::bereiche(&be) {
                match ecu_werkbank::service::schnappschuss(&be, cfg.tx, cfg.rx, a, b, |d, n| {
                    let _ = tx.send(SvzMsg::Fortschritt(d, n));
                }) {
                    Ok(s) => alle.extend(s),
                    Err(e) => {
                        let _ = tx.send(SvzMsg::Fertig(Err(format!("{e:#}"))));
                        return;
                    }
                }
            }
            let _ = tx.send(SvzMsg::Fertig(Ok(alle)));
        });
        self.svz_job = Some((rx, name.to_string()));
        self.svz_fortschritt = (0, 0);
    }

    pub(super) fn ui_service_anzeige(&mut self, ui: &mut egui::Ui) {
        use ecu_werkbank::service::{self as sv, Art};
        ui.heading(tr!("Service-Anzeige (Inspektion / Ölwechsel)"));
        ui.label(tr!("Liest, wann Inspektion und Ölwechsel fällig sind, und stellt den Service nach dem Ölwechsel zurück."));
        let be = self.backend();
        let veh = self.active_vehicle.as_deref().and_then(ecu_werkbank::vehicle::load);
        let (cfg, herkunft) = sv::config_mit_herkunft(&be, veh.as_ref());
        let ordner = sv::ordner(&be, veh.as_ref());

        // Hintergrund-Lesen abholen
        if let Some((rx, name)) = &self.svz_job {
            let name = name.clone();
            let mut fertig = None;
            while let Ok(m) = rx.try_recv() {
                match m {
                    SvzMsg::Fortschritt(d, n) => self.svz_fortschritt = (d, n),
                    SvzMsg::Fertig(r) => fertig = Some(r),
                }
            }
            match fertig {
                Some(Ok(s)) => {
                    self.svz_job = None;
                    match &ordner {
                        Some(o) => match sv::speichern(&s, &sv::merk_pfad(o, &name)) {
                            Ok(()) => self.status = tr!("Stand „{name}“ gemerkt: {a} Kennungen.", name = name, a = s.len()),
                            Err(e) => self.status = tr!("Speichern fehlgeschlagen: {e}", e = format!("{:#}", e)),
                        },
                        None => self.status = tr!("Bitte zuerst ein Fahrzeug wählen."),
                    }
                }
                Some(Err(e)) => {
                    self.svz_job = None;
                    self.status = tr!("Lesen fehlgeschlagen: {e}", e = e);
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr!("Lese Kombi … Kennung {a}, {b} lesbar", a = format!("{:#06X}", self.svz_fortschritt.0), b = self.svz_fortschritt.1));
                    });
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
                }
            }
        }
        let busy = self.svz_job.is_some();

        if veh.is_none() && !matches!(be, Backend::Demo) {
            ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Bitte oben ein Fahrzeug wählen — die Service-Kennungen werden je Fahrzeug gespeichert."));
        }

        // --- Herkunft der Kennungen / Fahrzeug-Datenbank ---
        let kombi_teil = veh.as_ref().and_then(sv::kombi_teilenummer);
        match &herkunft {
            sv::Herkunft::Datenbank(t) => {
                ui.colored_label(stufe_color(Stufe::Gut), tr!("📚 Service-Kennungen aus der Fahrzeug-Datenbank — passend zum Kombi {a}.", a = t));
            }
            sv::Herkunft::Fahrzeug if cfg.kann_zuruecksetzen() => {
                if let (Some(t), Some(v)) = (&kombi_teil, &veh) {
                    let schon = sv::datenbank_wissen(t).config.as_ref() == Some(&cfg);
                    ui.horizontal_wrapped(|ui| {
                        if schon {
                            ui.weak(tr!("In der Fahrzeug-Datenbank für Kombi {a} gespeichert ✓", a = t));
                        } else if ui
                            .button(tr!("📚 In die Fahrzeug-Datenbank übernehmen"))
                            .on_hover_text(tr!("Dann findet jedes Auto mit dem Kombi {a} die Service-Rückstellung automatisch.", a = t))
                            .clicked()
                        {
                            self.status = match sv::in_datenbank(v, &cfg) {
                                Ok(t) => tr!("Service-Kennungen für Kombi {a} in der Fahrzeug-Datenbank gespeichert.", a = t),
                                Err(e) => tr!("Datenbank: {e}", e = format!("{:#}", e)),
                            };
                        }
                    });
                }
            }
            _ => {}
        }
        if let Some(t) = &kombi_teil {
            let namen = sv::datenbank_wissen(t).namen;
            if !namen.is_empty() {
                egui::CollapsingHeader::new(tr!("Service-Anpassungen dieses Kombis laut Fahrzeug-Datenbank ({a})", a = namen.len())).id_source("svz_db_namen").show(ui, |ui| {
                    ui.weak(tr!("Nur die Namen — welche Kennung dahintersteckt und welcher Wert bei der Rückstellung geschrieben wird, lernt ECUWerkbank unten einmal am Auto."));
                    for n in namen {
                        ui.label(format!("· {n}"));
                    }
                });
            }
        }

        // --- Werte ---
        ui.add_space(6.0);
        if cfg.werte.is_empty() {
            ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Für dieses Fahrzeug ist die Service-Rückstellung noch nicht eingerichtet. Unten „Rückstellung einrichten“."));
        } else {
            // beim ersten Öffnen einmal selbst lesen (nur lesen, 4 Kennungen)
            let key = Some(veh.as_ref().map(|v| v.key.clone()));
            let auto = !busy && self.svz_gelesen_fuer != key;
            if auto {
                self.svz_gelesen_fuer = key;
            }
            ui.horizontal(|ui| {
                if auto || ui.add_enabled(!busy, egui::Button::new(tr!("⬇ Service-Werte lesen"))).clicked() {
                    match sv::lesen(&be, &cfg) {
                        Ok(v) => {
                            self.svz_vals = v;
                            self.status = tr!("Service-Werte gelesen.");
                        }
                        Err(e) => self.status = tr!("Service: {e}", e = format!("{:#}", e)),
                    }
                }
            });
            if !self.svz_vals.is_empty() {
                egui::Grid::new("svz_grid").striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
                    for g in &self.svz_vals {
                        dot(ui, stufe_color(g.stufe()));
                        ui.label(g.art.text());
                        ui.strong(g.wert_text());
                        ui.weak(format!("{:#06X}", g.did));
                        ui.end_row();
                    }
                });
            }
        }

        // --- Zurückstellen ---
        ui.add_space(10.0);
        ui.separator();
        ui.strong(tr!("Service zurückstellen"));
        if cfg.kann_zuruecksetzen() {
            ui.label(tr!("Schreibt die neuen Service-Werte ins Kombi. Vorher werden die alten Werte gesichert; danach wird nachgelesen."));
            for z in cfg.vorschau() {
                ui.monospace(z);
            }
            ui.checkbox(&mut self.svz_bestaetigen, tr!("Service wurde gemacht — Zündung an, Motor aus. Jetzt zurückstellen."));
            if veh.is_some() {
                ui.checkbox(&mut self.svz_wartung, tr!("Ölwechsel dabei im Wartungsplan als heute erledigt eintragen"));
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(self.svz_bestaetigen && !busy, egui::Button::new(tr!("↺ Service zurückstellen"))).clicked() {
                    self.svz_bestaetigen = false;
                    match sv::zuruecksetzen(&be, &cfg, ordner.as_deref()) {
                        Ok(r) => {
                            self.svz_lines = r.zeilen.clone();
                            if let Some(p) = &r.sicherung {
                                self.svz_lines.push(tr!("Alte Werte gesichert: {a}", a = p.display()));
                            }
                            self.status = if r.ok { tr!("Service zurückgestellt ✓") } else { tr!("Rückstellung nicht vollständig — siehe unten.") };
                            if let (true, true, Some(mut v)) = (r.ok, self.svz_wartung, veh.clone()) {
                                let n = sv::wartung_eintragen(&mut v);
                                if ecu_werkbank::vehicle::save(&v).is_ok() {
                                    self.svz_lines.push(tr!("Wartungsplan: {a} als heute erledigt eingetragen.", a = n.join(", ")));
                                }
                            }
                            if let Ok(v) = sv::lesen(&be, &cfg) {
                                self.svz_vals = v;
                            }
                        }
                        Err(e) => self.status = tr!("Rückstellung: {e}", e = format!("{:#}", e)),
                    }
                }
                let letzte = ordner.as_deref().and_then(sv::neueste_sicherung);
                let geklickt = ui.add_enabled(letzte.is_some() && !busy, egui::Button::new(tr!("Letzte Rückstellung rückgängig"))).on_hover_text(tr!("Schreibt die vorher gesicherten Werte zurück")).clicked();
                if let (true, Some(p)) = (geklickt, letzte) {
                    match sv::sicherung_zurueck(&be, &cfg, &p) {
                        Ok(z) => {
                            self.svz_lines = z;
                            self.status = tr!("Alte Service-Werte zurückgeschrieben.");
                            if let Ok(v) = sv::lesen(&be, &cfg) {
                                self.svz_vals = v;
                            }
                        }
                        Err(e) => self.status = tr!("Rückgängig: {e}", e = format!("{:#}", e)),
                    }
                }
            });
        } else {
            ui.label(tr!("Über ECUWerkbank noch nicht eingerichtet. So geht es mit den Tasten am Kombi:"));
        }
        egui::CollapsingHeader::new(tr!("Anleitung: mit den Tasten am Kombi zurückstellen")).default_open(!cfg.kann_zuruecksetzen()).show(ui, |ui| {
            for z in sv::anleitung_tasten(veh.as_ref().map(|v| v.bezeichnung.as_str()).unwrap_or("golf")) {
                ui.label(z);
            }
        });
        for l in &self.svz_lines {
            ui.monospace(l);
        }

        // --- Lernen ---
        ui.add_space(10.0);
        ui.separator();
        egui::CollapsingHeader::new(tr!("Rückstellung einrichten (einmal je Fahrzeug)")).default_open(cfg.werte.is_empty()).show(ui, |ui| {
            ui.strong(tr!("Werte aus der Kombi-Anzeige suchen (ohne Tasten)"));
            ui.label(tr!("Lies im Kombi ab, was gerade angezeigt wird (z. B. „Ölwechsel in 4 650 km / 153 Tagen“), und trag es hier ein. Überfällig = negativ. Leere Felder werden übersprungen."));
            let arten = [Art::KmBisInspektion, Art::TageBisInspektion, Art::KmBisOelwechsel, Art::TageBisOelwechsel];
            let namen = [tr!("km bis Inspektion"), tr!("Tage bis Inspektion"), tr!("km bis Ölwechsel"), tr!("Tage bis Ölwechsel")];
            egui::Grid::new("svz_ziel").spacing([10.0, 4.0]).show(ui, |ui| {
                ui.label("");
                ui.weak(tr!("Anzeige jetzt"));
                ui.weak(tr!("Neu nach dem Service"));
                ui.end_row();
                for ((name, ziel), neu) in namen.iter().zip(self.svz_ziel.iter_mut()).zip(self.svz_neu.iter_mut()) {
                    ui.label(name.as_str());
                    ui.add(egui::TextEdit::singleline(ziel).desired_width(70.0));
                    ui.add(egui::TextEdit::singleline(neu).desired_width(70.0));
                    ui.end_row();
                }
            });
            ui.weak(tr!("„Neu“: Intervall aus dem Serviceheft. Vorbelegt: Inspektion 30 000 km / 730 Tage, Ölwechsel 15 000 km / 365 Tage."));
            let quelle = ordner.as_ref().and_then(|o| ["aktuell", "vorher"].into_iter().find(|n| sv::merk_pfad(o, n).exists()).map(|n| (sv::merk_pfad(o, n), n)));
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy && ordner.is_some(), egui::Button::new(tr!("1. ⬇ Kombi jetzt lesen"))).on_hover_text(tr!("Liest alle Kennungen im Kombi (nur lesen, ca. 1 Minute)")).clicked() {
                    self.svz_merken("aktuell");
                }
                let geklickt = ui.add_enabled(!busy && quelle.is_some(), egui::Button::new(tr!("2. 🔍 Passende Kennungen suchen"))).clicked();
                if let (true, Some((p, _))) = (geklickt, &quelle) {
                    let ziele: Vec<(Art, f64)> = arten.iter().zip(&self.svz_ziel).filter_map(|(a, t)| t.trim().replace(',', ".").parse().ok().map(|w| (*a, w))).collect();
                    if ziele.is_empty() {
                        self.status = tr!("Bitte mindestens einen Wert aus der Anzeige eintragen.");
                    } else {
                        match sv::laden(p) {
                            Ok(s) => {
                                let k = sv::suche(&s, &ziele);
                                // eindeutige Treffer vorwählen, mehrdeutige muss man selbst wählen
                                self.svz_kandidaten = k.iter().map(|c| (c.clone(), k.iter().filter(|x| x.art == c.art).count() == 1)).collect();
                                self.svz_lines = if k.is_empty() { vec![tr!("Keine passende Kennung gefunden. Werte genau so wie im Kombi eingetragen?")] } else { vec![] };
                            }
                            Err(e) => self.status = tr!("Laden: {e}", e = format!("{:#}", e)),
                        }
                    }
                }
            });
            if let Some((_, n)) = &quelle {
                ui.weak(tr!("Gesucht wird im Stand „{n}“.", n = n));
            }
            if !self.svz_kandidaten.is_empty() {
                ui.add_space(4.0);
                ui.label(tr!("Treffer — je Wert genau eine Kennung anhaken:"));
                egui::Grid::new("svz_kand").striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                    for (c, an) in self.svz_kandidaten.iter_mut() {
                        ui.checkbox(an, "");
                        ui.monospace(format!("{:#06X}", c.wert.did));
                        ui.label(tr!("Byte {b}, ×{c}", b = c.wert.offset, c = c.wert.faktor));
                        ui.strong(format!("{:.0} {}", c.gelesen, c.art.einheit()));
                        ui.label(c.art.text());
                        ui.end_row();
                    }
                });
                let gewaehlt: Vec<sv::Kandidat> = self.svz_kandidaten.iter().filter(|(_, an)| *an).map(|(c, _)| c.clone()).collect();
                let intervall: Vec<(Art, f64)> = arten.iter().zip(&self.svz_neu).filter_map(|(a, t)| t.trim().replace(',', ".").parse().ok().map(|w| (*a, w))).collect();
                let plan = sv::aus_suche(&cfg, &gewaehlt, &intervall);
                match &plan {
                    Ok(c) => {
                        ui.label(tr!("Bei der Rückstellung wird dann geschrieben:"));
                        for z in c.vorschau() {
                            ui.monospace(z);
                        }
                    }
                    Err(e) => {
                        ui.colored_label(stufe_color(Stufe::Hinweis), format!("{e:#}"));
                    }
                }
                ui.weak(tr!("Nur Kennungen wählen, deren Wert genau zur Anzeige passt. Vor jeder Rückstellung werden die alten Werte gesichert und lassen sich zurückschreiben."));
                let geklickt = ui.add_enabled(plan.is_ok() && veh.is_some(), egui::Button::new(tr!("✔ Als Service-Rückstellung einrichten"))).clicked();
                if let (true, Ok(c), Some(mut v)) = (geklickt, plan, veh.clone()) {
                    v.service = Some(c);
                    self.status = match ecu_werkbank::vehicle::save(&v) {
                        Ok(()) => {
                            self.svz_kandidaten.clear();
                            self.svz_gelesen_fuer = None; // Werte neu lesen
                            tr!("Eingerichtet — oben „Service zurückstellen“ ist jetzt möglich.")
                        }
                        Err(e) => tr!("Speichern: {e}", e = format!("{:#}", e)),
                    };
                }
                if veh.is_none() {
                    ui.weak(tr!("Zum Einrichten oben ein Fahrzeug wählen."));
                }
            }
            ui.add_space(8.0);
            ui.separator();
            ui.strong(tr!("Oder am sichersten: Vorher/Nachher mit einer Tasten-Rückstellung (liest nur)"));
            let gemerkt = |n: &str| ordner.as_ref().is_some_and(|o| sv::merk_pfad(o, n).exists());
            ui.horizontal(|ui| {
                ui.label("1.");
                if ui.add_enabled(!busy && ordner.is_some(), egui::Button::new(tr!("Stand „vorher“ merken"))).clicked() {
                    self.svz_merken("vorher");
                }
                if gemerkt("vorher") {
                    ui.label("✓");
                }
            });
            ui.label(tr!("2. Service mit den Tasten am Kombi zurückstellen (Anleitung oben)."));
            ui.horizontal(|ui| {
                ui.label("3.");
                if ui.add_enabled(!busy && ordner.is_some(), egui::Button::new(tr!("Stand „nachher“ merken"))).clicked() {
                    self.svz_merken("nachher");
                }
                if gemerkt("nachher") {
                    ui.label("✓");
                }
            });
            ui.horizontal(|ui| {
                ui.label("4.");
                let geklickt = ui.add_enabled(!busy && gemerkt("vorher") && gemerkt("nachher"), egui::Button::new(tr!("Vergleichen"))).clicked();
                if let (true, Some(o)) = (geklickt, &ordner) {
                    match (sv::laden(&sv::merk_pfad(o, "vorher")), sv::laden(&sv::merk_pfad(o, "nachher"))) {
                        (Ok(a), Ok(b)) => {
                            let ruhe = sv::laden(&sv::merk_pfad(o, "ruhe")).ok();
                            let d = sv::vergleich(&a, &b, ruhe.as_ref());
                            self.svz_lines = d.iter().map(|x| x.zeile()).collect();
                            if d.is_empty() {
                                self.svz_lines.push(tr!("Keine Änderung — wurde zwischen den Ständen zurückgestellt?"));
                            }
                            self.svz_vorschlag = sv::vorschlag(&d);
                        }
                        (Err(e), _) | (_, Err(e)) => self.status = tr!("Laden: {e}", e = format!("{:#}", e)),
                    }
                }
            });
            ui.weak(tr!("Tipp: Ändern sich Werte, die nichts mit dem Service zu tun haben (Uhrzeit …), vorher zweimal ohne Rückstellung merken: der zweite Stand heißt „ruhe“."));
            if ui.add_enabled(!busy && ordner.is_some(), egui::Button::new(tr!("Stand „ruhe“ merken"))).clicked() {
                self.svz_merken("ruhe");
            }
            if !self.svz_vorschlag.is_empty() {
                ui.add_space(6.0);
                ui.strong(tr!("Vorschlag — Art bitte mit der Anzeige im Kombi vergleichen:"));
                let mut weg = None;
                egui::Grid::new("svz_vor").spacing([12.0, 4.0]).show(ui, |ui| {
                    for (i, w) in self.svz_vorschlag.iter_mut().enumerate() {
                        ui.monospace(format!("{:#06X}", w.did));
                        egui::ComboBox::from_id_source(("svz_art", i)).selected_text(format!("{} ({})", w.art.text(), w.art.einheit())).show_ui(ui, |ui| {
                            for a in Art::ALLE {
                                ui.selectable_value(&mut w.art, a, format!("{} ({})", a.text(), a.einheit()));
                            }
                        });
                        ui.monospace(tr!("Rückstellwert {a}", a = w.reset));
                        if ui.small_button("✕").on_hover_text(tr!("Nicht übernehmen")).clicked() {
                            weg = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = weg {
                    self.svz_vorschlag.remove(i);
                }
                let geklickt = ui.add_enabled(veh.is_some(), egui::Button::new(tr!("✔ Ins Fahrzeug übernehmen"))).clicked();
                if let (true, Some(mut v)) = (geklickt, veh.clone()) {
                    v.service = Some(sv::Config { werte: std::mem::take(&mut self.svz_vorschlag), ..cfg.clone() });
                    self.status = match ecu_werkbank::vehicle::save(&v) {
                        Ok(()) => tr!("Service-Kennungen gespeichert."),
                        Err(e) => tr!("Speichern: {e}", e = format!("{:#}", e)),
                    };
                }
            }

        });
    }

    pub(super) fn dpf_cfg(&self) -> ecu_werkbank::dpf::Config {
        let tx = parse_hex(&self.dpf_tx).unwrap_or(0x7E0);
        let q = |i: usize| parse_hex(&self.dpf_did[i]).map(|did| ecu_werkbank::dpf::Quelle { tx, rx: ecu_werkbank::uds::antwort_id(tx), did, faktor: self.dpf_fak[i], offset: 0.0 });
        ecu_werkbank::dpf::Config { russ: q(0), km_seit_regeneration: q(1), differenzdruck: q(2), abgastemperatur: q(3), russ_grenze_g: Some(self.dpf_grenze) }
    }

    pub(super) fn ui_dpf(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Partikelfilter (Diesel)"));
        ui.label(tr!("Rußbeladung, km seit der letzten Regeneration, Differenzdruck und Abgastemperatur mit Ampel und Fahrhinweisen."));
        let demo = matches!(self.backend(), Backend::Demo);
        let veh = self.active_vehicle.as_deref().and_then(ecu_werkbank::vehicle::load);
        // Konfiguration laden
        let key = veh.as_ref().map(|v| v.key.clone());
        if self.dpf_loaded_for != key {
            self.dpf_loaded_for = key.clone();
            let c = veh.as_ref().and_then(|v| v.dpf.clone()).or_else(|| demo.then(ecu_werkbank::dpf::demo_config));
            if let Some(c) = c {
                for (i, q) in [&c.russ, &c.km_seit_regeneration, &c.differenzdruck, &c.abgastemperatur].iter().enumerate() {
                    self.dpf_did[i] = q.as_ref().map(|q| format!("{:#06x}", q.did)).unwrap_or_default();
                    self.dpf_fak[i] = q.as_ref().map(|q| q.faktor).unwrap_or(1.0);
                    if let Some(q) = q {
                        self.dpf_tx = format!("{:#05x}", q.tx);
                    }
                }
                self.dpf_grenze = c.russ_grenze_g.unwrap_or(24.0);
            }
        }
        ui.horizontal(|ui| {
            if ui.button(tr!("⬇ Werte lesen")).clicked() {
                let c = self.dpf_cfg();
                let be = self.backend();
                let fz = veh.as_ref().map(|v| (v.key.clone(), v.km));
                self.im_hintergrund(tr!("Partikelfilter lesen"), move || {
                    let r = ecu_werkbank::dpf::read(&be, &c);
                    Box::new(move |app: &mut App| match r {
                        Ok(w) => {
                            let r = ecu_werkbank::dpf::bewerten(&w, &c);
                            if let Some((key, km)) = &fz {
                                let mut tr = Vec::new();
                                if let Some(x) = w.russ_g { tr.push((tr!("DPF Rußbeladung").to_string(), x, "g".to_string())); }
                                if let Some(x) = w.diff_mbar { tr.push((tr!("DPF Differenzdruck").to_string(), x, "mbar".to_string())); }
                                let _ = ecu_werkbank::trend::add(key, *km, &tr);
                            }
                            app.dpf_vals = Some((w, r));
                            app.status = tr!("Partikelfilter gelesen.");
                        }
                        Err(e) => app.status = tr!("Partikelfilter: {e}", e = format!("{:#}", e)),
                    })
                });
            }
        });
        if let Some((w, r)) = &self.dpf_vals {
            ui.horizontal_wrapped(|ui| {
                for (n, v, u) in [(trn!("Ruß"), w.russ_g, "g"), (trn!("seit Regeneration"), w.km, "km"), (trn!("Differenzdruck"), w.diff_mbar, "mbar"), (trn!("Abgastemp."), w.temp_c, "°C")] {
                    ui.vertical(|ui| {
                        ui.weak(ecu_werkbank::i18n::t(n));
                        ui.heading(v.map(|x| format!("{x:.1} {u}")).unwrap_or_else(|| "—".into()));
                    });
                    ui.add_space(20.0);
                }
            });
            for (st, t) in r {
                ui.horizontal_wrapped(|ui| {
                    dot(ui, stufe_color(*st));
                    ui.label(t);
                });
            }
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new(tr!("Kennungen für dieses Fahrzeug (motorspezifisch)")).default_open(!demo && veh.as_ref().is_some_and(|v| v.dpf.is_none())).show(ui, |ui| {
            ui.weak(tr!("UDS-Kennungen am Motorsteuergerät; Wert = Rohwert × Faktor. Herausfinden per Mitschnitt + Lernhilfe (Werkstattgerät zeigt die DPF-Werte an)."));
            ui.horizontal(|ui| {
                ui.label(tr!("Motor-Steuergerät TX:"));
                ui.add_sized([60.0, 20.0], egui::TextEdit::singleline(&mut self.dpf_tx));
                ui.label(tr!("Regeneration ab (g):"));
                ui.add(egui::DragValue::new(&mut self.dpf_grenze).speed(0.5).range(5.0..=80.0));
            });
            egui::Grid::new("dpfcfg").spacing([8.0, 3.0]).show(ui, |ui| {
                for (i, n) in [tr!("Rußbeladung (g)"), tr!("km seit Regeneration"), tr!("Differenzdruck (mbar)"), tr!("Abgastemperatur (°C)")].iter().enumerate() {
                    ui.label(n.as_str());
                    ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut self.dpf_did[i]).hint_text("DID"));
                    ui.add(egui::DragValue::new(&mut self.dpf_fak[i]).speed(0.001).prefix("× "));
                    ui.end_row();
                }
            });
            if let Some(mut v) = veh.clone() {
                if ui.button(tr!("💾 Für dieses Fahrzeug speichern")).clicked() {
                    v.dpf = Some(self.dpf_cfg());
                    self.status = match ecu_werkbank::vehicle::save(&v) {
                        Ok(()) => tr!("Partikelfilter-Kennungen gespeichert."),
                        Err(e) => format!("{e:#}"),
                    };
                }
            }
        });
    }

    pub(super) fn ui_wissen(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Fehlerbilder-Bibliothek"));
        ui.label(tr!("Eigene Erfahrungen je Modell und Fehlercode. Erscheinen in der geführten Fehlersuche — zusätzlich werden Reparatur-Notizen von Fahrzeugen desselben Modells automatisch berücksichtigt."));
        if self.ws_modell.is_empty() {
            self.ws_modell = self.vehicle_model();
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(&mut self.ws_code).hint_text("P0300"));
            ui.add_sized([150.0, 20.0], egui::TextEdit::singleline(&mut self.ws_modell).hint_text(tr!("Modell, z. B. Golf 6 TSI")));
            ui.add_sized([320.0, 20.0], egui::TextEdit::singleline(&mut self.ws_text).hint_text(tr!("Erfahrung, z. B. meist Zündspule Zyl. 3")));
            if ui.button(tr!("+ Speichern")).clicked() && !self.ws_code.trim().is_empty() && !self.ws_text.trim().is_empty() {
                let e = ecu_werkbank::wissen::Eintrag { modell: self.ws_modell.trim().to_string(), code: self.ws_code.trim().to_string(), text: self.ws_text.trim().to_string(), datum: String::new() };
                self.status = match ecu_werkbank::wissen::hinzufuegen(e) {
                    Ok(()) => {
                        self.ws_text.clear();
                        tr!("Fehlerbild gespeichert.")
                    }
                    Err(e) => format!("{e:#}"),
                };
            }
        });
        ui.separator();
        let mut del = None;
        egui::Grid::new("wissen").striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
            for h in [trn!("Code"), trn!("Modell"), trn!("Erfahrung"), trn!("Datum"), ""] {
                ui.strong(ecu_werkbank::i18n::t(h));
            }
            ui.end_row();
            for (i, e) in ecu_werkbank::wissen::eigene().iter().enumerate() {
                ui.monospace(&e.code);
                ui.label(if e.modell.is_empty() { "alle" } else { &e.modell });
                ui.label(&e.text);
                ui.weak(&e.datum);
                if ui.small_button("🗑").clicked() {
                    del = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = del {
            let _ = ecu_werkbank::wissen::loeschen(i);
        }

        ui.add_space(14.0);
        ui.heading(tr!("Eigene Fehlertexte"));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Eigene Klartexte zu Fehlercodes stehen jetzt in der Fahrzeug-Datenbank unter „Eigene Texte“."));
            if ui.button(tr!("➜ Öffnen")).clicked() {
                self.tab = Tab::Datenbank;
                self.dbs_eigene_texte_oeffnen("fehler");
            }
        });
        ui.add_space(10.0);
        self.ui_fehlergeruest(ui);
        ui.add_space(14.0);
        ui.separator();
        // Bildschirmfoto VAG_TAB=lexikon: gleich aufgeklappt
        let offen = std::env::var("VAG_TAB").is_ok_and(|t| t == "lexikon");
        egui::CollapsingHeader::new(tr!("📚 Grundeinstellungen & Werkstattfunktionen (Referenz)"))
            .id_source("grund_lexikon")
            .default_open(offen)
            .show(ui, |ui| self.ui_grund_lexikon(ui));
    }

    /// Alle möglichen Fehlercodes aus dem Auto lesen und Texte direkt eintragen.
    pub(super) fn ui_fehlergeruest(&mut self, ui: &mut egui::Ui) {
        ui.strong(tr!("Alle möglichen Fehlercodes aus dem Auto"));
        ui.label(tr!("Fragt die neueren Steuergeräte (UDS) nach allen Fehlercodes, die sie kennen — nicht nur den gespeicherten. Nur lesen. Danach kannst du zu jedem Code gleich den Klartext eintragen. Ältere Steuergeräte (TP 2.0) kennen diese Abfrage nicht."));
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.fg_job.is_none(), egui::Button::new(tr!("🔍 Fehlercodes lesen"))).clicked() {
                let be = self.backend();
                let labels_dir = self.labels_dir.clone();
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
                    let r = ecu_werkbank::fehlergeruest::lesen(&be, &db, |p| {
                        let _ = tx.send(FgMsg::Progress(p.to_string()));
                    });
                    let _ = tx.send(FgMsg::Done(r.map_err(|e| format!("{e:#}"))));
                });
                self.fg_job = Some(rx);
                self.fg_progress.clear();
            }
            if self.fg_job.is_some() {
                ui.spinner();
                ui.weak(tr!("Steuergerät {a} …", a = &self.fg_progress));
            }
            if ui.button(tr!("📂 Ausgefüllte Datei übernehmen")).on_hover_text(tr!("Eine Datei aus „ecu-werkbank fehler-geruest“ bzw. „Als Datei speichern“")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("TOML", &["toml"]).pick_file() {
                    self.status = match ecu_werkbank::ablage::lesen(&p).map_err(anyhow::Error::from).and_then(|t| ecu_werkbank::fehlergeruest::uebernehmen(&t)) {
                        Ok((n, g, _)) => {
                            for (c, t) in ecu_werkbank::fehlertexte::alle() {
                                self.db.set_dtc_text(&c, &t);
                            }
                            self.fg_texte_neu_laden();
                            tr!("Übernommen: {a} neu, {b} geändert.", a = n, b = g)
                        }
                        Err(e) => format!("{e:#}"),
                    };
                }
            }
        });
        if self.fg_geraete.is_empty() {
            return;
        }
        let eigene = ecu_werkbank::fehlertexte::alle();
        let geaendert: Vec<(String, String)> =
            self.fg_texte.iter().filter(|(c, t)| !t.trim().is_empty() && eigene.get(*c).map(|e| e.as_str()) != Some(t.trim())).map(|(c, t)| (c.clone(), t.clone())).collect();
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!geaendert.is_empty(), egui::Button::new(tr!("💾 Texte speichern ({a})", a = geaendert.len()))).clicked() {
                self.status = match ecu_werkbank::fehlertexte::mehrere_setzen(&geaendert) {
                    Ok(()) => {
                        for (c, t) in &geaendert {
                            self.db.set_dtc_text(c, t.trim());
                        }
                        tr!("{a} Fehlertext(e) gespeichert.", a = geaendert.len())
                    }
                    Err(e) => format!("{e:#}"),
                };
            }
            ui.checkbox(&mut self.fg_nur_leer, tr!("nur Codes ohne Text"));
            if ui.button(tr!("📄 Als Datei speichern")).on_hover_text(tr!("Zum Ausfüllen in Ruhe, z. B. am anderen Rechner — danach „Ausgefüllte Datei übernehmen“")).clicked() {
                let v = ecu_werkbank::vehicle::resolve(None, self.active_vehicle.as_deref()).ok();
                let name = v.as_ref().map(|v| v.label()).unwrap_or_default();
                let text = ecu_werkbank::fehlergeruest::geruest(&self.fg_geraete, &self.db, &name, &ecu_werkbank::report::now_local());
                let datei = ecu_werkbank::vehicle::stamped("fehlertexte-geruest", "toml");
                let pfad = v.and_then(|v| v.sub("berichte").ok()).unwrap_or_else(ecu_werkbank::history::data_dir).join(datei);
                self.status = match ecu_werkbank::ablage::schreiben(&pfad, text) {
                    Ok(()) => tr!("Gespeichert: {a}", a = pfad.display()),
                    Err(e) => format!("{e:#}"),
                };
            }
        });
        let nur_leer = self.fg_nur_leer;
        // kurze Listen gleich offen, lange zum Aufklappen
        let offen = self.fg_geraete.iter().map(|g| g.codes.len()).sum::<usize>() <= 40;
        for g in &self.fg_geraete {
            let mit_text = g.codes.keys().filter(|c| eigene.contains_key(*c)).count();
            let titel = tr!("{a}  {b}  {c} — {d} mögliche Fehler, {e} mit Text", a = g.name, b = format!("{:#05x}", g.tx), c = g.teil, d = g.codes.len(), e = mit_text);
            egui::CollapsingHeader::new(titel).id_source(("fg", g.tx)).default_open(offen).show(ui, |ui| {
                if !g.vollstaendig {
                    ui.weak(tr!("Dieses Steuergerät kennt die Abfrage nicht — Ersatzweg, die Liste ist evtl. unvollständig."));
                }
                egui::Grid::new(("fggrid", g.tx)).striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
                    for (code, arten) in &g.codes {
                        if nur_leer && eigene.contains_key(code) {
                            continue;
                        }
                        ui.monospace(code);
                        let arten = ecu_werkbank::fehlergeruest::arten_text(arten);
                        ui.allocate_ui(egui::vec2(230.0, 18.0), |ui| {
                            ui.set_min_width(230.0);
                            ui.add(egui::Label::new(egui::RichText::new(&arten).weak()).truncate()).on_hover_text(&arten);
                        });
                        let hinweis = self.db.dtc_description(code).filter(|t| !t.is_empty()).map(str::to_string).unwrap_or_else(|| tr!("Klartext eintragen …"));
                        let feld = self.fg_texte.entry(code.clone()).or_default();
                        ui.add_sized([360.0, 20.0], egui::TextEdit::singleline(feld).hint_text(hinweis));
                        ui.end_row();
                    }
                });
            });
        }
    }

    pub(super) fn fg_texte_neu_laden(&mut self) {
        let eigene = ecu_werkbank::fehlertexte::alle();
        self.fg_texte = self.fg_geraete.iter().flat_map(|g| g.codes.keys()).map(|c| (c.clone(), eigene.get(c).cloned().unwrap_or_default())).collect();
    }

    pub(super) fn poll_fehlergeruest(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.fg_job else { return };
        loop {
            match rx.try_recv() {
                Ok(FgMsg::Progress(p)) => self.fg_progress = p,
                Ok(FgMsg::Done(r)) => {
                    self.fg_job = None;
                    self.status = match r {
                        Ok(g) if g.is_empty() => tr!("Kein UDS-Steuergerät hat seine Fehlercodes gemeldet. (TP 2.0 / KWP2000 kennt diese Abfrage nicht.)"),
                        Ok(g) => {
                            let n: usize = g.iter().map(|x| x.codes.len()).sum();
                            self.fg_geraete = g;
                            self.fg_texte_neu_laden();
                            tr!("{a} Steuergerät(e), {b} mögliche Fehlercodes gelesen.", a = self.fg_geraete.len(), b = n)
                        }
                        Err(e) => e,
                    };
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.fg_job = None;
                    return;
                }
            }
        }
    }

    pub(super) fn ui_testlauf(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Testlauf am Fahrzeug"));
        ui.label(tr!("Fragt in einem Durchgang alles Wichtige ab — nur lesen, nichts wird gelöscht oder geschrieben: Verbindung, Fahrgestellnummer, alle Steuergeräte mit Fehlerspeicher, Messwertblöcke des Motors und die Suche nach dem Kilometerstand im Kombi. Das Protokoll landet im Fahrzeugordner (berichte)."));
        ui.horizontal(|ui| {
            ui.label(tr!("Tachostand:"));
            ui.add_sized([90.0, 20.0], egui::TextEdit::singleline(&mut self.tl_km).hint_text("z. B. 154300"))
                .on_hover_text(tr!("Kilometerstand laut Tacho — damit findet der Testlauf die Stelle im Kombi"));
            ui.label("km");
            if ui.add_enabled(self.tl_job.is_none(), egui::Button::new(tr!("▶ Testlauf starten"))).clicked() {
                let km = self.tl_km.trim().replace('.', "").parse::<u32>().ok();
                let be = self.backend();
                let labels_dir = self.labels_dir.clone();
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
                    let s = ecu_werkbank::autotest::run(&be, &db, km, |p| {
                        let _ = tx.send(TlMsg::Progress(p.to_string()));
                    });
                    let text = ecu_werkbank::autotest::to_text(&s, &be.describe());
                    let name = ecu_werkbank::vehicle::stamped("testlauf", "txt");
                    let path = ecu_werkbank::profil::quick_vin(&be)
                        .and_then(|v| ecu_werkbank::vehicle::find_by_vin(&v))
                        .and_then(|v| v.sub("berichte").ok())
                        .unwrap_or_else(ecu_werkbank::history::data_dir)
                        .join(name);
                    let saved = ecu_werkbank::ablage::schreiben(&path, &text).map(|_| path.display().to_string()).map_err(|e| e.to_string());
                    let _ = tx.send(TlMsg::Done(text, saved));
                });
                self.tl_job = Some(rx);
                self.tl_text.clear();
            }
            if self.tl_job.is_some() {
                ui.spinner();
                ui.weak(&self.tl_progress);
            }
        });
        ui.separator();
        egui::ScrollArea::both().id_source("tlscroll").show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(&self.tl_text).monospace()).extend());
        });
    }

    pub(super) fn poll_testlauf(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.tl_job else { return };
        loop {
            match rx.try_recv() {
                Ok(TlMsg::Progress(p)) => self.tl_progress = p,
                Ok(TlMsg::Done(text, saved)) => {
                    self.tl_text = text;
                    self.status = match saved {
                        Ok(p) => tr!("Testlauf gespeichert: {p}", p = p),
                        Err(e) => tr!("Testlauf: Speichern fehlgeschlagen: {e}", e = e),
                    };
                    self.tl_job = None;
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.tl_job = None;
                    return;
                }
            }
        }
    }

    pub(super) fn poll_sicherheit(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.si_job else { return };
        match rx.try_recv() {
            Ok(Ok(r)) => {
                self.si_report = Some(r);
                self.si_job = None;
            }
            Ok(Err(e)) => {
                self.status = tr!("Sicherheit & Assistenz: {e}", e = e);
                self.si_job = None;
            }
            Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        }
    }

    /// Sicherheit & Assistenz: alle Steuergeräte im Hintergrund abfragen (nur lesen).
    pub(super) fn si_starten(&mut self) {
        let be = self.backend();
        let labels_dir = self.labels_dir.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
            let _ = tx.send(ecu_werkbank::report::scan(&be, &db, true, |_| {}).map_err(|e| format!("{e:#}")));
        });
        self.si_job = Some(rx);
    }

    pub(super) fn ui_sicherheit(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Sicherheit & Assistenz"));
        ui.label(tr!("Airbag, Bremsen/ESP, Lenkung, Parkbremse und Fahrerassistenz übersichtlich — nur lesen. Arbeiten an Airbag-Systemen gehören in die Fachwerkstatt."));
        ui.horizontal(|ui| {
            if ui.add_enabled(self.si_job.is_none(), egui::Button::new(tr!("⬇ Steuergeräte abfragen"))).clicked() {
                self.si_starten();
            }
            if self.si_job.is_some() {
                ui.spinner();
            }
        });
        let Some(r) = self.si_report.clone() else {
            ui.weak(tr!("Noch nicht abgefragt."));
            return;
        };
        let mut groups: std::collections::BTreeMap<&str, Vec<&ecu_werkbank::report::ModuleReport>> = Default::default();
        for m in &r.modules {
            if ecu_werkbank::systems::sicherheit_modul(&m.protocol, m.address, &m.name) {
                groups.entry(ecu_werkbank::systems::gruppe_modul(&m.protocol, m.address, &m.name)).or_default().push(m);
            }
        }
        if groups.is_empty() {
            ui.weak(tr!("Keine Sicherheits-/Assistenz-Steuergeräte mit bekannter Adresse gefunden. Bei neueren Fahrzeugen die Adressen über Labels zuordnen."));
        }
        let model = self.vehicle_model();
        for (g, mods) in groups {
            ui.add_space(6.0);
            ui.strong(g);
            for m in mods {
                let a = m.address.unwrap_or(0);
                let name = if m.name.is_empty() && m.protocol != "TP 2.0" { ecu_werkbank::systems::name(a).to_string() } else { m.name.clone() };
                ui.horizontal(|ui| {
                    dot(ui, stufe_color(if m.dtc_count() > 0 { Stufe::Warnung } else { Stufe::Gut }));
                    ui.label(tr!("{a} {name}  —  {b} Fehler  {c}", a = m.addr_text(), name = name, b = m.dtc_count(), c = m.ident.first().cloned().unwrap_or_default()));
                });
                for (i, e) in m.dtcs.iter().flatten().enumerate() {
                    let text = if e.text.is_empty() { self.db.dtc_description(&e.code).unwrap_or("").to_string() } else { e.text.clone() };
                    // km-Stand beim Speichern gleich in der Zeile
                    let km = e.umgebung.iter().find(|(k, _)| k == "Kilometerstand" || k == "Mileage").map(|(_, v)| tr!("  · gespeichert bei {a}", a = v)).unwrap_or_default();
                    let line = format!("      {}  {text}{km}", e.code);
                    self.dtc_row(ui, &line, &model, &[]);
                    umgebung_zeigen(ui, ("scan_umg", m.addr_text(), i), &e.umgebung);
                }
            }
        }
    }
}
