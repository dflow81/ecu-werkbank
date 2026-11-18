//! GUI: Dateien ins Fenster ziehen. Je nach Endung öffnet sich die passende
//! Seite. Es wird nur geöffnet/angezeigt — nie etwas ins Auto geschrieben
//! oder eine Sicherung zurückgespielt.

use super::*;

/// Was mit einer abgelegten Datei passiert.
#[derive(Debug, PartialEq)]
pub(super) enum Ziel {
    Mitschnitt,
    Dbc,
    Kurven,
    Label,
    Datensicherung,
    Unbekannt,
}

pub(super) fn ziel_fuer(p: &std::path::Path) -> Ziel {
    let endung = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match endung.as_str() {
        "log" | "txt" | "asc" | "candump" => Ziel::Mitschnitt,
        "dbc" => Ziel::Dbc,
        "csv" => Ziel::Kurven,
        "toml" => Ziel::Label,
        "zip" => Ziel::Datensicherung,
        _ => Ziel::Unbekannt,
    }
}

impl App {
    /// Abgelegte Dateien verarbeiten und beim Darüberziehen einen Hinweis zeigen.
    pub(super) fn ablegen_tick(&mut self, ctx: &egui::Context) {
        let (schwebt, abgelegt) = ctx.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone()));
        if schwebt {
            let rect = ctx.screen_rect();
            let maler = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("ablegen")));
            maler.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(170));
            maler.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tr!("Datei hier ablegen\nMitschnitt (.log/.asc) · DBC · Aufzeichnung (.csv) · Label (.toml)"),
                egui::FontId::proportional(22.0),
                egui::Color32::WHITE,
            );
        }
        for f in abgelegt {
            let Some(p) = f.path else { continue };
            match ziel_fuer(&p) {
                Ziel::Mitschnitt => {
                    self.load_log(p);
                    self.tab = Tab::Mitschnitt;
                }
                Ziel::Dbc => {
                    self.cm_dbc_laden(&p);
                    self.tab = Tab::CanMonitor;
                }
                Ziel::Kurven => {
                    self.scope_csv_laden(&p);
                    self.tab = Tab::Scope;
                }
                Ziel::Label => {
                    self.ed_open(p);
                    self.tab = Tab::Editor;
                }
                Ziel::Datensicherung => {
                    self.status = tr!("Datensicherung zurückspielen bitte bewusst über Fahrzeug → Fahrzeuge & Wartung → Datensicherung.");
                    self.tab = Tab::Fahrzeuge;
                }
                Ziel::Unbekannt => self.status = tr!("Mit {a} kann ECUWerkbank nichts anfangen.", a = p.display()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endungen() {
        let z = |s: &str| ziel_fuer(std::path::Path::new(s));
        assert_eq!(z("/a/fahrt.LOG"), Ziel::Mitschnitt);
        assert_eq!(z("x.asc"), Ziel::Mitschnitt);
        assert_eq!(z("vw_pq.dbc"), Ziel::Dbc);
        assert_eq!(z("scope.csv"), Ziel::Kurven);
        assert_eq!(z("09-bordnetz.toml"), Ziel::Label);
        assert_eq!(z("sicherung.zip"), Ziel::Datensicherung);
        assert_eq!(z("bild.png"), Ziel::Unbekannt);
        assert_eq!(z("ohne_endung"), Ziel::Unbekannt);
    }
}
