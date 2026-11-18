//! GUI: Mitschnitt auswerten und aufzeichnen

use super::*;

impl App {
    pub(super) fn load_log(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let d = ecu_werkbank::canlog::Decoder::decode_text(&text);
                self.log_lines = d.lines().iter().map(|l| gui_text(l)).collect();
                self.log_report = gui_text(&d.pruefbericht());
                self.log_path = path.display().to_string();
                self.log_text = text;
                self.learn.clear();
                self.status = tr!("{a} Nachricht(en) dekodiert.", a = self.log_lines.len());
            }
            Err(e) => self.status = tr!("Log lesen: {e}", e = e),
        }
    }

    pub(super) fn ui_mitschnitt(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Mitschnitt auswerten"));
        ui.label(tr!("Liest candump-Logs (candump -L can0 > log.txt oder ecu-werkbank record) und übersetzt sie in Klartext. \
                  Der Prüfbericht zeigt, ob die TP-2.0-Annahmen zu deinem Fahrzeug passen."));
        ui.horizontal(|ui| {
            if ui.button(tr!("📂 Log öffnen…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Log", &["log", "txt", "asc"]).pick_file() {
                    self.load_log(p);
                }
            }
            if ui.button(tr!("🚗 Beispiel aus Demo-Fahrzeug")).clicked() {
                self.log_text = ecu_werkbank::canlog::demo_log();
                self.learn.clear();
                let d = ecu_werkbank::canlog::Decoder::decode_text(&self.log_text);
                self.log_lines = d.lines().iter().map(|l| gui_text(l)).collect();
                self.log_report = gui_text(&d.pruefbericht());
                self.log_path = tr!("(Beispiel)");
            }
            if !self.log_path.is_empty() {
                ui.weak(&self.log_path);
            }
        });
        self.ui_record(ui);
        self.ui_learn(ui);
        if self.log_lines.is_empty() && self.log_report.is_empty() {
            ui.add_space(8.0);
            ui.weak(tr!("Noch kein Log geladen."));
            return;
        }
        ui.add_space(6.0);
        egui::CollapsingHeader::new(tr!("Prüfbericht")).default_open(true).show(ui, |ui| {
            ui.monospace(&self.log_report);
        });
        ui.horizontal(|ui| {
            ui.label(tr!("Filter:"));
            ui.add(egui::TextEdit::singleline(&mut self.log_filter).hint_text(tr!("z. B. KWP, 0x01, Fehler")).desired_width(200.0));
        });
        let f = self.log_filter.to_lowercase();
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            for l in self.log_lines.iter().filter(|l| f.is_empty() || l.to_lowercase().contains(&f)) {
                ui.monospace(l);
            }
        });
    }
}

impl App {
    /// Mitschnitt aufzeichnen (SocketCAN) — Datei im Fahrzeugordner.
    pub(super) fn ui_record(&mut self, ui: &mut egui::Ui) {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Arc;
        ui.horizontal_wrapped(|ui| {
            match &self.rec_job {
                None => {
                    if ui.button(tr!("⏺ Mitschnitt aufzeichnen")).on_hover_text(tr!("Allen CAN-Verkehr in eine Datei schreiben, z. B. während ein Werkstattgerät arbeitet")).clicked() {
                        let Some(dir) = self.vehicle_sub("mitschnitte") else { return };
                        let path = dir.join(ecu_werkbank::vehicle::stamped("mitschnitt", "log"));
                        match self.backend() {
                            Backend::Demo => {
                                let _ = std::fs::write(&path, ecu_werkbank::canlog::demo_log_aktion());
                                self.load_log(path);
                                self.status = tr!("Demo: Beispiel-Mitschnitt mit Werkstatt-Aktion erzeugt.");
                            }
                            Backend::SocketCan { iface } => {
                                let stop = Arc::new(AtomicBool::new(false));
                                let count = Arc::new(AtomicUsize::new(0));
                                let (s2, c2, p2) = (stop.clone(), count.clone(), path.clone());
                                std::thread::spawn(move || {
                                    use ecu_werkbank::can::{CanSocket, EmbeddedFrame, Id, Socket};
                                    use std::io::Write;
                                    let Ok(sock) = CanSocket::open(&iface) else { return };
                                    let _belegt = ecu_werkbank::bus::Belegt::neu();
                                    let _ = sock.set_read_timeout(std::time::Duration::from_millis(200));
                                    let Ok(f) = std::fs::File::create(&p2) else { return };
                                    let mut w = std::io::BufWriter::new(f);
                                    while !s2.load(Ordering::SeqCst) {
                                        let Ok(frame) = sock.read_frame() else { continue };
                                        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                                        let id = match frame.id() {
                                            Id::Standard(x) => u32::from(x.as_raw()),
                                            Id::Extended(x) => x.as_raw(),
                                        };
                                        let _ = writeln!(w, "{}", ecu_werkbank::canlog::format_line(t, &iface, id, frame.data()));
                                        c2.fetch_add(1, Ordering::SeqCst);
                                    }
                                    let _ = w.flush();
                                });
                                self.rec_job = Some((stop, count, path, std::time::Instant::now()));
                            }
                            _ => self.status = tr!("Mitschneiden geht nur mit SocketCAN (oder Demo)."),
                        }
                    }
                }
                Some((stop, count, path, start)) => {
                    ui.colored_label(stufe_color(Stufe::Warnung), tr!("⏺ läuft {a} s · {b} Frames", a = format!("{:.0}", start.elapsed().as_secs_f64()), b = count.load(Ordering::SeqCst)));
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                    if ui.button(tr!("⏹ Stoppen und auswerten")).clicked() {
                        stop.store(true, Ordering::SeqCst);
                        let p = path.clone();
                        self.rec_job = None;
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        self.load_log(p);
                    }
                }
            }
        });
    }
}
