//! GUI: Skripte (Rhai) schreiben und ausführen — nur lesende Funktionen am Auto.
//! Logik in ecu_werkbank::skript.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub(super) struct SkriptSeite {
    code: String,
    datei: Option<PathBuf>,
    ausgabe: Arc<Mutex<Vec<String>>>,
    stopp: Option<Arc<AtomicBool>>,
    lauf: Option<mpsc::Receiver<Result<(), String>>>,
}

impl Default for SkriptSeite {
    fn default() -> Self {
        Self {
            code: include_str!("../../../beispiele/messwerte.rhai").to_string(),
            datei: None,
            ausgabe: Arc::default(),
            stopp: None,
            lauf: None,
        }
    }
}

impl App {
    pub(super) fn ui_skripte(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Skripte"));
        ui.label(tr!("Eigene Abläufe als kleines Programm (Sprache Rhai): Messwerte lesen, vergleichen, wiederholen. Skripte können nur lesen — schreiben ans Auto geht nur über die normalen Seiten mit Bestätigung."));
        ui.weak(tr!("Funktionen: obd(pid), obd_vin(), uds_lesen(tx, did), uds_text(tx, did), uds_fehler(tx), kwp_block(adresse, block), kwp_fehler(adresse), fehlertext(code), hex(liste), warte(ms), zeit(), print(…)"));

        // Ende eines Laufs abholen
        if let Some(rx) = &self.sk.lauf {
            match rx.try_recv() {
                Ok(r) => {
                    let zeile = match r {
                        Ok(()) => tr!("— fertig —"),
                        Err(e) => format!("⚠ {e}"),
                    };
                    if let Ok(mut a) = self.sk.ausgabe.lock() {
                        a.push(zeile);
                    }
                    self.sk.lauf = None;
                    self.sk.stopp = None;
                }
                Err(mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
                Err(mpsc::TryRecvError::Disconnected) => self.sk.lauf = None,
            }
        }
        let laeuft = self.sk.lauf.is_some();

        ui.horizontal_wrapped(|ui| {
            if !laeuft {
                if ui.button(tr!("▶ Ausführen")).clicked() {
                    self.sk_starten();
                }
            } else {
                ui.spinner();
                if ui.button(tr!("⏹ Stopp")).clicked() {
                    if let Some(s) = &self.sk.stopp {
                        s.store(true, Ordering::Relaxed);
                    }
                }
            }
            if ui.add_enabled(!laeuft, egui::Button::new(tr!("📂 Öffnen…"))).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Rhai", &["rhai"]).pick_file() {
                    match ecu_werkbank::ablage::lesen(&p) {
                        Ok(t) => {
                            self.sk.code = t;
                            self.status = tr!("Skript geladen: {a}", a = p.display());
                            self.sk.datei = Some(p);
                        }
                        Err(e) => self.status = tr!("Skript: {e}", e = e),
                    }
                }
            }
            if ui.button(tr!("💾 Speichern…")).clicked() {
                let ziel = self.sk.datei.clone().or_else(|| rfd::FileDialog::new().add_filter("Rhai", &["rhai"]).set_file_name("skript.rhai").save_file());
                if let Some(p) = ziel {
                    self.status = match ecu_werkbank::ablage::schreiben(&p, &self.sk.code) {
                        Ok(()) => tr!("Skript gespeichert: {a}", a = p.display()),
                        Err(e) => tr!("Skript: {e}", e = e),
                    };
                    self.sk.datei = Some(p);
                }
            }
            if ui.button(tr!("🧹 Ausgabe leeren")).clicked() {
                if let Ok(mut a) = self.sk.ausgabe.lock() {
                    a.clear();
                }
            }
            if let Some(p) = &self.sk.datei {
                ui.weak(p.display().to_string());
            }
        });

        ui.add(egui::TextEdit::multiline(&mut self.sk.code).code_editor().desired_rows(14).desired_width(f32::INFINITY).interactive(!laeuft));
        ui.add_space(4.0);
        ui.strong(tr!("Ausgabe"));
        let zeilen = self.sk.ausgabe.lock().map(|a| a.clone()).unwrap_or_default();
        egui::ScrollArea::vertical().id_source("skript_ausgabe").max_height(220.0).stick_to_bottom(true).show(ui, |ui| {
            for z in zeilen {
                ui.monospace(z);
            }
        });
    }

    /// Bildschirmfoto: Beispiel-Skript gleich starten.
    pub(super) fn sk_demo_start(&mut self) {
        self.sk_starten();
    }

    fn sk_starten(&mut self) {
        let stopp = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let ausgabe = self.sk.ausgabe.clone();
        if let Ok(mut a) = ausgabe.lock() {
            a.clear();
        }
        let (be, code, s2, dir) = (self.backend(), self.sk.code.clone(), stopp.clone(), self.labels_dir.clone());
        std::thread::spawn(move || {
            let db = Arc::new(LabelDb::load_dir(&dir).unwrap_or_default());
            let r = ecu_werkbank::skript::ausfuehren(be, db, &code, move |s| {
                if let Ok(mut a) = ausgabe.lock() {
                    a.push(s.to_string());
                    if a.len() > 5000 {
                        a.drain(..1000);
                    }
                }
            }, s2);
            let _ = tx.send(r.map_err(|e| format!("{e:#}")));
        });
        self.sk.stopp = Some(stopp);
        self.sk.lauf = Some(rx);
        self.status = tr!("Skript läuft …");
    }
}
