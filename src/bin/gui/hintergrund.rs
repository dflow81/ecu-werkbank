//! GUI: Bus-Arbeit im Hintergrund.
//!
//! Alles, was mit dem Auto spricht, läuft über **einen** Arbeiter-Thread.
//! Die Aufträge werden der Reihe nach erledigt — nie zwei gleichzeitig auf
//! dem Bus. Der Zeichen-Thread bleibt frei: das Fenster friert nicht ein,
//! wenn ein Steuergerät langsam oder gar nicht antwortet.
//!
//! Ein Auftrag besteht aus zwei Teilen:
//! 1. `arbeit` läuft im Hintergrund (Bus, kein Zugriff auf die App),
//! 2. ihr Ergebnis ist eine Funktion, die danach im Zeichen-Thread die App
//!    aktualisiert (Texte, Listen, Status).
//!
//! ```ignore
//! let be = self.backend();
//! self.im_hintergrund(tr!("Fehler lesen"), move || {
//!     let r = lese(&be);                 // Hintergrund
//!     Box::new(move |app: &mut App| {    // danach im Zeichen-Thread
//!         app.zeilen = r;
//!     })
//! });
//! ```

use super::*;

/// Was nach der Bus-Arbeit im Zeichen-Thread an der App geändert wird.
pub(super) type Ergebnis<T = App> = Box<dyn FnOnce(&mut T) + Send>;

type Arbeit<T> = Box<dyn FnOnce() -> Ergebnis<T> + Send>;

/// Der Arbeiter-Thread und die Liste der wartenden Aufträge (Texte für die Anzeige).
pub(super) struct BusArbeiter<T: 'static = App> {
    auftraege: Option<mpsc::Sender<Arbeit<T>>>,
    ergebnisse: Option<mpsc::Receiver<Ergebnis<T>>>,
    /// Wartende/laufende Aufträge in Reihenfolge; der erste läuft gerade.
    pub(super) wartend: Vec<String>,
}

impl<T: 'static> Default for BusArbeiter<T> {
    fn default() -> Self {
        Self { auftraege: None, ergebnisse: None, wartend: Vec::new() }
    }
}

/// Ergebnis eines Einreihen-Versuchs.
#[derive(Debug, PartialEq)]
pub(super) enum Eingereiht {
    Ja,
    LaeuftSchon,
    ArbeiterTot,
}

impl<T: 'static> BusArbeiter<T> {
    /// Thread beim ersten Auftrag starten.
    fn sender(&mut self, fehler: fn(String) -> Ergebnis<T>) -> &mpsc::Sender<Arbeit<T>> {
        if self.auftraege.is_none() {
            let (atx, arx) = mpsc::channel::<Arbeit<T>>();
            let (etx, erx) = mpsc::channel::<Ergebnis<T>>();
            std::thread::Builder::new()
                .name("bus-arbeiter".into())
                .spawn(move || {
                    for arbeit in arx {
                        // Ein Fehler (panic) in einem Auftrag darf den Arbeiter nicht beenden,
                        // sonst hinge jeder weitere Auftrag für immer.
                        let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(arbeit)).unwrap_or_else(|p| {
                            let grund = p
                                .downcast_ref::<&str>()
                                .map(|s| s.to_string())
                                .or_else(|| p.downcast_ref::<String>().cloned())
                                .unwrap_or_default();
                            fehler(grund)
                        });
                        if etx.send(e).is_err() {
                            break; // Fenster geschlossen
                        }
                    }
                })
                .expect("Thread starten");
            self.auftraege = Some(atx);
            self.ergebnisse = Some(erx);
        }
        self.auftraege.as_ref().expect("gerade gesetzt")
    }

    /// Auftrag anhängen. Gleicher Text, der schon wartet → nicht doppelt (Doppelklick).
    pub(super) fn einreihen(&mut self, text: String, arbeit: Arbeit<T>, fehler: fn(String) -> Ergebnis<T>) -> Eingereiht {
        if self.wartend.contains(&text) {
            return Eingereiht::LaeuftSchon;
        }
        if self.sender(fehler).send(arbeit).is_err() {
            return Eingereiht::ArbeiterTot;
        }
        self.wartend.push(text);
        Eingereiht::Ja
    }

    /// Fertige Ergebnisse abholen (in Reihenfolge) und aus der Warteliste nehmen.
    pub(super) fn abholen(&mut self) -> Vec<Ergebnis<T>> {
        let mut fertig = Vec::new();
        if let Some(rx) = &self.ergebnisse {
            while let Ok(e) = rx.try_recv() {
                fertig.push(e);
            }
        }
        let n = fertig.len().min(self.wartend.len());
        self.wartend.drain(..n);
        fertig
    }

    /// Text des gerade laufenden Auftrags.
    pub(super) fn aktuell(&self) -> Option<&str> {
        self.wartend.first().map(String::as_str)
    }
}

fn interner_fehler(grund: String) -> Ergebnis {
    Box::new(move |app: &mut App| app.status = tr!("Interner Fehler im Hintergrund: {a}", a = grund))
}

