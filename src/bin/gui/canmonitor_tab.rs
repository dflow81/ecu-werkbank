//! GUI: Live-CAN-Monitor (Sniffer). Logik in ecu_werkbank::canmonitor.
//! Nur zuhören — sendet nie etwas auf den Bus.

use super::*;
use ecu_werkbank::canmonitor::{Lauf, Monitor, Quelle};

/// Zustand der Seite.
#[derive(Default)]
pub(super) struct CanMonitorSeite {
    lauf: Option<Lauf>,
    filter: String,
    nur_geaenderte: bool,
    gewaehlt: Option<u32>,
    dbc: Option<(String, ecu_werkbank::dbc::Dbc)>,
    /// schon einmal versucht, die CAN-Botschaften aus der Fahrzeug-Datenbank zu holen
    db_versucht: bool,
}

/// Wie lange ein geändertes Byte hervorgehoben bleibt (s).
const FRISCH: f64 = 1.0;

impl App {
    pub(super) fn ui_canmonitor(&mut self, ui: &mut egui::Ui) {
        // Beim ersten Öffnen ohne eigene DBC: Botschaften aus der Fahrzeug-Datenbank nehmen
        if self.cm.dbc.is_none() && !self.cm.db_versucht {
            self.cm_dbc_aus_datenbank(false);
        }
        ui.heading(tr!("CAN-Monitor"));
        ui.label(tr!("Zeigt live alle Botschaften auf dem Bus: je CAN-ID die Daten, wie oft sie kommt und welche Bits sich ändern. Nur zuhören — es wird nichts gesendet."));
        ui.weak(tr!("Signal finden: „Rauschen merken“ drücken, dann am Auto etwas betätigen (Blinker, Fenster, Licht). Hervorgehoben werden dann nur noch die Bits, die sich dadurch ändern."));
        ui.add_space(4.0);

        let quelle = match self.backend() {
            Backend::SocketCan { iface } => Some(Quelle::SocketCan(iface)),
            Backend::Demo => Some(Quelle::Demo),
            _ => None,
        };
        let laeuft = self.cm.lauf.is_some();
        ui.horizontal_wrapped(|ui| {
            if !laeuft {
                let knopf = ui.add_enabled(quelle.is_some(), egui::Button::new(tr!("▶ Mithören starten")));
                let knopf = knopf.on_disabled_hover_text(tr!("Geht nur mit SocketCAN-Adapter oder im Demo-Fahrzeug — ein ELM327 kann den Bus nicht mithören."));
                if knopf.clicked() {
                    if let Some(q) = quelle.clone() {
                        match Lauf::starten(q) {
                            Ok(l) => {
                                self.cm.lauf = Some(l);
                                self.status = tr!("CAN-Monitor läuft (nur zuhören).");
                            }
                            Err(e) => self.status = tr!("CAN-Monitor: {e}", e = format!("{e:#}")),
                        }
                    }
                }
            } else if ui.button(tr!("⏹ Stoppen")).clicked() {
                self.cm_mitschnitt_ende();
                self.cm.lauf = None;
                self.status = tr!("CAN-Monitor gestoppt.");
            }
            if let Some(l) = &self.cm.lauf {
                let gemerkt = l.monitor.lock().map(|m| m.gemerkt.is_some()).unwrap_or(false);
                if ui.button(tr!("🔇 Rauschen merken")).on_hover_text(tr!("Alles, was sich bis jetzt von selbst geändert hat (Zähler, Prüfsummen), wird ab jetzt nicht mehr hervorgehoben.")).clicked() {
                    if let Ok(mut m) = l.monitor.lock() {
                        m.rauschen_merken();
                    }
                }
                if gemerkt && ui.button(tr!("Rauschen vergessen")).clicked() {
                    if let Ok(mut m) = l.monitor.lock() {
                        m.rauschen_vergessen();
                    }
                }
                if ui.button(tr!("🧹 Leeren")).clicked() {
                    if let Ok(mut m) = l.monitor.lock() {
                        *m = Monitor::default();
                    }
                    self.cm.gewaehlt = None;
                }
            }
        });

        // Mitschnitt, DBC, Demo-Blinker
        ui.horizontal_wrapped(|ui| {
            if let Some(l) = &self.cm.lauf {
                match l.schneidet_mit() {
                    None => {
                        if ui.button(tr!("⏺ Mitschnitt speichern")).on_hover_text(tr!("Schreibt alle Frames als candump-Datei in den Fahrzeugordner (aufzeichnungen).")).clicked() {
                            match self.vehicle_sub("aufzeichnungen") {
                                Some(dir) => {
                                    let p = dir.join(ecu_werkbank::vehicle::stamped("can-monitor", "log"));
                                    self.status = match l.mitschnitt_start(p.clone()) {
                                        Ok(()) => tr!("Mitschnitt läuft: {a}", a = p.display()),
                                        Err(e) => tr!("Mitschnitt: {e}", e = format!("{e:#}")),
                                    };
                                }
                                None => self.status = tr!("Kein Ordner für die Aufzeichnung verfügbar."),
                            }
                        }
                    }
                    Some(n) => {
                        if ui.button(tr!("⏹ Mitschnitt beenden ({n} Frames)", n = n)).clicked() {
                            if let Some((p, n)) = l.mitschnitt_ende() {
                                self.status = tr!("Mitschnitt gespeichert: {n} Frames in {a}", n = n, a = p.display());
                            }
                        }
                    }
                }
                if matches!(quelle, Some(Quelle::Demo)) {
                    let mut an = l.demo_aktion.load(std::sync::atomic::Ordering::Relaxed);
                    if ui.checkbox(&mut an, tr!("Demo: Blinker einschalten")).changed() {
                        l.demo_aktion.store(an, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
            platz(ui, 240.0);
            if ui.button(tr!("📂 DBC-Datei laden…")).on_hover_text(tr!("Eigene DBC-Datei: zeigt Botschaftsnamen und entschlüsselte Signale.")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("DBC", &["dbc"]).set_title(tr!("DBC-Datei wählen")).pick_file() {
                    self.cm_dbc_laden(&p);
                }
            }
            if ui.button(tr!("🗄 Aus der Datenbank")).on_hover_text(tr!("CAN-Botschaften und Signale aus der Fahrzeug-Datenbank (importierte DBC-Dateien und Datenpakete)")).clicked() {
                self.cm_dbc_aus_datenbank(true);
            }
            if let Some((name, _)) = &self.cm.dbc {
                ui.weak(name);
                if ui.small_button("✖").on_hover_text(tr!("DBC entfernen")).clicked() {
                    self.cm.dbc = None;
                }
            }
        });

        let Some(l) = &self.cm.lauf else {
            return;
        };
        if let Some(e) = l.fehler.lock().ok().and_then(|g| g.clone()) {
            ui.colored_label(stufe_color(Stufe::Warnung), tr!("Lesefehler: {e}", e = e));
        }
        // Momentaufnahme, damit der Lese-Thread nicht aufs Zeichnen wartet
        let m = l.monitor.lock().map(|m| m.clone()).unwrap_or_default();
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));

        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Filter (ID):"));
            ui.add(egui::TextEdit::singleline(&mut self.cm.filter).desired_width(80.0).hint_text("3A0"));
            ui.checkbox(&mut self.cm.nur_geaenderte, tr!("nur IDs mit Änderungen"));
            ui.weak(tr!("{a} IDs · {b} Frames", a = m.anzahl_ids(), b = m.frames));
            if m.gemerkt.is_some() {
                ui.weak(tr!("· Rauschen gemerkt"));
            }
        });

        let filter = self.cm.filter.trim().trim_start_matches("0x").to_uppercase();
        let jetzt = m.zuletzt;
        let warn = stufe_color(Stufe::Hinweis);
        let mut pick = None;
        egui::ScrollArea::vertical().id_source("canmon").max_height(340.0).show(ui, |ui| {
            egui::Grid::new("canmon_grid").striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                for h in [trn!("ID"), trn!("Name"), trn!("Daten"), trn!("Anzahl"), trn!("Rate")] {
                    ui.strong(ecu_werkbank::i18n::t(h));
                }
                ui.end_row();
                for e in m.eintraege() {
                    let id_txt = if e.extended { format!("{:08X}", e.id) } else { format!("{:03X}", e.id) };
                    if !filter.is_empty() && !id_txt.contains(&filter) {
                        continue;
                    }
                    if self.cm.nur_geaenderte && e.geaendert.iter().all(|g| *g == 0) {
                        continue;
                    }
                    let neu = m.ist_neu(e);
                    let id_label = if neu { format!("{id_txt} ★") } else { id_txt.clone() };
                    let mut id_rt = egui::RichText::new(id_label).monospace();
                    if neu {
                        id_rt = id_rt.color(warn).strong();
                    }
                    if ui.selectable_label(self.cm.gewaehlt == Some(e.id), id_rt).on_hover_text(if neu { tr!("Neu seit „Rauschen merken“") } else { String::new() }).clicked() {
                        pick = Some(e.id);
                    }
                    let name = self.cm.dbc.as_ref().and_then(|(_, d)| d.botschaften.get(&e.id)).map(|b| b.name.clone()).unwrap_or_default();
                    ui.label(name);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (i, b) in e.daten.iter().enumerate() {
                            let mut t = egui::RichText::new(format!("{b:02X}")).monospace();
                            if e.frisch(i, jetzt, FRISCH) {
                                t = t.color(warn).strong();
                            } else if e.geaendert.get(i).is_some_and(|g| *g != 0) {
                                t = t.underline();
                            }
                            ui.label(t);
                        }
                    });
                    ui.monospace(e.anzahl.to_string());
                    ui.monospace(if e.rate_hz() > 0.0 { format!("{:.1} Hz", e.rate_hz()) } else { "—".into() });
                    ui.end_row();
                }
            });
        });
        if let Some(id) = pick {
            self.cm.gewaehlt = if self.cm.gewaehlt == Some(id) { None } else { Some(id) };
        }
        ui.weak(tr!("Farbig: gerade geändert · unterstrichen: hat sich schon einmal geändert · ★ neu seit „Rauschen merken“. Klick auf eine ID zeigt die Bits."));

        // Bit-Ansicht der gewählten ID
        let Some(e) = self.cm.gewaehlt.and_then(|id| m.eintraege().find(|e| e.id == id)) else {
            return;
        };
        ui.add_space(6.0);
        ui.separator();
        ui.strong(tr!("ID {a}: Bits (Byte 0 oben, Bit 7 links)", a = format!("{:03X}", e.id)));
        egui::Grid::new("canmon_bits").spacing([6.0, 2.0]).show(ui, |ui| {
            ui.weak("");
            for bit in (0..8).rev() {
                ui.weak(bit.to_string());
            }
            ui.weak(tr!("hex"));
            ui.end_row();
            for (i, b) in e.daten.iter().enumerate() {
                ui.weak(tr!("Byte {i}", i = i));
                for bit in (0..8).rev() {
                    let maske = 1u8 << bit;
                    let wert = if b & maske != 0 { "1" } else { "0" };
                    let mut t = egui::RichText::new(wert).monospace();
                    if e.geaendert.get(i).is_some_and(|g| g & maske != 0) {
                        t = t.color(warn).strong();
                    } else if e.rauschen.get(i).is_some_and(|r| r & maske != 0) {
                        t = t.weak();
                    }
                    ui.label(t);
                }
                ui.monospace(format!("{b:02X}"));
                ui.end_row();
            }
        });
        ui.weak(tr!("Farbig: Bit hat sich geändert · blass: gilt als Rauschen."));
        if let Some((_, dbc)) = &self.cm.dbc {
            let werte = dbc.entschluesseln(e.id, &e.daten);
            if !werte.is_empty() {
                ui.add_space(4.0);
                ui.strong(tr!("Signale aus der DBC"));
                let botschaft = dbc.botschaften.get(&e.id).map(|b| b.name.clone()).unwrap_or_default();
                let mut neu = None;
                for w in werte {
                    ui.horizontal(|ui| {
                        let schon = self.scope_series.iter().any(|s| matches!(&s.kind, ChanKind::Can { id, signal } if *id == e.id && *signal == w.signal));
                        if ui.add_enabled(!schon, egui::Button::new("📈").small()).on_hover_text(tr!("In die Live-Grafik übernehmen")).clicked() {
                            neu = Some((w.signal.clone(), w.einheit.clone()));
                        }
                        ui.monospace(format!("{} = {}", w.signal, w.anzeige()));
                    });
                }
                if let Some((signal, einheit)) = neu {
                    let name = if botschaft.is_empty() { signal.clone() } else { format!("{botschaft}.{signal}") };
                    self.scope_series.push(Series::new(&name, &einheit, ChanKind::Can { id: e.id, signal }));
                    self.status = tr!("„{a}“ ist in der Live-Grafik. Dort „Start“ drücken; der CAN-Monitor muss weiterlaufen.", a = name);
                }
            }
        }
    }

    /// Aktueller Stand des CAN-Monitors (None, wenn er nicht läuft).
    pub(super) fn cm_momentaufnahme(&self) -> Option<Monitor> {
        self.cm.lauf.as_ref().and_then(|l| l.monitor.lock().ok().map(|m| m.clone()))
    }

    pub(super) fn cm_dbc(&self) -> Option<&ecu_werkbank::dbc::Dbc> {
        self.cm.dbc.as_ref().map(|(_, d)| d)
    }

    /// CAN-Botschaften aus der Fahrzeug-Datenbank übernehmen. `melden`: auch
    /// sagen, wenn nichts da ist (beim Klick, nicht beim stillen ersten Versuch).
    pub(super) fn cm_dbc_aus_datenbank(&mut self, melden: bool) {
        self.cm.db_versucht = true;
        match ecu_werkbank::db::eigen::mit_db(|d| d.dbc_alle()) {
            Ok(d) if !d.botschaften.is_empty() => {
                self.status = tr!("CAN-Botschaften aus der Fahrzeug-Datenbank: {a}", a = d.botschaften.len());
                self.cm.dbc = Some((tr!("Fahrzeug-Datenbank"), d));
            }
            Ok(_) if melden => self.status = tr!("In der Fahrzeug-Datenbank stehen noch keine CAN-Botschaften (DBC-Datei unter Werkzeuge → Fahrzeug-Datenbank importieren)."),
            Ok(_) => {}
            Err(e) if melden => self.status = tr!("Fehler: {e}", e = format!("{e:#}")),
            Err(_) => {}
        }
    }

    /// Eigene DBC-Datei für Namen und Signale laden.
    pub(super) fn cm_dbc_laden(&mut self, p: &std::path::Path) {
        match std::fs::read(p) {
            Ok(b) => {
                let d = ecu_werkbank::dbc::Dbc::lesen(&String::from_utf8_lossy(&b));
                self.status = tr!("DBC geladen: {a} Botschaften.", a = d.botschaften.len());
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.cm.dbc = Some((name, d));
            }
            Err(e) => self.status = tr!("DBC: {e}", e = e),
        }
    }

    /// Demo-Bildschirmfoto: Monitor mit laufendem Lauf vorbelegen.
    pub(super) fn cm_demo_start(&mut self, l: Lauf) {
        self.cm.lauf = Some(l);
        self.cm.gewaehlt = Some(0x3A0);
    }

    fn cm_mitschnitt_ende(&mut self) {
        if let Some((p, n)) = self.cm.lauf.as_ref().and_then(|l| l.mitschnitt_ende()) {
            self.status = tr!("Mitschnitt gespeichert: {n} Frames in {a}", n = n, a = p.display());
        }
    }
}
