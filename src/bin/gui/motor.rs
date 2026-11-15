//! GUI: Motor-Checks (Steuerkette, Ladedruck/Kraftstoffdruck), Fahrt-Rekorder
//! und Anpassungs-Sicherung. Die Logik steckt in `ecu_werkbank::motorcheck`,
//! `ecu_werkbank::rekorder` und `ecu_werkbank::anpassung`; hier nur Bedienung.

use super::{dot, stufe_color, App};
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use ecu_werkbank::kwp::BlockValue;
use ecu_werkbank::limits::Stufe;
use ecu_werkbank::motorcheck::{self, Druck, Feld, KetteMessung, Probe};
use ecu_werkbank::rekorder::{self, Grenze, Meldung};
use ecu_werkbank::{anpassung, tr};

enum Msg {
    Kandidaten(Result<Vec<(u8, Vec<BlockValue>)>, String>),
    Kette(Result<KetteMessung, String>),
    Probe(Probe),
    Druck(Result<(Vec<Probe>, String), String>),
}

enum RkMsg {
    M(Meldung),
    Ende(Result<(), String>),
}

enum AnpMsg {
    Text(String),
    Fertig(Result<PathBuf, String>),
    Geprueft(Result<Vec<anpassung::Punkt>, String>),
    Geschrieben(Result<(Vec<anpassung::Geschrieben>, Option<PathBuf>), String>),
}

pub struct State {
    geladen_fuer: Option<String>,
    job: Option<mpsc::Receiver<Msg>>,
    job_text: String,
    kette_feld: String,
    winkel: Vec<Feld>,
    kette_erg: Option<(KetteMessung, Vec<(Stufe, String)>)>,
    kraftstoff: bool,
    soll: String,
    ist: String,
    sekunden: u32,
    druck_kand: Vec<[Feld; 2]>,
    live: Vec<Probe>,
    druck_erg: Option<(Vec<(Stufe, String)>, String)>,
    stop: Arc<AtomicBool>,
    // Rekorder
    rk_bloecke: String,
    rk_grenzen: String,
    rk_vorlauf: u32,
    rk_nachlauf: u32,
    rk_job: Option<mpsc::Receiver<RkMsg>>,
    rk_stop: Arc<AtomicBool>,
    rk_merken: Arc<AtomicBool>,
    rk_spalten: Vec<String>,
    rk_letzte: Vec<Option<f64>>,
    rk_n: usize,
    rk_log: Vec<String>,
    // Anpassungen
    anp_job: Option<mpsc::Receiver<AnpMsg>>,
    anp_text: String,
    anp_liste: Option<Vec<(PathBuf, anpassung::Sicherung)>>,
    // Zurückschreiben
    anp_datei: Option<PathBuf>,
    anp_geraet: usize,
    anp_punkte: Option<Vec<anpassung::Punkt>>,
    anp_haken: std::collections::BTreeSet<u16>,
    anp_ok: bool,
    /// Namen der Kennungen: Eingaben, „nur ohne Namen“
    anp_namen: std::collections::BTreeMap<String, String>,
    anp_nur_leer: bool,
    anp_erg: Vec<String>,
    anp_login_bc: String,
    anp_login_level: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            geladen_fuer: None,
            job: None,
            job_text: String::new(),
            kette_feld: String::new(),
            winkel: Vec::new(),
            kette_erg: None,
            kraftstoff: false,
            soll: "115/3".into(),
            ist: "115/4".into(),
            sekunden: 20,
            druck_kand: Vec::new(),
            live: Vec::new(),
            druck_erg: None,
            stop: Arc::new(AtomicBool::new(false)),
            rk_bloecke: "1,2,3".into(),
            rk_grenzen: String::new(),
            rk_vorlauf: 30,
            rk_nachlauf: 10,
            rk_job: None,
            rk_stop: Arc::new(AtomicBool::new(false)),
            rk_merken: Arc::new(AtomicBool::new(false)),
            rk_spalten: Vec::new(),
            rk_letzte: Vec::new(),
            rk_n: 0,
            rk_log: Vec::new(),
            anp_job: None,
            anp_text: String::new(),
            anp_liste: None,
            anp_datei: None,
            anp_geraet: 0,
            anp_punkte: None,
            anp_haken: Default::default(),
            anp_ok: false,
            anp_namen: Default::default(),
            anp_nur_leer: false,
            anp_erg: Vec::new(),
            anp_login_bc: String::new(),
            anp_login_level: "3".into(),
        }
    }
}

fn ampel(ui: &mut egui::Ui, l: &[(Stufe, String)]) {
    for (st, t) in l {
        ui.horizontal_wrapped(|ui| {
            dot(ui, stufe_color(*st));
            ui.label(t);
        });
    }
}

impl App {
    fn mc_fahrzeug(&self) -> ecu_werkbank::vehicle::Vehicle {
        ecu_werkbank::vehicle::resolve(None, self.active_vehicle.as_deref()).unwrap_or_default()
    }