impl App {
    /// Bus-Arbeit in die Warteschlange stellen (siehe Modul-Beschreibung).
    pub(super) fn im_hintergrund(&mut self, text: impl Into<String>, arbeit: impl FnOnce() -> Ergebnis + Send + 'static) {
        let text = text.into();
        self.status = match self.bus.einreihen(text.clone(), Box::new(arbeit), interner_fehler) {
            Eingereiht::Ja => tr!("{a} …", a = text),
            Eingereiht::LaeuftSchon => tr!("„{a}“ läuft schon — bitte warten.", a = text),
            Eingereiht::ArbeiterTot => tr!("Hintergrund-Arbeiter läuft nicht mehr — Programm neu starten."),
        };
    }

    /// Wie `im_hintergrund`, ändert aber die Statuszeile nicht (für Live-Takte).
    pub(super) fn im_hintergrund_still(&mut self, text: impl Into<String>, arbeit: impl FnOnce() -> Ergebnis + Send + 'static) {
        if self.bus.einreihen(text.into(), Box::new(arbeit), interner_fehler) == Eingereiht::ArbeiterTot {
            self.status = tr!("Hintergrund-Arbeiter läuft nicht mehr — Programm neu starten.");
        }
    }

    /// Verbindungs-Prüfung (Adapter, Bitrate, Steuergeräte) im Hintergrund.
    pub(super) fn verbindung_pruefen(&mut self) {
        let be = self.backend();
        self.im_hintergrund(tr!("Verbindung prüfen"), move || {
            let r = ecu_werkbank::connect::check(&be);
            Box::new(move |app: &mut App| {
                app.con_result = Some(r);
                app.status = tr!("Verbindung geprüft.");
            })
        });
    }

    /// OBD-Freeze-Frame (Umgebungsbedingungen) im Hintergrund lesen.
    pub(super) fn freeze_frame_lesen(&mut self) {
        let be = self.backend();
        self.im_hintergrund(tr!("Freeze-Frame lesen"), move || {
            let r = be.open(obd::DEFAULT_TX, obd::DEFAULT_RX).and_then(|tp| Obd::new(&*tp).freeze_frame());
            Box::new(move |app: &mut App| match r {
                Ok(f) => {
                    app.obd_freeze = Some(f);
                    app.status = tr!("Freeze-Frame gelesen.");
                }
                Err(e) => {
                    app.obd_freeze = Some(None);
                    app.status = tr!("Freeze-Frame: {e}", e = format!("{:#}", e));
                }
            })
        });
    }

    /// Läuft gerade Bus-Arbeit (oder wartet welche)?
    pub(super) fn bus_beschaeftigt(&self) -> bool {
        !self.bus.wartend.is_empty()
    }

    /// Fertige Aufträge übernehmen. Einmal je Bild aufrufen.
    pub(super) fn hintergrund_tick(&mut self, ctx: &egui::Context) {
        let fertig = self.bus.abholen();
        // neue Ergebnisse auch zeigen, ohne dass erst die Maus bewegt wird
        if !fertig.is_empty() {
            ctx.request_repaint();
        }
        for e in fertig {
            e(self);
        }
        if self.bus_beschaeftigt() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// Anzeige in der Statuszeile, solange etwas läuft.
    pub(super) fn ui_hintergrund(&self, ui: &mut egui::Ui) {
        // Die Meldung selbst steht in `status`; hier nur Drehrad und Warteschlange.
        if let Some(t) = self.bus.aktuell() {
            ui.spinner().on_hover_text(tr!("Läuft: {a}", a = t));
            let mehr = self.bus.wartend.len() - 1;
            if mehr > 0 {
                ui.weak(tr!("(+{n} warten)", n = mehr));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fehler(g: String) -> Ergebnis<String> {
        Box::new(move |s: &mut String| *s = format!("FEHLER {g}"))
    }

    fn warten(b: &mut BusArbeiter<String>, s: &mut String) {
        for _ in 0..300 {
            for e in b.abholen() {
                e(s);
            }
            if b.wartend.is_empty() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("Auftrag nicht fertig geworden");
    }

    #[test]
    fn reihenfolge_doppelte_und_panic() {
        let mut b = BusArbeiter::<String>::default();
        let mut s = String::new();
        let r = b.einreihen("eins".into(), Box::new(|| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            Box::new(|s: &mut String| s.push('1'))
        }), fehler);
        assert_eq!(r, Eingereiht::Ja);
        // gleicher Text wartet schon → abgelehnt
        let r = b.einreihen("eins".into(), Box::new(|| Box::new(|s: &mut String| s.push('X'))), fehler);
        assert_eq!(r, Eingereiht::LaeuftSchon);
        b.einreihen("zwei".into(), Box::new(|| Box::new(|s: &mut String| s.push('2'))), fehler);
        assert_eq!(b.aktuell(), Some("eins"));
        assert_eq!(b.wartend.len(), 2);
        warten(&mut b, &mut s);
        assert_eq!(s, "12");
        // panic im Auftrag: Arbeiter lebt weiter, Meldung kommt an
        b.einreihen("kaputt".into(), Box::new(|| panic!("absichtlich")), fehler);
        warten(&mut b, &mut s);
        assert_eq!(s, "FEHLER absichtlich");
        b.einreihen("danach".into(), Box::new(|| Box::new(|s: &mut String| *s = "ok".into())), fehler);
        warten(&mut b, &mut s);
        assert_eq!(s, "ok");
    }
}
