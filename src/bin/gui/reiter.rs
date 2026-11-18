//! GUI: Reiter-Verteilung und Kern-Reiter (Steuergeräte, Messwerte, Codierung, Service, Namen)

use super::*;

impl App {
    pub(super) fn reiter_zeigen(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            Tab::Start => self.ui_start(ui),
            Tab::Steuergeraete => self.ui_module(ui),
            Tab::Messwerte => self.ui_messwerte(ui),
            Tab::Scope => self.ui_scope(ui),
            Tab::Codierung => self.ui_codierung(ui),
            Tab::Service => self.ui_service(ui),
            Tab::Tp20 => self.ui_tp20(ui),
            Tab::Fremd => self.ui_fremd(ui),
            Tab::Obd => self.ui_obd(ui),
            Tab::Fahrzeuge => self.ui_fahrzeuge(ui),
            Tab::Batterie => self.ui_batterie(ui),
            Tab::Ziel => self.ui_ziel(ui),
            Tab::Diagnose => self.ui_diagnose(ui),
            Tab::Software => self.ui_software(ui),
            Tab::Nachruestung => self.ui_nachruestung(ui),
            Tab::Dpf => self.ui_dpf(ui),
            Tab::ServiceAnzeige => self.ui_service_anzeige(ui),
            Tab::Wissen => self.ui_wissen(ui),
            Tab::Testlauf => self.ui_testlauf(ui),
            Tab::Sicherheit => self.ui_sicherheit(ui),
            Tab::Mitschnitt => self.ui_mitschnitt(ui),
            Tab::CanMonitor => self.ui_canmonitor(ui),
            Tab::Historie => self.ui_historie(ui),
            Tab::Sicherung => self.ui_sicherung(ui),
            Tab::Namen => self.ui_namen(ui),
            Tab::Editor => self.ui_editor(ui),
            Tab::Motor => self.ui_motor(ui),
            Tab::Rekorder => self.ui_rekorder(ui),
            Tab::Ident => self.ui_ident(ui),
            Tab::GrundLexikon => self.ui_grund_lexikon(ui),
        }
    }

    pub(super) fn ui_module(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.strong(tr!("Fehlerspeicher-Protokoll:"));
            ui.add(
                egui::TextEdit::singleline(&mut self.pdf_note)
                    .hint_text(tr!("Notiz fürs PDF, z. B. Kennzeichen, km-Stand"))
                    .desired_width(280.0),
            );
            ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(&mut self.pdf_km).hint_text(tr!("km-Stand")))
                .on_hover_text(tr!("Kilometerstand, falls ihn das Auto nicht selbst meldet"));
            if ui.add_enabled(self.pdf_job.is_none(), egui::Button::new(tr!("📄 Als PDF speichern"))).clicked() {
                self.start_pdf();
            }
            ui.checkbox(&mut self.voll_scan, tr!("Voll-Scan"))
                .on_hover_text(tr!("Alle Adressen absuchen statt nur die bekannten Steuergeräte des Fahrzeugs (findet auch neu verbaute). Ohne Haken: Schnell-Scan, sobald das Fahrzeug einmal voll gescannt wurde."));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("🔍 Scan (OBD-Bereich)")).clicked() {
                self.run_scan();
            }
            if ui.button(tr!("🔎 Auto-Scan")).on_hover_text(tr!("Alle Steuergeräte aus Labels + OBD-Bereich")).clicked() {
                self.run_autoscan();
            }
            if ui.button(tr!("🔎 Fahrzeugtyp erkennen")).on_hover_text(tr!("Spricht das Auto UDS (neuer) oder TP 2.0 (ca. 2004–2010)?")).clicked() {
                let be = self.backend();
                self.im_hintergrund(tr!("Fahrzeugtyp erkennen"), move || {
                    let uds = be.open(0x7E0, 0x7E8).and_then(|tp| Uds::new(&*tp).diagnostic_session(0x01).map(|_| ())).is_ok();
                    let tp20 = be.supports_tp20() && be.open_tp20(0x01, std::time::Duration::from_millis(800)).is_ok();
                    Box::new(move |app: &mut App| {
                        app.status = match (uds, tp20) {
                            (true, true) => tr!("Fahrzeug spricht UDS und TP 2.0 — alle Reiter nutzbar."),
                            (true, false) => tr!("Fahrzeug spricht UDS (neuere Generation)."),
                            (false, true) => tr!("Fahrzeug spricht TP 2.0 (ca. 2004–2010) — Reiter „TP 2.0“ nutzen."),
                            (false, false) => tr!("Keine CAN-Diagnose erkannt — „🔌 Verbindung“ prüfen (Zündung, Bitrate, Adapter)."),
                        };
                    })
                });
            }
            if !self.clr_confirm {
                if ui.button(tr!("✖ Alle Fehler löschen…")).on_hover_text(tr!("Fehlerspeicher aller Steuergeräte löschen")).clicked() {
                    self.clr_confirm = true;
                }
            } else {
                ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Löscht ALLE Fehler und die Readiness (AU erst nach Fahrt wieder möglich)."));
                if ui.button(tr!("Ja, alle löschen")).clicked() {
                    self.clr_confirm = false;
                    let be = self.backend();
                    let dir = self.labels_dir.clone();
                    self.im_hintergrund(tr!("Alle Fehler löschen"), move || {
                        let db = LabelDb::load_dir(&dir).unwrap_or_default();
                        let r = ecu_werkbank::report::scan(&be, &db, true, |_| {}).map(|r| {
                            let res = ecu_werkbank::repair::clear_all(&be, &r);
                            (res.iter().filter(|(_, x)| x.is_ok()).count(), res.len())
                        });
                        Box::new(move |app: &mut App| match r {
                            Ok((ok, n)) => {
                                app.status = tr!("{ok} von {a} Steuergerät(en) gelöscht.", ok = ok, a = n);
                                app.dtc_results.clear();
                            }
                            Err(e) => app.status = tr!("Löschen: {e}", e = format!("{:#}", e)),
                        })
                    });
                }
                if ui.button(tr!("Abbrechen")).clicked() {
                    self.clr_confirm = false;
                }
            }
        });
        ui.add_space(6.0);
        ui.separator();
        self.ui_fahrzeug_module(ui);
        // Steuergerät von Hand wählen (Diagnoseadresse + CAN-ID aus Labels) —
        // nur aufgeklappt, solange es noch keinen Scan gibt
        let hat_scan = self.active_vehicle.as_deref().is_some_and(|k| !ecu_werkbank::history::list(k).is_empty());
        egui::CollapsingHeader::new(tr!("Steuergerät von Hand wählen (Diagnoseadresse)"))
            .id_source("hand_auswahl")
            .default_open(!hat_scan)
            .show(ui, |ui| self.ui_hand_auswahl(ui));
    }

    /// Steuergerät über Diagnoseadresse wählen und dessen Funktionen.
    pub(super) fn ui_hand_auswahl(&mut self, ui: &mut egui::Ui) {
        // --- VCDS-artiger Ablauf (eigenes Design): Steuergerät wählen » Funktionen ---
        ui.label(tr!("Gängige Diagnoseadressen (für neuere UDS-Fahrzeuge mit Labels). Klick wählt das Steuergerät; CAN-IDs kommen aus deinem Label (falls hinterlegt)."));
        ui.add_space(6.0);

        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            egui::Grid::new("modgrid").spacing([8.0, 6.0]).show(ui, |ui| {
                for (i, (addr, name)) in COMMON_MODULES.iter().enumerate() {
                    let have_label = self.db.ecu_name(*addr).is_some();
                    let sel = self.selected_ecu == Some(*addr);
                    let text = format!("{addr:#04x}  {}{}", ecu_werkbank::i18n::t(name), if have_label { " *" } else { "" });
                    if ui.selectable_label(sel, text).clicked() {
                        self.selected_ecu = Some(*addr);
                        if let Some((tx, rx)) = self.db.ecu_can(*addr) {
                            self.dtc_tx = format!("{tx:#05x}");
                            self.dtc_rx = format!("{rx:#05x}");
                        }
                    }
                    if (i + 1) % 4 == 0 {
                        ui.end_row();
                    }
                }
            });
        });
        ui.weak(tr!("* = eigenes Label vorhanden"));

        ui.add_space(10.0);
        ui.separator();

        let addr = match self.selected_ecu {
            Some(a) => a,
            None => {
                ui.weak(tr!("Kein Steuergerät gewählt."));
                return;
            }
        };
        let name = self.db.ecu_name(addr).unwrap_or("").to_string();
        ui.heading(tr!("Funktionen — {addr} {name}", addr = format!("{:#04x}", addr), name = name));
        ui.horizontal(|ui| {
            ui.label(tr!("TX:"));
            ui.add(egui::TextEdit::singleline(&mut self.dtc_tx).desired_width(70.0));
            ui.label(tr!("RX:"));
            ui.add(egui::TextEdit::singleline(&mut self.dtc_rx).desired_width(70.0));
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("🛠 Fehlercodes lesen")).clicked() {
                self.run_dtc();
            }
            if ui.button(tr!("🧹 Fehler löschen")).clicked() {
                self.run_clear();
            }
            if ui.button(tr!("📈 Messwerte")).clicked() {
                self.selected_ecu = Some(addr);
                self.tab = Tab::Messwerte;
            }
            if ui.button(tr!("🔧 Codierung")).clicked() {
                self.code_ecu = Some(addr);
                self.code_tx = self.dtc_tx.clone();
                self.code_rx = self.dtc_rx.clone();
                self.code_tp20 = None;
                self.code_quelle = None;
                self.code_read = None;
                self.tab = Tab::Codierung;
            }
            if ui.button(tr!("🚗 TP 2.0 (ältere Fahrzeuge)")).clicked() {
                self.tp_addr = format!("{addr:#04x}");
                self.tab = Tab::Tp20;
            }
            if ui.button(tr!("⚙ Grundeinstellung/Stellglied")).clicked() {
                self.svc_tx = self.dtc_tx.clone();
                self.svc_rx = self.dtc_rx.clone();
                self.tab = Tab::Service;
            }
        });
        ui.add_space(6.0);
        ui.separator();
        if !self.dtc_info.is_empty() {
            ui.label(&self.dtc_info);
        }
        if self.dtc_results.is_empty() {
            if self.dtc_info.is_empty() {
                ui.weak(tr!("Noch keine Fehlercodes gelesen."));
            }
        } else {
            ui.strong(tr!("{a} Fehlercode(s):", a = self.dtc_results.len()));
            let model = self.vehicle_model();
            for line in self.dtc_results.clone() {
                let notes = code_in(&line).map(|c| self.repairs_for(&c)).unwrap_or_default();
                self.dtc_row(ui, &line, &model, &notes);
            }
        }
    }

    pub(super) fn ui_messwerte(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(tr!("Messwerte"));
            if let Some(a) = self.selected_ecu {
                ui.weak(tr!("(gefiltert: ECU {a})", a = format!("{:#04x}", a)));
                if ui.small_button(tr!("Filter aus")).clicked() {
                    self.selected_ecu = None;
                }
            }
        });
        ui.label(tr!("Rohbytes (hex) eingeben: Wert wird live über dein Label skaliert. Offline nutzbar."));
        ui.add_space(6.0);

        let rows = self.db.measurements();
        let rows: Vec<_> = rows
            .into_iter()
            .filter(|(e, _, _)| self.selected_ecu.is_none_or(|s| *e == s))
            .collect();

        if rows.is_empty() {
            ui.weak(tr!("Keine Messwert-Labels. Im Label-Editor anlegen (z. B. „⬇ Aus letztem Scan anlegen“) oder Namen unter „Eigene Namen“ eintragen."));
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("meas").striped(true).num_columns(5).show(ui, |ui| {
                ui.strong("ECU");
                ui.strong("DID");
                ui.strong(tr!("Name"));
                ui.strong(tr!("Rohbytes (hex)"));
                ui.strong(tr!("Wert"));
                ui.end_row();
                for (ecu, id, m) in rows {
                    ui.monospace(format!("{ecu:#04x}"));
                    ui.monospace(format!("{id:#06x}"));
                    ui.label(if m.name.is_empty() { "—" } else { &m.name });

                    let buf = self.meas_inputs.entry((ecu, id)).or_default();
                    ui.add(egui::TextEdit::singleline(buf).desired_width(120.0).hint_text("z. B. 1180"));

                    // Live-Dekodierung
                    let out = match parse_hex_bytes(buf) {
                        Ok(bytes) if !bytes.is_empty() => {
                            let scaled = ecu_werkbank::raw_to_f64(&bytes) * m.factor + m.offset;
                            let unit = if m.unit.is_empty() { String::new() } else { format!(" {}", m.unit) };
                            format!("{:.2}{}", scaled, unit)
                        }
                        Ok(_) => "—".into(),
                        Err(_) => tr!("ungültig"),
                    };
                    ui.monospace(out);
                    ui.end_row();
                }
            });
        });
    }

    /// Schlüssel der eigenen Codiertexte: TP 2.0 = 0x200 + Adresse, sonst TX.
    pub(super) fn code_text_key(&self) -> Option<u16> {
        match self.code_tp20 {
            Some(a) => Some(ecu_werkbank::codiertexte::tp20_key(a)),
            None => parse_hex(&self.code_tx),
        }
    }

    /// Codier-Helfer: Codierung lesen (Steuergerät oder letzte Sicherung),
    /// Bits ändern und selbst benennen (eigene Texte, sonst `[[coding]]`-Labels).
    /// Codierung mit Rückgängig: jeder geänderte Stand kommt auf einen Stapel.
    pub(super) fn ui_codierung(&mut self, ui: &mut egui::Ui) {
        let vorher = self.code_bytes.clone();
        let mut rueck = false;
        let mut zum_start = false;
        self.ui_codierung_seite(ui, &mut rueck, &mut zum_start);
        if rueck {
            if let Some(alt) = self.code_undo.pop() {
                self.code_bytes = alt;
            }
        } else if zum_start {
            if let Some((_, b)) = &self.code_quelle {
                let start: String = b.iter().map(|x| format!("{x:02X}")).collect();
                if start != self.code_bytes {
                    self.code_undo.push(std::mem::replace(&mut self.code_bytes, start));
                }
            }
        } else if self.code_bytes != vorher {
            // Tippen im Hex-Feld: aufeinanderfolgende Zeichen als ein Schritt
            let gleiche_laenge = vorher.len() == self.code_bytes.len();
            if gleiche_laenge || self.code_undo.last().is_none_or(|l| l.len() == vorher.len()) {
                self.code_undo.push(vorher);
            }
            if self.code_undo.len() > 100 {
                self.code_undo.remove(0);
            }
        }
    }

    fn ui_codierung_seite(&mut self, ui: &mut egui::Ui, rueck: &mut bool, zum_start: &mut bool) {
        use ecu_werkbank::codiertexte;
        ui.horizontal(|ui| {
            ui.heading(tr!("Codierung"));
            ui.add_space(12.0);
            if ui.add_enabled(!self.code_undo.is_empty(), egui::Button::new(tr!("↶ Rückgängig"))).on_hover_text(tr!("Letzte Änderung an der Codierung zurücknehmen ({n} Schritte gemerkt)", n = self.code_undo.len())).clicked() {
                *rueck = true;
            }
            if let Some((q, b)) = &self.code_quelle {
                let start: String = b.iter().map(|x| format!("{x:02X}")).collect();
                if ui.add_enabled(start != self.code_bytes, egui::Button::new(tr!("⟲ Alles zurück"))).on_hover_text(tr!("Zurück auf {a}", a = q)).clicked() {
                    *zum_start = true;
                }
            }
        });
        ui.label(tr!("Codierung lesen, Bits ändern und selbst benennen. Deine Texte gelten für das Steuergerät (CAN-ID) und bleiben gespeichert."));
        ui.add_space(4.0);

        // Steuergeräte aus der neuesten Codier-Sicherung (nur UDS mit Codierung)
        if self.code_bk.is_none() {
            // nur Sicherungen des gewählten Fahrzeugs
            let dir = self.active_vehicle.as_deref().map(ecu_werkbank::backup::backup_dir);
            self.code_bk = Some(
                ecu_werkbank::backup::list_all()
                    .into_iter()
                    .find(|(p, _)| dir.as_ref().is_some_and(|d| p.starts_with(d)))
                    .map(|(_, b)| (backup_label(&b), b.created.clone(), b.entries.into_iter().filter(|e| e.hat_codierung()).collect()))
                    .unwrap_or_default(),
            );
        }
        let mut laden: Option<ecu_werkbank::backup::CodingEntry> = None;
        let mut neu_laden = false;
        if let Some((info, created, entries)) = &self.code_bk {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("Aus der Sicherung:"));
                if entries.is_empty() {
                    ui.weak(tr!("keine (Reiter „Codier-Sicherung“ legt eine an)"));
                } else {
                    let sel = self.code_text_key();
                    let akt = entries.iter().find(|e| Some(e.text_key()) == sel).map(|e| e.title()).unwrap_or_else(|| tr!("— Steuergerät wählen —"));
                    platz(ui, 300.0);
                    egui::ComboBox::from_id_source("code_bk").selected_text(akt).width(280.0).show_ui(ui, |ui| {
                        for e in entries {
                            let n = self.code_texte.anzahl(e.text_key());
                            let txt = if n > 0 { tr!("{a}  ({n} Texte)", a = e.title(), n = n) } else { e.title() };
                            if ui.selectable_label(Some(e.text_key()) == sel, txt).clicked() {
                                laden = Some(e.clone());
                            }
                        }
                    });
                    ui.weak(info.as_str()).on_hover_text(tr!("Neueste Sicherung vom {a}", a = created));
                }
                if ui.small_button("↻").on_hover_text(tr!("Sicherungen neu laden")).clicked() {
                    neu_laden = true;
                }
            });
            if let Some(e) = &laden {
                let b = e.bytes();
                self.code_tp20 = if e.protocol == "TP 2.0" { e.address.map(|a| a as u8) } else { None };
                self.code_kurz = None;
                self.code_tx = format!("{:#05x}", e.tx);
                self.code_rx = format!("{:#05x}", e.rx);
                self.code_ecu = e.address.or_else(|| self.db.ecus_with_can().into_iter().find(|x| x.1 == e.tx).map(|x| x.0));
                self.code_bytes = b.iter().map(|x| format!("{x:02X}")).collect();
                self.code_quelle = Some((tr!("Sicherung vom {a}", a = created), b));
                self.code_read = None;
                self.code_confirm = false;
                self.code_edit = None;
            }
        }
        if neu_laden {
            self.code_bk = None;
        }

        if let Some(addr) = self.code_tp20 {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("TP 2.0 · Adresse {a}", a = format!("{addr:02X}")));
                if ui.button(tr!("⬇ Vom Steuergerät lesen")).clicked() {
                    // lang (0x0600 / Gateway 0x9A) oder kurz (Identifikation 0x9B)
                    let be = self.backend();
                    self.im_hintergrund(tr!("Codierung lesen"), move || {
                        let res = (|| -> anyhow::Result<kwp::TpCodierung> {
                            let mut ch = kwp::open_session(&be, addr)?;
                            let mut k = Kwp::new(&mut ch);
                            let raw = k.read_ecu_id(kwp::ECU_ID_VAG).unwrap_or_default();
                            kwp::codierung_lesen(&mut k, &raw).ok_or_else(|| anyhow::anyhow!(tr!("Keine Codierung lesbar")))
                        })();
                        Box::new(move |app: &mut App| match res {
                            Ok(kwp::TpCodierung::Lang(b)) => {
                                app.code_kurz = None;
                                app.code_bytes = b.iter().map(|x| format!("{x:02X}")).collect();
                                app.status = tr!("Codierung gelesen: {a}", a = ecu_werkbank::backup::hex(&b));
                                app.code_quelle = Some((tr!("Stand im Steuergerät"), b.clone()));
                                app.code_read = Some(b);
                                app.code_confirm = false;
                            }
                            Ok(kwp::TpCodierung::Kurz { wert, .. }) => {
                                app.code_kurz = Some(wert);
                                app.code_kurz_neu = format!("{wert:07}");
                                app.code_kurz_ok = false;
                                app.code_quelle = None;
                                app.code_read = None;
                                app.status = tr!("Kurzcodierung gelesen: {a}", a = format!("{wert:07}"));
                            }
                            Err(e) => app.status = tr!("Lesen: {e}", e = format!("{:#}", e)),
                        })
                    });
                }
                if ui.small_button(tr!("UDS-Steuergerät eingeben")).clicked() {
                    self.code_tp20 = None;
                    self.code_quelle = None;
                    self.code_read = None;
                }
            });
            if let Some(alt) = self.code_kurz {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(tr!("Kurzcodierung: {a}", a = format!("{alt:07}")));
                    ui.label(tr!("neu:"));
                    ui.add(egui::TextEdit::singleline(&mut self.code_kurz_neu).desired_width(90.0));
                });
                let neu = self.code_kurz_neu.trim().parse::<u32>().ok().filter(|n| *n <= 0xFF_FFFF);
                ui.checkbox(&mut self.code_kurz_ok, tr!("Alten Wert {a} notiert — neue Codierung schreiben", a = format!("{alt:07}")));
                if ui.add_enabled(self.code_kurz_ok && neu.is_some(), egui::Button::new(tr!("✍ Kurzcodierung schreiben"))).clicked() {
                    let neu = neu.expect("geprüft");
                    self.code_kurz_ok = false;
                    let be = self.backend();
                    self.im_hintergrund(tr!("Kurzcodierung schreiben"), move || {
                        let r = kwp::kurzcodierung_schreiben(&be, addr, neu);
                        Box::new(move |app: &mut App| match r {
                            Ok((vorher, danach)) if danach == neu => {
                                app.code_kurz = Some(danach);
                                app.status = tr!("Geschrieben und kontrolliert: {a} → {b}.", a = format!("{vorher:07}"), b = format!("{danach:07}"));
                            }
                            Ok((vorher, danach)) => {
                                app.code_kurz = Some(danach);
                                app.status = tr!("Achtung: danach gelesen {a}, erwartet {b}. Alter Wert war {c}.", a = format!("{danach:07}"), b = format!("{neu:07}"), c = format!("{vorher:07}"));
                            }
                            Err(e) => app.status = tr!("Schreiben: {e}", e = format!("{:#}", e)),
                        })
                    });
                }
                if neu.is_none() {
                    ui.weak(tr!("Bitte eine Zahl mit höchstens 7 Stellen eingeben."));
                }
            } else {
                ui.weak(tr!("Lange Codierung (z. B. Bordnetz, ABS, Gateway) und Kurzcodierung (Türen, Motor, Lenksäule): lesen und schreiben."));
                if !kwp::darf_senden(&self.backend(), kwp::LANGCODIERUNG_GEPRUEFT) {
                    ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Lange Codierung schreiben ist am Auto noch nicht bestätigt: es läuft erst ein Probelauf (zeigt, was gesendet würde, sendet nichts)."));
                }
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("Steuergerät TX/RX:"));
                ui.add_sized([60.0, 20.0], egui::TextEdit::singleline(&mut self.code_tx));
                ui.add_sized([60.0, 20.0], egui::TextEdit::singleline(&mut self.code_rx));
                if ui.button(tr!("⬇ Vom Steuergerät lesen")).clicked() {
                    let did = self.code_ecu.and_then(|a| self.db.coding_did(a)).unwrap_or(ecu_werkbank::backup::VAG_CODING_DID);
                    match (parse_hex(&self.code_tx), parse_hex(&self.code_rx)) {
                        (Some(tx), Some(rx)) => {
                            let be = self.backend();
                            self.im_hintergrund(tr!("Codierung lesen"), move || {
                                let res = (|| -> anyhow::Result<Vec<u8>> {
                                    let tp = be.open(tx, rx)?;
                                    let uds = Uds::new(&*tp);
                                    uds.diagnostic_session(0x01)?;
                                    uds.read_data_by_id(did)
                                })();
                                Box::new(move |app: &mut App| match res {
                                    Ok(b) => {
                                        app.code_bytes = b.iter().map(|x| format!("{x:02X}")).collect();
                                        app.status = tr!("Codierung gelesen: {a}", a = ecu_werkbank::backup::hex(&b));
                                        if app.code_ecu.is_none() {
                                            app.code_ecu = app.db.ecus_with_can().into_iter().find(|x| x.1 == tx).map(|x| x.0);
                                        }
                                        app.code_quelle = Some((tr!("Stand im Steuergerät"), b.clone()));
                                        app.code_read = Some(b);
                                        app.code_confirm = false;
                                    }
                                    Err(e) => app.status = tr!("Lesen: {e}", e = format!("{:#}", e)),
                                })
                            });
                        }
                        (None, _) => self.status = tr!("Lesen: {e}", e = tr!("TX ungültig")),
                        (_, None) => self.status = tr!("Lesen: {e}", e = tr!("RX ungültig")),
                    }
                }
            });
        }

        // Schreiben: nur gegen einen frisch gelesenen Stand (UDS oder TP 2.0 lang)
        if let (Some(old), Ok(new)) = (self.code_read.clone(), parse_hex_bytes(&self.code_bytes)) {
            if old != new {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Geändert: {a}  zu  {b}", a = ecu_werkbank::backup::hex(&old), b = ecu_werkbank::backup::hex(&new)));
                    if !self.code_confirm {
                        if ui.button(tr!("⬆ Schreiben (mit Sicherung)…")).clicked() {
                            self.code_confirm = true;
                        }
                    } else {
                        if self.code_tp20.is_some() && ui.button(tr!("Ja, schreiben")).clicked() {
                            self.code_confirm = false;
                            let addr = self.code_tp20.unwrap_or_default();
                            // alten Stand vorher in eine Codier-Sicherung
                            let sicherung = ecu_werkbank::backup::CodingBackup {
                                created: ecu_werkbank::report::now_local(),
                                timestamp: ecu_werkbank::report::unix_now(),
                                vin: None,
                                transport: self.backend().describe(),
                                vehicle: ecu_werkbank::vehicle::resolve(None, self.active_vehicle.as_deref()).map(|v| v.key).unwrap_or_default(),
                                entries: vec![ecu_werkbank::backup::CodingEntry {
                                    protocol: "TP 2.0".into(),
                                    address: Some(u16::from(addr)),
                                    name: String::new(),
                                    tx: 0,
                                    rx: 0,
                                    did: Some(kwp::CODING_DID),
                                    part: String::new(),
                                    data_hex: ecu_werkbank::backup::hex(&old),
                                    note: tr!("vor dem Schreiben der langen Codierung"),
                                }],
                            };
                            let be = self.backend();
                            let new = new.clone();
                            self.im_hintergrund(tr!("Codierung schreiben"), move || {
                                let sichern = kwp::darf_senden(&be, kwp::LANGCODIERUNG_GEPRUEFT || kwp::GATEWAY_CODIERUNG_GEPRUEFT);
                                let geschrieben = match if sichern { ecu_werkbank::backup::save(&sicherung).map(|_| ()) } else { Ok(()) } {
                                    Ok(_) => kwp::langcodierung_schreiben(&be, addr, &new),
                                    Err(e) => Err(e.context(tr!("Sicherung vorher fehlgeschlagen — nichts geschrieben"))),
                                };
                                Box::new(move |app: &mut App| {
                                    app.status = match geschrieben {
                                        Ok(kwp::Schreiben::Probelauf { anfrage, .. }) => tr!("Probelauf — nichts gesendet (noch nicht am Auto bestätigt). Würde senden: {a}", a = ecu_werkbank::backup::hex(&anfrage)),
                                        Ok(kwp::Schreiben::Geschrieben { alt, danach }) => {
                                            let t = if danach == new {
                                                tr!("Codierung geschrieben und kontrolliert. Alter Stand: {a}", a = ecu_werkbank::backup::hex(&alt))
                                            } else {
                                                tr!("Achtung: danach gelesen {a}, erwartet {b}. Alter Stand: {c}", a = ecu_werkbank::backup::hex(&danach), b = ecu_werkbank::backup::hex(&new), c = ecu_werkbank::backup::hex(&alt))
                                            };
                                            app.code_read = Some(danach.clone());
                                            app.code_quelle = Some((tr!("Stand im Steuergerät"), danach));
                                            t
                                        }
                                        Err(e) => tr!("Schreiben fehlgeschlagen: {e}", e = format!("{:#}", e)),
                                    };
                                })
                            });
                        } else if self.code_tp20.is_none() && ui.button(tr!("Ja, schreiben")).clicked() {
                            self.code_confirm = false;
                            let did = self.code_ecu.and_then(|a| self.db.coding_did(a)).unwrap_or(ecu_werkbank::backup::VAG_CODING_DID);
                            let (tx, rx) = (parse_hex(&self.code_tx).unwrap_or(0x7E0), parse_hex(&self.code_rx).unwrap_or(0x7E8));
                            let mk = |d: &[u8]| ecu_werkbank::backup::CodingEntry { protocol: "UDS".into(), address: self.code_ecu, name: String::new(), tx, rx, did: Some(did), part: String::new(), data_hex: ecu_werkbank::backup::hex(d), note: String::new() };
                            let b = ecu_werkbank::backup::CodingBackup {
                                created: ecu_werkbank::report::now_local(),
                                timestamp: ecu_werkbank::report::unix_now(),
                                vin: None,
                                transport: self.backend().describe(),
                                vehicle: ecu_werkbank::vehicle::resolve(None, self.active_vehicle.as_deref()).map(|v| v.key).unwrap_or_default(),
                                entries: vec![mk(&old)],
                            };
                            let login = ecu_werkbank::login::fuer(&b.vehicle, tx);
                            let be = self.backend();
                            let eintrag = mk(&new);
                            let new = new.clone();
                            self.im_hintergrund(tr!("Codierung schreiben"), move || {
                                let r = ecu_werkbank::backup::save(&b).and_then(|_| ecu_werkbank::backup::restore(&be, &eintrag, login.as_ref()));
                                Box::new(move |app: &mut App| {
                                    app.status = match r {
                                        Ok(_) => {
                                            app.code_read = Some(new.clone());
                                            app.code_quelle = Some((tr!("Stand im Steuergerät"), new));
                                            tr!("Codierung geschrieben und kontrolliert (alter Stand in Codier-Sicherung).")
                                        }
                                        Err(e) => tr!("Schreiben fehlgeschlagen: {e}", e = format!("{:#}", e)),
                                    };
                                })
                            });
                        }
                        if ui.button(tr!("Abbrechen")).clicked() {
                            self.code_confirm = false;
                        }
                    }
                });
            }
        }

        let ecus = self.db.ecus();
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Codierung (hex):"));
            ui.add(egui::TextEdit::singleline(&mut self.code_bytes).desired_width(260.0).hint_text("z. B. 110007"));
            if !ecus.is_empty() {
                platz(ui, 280.0);
                ui.label(tr!("Labels von:"));
                egui::ComboBox::from_id_source("code_ecu")
                    .selected_text(
                        self.code_ecu
                            .map(|a| format!("{a:#04x} {}", self.db.ecu_name(a).unwrap_or("")))
                            .unwrap_or_else(|| tr!("— keine —")),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.code_ecu, None, tr!("— keine —"));
                        for (addr, name) in &ecus {
                            ui.selectable_value(&mut self.code_ecu, Some(*addr), format!("{addr:#04x} {name}"));
                        }
                    });
            }
        });

        let mut bytes = match parse_hex_bytes(&self.code_bytes) {
            Ok(b) if !b.is_empty() => b,
            Ok(_) => {
                ui.add_space(8.0);
                ui.weak(tr!("Steuergerät aus der Sicherung wählen, vom Steuergerät lesen oder die Codierung oben eintragen."));
                return;
            }
            Err(_) => {
                ui.add_space(8.0);
                ui.weak(tr!("Ungültige Hex-Bytes."));
                return;
            }
        };
        let tx = self.code_text_key();
        let tx_key = tx.unwrap_or(u16::MAX);
        let labels: Vec<ecu_werkbank::labels::Coding> = self.code_ecu.and_then(|a| self.db.coding(a)).map(|c| c.to_vec()).unwrap_or_default();
        // Namen vorab (eigener Text, sonst Label): je Byte (Byte-Text, 8 Bit-Texte)
        let namen: Vec<(Option<String>, Vec<Option<String>>)> = (0..bytes.len())
            .map(|i| (self.code_texte.name(tx_key, &labels, i, None), (0..8u8).map(|b| self.code_texte.name(tx_key, &labels, i, Some(b))).collect()))
            .collect();
        let name = |i: usize, bit: Option<u8>| -> Option<String> {
            namen.get(i).and_then(|(b, bits)| match bit {
                None => b.clone(),
                Some(x) => bits[x as usize].clone(),
            })
        };
        let alt = self.code_quelle.as_ref().map(|(_, b)| b.clone());
        let mut edit_req: Option<(usize, Option<u8>)> = None;
        let kann_texte = tx.is_some();

        // Was ist anders als der Ausgangsstand?
        if let (Some((q, _)), Some(alt)) = (&self.code_quelle, &alt) {
            let diff = codiertexte::unterschiede(alt, &bytes);
            if !diff.is_empty() {
                ui.add_space(6.0);
                ui.strong(tr!("Geändert gegenüber {a}:", a = q));
                for (i, bit, a, b) in &diff {
                    ui.horizontal(|ui| {
                        let ab = |x: bool| if x { tr!("ein") } else { tr!("aus") };
                        ui.monospace(tr!("Byte {i} Bit {bit}: {a} → {b}", i = i, bit = bit, a = ab(*a), b = ab(*b)));
                        match name(*i, Some(*bit)) {
                            Some(n) => ui.label(n),
                            None => ui.weak(tr!("noch ohne Namen")),
                        };
                        if kann_texte && ui.small_button("✏").on_hover_text(tr!("Benennen")).clicked() {
                            edit_req = Some((*i, Some(*bit)));
                        }
                    });
                }
                ui.weak(tr!("Tipp: Nach dem Schreiben die Wirkung am Auto ansehen und dem Bit gleich einen Namen geben."));
                if self.code_read.is_none() && self.code_tp20.is_none() {
                    ui.weak(tr!("Zum Schreiben erst „Vom Steuergerät lesen“ — geschrieben wird nur gegen den aktuellen Stand."));
                }
            }
        }

        // Abhängigkeiten aus den Labels (benoetigt / schliesst_aus)
        let regeln = codiertexte::regeln_pruefen(&labels, &bytes);
        if !regeln.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(stufe_color(Stufe::Warnung), tr!("⚠ Passt nicht zusammen (laut deinen Labels):"));
            for h in &regeln {
                ui.colored_label(stufe_color(Stufe::Warnung), format!("   {h}"));
            }
        }

        // Text-Editor
        let mut speichern: Option<Option<(usize, Option<u8>, String)>> = None; // Some(None) = schließen
        if let (Some((byte, bit, text)), Some(tx)) = (&mut self.code_edit, tx) {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let wo = match bit {
                    Some(b) => tr!("Byte {a} Bit {b}", a = *byte, b = *b),
                    None => tr!("Byte {a}", a = *byte),
                };
                ui.label(tr!("{a}:", a = wo));
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(280.0).hint_text(tr!("z. B. Tagfahrlicht")));
                if ui.button(tr!("💾 Speichern")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    speichern = Some(Some((*byte, *bit, text.clone())));
                }
                if ui.button(tr!("Abbrechen")).clicked() {
                    speichern = Some(None);
                }
            });
            ui.weak(tr!("Leer speichern löscht den Text. Gilt für Steuergerät {a}.", a = format!("{tx:03X}")));
        }
        if !kann_texte {
            ui.weak(tr!("Für eigene Texte oben die CAN-ID (TX) des Steuergeräts eintragen."));
        }

        ui.add_space(4.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong(tr!("Bits"));
            ui.checkbox(&mut self.code_nur_benannt, tr!("nur benannte zeigen"));
        });
        let nur_benannt = self.code_nur_benannt;
        let mut changed = false;
        egui::ScrollArea::vertical().id_source("code_bits").auto_shrink([false, true]).show(ui, |ui| {
            for i in 0..bytes.len() {
                let benannt = (0..8u8).filter(|b| name(i, Some(*b)).is_some()).count();
                let byte_name = name(i, None);
                if nur_benannt && benannt == 0 && byte_name.is_none() {
                    continue;
                }
                let v = bytes[i];
                let mut titel = format!("Byte {i:02} = {v:02X}  ({})", codiertexte::bits(v));
                if let Some(n) = &byte_name {
                    titel += &format!("   {n}");
                }
                if benannt > 0 {
                    titel += &tr!("   · {a} benannt", a = benannt);
                }
                egui::CollapsingHeader::new(egui::RichText::new(titel).monospace())
                    .id_source(("code_byte", i))
                    .default_open(bytes.len() <= 4 || benannt > 0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.weak(tr!("Ganzes Byte:"));
                            match &byte_name {
                                Some(n) => ui.label(n),
                                None => ui.weak("—"),
                            };
                            if kann_texte && ui.small_button("✏").on_hover_text(tr!("Text zum Byte")).clicked() {
                                edit_req = Some((i, None));
                            }
                        });
                        for bit in 0..8u8 {
                            let n = name(i, Some(bit));
                            if nur_benannt && n.is_none() {
                                continue;
                            }
                            let mut on = (bytes[i] >> bit) & 1 == 1;
                            let geaendert = alt.as_ref().is_some_and(|a| ((a.get(i).copied().unwrap_or(0) >> bit) & 1 == 1) != on);
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut on, format!("Bit {bit}")).changed() && ecu_werkbank::set_coding_bit(&mut bytes, i, bit, on) {
                                    changed = true;
                                }
                                let txt = n.clone().unwrap_or_else(|| "—".into());
                                if geaendert {
                                    ui.colored_label(stufe_color(Stufe::Hinweis), txt).on_hover_text(tr!("geändert"));
                                } else if n.is_some() {
                                    ui.label(txt);
                                } else {
                                    ui.weak(txt);
                                }
                                if kann_texte && ui.small_button("✏").on_hover_text(tr!("Benennen")).clicked() {
                                    edit_req = Some((i, Some(bit)));
                                }
                            });
                        }
                    });
            }
        });
        if changed {
            self.code_bytes = bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join("");
            self.code_confirm = false;
        }

        if let Some((i, bit)) = edit_req {
            let jetzt = self.code_texte.eigen(tx_key, i, bit).unwrap_or_default().to_string();
            self.code_edit = Some((i, bit, jetzt));
        }
        match speichern {
            Some(None) => self.code_edit = None,
            Some(Some((byte, bit, text))) => {
                self.code_edit = None;
                self.status = match codiertexte::setzen(tx_key, byte, bit, &text) {
                    Ok(()) => {
                        self.code_texte = codiertexte::Texte::laden();
                        tr!("Text gespeichert.")
                    }
                    Err(e) => tr!("Speichern: {e}", e = format!("{:#}", e)),
                };
            }
            None => {}
        }
    }

    /// (3) Service: Grundeinstellung (RoutineControl) + Stellglied (IOControl).
    pub(super) fn ui_service(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Experten: Routine & Stellglied (UDS)"));
        ui.label(tr!("Sendet echte UDS-Dienste; braucht Adapter + Fahrzeug. IDs/Parameter sind ECU-spezifisch."));
        ui.weak(tr!("Vor dem Senden kommt immer eine Rückfrage. Nur am stehenden Auto, Zündung an, Motor aus (außer die Routine verlangt etwas anderes)."));
        ui.horizontal(|ui| {
            ui.label(tr!("TX:"));
            ui.add(egui::TextEdit::singleline(&mut self.svc_tx).desired_width(70.0));
            ui.label(tr!("RX:"));
            ui.add(egui::TextEdit::singleline(&mut self.svc_rx).desired_width(70.0));
        });
        ui.add_space(8.0);
        ui.separator();
        ui.strong("Grundeinstellung / Routine (0x31)");
        ui.horizontal(|ui| {
            ui.label(tr!("Routine-ID:"));
            ui.add(egui::TextEdit::singleline(&mut self.routine_id).desired_width(80.0));
            ui.label(tr!("Sub:"));
            ui.add(egui::TextEdit::singleline(&mut self.routine_sub).desired_width(40.0))
                .on_hover_text(tr!("1 = starten, 2 = stoppen, 3 = Ergebnis abfragen (hex oder dezimal)"));
            ui.label(tr!("Daten(hex):"));
            ui.add(egui::TextEdit::singleline(&mut self.routine_data).desired_width(120.0));
            if ui.add_enabled(self.svc_plan.is_none(), egui::Button::new(tr!("Routine senden…"))).clicked() {
                match self.svc_plan_bauen(true) {
                    Ok(p) => self.svc_plan = Some(p),
                    Err(e) => self.svc_result = tr!("Fehler: {e}", e = format!("{e:#}")),
                }
            }
        });
        ui.add_space(8.0);
        ui.separator();
        ui.strong("Stellglieddiagnose / IO-Control (0x2F)");
        ui.horizontal(|ui| {
            ui.label(tr!("DID:"));
            ui.add(egui::TextEdit::singleline(&mut self.io_did).desired_width(80.0));
            ui.label(tr!("Control:"));
            ui.add(egui::TextEdit::singleline(&mut self.io_control).desired_width(60.0));
            ui.label(tr!("State(hex):"));
            ui.add(egui::TextEdit::singleline(&mut self.io_state).desired_width(100.0));
            if ui.add_enabled(self.svc_plan.is_none(), egui::Button::new(tr!("Stellglied ansteuern…"))).clicked() {
                match self.svc_plan_bauen(false) {
                    Ok(p) => self.svc_plan = Some(p),
                    Err(e) => self.svc_result = tr!("Fehler: {e}", e = format!("{e:#}")),
                }
            }
        });
        // Rückfrage: erst nach „Ja, senden“ geht etwas ans Auto
        if let Some(plan) = self.svc_plan.clone() {
            ui.add_space(8.0);
            let mut antwort = None;
            egui::Frame::group(ui.style()).stroke(egui::Stroke::new(1.5_f32, stufe_color(Stufe::Warnung))).show(ui, |ui| {
                ui.colored_label(stufe_color(Stufe::Warnung), tr!("⚠ Wirklich ans Auto senden?"));
                ui.label(plan.text());
                ui.weak(tr!("Das Steuergerät führt den Befehl sofort aus. Falsche IDs können Bauteile bewegen oder Werte verstellen."));
                ui.horizontal(|ui| {
                    if ui.button(tr!("✅ Ja, senden")).clicked() {
                        antwort = Some(true);
                    }
                    if ui.button(tr!("Abbrechen")).clicked() {
                        antwort = Some(false);
                    }
                });
            });
            match antwort {
                Some(true) => {
                    self.svc_plan = None;
                    self.svc_senden(plan);
                }
                Some(false) => {
                    self.svc_plan = None;
                    self.svc_result = tr!("Abgebrochen — nichts gesendet.");
                }
                None => {}
            }
        }
        if !self.svc_result.is_empty() {
            ui.add_space(8.0);
            ui.monospace(&self.svc_result);
        }
    }

    /// Schnell-Eingabe für eigene Namen von Messwertblöcken/Feldern.
    /// Gespeichert in messwerttexte.toml; gilt danach überall (Live-Werte,
    /// Testlauf, Rekorder, Kurven). Dein eigener Inhalt, kein fremder.
    pub(super) fn ui_namen(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Eigene Namen"));
        ui.label(tr!("Gib Messwertblöcken und einzelnen Feldern eigene Namen. Sie gelten danach überall — Live-Werte, Testlauf, Rekorder und Kurven — und bleiben gespeichert."));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(tr!("Steuergerät (Adresse, z. B. 0x09):"));
            ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut self.qn_addr));
            ui.label(tr!("Block:"));
            ui.add_sized([55.0, 20.0], egui::TextEdit::singleline(&mut self.qn_block).hint_text("1"));
            ui.label(tr!("Feld (leer = Blockname):"));
            ui.add_sized([55.0, 20.0], egui::TextEdit::singleline(&mut self.qn_feld).hint_text("1–4"));
        });
        ui.horizontal(|ui| {
            ui.label(tr!("Name:"));
            ui.add_sized([300.0, 20.0], egui::TextEdit::singleline(&mut self.qn_name).hint_text(tr!("z. B. Kühlmitteltemperatur")));
            if ui.button(tr!("+ Speichern")).clicked() {
                self.namen_speichern();
            }
        });
        ui.weak(tr!("Adresse 2-stellig hex (TP 2.0 / KWP-Block-Schema). Feld 1–4; leer = Name des ganzen Blocks. Tipp: die Namen zeigt dir ein Werkstatt-Diagnoseprogramm."));

        ui.add_space(10.0);
        ui.separator();
        ui.strong(tr!("📋 Ganzes Steuergerät benennen"));
        ui.label(tr!("Liest alle Messwertblöcke eines Steuergeräts (nur lesen) und zeigt jedes Feld mit seinem aktuellen Wert — du trägst nur noch die Namen ein. Geht bei älteren Steuergeräten (TP 2.0)."));
        self.bs_poll(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Steuergerät (TP-2.0-Adresse):"));
            ui.add_sized([70.0, 20.0], egui::TextEdit::singleline(&mut self.bn_addr).hint_text("0x01"));
            let addr = parse_hex(&self.bn_addr).and_then(|a| u8::try_from(a).ok());
            let busy = self.bs_job.is_some();
            let ok = self.backend().supports_tp20();
            if ui
                .add_enabled(!busy && ok && addr.is_some(), egui::Button::new(tr!("🔍 Alle Blöcke lesen")))
                .on_disabled_hover_text(tr!("Geht nur über TP 2.0 (CAN-Interface oder Demo)."))
                .clicked()
            {
                self.bs_start(addr.expect("geprüft"));
            }
            if busy {
                ui.add(egui::ProgressBar::new(f32::from(self.bs_at) / 255.0).desired_width(160.0).text(tr!("Block {a}", a = self.bs_at)));
            }
        });
        ui.weak(tr!("Adressen am Golf 6: 01 Motor, 03 ABS, 09 Lenkung, 1D Einparkhilfe, 20 Bordnetz, 22–25 Türen, 2A Lenksäule, 43 Anhänger."));
        if let Some((addr, bloecke)) = self.bs_result.clone() {
            if !bloecke.is_empty() && self.bs_job.is_none() {
                ui.add_space(4.0);
                self.bs_namen_ui(ui, addr, &bloecke);
            }
        }

        ui.add_space(10.0);
        ui.separator();
        let eigene = ecu_werkbank::messwerttexte::alle();
        if eigene.is_empty() {
            ui.weak(tr!("Noch keine eigenen Namen gespeichert."));
        } else {
            ui.strong(tr!("Gespeicherte Namen ({a}):", a = eigene.len()));
            let mut loeschen: Option<String> = None;
            egui::ScrollArea::vertical().id_source("qn_tab").max_height(300.0).show(ui, |ui| {
                egui::Grid::new("qn").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                    ui.strong(tr!("Adr-Block-Feld"));
                    ui.strong(tr!("Name"));
                    ui.label("");
                    ui.end_row();
                    for (k, v) in &eigene {
                        ui.monospace(k);
                        ui.label(v);
                        if ui.small_button("🗑").clicked() {
                            loeschen = Some(k.clone());
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some(k) = loeschen {
                if let Some((a, b, f)) = Self::namen_schluessel(&k) {
                    let _ = ecu_werkbank::messwerttexte::setzen(a, b, f, "");
                    self.mw_texte = ecu_werkbank::messwerttexte::Texte::laden();
                    self.status = tr!("Name gelöscht: {a}", a = k);
                }
            }
        }

        ui.add_space(12.0);
        ui.separator();
        let (e, d, m, c) = self.db.len();
        ui.weak(tr!("Geladene Labels: {e} Steuergerät(e), {d} DTC-Text(e), {m} Messwert(e), {c} Anpassung/Codierung.", e = e, d = d, m = m, c = c));
        if ui.small_button(tr!("↻ Labels neu laden")).clicked() {
            self.reload_labels();
            self.status = tr!("Labels neu geladen.");
        }
    }

    /// Adresse (u8, hex), Block (u8) und optionales Feld aus der Eingabe lesen.
    pub(super) fn namen_speichern(&mut self) {
        let Some(a) = parse_hex(&self.qn_addr).filter(|a| *a <= 0xFF).map(|a| a as u8) else {
            self.status = tr!("Adresse nicht verstanden — 2-stellig hex, z. B. 0x09.");
            return;
        };
        let Ok(b) = self.qn_block.trim().parse::<u8>() else {
            self.status = tr!("Block nicht verstanden — eine Zahl 1–255.");
            return;
        };
        let feld = if self.qn_feld.trim().is_empty() {
            None
        } else {
            match self.qn_feld.trim().parse::<u8>() {
                Ok(f) if f >= 1 => Some(f),
                _ => {
                    self.status = tr!("Feld nicht verstanden — leer lassen oder 1–4.");
                    return;
                }
            }
        };
        let name = self.qn_name.trim();
        if name.is_empty() {
            self.status = tr!("Bitte einen Namen eingeben.");
            return;
        }
        self.status = match ecu_werkbank::messwerttexte::setzen(a, b, feld, name) {
            Ok(()) => {
                self.qn_name.clear();
                self.mw_texte = ecu_werkbank::messwerttexte::Texte::laden();
                tr!("Name gespeichert.")
            }
            Err(e) => format!("{e:#}"),
        };
    }

    /// Schlüssel „09-001" / „09-001-2" zurück in (Adresse, Block, Feld).
    pub(super) fn namen_schluessel(k: &str) -> Option<(u8, u8, Option<u8>)> {
        let mut it = k.split('-');
        let a = u8::from_str_radix(it.next()?, 16).ok()?;
        let b = it.next()?.parse::<u8>().ok()?;
        let f = match it.next() {
            Some(s) => Some(s.parse::<u8>().ok()?),
            None => None,
        };
        Some((a, b, f))
    }
}
