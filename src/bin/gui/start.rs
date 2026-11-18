//! GUI: Startseite

use super::*;

impl App {
    /// Startseite: Fahrzeug-Übersicht + Kacheln für die häufigsten Aufgaben.
    /// „Fahrzeug auf einen Blick“: Kennzahlen, Steuergeräte mit Fehlern.
    /// Liefert den Index eines angeklickten Steuergeräts (im letzten Scan) und
    /// den Namen des Wartungspostens, der in der Karte steht.
    pub(super) fn fahrzeug_blick(&self, ui: &mut egui::Ui, v: &ecu_werkbank::vehicle::Vehicle) -> (Option<usize>, Option<String>) {
        use ecu_werkbank::blick::vor_text;
        use ecu_werkbank::report::group_thousands;
        let b = ecu_werkbank::blick::blick(v);
        ui.add_space(8.0);
        // ---- Kennzahlen ----
        let karte = |ui: &mut egui::Ui, st: Option<Stufe>, titel: &str, wert: String, unten: String| {
            platz(ui, 215.0);
            egui::Frame::group(ui.style()).inner_margin(egui::Margin::symmetric(12.0, 8.0)).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    ui.set_min_height(70.0);
                    ui.horizontal(|ui| {
                        if let Some(st) = st {
                            dot(ui, stufe_color(st));
                        }
                        ui.weak(ecu_werkbank::i18n::t(titel));
                    });
                    ui.add(egui::Label::new(egui::RichText::new(wert).size(19.0).strong()).truncate());
                    if !unten.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(unten).small().weak()).wrap());
                    }
                });
            });
        };
        ui.horizontal_wrapped(|ui| {
            match &b.km {
                Some((k, d)) => karte(
                    ui,
                    None,
                    trn!("Kilometerstand"),
                    format!("{} km", group_thousands(*k)),
                    [if d.is_empty() { String::new() } else { tr!("Stand {d}", d = d) }, b.km_pro_jahr.map(|j| tr!("≈ {a} km/Jahr", a = group_thousands(j))).unwrap_or_default()]
                        .into_iter()
                        .filter(|x| !x.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · "),
                ),
                None => karte(ui, Some(Stufe::Hinweis), trn!("Kilometerstand"), "—".into(), tr!("unter 🚘 Fahrzeuge eintragen")),
            }
            match b.fehler {
                Some(n) => {
                    let st = if n == 0 { Stufe::Gut } else if b.module_mit_fehlern.iter().any(|m| m.3 == Stufe::Warnung) { Stufe::Warnung } else { Stufe::Hinweis };
                    let vergleich = match b.fehler_vorher {
                        Some(p) if p < n => tr!("{a} mehr als beim Scan davor", a = n - p),
                        Some(p) if p > n => tr!("{a} weniger als beim Scan davor", a = p - n),
                        Some(_) => tr!("wie beim Scan davor"),
                        None => tr!("erster Scan"),
                    };
                    karte(ui, Some(st), trn!("Fehler gespeichert"), n.to_string(), vergleich);
                }
                None => karte(ui, Some(Stufe::Hinweis), trn!("Fehler gespeichert"), "—".into(), tr!("noch kein Scan")),
            }
            match &b.letzter_scan {
                Some((d, tage)) => karte(
                    ui,
                    Some(if *tage > 180 { Stufe::Hinweis } else { Stufe::Gut }),
                    trn!("Letzter Scan"),
                    vor_text(*tage),
                    tr!("{d} · {a} Steuergeräte", d = d, a = b.steuergeraete.unwrap_or(0)),
                ),
                None => karte(ui, Some(Stufe::Hinweis), trn!("Letzter Scan"), "—".into(), tr!("„Fehlerspeicher-PDF“ oder Schnell-Check starten")),
            }
            match &b.wartung {
                Some((st, name, text)) => karte(ui, Some(*st), trn!("Nächste Wartung"), name.clone(), text.clone()),
                None => karte(ui, None, trn!("Nächste Wartung"), "—".into(), tr!("keine Posten angelegt")),
            }
        });
        // ---- Steuergeräte mit Fehlern ----
        let mut ziel = None;
        if !b.module_mit_fehlern.is_empty() {
            ui.add_space(4.0);
            ui.weak(tr!("Steuergeräte mit Fehlern (letzter Scan) — anklicken zum Lesen oder Löschen:"));
            ui.horizontal_wrapped(|ui| {
                for (i, titel, n, st) in &b.module_mit_fehlern {
                    let t = egui::RichText::new(format!("● {titel}  ({n})")).color(stufe_color(*st));
                    if ui.button(t).clicked() {
                        ziel = Some(*i);
                    }
                }
            });
        }
        (ziel, b.wartung.map(|w| w.1))
    }

    pub(super) fn ui_start(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_source("startscroll").show(ui, |ui| {
            // ---- Fahrzeug ----
            let veh = self.active_vehicle.as_deref().and_then(ecu_werkbank::vehicle::load);
            match &veh {
                Some(v) => {
                    ui.label(egui::RichText::new(v.label()).size(24.0).strong());
                    if !v.vin.is_empty() && v.label() != v.vin {
                        ui.weak(format!("VIN {}", v.vin));
                    }
                    let (ziel, wartung_karte) = self.fahrzeug_blick(ui, v);
                    if let Some(ziel) = ziel {
                        self.tab = Tab::Steuergeraete;
                        self.fz_sel = Some(ziel);
                        self.fz_lines.clear();
                        self.fz_info.clear();
                    }
                    // der dringendste Posten steht schon in der Karte „Nächste Wartung“
                    let mut hints: Vec<(Stufe, String)> = v
                        .wartung_offen()
                        .into_iter()
                        .filter(|(w, _)| wartung_karte.as_deref() != Some(w.name.as_str()))
                        .map(|(w, f)| (f.stufe, tr!("Wartung {a}: {b}", a = w.name, b = f.text)))
                        .collect();
                    hints.extend(ecu_werkbank::trend::analyze(&ecu_werkbank::trend::series(&v.key)));
                    hints.sort_by_key(|h| std::cmp::Reverse(h.0));
                    for (st, t) in hints.iter().take(5) {
                        ui.horizontal_wrapped(|ui| {
                            dot(ui, stufe_color(*st));
                            ui.label(t);
                        });
                    }
                    if let Some(u) = ecu_werkbank::history::uebersicht(&v.key) {
                        fehler_uebersicht(ui, &u);
                    }
                }
                None => {
                    ui.label(egui::RichText::new(tr!("Kein Fahrzeug gewählt")).size(22.0).strong());
                    ui.weak(tr!("Oben bei 🚘 ein Fahrzeug wählen oder anlegen — oder einfach scannen: mit lesbarer Fahrgestellnummer legt ECUWerkbank das Fahrzeug selbst an."));
                }
            }
            if let Backend::SocketCan { iface } = self.backend() {
                if !std::path::Path::new(&format!("/sys/class/net/{iface}")).exists() {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        dot(ui, stufe_color(Stufe::Hinweis));
                        ui.label(tr!("Kein CAN-Adapter ('{iface}') gefunden. Adapter anstecken — oder oben „🚗 Demo-Fahrzeug“ wählen, um alles auszuprobieren.", iface = iface));
                    });
                }
            }

            ui.add_space(14.0);
            ui.label(egui::RichText::new(tr!("Was möchtest du tun?")).size(17.0).strong());
            ui.add_space(6.0);

            let tiles: [(&str, &str, u8); 12] = [
                (trn!("✅ Schnell-Check"), trn!("Alles prüfen, Ampel + PDF"), 0),
                (trn!("📄 Fehlerspeicher-PDF"), trn!("Alle Steuergeräte auslesen"), 1),
                (trn!("🔧 Steuergeräte"), trn!("Fehler je Steuergerät lesen/löschen"), 9),
                (trn!("🔌 Verbindung prüfen"), trn!("Adapter, Bitrate, Steuergeräte"), 2),
                (trn!("🔎 Fehlersuche"), trn!("Prüfplan zu einem Fehlercode"), 3),
                (trn!("📈 Live-Werte"), trn!("Grafik, Armaturen, Handy"), 4),
                (trn!("🔋 Batterietest"), trn!("Ruhe-, Start-, Ladespannung"), 5),
                (trn!("⚙ Gemisch-Check"), trn!("Falschluft, Kraftstoffdruck …"), 6),
                (trn!("🔍 Gebrauchtwagen-Check"), trn!("Vor dem Kauf prüfen"), 7),
                (trn!("📂 Fahrzeugakte"), trn!("Verlauf, Wartung, Reparaturen"), 8),
                (trn!("▶ Testlauf"), trn!("Alles lesen + Protokoll"), 10),
                (trn!("🔍 Fehlerbilder"), trn!("Eigene Erfahrungen + Texte"), 11),
            ];
            // Spaltenzahl nach Fensterbreite: 1 bis 4 Kacheln je Zeile
            let breite = ui.available_width();
            let spalten = (((breite + 12.0) / (190.0 + 12.0)).floor() as usize).clamp(1, 4);
            let w = ((breite - 12.0 * (spalten - 1) as f32) / spalten as f32).min(280.0);
            let mut clicked = None;
            egui::Grid::new("tiles").spacing([12.0, 12.0]).show(ui, |ui| {
                for (i, (title, sub, id)) in tiles.iter().enumerate() {
                    let text = egui::RichText::new(format!("{}\n{}", ecu_werkbank::i18n::t(title), ecu_werkbank::i18n::t(sub))).size(14.0);
                    if ui.add_sized([w, 64.0], egui::Button::new(text).wrap()).clicked() {
                        clicked = Some(*id);
                    }
                    if (i + 1) % spalten == 0 {
                        ui.end_row();
                    }
                }
            });
            match clicked {
                Some(0) => self.start_check(),
                Some(1) => self.start_pdf(),
                Some(2) => self.verbindung_pruefen(),
                Some(3) => self.tab = Tab::Diagnose,
                Some(9) => self.tab = Tab::Steuergeraete,
                Some(10) => self.tab = Tab::Testlauf,
                Some(11) => self.tab = Tab::Wissen,
                Some(4) => self.tab = Tab::Scope,
                Some(5) => self.tab = Tab::Batterie,
                Some(6) => {
                    self.tab = Tab::Diagnose;
                    self.status = tr!("Gemisch-Assistent: unten im Reiter „Prüfen & Fehlersuche“.");
                }
                Some(7) => {
                    self.tab = Tab::Diagnose;
                    self.start_kaufcheck();
                }
                Some(8) => match veh {
                    Some(v) => {
                        let res = v.sub("berichte").and_then(|d| {
                            let p = d.join(ecu_werkbank::vehicle::stamped("fahrzeugakte", "pdf"));
                            std::fs::write(&p, ecu_werkbank::akte::to_pdf(&v, &self.db))?;
                            Ok(p)
                        });
                        match res {
                            Ok(p) => {
                                open_path(&p, &mut self.status);
                                self.pdf_last = Some(p);
                            }
                            Err(e) => self.status = format!("{e:#}"),
                        }
                    }
                    None => self.tab = Tab::Fahrzeuge,
                },
                _ => {}
            }
            if self.chk_job.is_some() || self.pdf_job.is_some() || self.kc_job.is_some() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr!("läuft … {a}{b}{c}", a = self.chk_progress, b = self.pdf_progress, c = self.kc_progress));
                });
            }
        });
    }
}
