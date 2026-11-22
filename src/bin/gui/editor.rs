//! GUI: Eigene Labels (Quelle „Eigene Labels“ der Fahrzeug-Datenbank).
//! Bearbeitet werden sie auf der Seite Werkzeuge → Fahrzeug-Datenbank; hier
//! nur noch das Anlegen aus dem letzten Scan.

use super::*;

impl App {
    /// Für jedes Steuergerät aus dem letzten Scan des aktiven Fahrzeugs einen
    /// vorbefüllten Eintrag in „Eigene Labels“ anlegen — nur dort, wo es noch
    /// keinen gibt. Überschreibt nie etwas; Inhalt stammt allein aus dem eigenen Scan.
    pub(super) fn labels_aus_scan(&mut self) {
        let Some(key) = self.active_vehicle.clone() else {
            self.status = tr!("Kein Fahrzeug gewählt — oben ein Fahrzeug wählen und einen Scan machen.");
            return;
        };
        let Some((_, last)) = ecu_werkbank::history::list(&key).pop() else {
            self.status = tr!("Noch kein Scan für dieses Fahrzeug gespeichert.");
            return;
        };
        // Adressen, zu denen es schon einen Eintrag gibt
        let vorhanden: std::collections::HashSet<u16> = self.db.ecus().into_iter().map(|(a, _)| a).collect();
        let mut angelegt = Vec::new();
        let mut ohne_adresse = 0;
        for m in &last.modules {
            let Some(addr) = m.address else {
                ohne_adresse += 1;
                continue;
            };
            if vorhanden.contains(&addr) {
                continue;
            }
            // Nur bei UDS sind das feste Diagnose-IDs. Bei TP 2.0 steht im Scan die
            // dynamisch vergebene Kanal-ID — als tx/rx würde der Scan sie als UDS-Ziel abfragen.
            let (tx, rx) = match m.protocol.as_str() {
                "UDS" => ecu_werkbank::profil::parse_can(&m.can).map(|(t, r)| (Some(t), Some(r))).unwrap_or((None, None)),
                _ => (None, None),
            };
            let lf = ecu_werkbank::labels::skelett(addr, &m.name, m.ident.first().map(String::as_str), tx, rx);
            if ecu_werkbank::db::eigen::mit_db(|db| db.label_speichern(None, &lf)).is_ok() {
                angelegt.push(format!("{addr:02X} {}", m.name));
            }
        }
        if angelegt.is_empty() {
            self.status = if ohne_adresse > 0 {
                tr!("Nichts angelegt — alle Steuergeräte haben schon einen Eintrag ({a} ohne Adresse übersprungen).", a = ohne_adresse)
            } else {
                tr!("Nichts angelegt — alle Steuergeräte aus dem Scan haben schon einen Eintrag.")
            };
        } else {
            self.reload_labels();
            self.status = tr!("{a} Einträge aus dem Scan angelegt: {b}", a = angelegt.len(), b = angelegt.join(", "));
        }
    }

    /// Fahrzeug-Datenbank mit der Quelle „Eigene Labels“ öffnen.
    pub(super) fn labels_quelle_zeigen(&mut self) {
        self.tab = Tab::Datenbank;
        if let Ok(q) = ecu_werkbank::db::eigen::mit_db(|d| d.quellen()) {
            self.dbs_quelle_waehlen(q.into_iter().find(|q| q.name == ecu_werkbank::db::labels::QUELLE).map(|q| q.id));
        }
    }

    /// Partikelfilter/Regenerationsfahrt nur anbieten, wenn das aktive Fahrzeug
    /// ein Diesel sein kann: DPF-Kennungen eingetragen, Bezeichnung mit TDI/Diesel …,
    /// oder Motor unbekannt (kein Fahrzeug, keine Bezeichnung).
    pub(super) fn diesel_moeglich(&self) -> bool {
        let key = self.active_vehicle.clone().unwrap_or_default();
        if let Some((k, d)) = self.diesel_cache.borrow().as_ref() {
            if *k == key {
                return *d;
            }
        }
        let d = Self::diesel_pruefen(&key);
        *self.diesel_cache.borrow_mut() = Some((key, d));
        d
    }

    fn diesel_pruefen(key: &str) -> bool {
        let Some(v) = ecu_werkbank::vehicle::load(key) else { return true };
        if v.dpf.is_some() {
            return true;
        }
        let b = v.bezeichnung.to_uppercase();
        if b.trim().is_empty() {
            return true;
        }
        ["TDI", "DIESEL", "SDI", "CDI", "HDI", "DCI", "CRDI", "D-4D", "JTD", "MULTIJET", "BLUEHDI", "BLUEMOTION TDI"].iter().any(|w| b.contains(w))
    }
}