    /// Gespeicherte Felder des Fahrzeugs in die Eingaben übernehmen (einmal je Fahrzeug).
    fn mc_laden(&mut self, key: &str) {
        if self.motor.geladen_fuer.as_deref() == Some(key) {
            return;
        }
        let e = motorcheck::laden(key);
        let f = |x: Feld| format!("{:03}/{}", x.block, x.feld);
        self.motor.kette_feld = e.kette.map(f).unwrap_or_default();
        let paar = if self.motor.kraftstoff { e.kraftstoff } else { e.ladedruck };
        if let Some(p) = paar {
            self.motor.soll = f(p[0]);
            self.motor.ist = f(p[1]);
        }
        self.motor.geladen_fuer = Some(key.to_string());
    }

    fn mc_poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.motor.job else { return };
        let mut fertig = false;
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Probe(p) => self.motor.live.push(p),
                Msg::Kandidaten(Ok(b)) => {
                    self.motor.winkel = motorcheck::winkel_kandidaten(&b);
                    self.motor.druck_kand = motorcheck::druck_kandidaten(&b);
                    self.status = tr!("Kandidaten: {a} Winkel-Feld(er), {b} Druck-Paar(e).", a = self.motor.winkel.len(), b = self.motor.druck_kand.len());
                    fertig = true;
                }
                Msg::Kette(Ok(m)) => {
                    let v = self.mc_fahrzeug();
                    let frueher: Vec<f64> = ecu_werkbank::trend::series(&v.key).get(motorcheck::TREND_KETTE).map(|p| p.iter().map(|x| x.wert).collect()).unwrap_or_default();
                    let bew = motorcheck::kette_bewerten(&m, &frueher);
                    if m.kuehlmittel.is_none_or(|t| t >= 80.0) {
                        let _ = ecu_werkbank::trend::add(&v.key, v.km, &[(motorcheck::TREND_KETTE.to_string(), m.wert, m.einheit.clone())]);
                    }
                    self.status = tr!("Steuerketten-Check fertig.");
                    self.motor.kette_erg = Some((m, bew));
                    fertig = true;
                }
                Msg::Druck(Ok((proben, einheit))) => {
                    let art = if self.motor.kraftstoff { Druck::Kraftstoff } else { Druck::Ladedruck };
                    let (erg, bew) = motorcheck::druck_bewerten(art, &proben, &einheit);
                    if let Some(r) = erg {
                        let v = self.mc_fahrzeug();
                        let _ = ecu_werkbank::trend::add(&v.key, v.km, &[(tr!("{a} größte Lücke", a = art.name()), r.luecke, einheit.clone())]);
                    }
                    self.motor.live = proben;
                    self.motor.druck_erg = Some((bew, einheit));
                    self.status = tr!("{a}-Prüfung fertig.", a = art.name());
                    fertig = true;
                }
                Msg::Kandidaten(Err(e)) | Msg::Kette(Err(e)) | Msg::Druck(Err(e)) => {
                    self.status = tr!("Fehler: {e}", e = e);
                    fertig = true;
                }
            }
        }
        if fertig {
            self.motor.job = None;
            ecu_werkbank::motorcheck::demo_ende(&self.backend());
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn mc_start(&mut self, text: String, f: impl FnOnce(ecu_werkbank::transport::Backend, mpsc::Sender<Msg>) + Send + 'static) {
        let be = self.backend();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || f(be, tx));
        self.motor.job = Some(rx);
        self.motor.job_text = text.clone();
        self.status = text;
    }

    pub(super) fn ui_motor(&mut self, ui: &mut egui::Ui) {
        self.mc_poll(ui.ctx());
        let v = self.mc_fahrzeug();
        self.mc_laden(&v.key);
        let busy = self.motor.job.is_some();
        ui.heading(tr!("Motor-Checks"));
        ui.label(tr!("Über die Messwertblöcke des Motorsteuergeräts (TP 2.0, Adresse 01) — nur lesen. Welcher Block/Feld es bei deinem Motor ist, legst du einmal fest; „Kandidaten suchen“ hilft. Fahrzeug: {a}", a = v.label()));
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(&self.motor.job_text);
                if !self.motor.live.is_empty() && ui.button(tr!("⏹ Stopp")).clicked() {
                    self.motor.stop.store(true, Ordering::Relaxed);
                }
            });
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical().id_source("motor").auto_shrink([false, true]).show(ui, |ui| {
            self.ui_kette(ui, busy, &v);
            ui.add_space(10.0);
            ui.separator();
            self.ui_druck(ui, busy, &v);
        });
    }

    fn ui_kette(&mut self, ui: &mut egui::Ui, busy: bool, v: &ecu_werkbank::vehicle::Vehicle) {
        ui.strong(tr!("⛓ Steuerkette (Phasenlage Nockenwelle)"));
        ui.label(tr!("Motor betriebswarm im Leerlauf, nichts tun. Die erste Messung ist der Bezugswert; wandert der Wert über die Zeit, längt sich die Kette oder der Versteller hat Spiel. Dazu werden die Fehler P0016/P0017/P0011/P0014 geprüft."));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Block/Feld:"));
            ui.add(egui::TextEdit::singleline(&mut self.motor.kette_feld).desired_width(70.0).hint_text("093/2"));
            if ui.add_enabled(!busy, egui::Button::new(tr!("🔍 Kandidaten suchen"))).on_hover_text(tr!("Liest Block 1–255 und zeigt Felder mit Winkel- bzw. Druckwerten")).clicked() {
                self.mc_start(tr!("Suche Kandidaten (Block 1–255) …"), |be, tx| {
                    let r = ecu_werkbank::kwp::blockscan(&be, 0x01, 1, 255, |_| {}).map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Msg::Kandidaten(r));
                });
            }
            let feld = Feld::parse(&self.motor.kette_feld);
            if ui.add_enabled(!busy && feld.is_some(), egui::Button::new(tr!("▶ Messen"))).clicked() {
                let feld = feld.expect("geprüft");
                let mut e = motorcheck::laden(&v.key);
                e.kette = Some(feld);
                let _ = motorcheck::speichern(&v.key, &e);
                motorcheck::demo_vorbereiten(&self.backend(), false);
                self.mc_start(tr!("Steuerketten-Check läuft …"), move |be, tx| {
                    let r = motorcheck::kette_messen(&be, feld, 10, |_| {}).map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Msg::Kette(r));
                });
            }
        });
        if !self.motor.winkel.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.weak(tr!("Winkel-Felder:"));
                for f in self.motor.winkel.clone() {
                    if ui.small_button(format!("{:03}/{}", f.block, f.feld)).clicked() {
                        self.motor.kette_feld = format!("{:03}/{}", f.block, f.feld);
                    }
                }
            });
        }
        if let Some((m, bew)) = &self.motor.kette_erg {
            ui.label(tr!("Phasenlage {a} {u} (Spanne {b}, {c} Proben, {d} 1/min{e})", a = format!("{:.1}", m.wert), u = m.einheit, b = format!("{:.1}", m.streuung), c = m.proben, d = format!("{:.0}", m.drehzahl), e = m.kuehlmittel.map(|t| format!(", {t:.0} °C")).unwrap_or_default()));
            ampel(ui, bew);
        }
        let verlauf = ecu_werkbank::trend::series(&v.key).get(motorcheck::TREND_KETTE).cloned().unwrap_or_default();
        if !verlauf.is_empty() {
            ui.weak(tr!("Verlauf: {a}", a = verlauf.iter().map(|p| format!("{} {}{}", p.datum, crate_num(p.wert), p.km.map(|k| format!(" ({k} km)")).unwrap_or_default())).collect::<Vec<_>>().join("  ·  ")));
        }
    }

    fn ui_druck(&mut self, ui: &mut egui::Ui, busy: bool, v: &ecu_werkbank::vehicle::Vehicle) {
        ui.strong(tr!("🌀 Ladedruck / Kraftstoffdruck bei Volllast"));
        ui.label(tr!("Motor warm, 3. Gang, ab ca. 1500 1/min voll beschleunigen — nur wo es sicher und erlaubt ist, am besten mit Beifahrer am Laptop. ECUWerkbank schreibt Soll und Ist mit und vergleicht."));
        ui.horizontal_wrapped(|ui| {
            let alt = self.motor.kraftstoff;
            ui.radio_value(&mut self.motor.kraftstoff, false, tr!("Ladedruck"));
            ui.radio_value(&mut self.motor.kraftstoff, true, tr!("Kraftstoffdruck"));
            if alt != self.motor.kraftstoff {
                self.motor.geladen_fuer = None;
                self.motor.druck_erg = None;
                self.motor.live.clear();
                if self.motor.kraftstoff {
                    self.motor.soll.clear();
                    self.motor.ist.clear();
                } else {
                    self.motor.soll = "115/3".into();
                    self.motor.ist = "115/4".into();
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Soll:"));
            ui.add(egui::TextEdit::singleline(&mut self.motor.soll).desired_width(60.0).hint_text("115/3"));
            ui.label(tr!("Ist:"));
            ui.add(egui::TextEdit::singleline(&mut self.motor.ist).desired_width(60.0).hint_text("115/4"));
            ui.label(tr!("Dauer:"));
            ui.add(egui::DragValue::new(&mut self.motor.sekunden).range(5..=60).suffix(" s"));
            let paar = Feld::parse(&self.motor.soll).zip(Feld::parse(&self.motor.ist)).map(|(a, b)| [a, b]);
            if ui.add_enabled(!busy && paar.is_some(), egui::Button::new(tr!("⏺ Aufzeichnen"))).clicked() {
                let paar = paar.expect("geprüft");
                let mut e = motorcheck::laden(&v.key);
                if self.motor.kraftstoff {
                    e.kraftstoff = Some(paar);
                } else {
                    e.ladedruck = Some(paar);
                }
                let _ = motorcheck::speichern(&v.key, &e);
                self.motor.live.clear();
                self.motor.druck_erg = None;
                self.motor.stop.store(false, Ordering::Relaxed);
                let stop = self.motor.stop.clone();
                let sek = f64::from(self.motor.sekunden);
                motorcheck::demo_vorbereiten(&self.backend(), true);
                self.mc_start(tr!("Aufzeichnung läuft — jetzt voll beschleunigen …"), move |be, tx| {
                    let t2 = tx.clone();
                    let r = motorcheck::druck_aufzeichnen(&be, paar, sek, |p| {
                        let _ = t2.send(Msg::Probe(*p));
                        !stop.load(Ordering::Relaxed)
                    })
                    .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Msg::Druck(r));
                });
            }
        });
        if !self.motor.druck_kand.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.weak(tr!("Druck-Paare:"));
                for p in self.motor.druck_kand.clone() {
                    if ui.small_button(format!("{:03}/{}+{}", p[0].block, p[0].feld, p[1].feld)).clicked() {
                        self.motor.soll = format!("{:03}/{}", p[0].block, p[0].feld);
                        self.motor.ist = format!("{:03}/{}", p[1].block, p[1].feld);
                    }
                }
            });
        }
        if let Some((bew, _)) = &self.motor.druck_erg {
            ampel(ui, bew);
        }
        if !self.motor.live.is_empty() {
            let soll: Vec<[f64; 2]> = self.motor.live.iter().map(|p| [p.t, p.soll]).collect();
            let ist: Vec<[f64; 2]> = self.motor.live.iter().map(|p| [p.t, p.ist]).collect();
            egui_plot::Plot::new("druck").height(220.0).x_axis_label("t [s]").legend(egui_plot::Legend::default()).show(ui, |pui| {
                pui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(soll)).name(tr!("Soll")).width(2.0_f32));
                pui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(ist)).name(tr!("Ist")).width(2.0_f32));
            });
        }
    }

    // -----------------------------------------------------------------------
    // Fahrt-Rekorder

    fn rk_poll(&mut self, ctx: &egui::Context) {
        if self.motor.rk_job.is_none() {
            return;
        }
        let mut ende = None;
        if let Some(rx) = &self.motor.rk_job {
            while let Ok(m) = rx.try_recv() {
                match m {
                    RkMsg::M(Meldung::Laeuft { messungen, spalten, letzte }) => {
                        self.motor.rk_n = messungen;
                        if let Some(s) = spalten {
                            self.motor.rk_spalten = s;
                        }
                        self.motor.rk_letzte = letzte;
                    }
                    RkMsg::M(Meldung::Ausgeloest(t)) => self.motor.rk_log.push(tr!("⏺ Ausgelöst: {a}", a = t)),
                    RkMsg::M(Meldung::Gespeichert(p, t)) => {
                        self.motor.rk_log.push(tr!("✓ Gespeichert ({a}): {b}", a = t, b = p.display()));
                        super::notify(&tr!("Fahrt-Rekorder"), &tr!("Gespeichert: {a}", a = t));
                    }
                    RkMsg::Ende(r) => ende = Some(r),
                }
            }
        }
        match ende {
            Some(Ok(())) => {
                self.motor.rk_job = None;
                self.status = tr!("Fahrt-Rekorder beendet.");
            }
            Some(Err(e)) => {
                self.motor.rk_job = None;
                self.status = tr!("Fahrt-Rekorder: {e}", e = e);
            }
            None => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
        }
    }

    pub(super) fn ui_rekorder(&mut self, ui: &mut egui::Ui) {
        self.rk_poll(ui.ctx());
        let v = self.mc_fahrzeug();
        let laeuft = self.motor.rk_job.is_some();
        ui.heading(tr!("Fahrt-Rekorder"));
        ui.label(tr!("Messwertblöcke des Motors laufen während der Fahrt mit. Bei einem neuen Fehler, wenn ein Wert seine Grenze verlässt, oder wenn du „Merken“ drückst, werden die Sekunden davor und danach als CSV gespeichert. Nur lesen."));
        ui.add_enabled_ui(!laeuft, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("Blöcke:"));
                ui.add(egui::TextEdit::singleline(&mut self.motor.rk_bloecke).desired_width(120.0).hint_text("1,2,3,115"));
                ui.label(tr!("Vorlauf:"));
                ui.add(egui::DragValue::new(&mut self.motor.rk_vorlauf).range(5..=120).suffix(" s"));
                ui.label(tr!("Nachlauf:"));
                ui.add(egui::DragValue::new(&mut self.motor.rk_nachlauf).range(0..=60).suffix(" s"));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("Grenzen:"));
                ui.add(egui::TextEdit::singleline(&mut self.motor.rk_grenzen).desired_width(260.0).hint_text("003/4>40, 001/2<70"));
                ui.weak(tr!("Block/Feld > oder < Wert, mit Komma getrennt"));
            });
        });
        let bloecke: Vec<u8> = self.motor.rk_bloecke.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        let grenzen: Result<Vec<Grenze>, String> = self
            .motor
            .rk_grenzen
            .split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(|x| Grenze::parse(x).ok_or_else(|| tr!("Grenze „{a}“ nicht verstanden", a = x)))
            .collect();
        if let Err(e) = &grenzen {
            ui.colored_label(stufe_color(Stufe::Warnung), e);
        }
        ui.horizontal(|ui| {
            if !laeuft {
                if ui.add_enabled(!bloecke.is_empty() && grenzen.is_ok(), egui::Button::new(tr!("▶ Rekorder starten"))).clicked() {
                    let ordner = v.dir().join("rekorder");
                    self.motor.rk_stop.store(false, Ordering::Relaxed);
                    self.motor.rk_merken.store(false, Ordering::Relaxed);
                    self.motor.rk_log.clear();
                    self.motor.rk_spalten.clear();
                    let (stop, merken) = (self.motor.rk_stop.clone(), self.motor.rk_merken.clone());
                    let (vor, nach) = (f64::from(self.motor.rk_vorlauf), f64::from(self.motor.rk_nachlauf));
                    let g = grenzen.clone().unwrap_or_default();
                    let be = self.backend();
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let t2 = tx.clone();
                        let r = rekorder::laufen(&be, &bloecke, &g, vor, nach, &ordner, &stop, &merken, |m| {
                            let _ = t2.send(RkMsg::M(m));
                        })
                        .map_err(|e| format!("{e:#}"));
                        let _ = tx.send(RkMsg::Ende(r));
                    });
                    self.motor.rk_job = Some(rx);
                    self.status = tr!("Fahrt-Rekorder läuft.");
                }
            } else {
                if ui.button(tr!("⏺ Merken")).on_hover_text(tr!("Diesen Moment speichern (Vorlauf + Nachlauf)")).clicked() {
                    self.motor.rk_merken.store(true, Ordering::Relaxed);
                }
                if ui.button(tr!("⏹ Stoppen")).clicked() {
                    self.motor.rk_stop.store(true, Ordering::Relaxed);
                }
                ui.label(tr!("{a} Messungen", a = self.motor.rk_n));
            }
            if ui.button(tr!("📂 Ordner")).clicked() {
                let d = v.dir().join("rekorder");
                let _ = std::fs::create_dir_all(&d);
                super::open_path(&d, &mut self.status);
            }
        });
        if !self.motor.rk_spalten.is_empty() {
            egui::Grid::new("rk_werte").striped(true).spacing([12.0, 2.0]).show(ui, |ui| {
                for (i, s) in self.motor.rk_spalten.iter().enumerate() {
                    ui.label(s);
                    ui.monospace(self.motor.rk_letzte.get(i).copied().flatten().map(crate_num).unwrap_or_else(|| "—".into()));
                    ui.end_row();
                }
            });
        }
        for l in &self.motor.rk_log {
            ui.label(l);
        }
    }

    // -----------------------------------------------------------------------
    // Anpassungen (auf der Seite „Codier-Sicherung“)

    pub(super) fn ui_anpassungen(&mut self, ui: &mut egui::Ui) {
        if let Some(rx) = &self.motor.anp_job {
            let mut fertig = None;
            while let Ok(m) = rx.try_recv() {
                match m {
                    AnpMsg::Text(t) => self.motor.anp_text = t,
                    AnpMsg::Fertig(r) => fertig = Some(r),
                    AnpMsg::Geprueft(r) => {
                        self.motor.anp_job = None;
                        self.motor.anp_haken.clear();
                        self.motor.anp_erg.clear();
                        match r {
                            Ok(p) => self.motor.anp_punkte = Some(p),
                            Err(e) => self.status = tr!("Anpassungen: {e}", e = e),
                        }
                        return;
                    }
                    AnpMsg::Geschrieben(r) => {
                        self.motor.anp_job = None;
                        self.motor.anp_punkte = None;
                        self.motor.anp_haken.clear();
                        self.motor.anp_ok = false;
                        self.motor.anp_liste = None;
                        match r {
                            Ok((erg, vorher)) => {
                                let ok = erg.iter().filter(|e| e.ergebnis.is_ok()).count();
                                self.status = tr!("Anpassungen: {a} von {b} zurückgeschrieben", a = ok, b = erg.len());
                                self.motor.anp_erg = erg
                                    .iter()
                                    .map(|e| match &e.ergebnis {
                                        Ok(()) => tr!("{a}  geschrieben und kontrolliert ✓", a = format!("{:#06X}", e.did)),
                                        Err(t) => tr!("{a}  nicht geschrieben: {b}", a = format!("{:#06X}", e.did), b = t),
                                    })
                                    .collect();
                                if let Some(p) = vorher {
                                    self.motor.anp_erg.push(tr!("Die Werte davor sind gesichert (zum Rückgängigmachen oben als Sicherung wählbar): {a}", a = p.display()));
                                }
                            }
                            Err(e) => self.status = tr!("Anpassungen: {e}", e = e),
                        }
                        return;
                    }
                }
            }
            match fertig {
                Some(Ok(p)) => {
                    self.status = tr!("Anpassungen gesichert: {a}", a = p.display());
                    self.motor.anp_job = None;
                    self.motor.anp_liste = None;
                }
                Some(Err(e)) => {
                    self.status = tr!("Anpassungen: {e}", e = e);
                    self.motor.anp_job = None;
                }
                None => ui.ctx().request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        let v = self.mc_fahrzeug();
        ui.add_space(10.0);
        ui.separator();
        ui.strong(tr!("Anpassungen"));
        ui.label(tr!("Liest von jedem UDS-Steuergerät alle Kennungen der typischen Anpassungsbereiche (nur lesen, je Steuergerät etwa 30–60 s). Vorher und nachher sichern — der Vergleich zeigt, was sich geändert hat."));
        ui.horizontal(|ui| {
            let busy = self.motor.anp_job.is_some();
            if ui.add_enabled(!busy, egui::Button::new(tr!("💾 Anpassungen sichern"))).clicked() {
                let be = self.backend();
                let labels_dir = self.labels_dir.clone();
                let aktiv = self.active_vehicle.clone();
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let db = ecu_werkbank::labels::LabelDb::load_dir(&labels_dir).unwrap_or_default();
                    // Fahrzeug: gewähltes, sonst über die VIN
                    let vin = if aktiv.is_none() { ecu_werkbank::profil::quick_vin(&be) } else { None };
                    let key = ecu_werkbank::vehicle::resolve(vin.as_deref(), aktiv.as_deref()).map(|v| v.key).unwrap_or_default();
                    let t2 = tx.clone();
                    let r = anpassung::sichern(&be, &db, true, |g, did| {
                        let _ = t2.send(AnpMsg::Text(format!("{g}  {did:#06X}")));
                    })
                    .and_then(|mut s| {
                        s.vehicle = key;
                        anpassung::speichern(&s)
                    })
                    .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(AnpMsg::Fertig(r));
                });
                self.motor.anp_job = Some(rx);
            }
            if busy {
                ui.spinner();
                ui.monospace(&self.motor.anp_text);
            }
        });
        let aktiv = self.active_vehicle.is_some();
        let alle = self.motor.anp_liste.get_or_insert_with(|| {
            if aktiv {
                anpassung::liste(&v.key)
            } else {
                // ohne gewähltes Fahrzeug: das mit der neuesten Sicherung
                ecu_werkbank::vehicle::list().iter().map(|x| anpassung::liste(&x.key)).max_by_key(|l| l.first().map(|x| x.1.timestamp)).unwrap_or_default()
            }
        });
        let alle = alle.clone();
        // Vergleich nur zwischen kompletten Sicherungen
        let liste: Vec<_> = alle.iter().filter(|x| x.1.vollstaendig()).cloned().collect();
        if liste.is_empty() {
            ui.weak(tr!("Noch keine Anpassungs-Sicherung für {a}.", a = v.label()));
            return;
        }
        let n: usize = liste[0].1.geraete.iter().map(|g| g.kennungen.len()).sum();
        ui.label(tr!("{a} Sicherung(en); neueste vom {b}: {c} Steuergerät(e), {d} Kennungen.", a = liste.len(), b = liste[0].1.created, c = liste[0].1.geraete.len(), d = n));
        if liste.len() >= 2 {
            let d = anpassung::vergleichen(&liste[1].1, &liste[0].1);
            ui.label(tr!("Vergleich {a} → {b}:", a = liste[1].1.created, b = liste[0].1.created));
            if d.is_empty() {
                ui.weak(tr!("Keine Kennung geändert."));
            }
            let namen = ecu_werkbank::kennungstexte::alle();
            for (g, aend) in d {
                // Steuergerät der Änderung, um die Namen zu finden
                let geraet = liste[0].1.geraete.iter().find(|x| x.titel() == g).map(ecu_werkbank::kennungstexte::geraet);
                ui.label(egui::RichText::new(&g).strong());
                for x in aend {
                    let name = geraet.and_then(|ge| namen.get(&ecu_werkbank::kennungstexte::schluessel(ge, x.did))).map(|n| format!("  [{n}]")).unwrap_or_default();
                    ui.monospace(format!("  {}{name}", x.zeile()));
                }
            }
        }
        ui.add_space(6.0);
        let neueste = liste[0].1.clone();
        egui::CollapsingHeader::new(tr!("✏ Kennungen benennen ({a} aus der neuesten Sicherung)", a = n)).id_source("anp_namen").show(ui, |ui| {
            self.anp_namen_ui(ui, &neueste);
        });
        ui.add_space(6.0);
        egui::CollapsingHeader::new(tr!("↩ Anpassungen zurückschreiben (z. B. nach Steuergeräte-Tausch)")).id_source("anp_zurueck").show(ui, |ui| {
            self.ui_anp_zurueck(ui, &alle);
        });
    }

    /// Alle Kennungen der Sicherung als Liste zum Benennen (ohne Auto).
    fn anp_namen_ui(&mut self, ui: &mut egui::Ui, s: &anpassung::Sicherung) {
        use ecu_werkbank::kennungstexte::{geraet, schluessel};
        let eigene = ecu_werkbank::kennungstexte::alle();
        let m = &mut self.motor;
        for g in &s.geraete {
            for k in g.kennungen.keys() {
                let Ok(did) = u16::from_str_radix(k, 16) else { continue };
                let key = schluessel(geraet(g), did);
                m.anp_namen.entry(key).or_insert_with_key(|k| eigene.get(k).cloned().unwrap_or_default());
            }
        }
        let geaendert: Vec<(String, String)> =
            m.anp_namen.iter().filter(|(k, t)| eigene.get(*k).map(String::as_str).unwrap_or("") != t.trim()).map(|(k, t)| (k.clone(), t.clone())).collect();
        ui.label(tr!("Jede gesicherte Kennung mit ihrem Inhalt — trag ein, was du weißt (z. B. aus einem anderen Diagnoseprogramm oder durch Vergleich vorher/nachher). Die Namen erscheinen danach im Vergleich. Am Auto wird nichts gelesen oder geschrieben."));
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!geaendert.is_empty(), egui::Button::new(tr!("💾 Namen speichern ({a})", a = geaendert.len()))).clicked() {
                self.status = match ecu_werkbank::kennungstexte::mehrere_setzen(&geaendert) {
                    Ok(()) => tr!("{a} Name(n) gespeichert.", a = geaendert.len()),
                    Err(e) => format!("{e:#}"),
                };
            }
            ui.checkbox(&mut m.anp_nur_leer, tr!("nur ohne Namen"));
        });
        let nur_leer = m.anp_nur_leer;
        for g in &s.geraete {
            let ge = geraet(g);
            let mit = g.kennungen.keys().filter(|k| u16::from_str_radix(k, 16).is_ok_and(|d| eigene.contains_key(&schluessel(ge, d)))).count();
            egui::CollapsingHeader::new(tr!("{a} — {b} Kennungen, {c} mit Namen", a = g.titel(), b = g.kennungen.len(), c = mit)).id_source(("anp_namen_g", ge)).show(ui, |ui| {
                egui::Grid::new(("anp_namen_grid", ge)).striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                    for (k, inhalt) in &g.kennungen {
                        let Ok(did) = u16::from_str_radix(k, 16) else { continue };
                        let key = schluessel(ge, did);
                        if nur_leer && eigene.contains_key(&key) {
                            continue;
                        }
                        ui.monospace(format!("{did:#06X}"));
                        ui.allocate_ui(egui::vec2(220.0, 18.0), |ui| {
                            ui.set_min_width(220.0);
                            ui.add(egui::Label::new(egui::RichText::new(inhalt).monospace().weak()).truncate()).on_hover_text(inhalt);
                        });
                        if let Some(f) = m.anp_namen.get_mut(&key) {
                            ui.add_sized([300.0, 20.0], egui::TextEdit::singleline(f).hint_text(tr!("Name eintragen …")));
                        }
                        ui.end_row();
                    }
                });
            });
        }
    }

    fn ui_anp_zurueck(&mut self, ui: &mut egui::Ui, alle: &[(PathBuf, anpassung::Sicherung)]) {
        let be = self.backend();
        let m = &mut self.motor;
        let busy = m.anp_job.is_some();
        ui.label(tr!("Schreibt gesicherte Werte zurück ins Steuergerät, z. B. nach dem Tausch. Erst vergleichen (nur lesen), dann die gewünschten Werte anhaken. Kilometerstand, Messwerte und Werte mit anderer Länge werden nie geschrieben. Zündung an, Motor aus, Ladegerät empfohlen."));
        let beschr = |s: &anpassung::Sicherung| {
            let mut t = tr!("{a} — {b} Steuergerät(e)", a = s.created, b = s.geraete.len());
            if !s.notiz.is_empty() {
                t += &format!(" — {}", s.notiz);
            }
            t
        };
        // Standard: die neueste komplette Sicherung
        if !m.anp_datei.as_ref().is_some_and(|p| alle.iter().any(|x| &x.0 == p)) {
            m.anp_datei = alle.iter().find(|x| x.1.vollstaendig()).or(alle.first()).map(|x| x.0.clone());
        }
        let Some((_, s)) = alle.iter().find(|x| Some(&x.0) == m.anp_datei.as_ref()) else { return };
        let mut gewechselt = false;
        egui::ComboBox::from_label(tr!("Sicherung")).selected_text(beschr(s)).width(360.0).show_ui(ui, |ui| {
            for (p, x) in alle {
                gewechselt |= ui.selectable_value(&mut m.anp_datei, Some(p.clone()), beschr(x)).changed();
            }
        });
        let Some((_, s)) = alle.iter().find(|x| Some(&x.0) == m.anp_datei.as_ref()) else { return };
        if gewechselt || m.anp_geraet >= s.geraete.len() {
            m.anp_geraet = 0;
        }
        if s.geraete.is_empty() {
            return;
        }
        let vorher_geraet = m.anp_geraet;
        egui::ComboBox::from_label(tr!("Steuergerät")).selected_text(s.geraete[m.anp_geraet].titel()).width(360.0).show_ui(ui, |ui| {
            for (i, g) in s.geraete.iter().enumerate() {
                ui.selectable_value(&mut m.anp_geraet, i, format!("{}  ({})", g.titel(), g.kennungen.len()));
            }
        });
        if gewechselt || vorher_geraet != m.anp_geraet {
            m.anp_punkte = None;
            m.anp_haken.clear();
            m.anp_erg.clear();
        }
        let g = s.geraete[m.anp_geraet].clone();
        if g.tp20.is_none() {
            let vorhanden = ecu_werkbank::login::fuer(&s.vehicle, g.tx).map(|mm| mm.beschreibung());
            egui::CollapsingHeader::new(tr!("🔑 Login (SecurityAccess), falls das Gerät das Schreiben verweigert")).id_source("anp_login").show(ui, |ui| {
                if let Some(b) = &vorhanden {
                    ui.label(tr!("Hinterlegt: {a}", a = b));
                }
                ui.label(tr!("SA2-Bytecode (hex) aus den Flash-Daten dieses Steuergeräts. Wir liefern keinen mit — er ist pro Gerät und Softwarestand verschieden."));
                ui.horizontal(|ui| {
                    ui.label(tr!("Seed-Level (ungerade):"));
                    ui.add(egui::TextEdit::singleline(&mut m.anp_login_level).desired_width(40.0));
                });
                ui.add(egui::TextEdit::multiline(&mut m.anp_login_bc).hint_text("68 02 81 49 …").desired_rows(2).desired_width(360.0));
                if ui.button(tr!("Login speichern")).clicked() {
                    match ecu_werkbank::backup::parse_hex(&m.anp_login_bc) {
                        Some(b) if !b.is_empty() => {
                            let level = m.anp_login_level.trim().parse::<u8>().unwrap_or(3);
                            let r = ecu_werkbank::login::setzen(&s.vehicle, g.tx, ecu_werkbank::login::Methode::Sa2 { level, bytecode: ecu_werkbank::backup::hex(&b) });
                            m.anp_erg = vec![match r {
                                Ok(()) => tr!("Login gespeichert ({a} Byte, Level {b}).", a = b.len(), b = level),
                                Err(e) => tr!("Login: {e}", e = format!("{e:#}")),
                            }];
                        }
                        _ => m.anp_erg = vec![tr!("Das ist kein gültiger Hex-Bytecode.")],
                    }
                }
            });
        } else if let Some(addr) = g.tp20 {
            let id = u16::from(addr);
            let vorhanden = ecu_werkbank::login::fuer(&s.vehicle, id).map(|mm| mm.beschreibung());
            egui::CollapsingHeader::new(tr!("🔑 Login (SecurityAccess), falls das Gerät das Schreiben verweigert")).id_source("anp_login_kwp").show(ui, |ui| {
                if let Some(b) = &vorhanden {
                    ui.label(tr!("Hinterlegt: {a}", a = b));
                }
                ui.label(tr!("Ältere Geräte (TP 2.0) nutzen einen festen Login — kein Bytecode nötig. Einschalten und testen."));
                if ui.button(tr!("KWP-Login (XorKey) aktivieren")).clicked() {
                    let r = ecu_werkbank::login::setzen(&s.vehicle, id, ecu_werkbank::login::Methode::KwpXor { ecu_index: ecu_werkbank::login::KWP_ECU_INDEX_STANDARD });
                    m.anp_erg = vec![match r {
                        Ok(()) => tr!("KWP-Login aktiviert (Index {a}).", a = ecu_werkbank::login::KWP_ECU_INDEX_STANDARD),
                        Err(e) => tr!("Login: {e}", e = format!("{e:#}")),
                    }];
                }
            });
        }
        if ui.add_enabled(!busy, egui::Button::new(tr!("🔍 Mit dem Auto vergleichen (nur lesen)"))).clicked() {
            let (tx, rx) = mpsc::channel();
            let s = s.clone();
            std::thread::spawn(move || {
                let t2 = tx.clone();
                let r = anpassung::fahrzeug_pruefen(&be, &s)
                    .and_then(|_| anpassung::pruefen(&be, &g, &s.vehicle, |i, n| {
                        let _ = t2.send(AnpMsg::Text(format!("{i}/{n}")));
                    }))
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(AnpMsg::Geprueft(r));
            });
            m.anp_job = Some(rx);
            return;
        }
        for z in &m.anp_erg {
            ui.monospace(z);
        }
        let Some(punkte) = &m.anp_punkte else { return };
        let gleich = punkte.iter().filter(|p| p.zustand == anpassung::Zustand::Gleich).count();
        ui.label(tr!("{a} gleich, {b} anders.", a = gleich, b = punkte.len() - gleich));
        for p in punkte.iter().filter(|p| p.zustand != anpassung::Zustand::Gleich) {
            if p.zustand == anpassung::Zustand::Anders {
                let mut an = m.anp_haken.contains(&p.did);
                if ui.checkbox(&mut an, egui::RichText::new(p.zeile()).monospace()).changed() {
                    if an {
                        m.anp_haken.insert(p.did);
                    } else {
                        m.anp_haken.remove(&p.did);
                    }
                }
            } else {
                ui.label(egui::RichText::new(format!("    {}", p.zeile())).monospace().weak());
            }
        }
        if !punkte.iter().any(|p| p.zustand == anpassung::Zustand::Anders) {
            return;
        }
        if ui.small_button(tr!("alle anhaken")).clicked() {
            m.anp_haken = punkte.iter().filter(|p| p.zustand == anpassung::Zustand::Anders).map(|p| p.did).collect();
        }
        ui.checkbox(&mut m.anp_ok, tr!("Ich will die angehakten Werte wirklich ins Steuergerät schreiben."));
        let n = m.anp_haken.len();
        if ui.add_enabled(!busy && m.anp_ok && n > 0, egui::Button::new(tr!("✍ {n} Wert(e) zurückschreiben", n = n))).clicked() {
            let (tx, rx) = mpsc::channel();
            let s = s.clone();
            let dids: Vec<u16> = m.anp_haken.iter().copied().collect();
            std::thread::spawn(move || {
                let r = anpassung::fahrzeug_pruefen(&be, &s)
                    .and_then(|_| anpassung::zurueckschreiben(&be, &g, &s.vehicle, &dids))
                    .and_then(|erg| anpassung::vorher_sichern(&s.vehicle, &g, &erg).map(|p| (erg, p)))
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(AnpMsg::Geschrieben(r));
            });
            m.anp_job = Some(rx);
            m.anp_text = tr!("schreibe …");
        }
    }
}

fn crate_num(v: f64) -> String {
    ecu_werkbank::csvlog::num(v)
}