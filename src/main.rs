//! ECUWerkbank — open VAG diagnostics over SocketCAN (UDS / ISO-TP).
//!
//! This is an independent reimplementation of standard, published automotive
//! diagnostic protocols (ISO 14229 UDS, ISO 15765-2 ISO-TP, ISO 15765-4 CAN).
//! It talks to control units with an open CAN interface. It does not contain,
//! clone, or depend on any proprietary diagnostic software or licensed adapter.

use ecu_werkbank::tr;
use std::path::PathBuf;
use ecu_werkbank::labels::LabelDb;
use ecu_werkbank::transport::Backend;
use ecu_werkbank::uds::Uds;
use ecu_werkbank::kwp::{self, Kwp};
use ecu_werkbank::{format_value, service, tp20};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ecu_werkbank::can::{CanSocket, EmbeddedFrame, Id, Socket};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "ecu-werkbank", version, about = "ECUWerkbank — offene Fahrzeugdiagnose über SocketCAN (UDS/ISO-TP, TP 2.0)")]
struct Cli {
    /// CAN-Interface für SocketCAN (z. B. can0, vcan0)
    #[arg(short, long, default_value = "can0")]
    iface: String,
    /// Transport: socketcan | elm327 (seriell USB/BT) | wifi (ELM327 TCP) | demo (ohne Adapter)
    #[arg(short, long, default_value = ecu_werkbank::transport::STANDARD_TRANSPORT)]
    transport: String,
    /// Serieller Port für ELM327 (USB /dev/ttyUSB0, BT /dev/rfcomm0; Windows COM3 …) bzw.
    /// "host:port" für wifi
    #[arg(long, default_value = ecu_werkbank::transport::STANDARD_PORT)]
    port: String,
    /// Baudrate für seriellen ELM327
    #[arg(long, default_value_t = 38400)]
    baud: u32,
    /// Ordner mit eigenen Label-Dateien (*.toml)
    /// (Standard: ./labels, sonst ~/.config/ecu-werkbank/labels)
    #[arg(short, long)]
    labels: Option<PathBuf>,
    /// Nur mit -t demo: Verläufe der Simulation, z. B. "drehzahl=schwanken:2000:3000:10;geschwindigkeit=halten:0"
    #[arg(long, global = true)]
    demo_muster: Option<String>,
    /// Aktives Fahrzeug (Kennzeichen, VIN oder Ordnername) — dort wird gespeichert
    #[arg(short, long, env = "VAG_FAHRZEUG", global = true)]
    fahrzeug: Option<String>,
    /// Sprache der Ausgaben (de, en …); Standard: gespeicherte Wahl bzw. Systemsprache
    #[arg(long, global = true)]
    sprache: Option<String>,
    #[command(subcommand)]
    cmd: Command,
}

impl Cli {
    fn backend(&self) -> ecu_werkbank::transport::Backend {
        use ecu_werkbank::transport::Backend;
        match self.transport.as_str() {
            "elm327" | "serial" => Backend::Elm327Serial {
                port: self.port.clone(),
                baud: self.baud,
            },
            "wifi" | "tcp" => Backend::Elm327Tcp {
                addr: self.port.clone(),
            },
            t if ecu_werkbank::sim::Modell::parse(t).is_some() => {
                ecu_werkbank::sim::set_modell(ecu_werkbank::sim::Modell::parse(t).unwrap_or_default());
                eprintln!("{}", ecu_werkbank::tr!("Hinweis: Simulation ({a}) — es wird kein echtes Auto gefragt.", a = t));
                Backend::Demo
            }
            _ => Backend::SocketCan {
                iface: self.iface.clone(),
            },
        }
    }
}

#[derive(Subcommand)]
enum DbAktion {
    /// Übersicht: Speicherort, Schema-Version, Anzahl Einträge (Standard)
    Status,
    /// Alle Quellen mit Art, Lizenz und Anzahl Einträge
    Quellen,
    /// Normdaten und die mitgelieferten Community-Daten übernehmen/aktualisieren
    Eingebaut,
    /// Datenpaket (TOML oder JSON) übernehmen; gleiche Quelle wird ersetzt
    Paket {
        #[arg(long)]
        datei: PathBuf,
        /// als Community-Daten (braucht eine Lizenz im Paket); Standard: eigene Daten
        #[arg(long)]
        community: bool,
    },
    /// CSV übernehmen: Fehlertexte (code;text) oder DIDs (did;name;einheit;faktor;offset)
    Csv {
        #[arg(long)]
        datei: PathBuf,
        #[arg(long, default_value = "")]
        lizenz: String,
    },
    /// Alles Weitergebbare als Datenpaket schreiben (.toml oder .json) — z. B. als Beitrag
    Export {
        #[arg(long)]
        ziel: PathBuf,
        /// Lizenz für das Paket, z. B. CC0-1.0
        #[arg(long)]
        lizenz: String,
        #[arg(long, default_value = "Eigene Fahrzeugdaten")]
        name: String,
    },
    /// Eigene Label-Dateien (TOML) übernehmen
    Labels {
        /// Ordner (Standard: der Label-Ordner)
        #[arg(long)]
        ordner: Option<PathBuf>,
    },
    /// DBC-Datei übernehmen (CAN-Botschaften und Signale)
    Dbc {
        #[arg(long)]
        datei: PathBuf,
        /// Lizenz der Datei (z. B. MIT); ohne Angabe wird sie nie weitergegeben
        #[arg(long, default_value = "")]
        lizenz: String,
        /// Hersteller, dem die Botschaften gehören (z. B. Volkswagen)
        #[arg(long)]
        hersteller: Option<String>,
    },
    /// Eine Quelle samt allen ihren Einträgen entfernen
    Entfernen {
        #[arg(long)]
        id: i64,
        /// wirklich entfernen (ohne: nur anzeigen, was entfernt würde)
        #[arg(long)]
        ja: bool,
    },
    /// Stimmige Kopie der Datenbank schreiben
    Sichern {
        #[arg(long)]
        ziel: PathBuf,
    },
}

#[derive(Subcommand)]
enum ServiceAktion {
    /// Service-Werte aus dem Kombi lesen (Standard)
    Lesen,
    /// Anleitung: Service über die Tasten am Kombi zurückstellen
    Anleitung,
    /// Alle Kennungen des Kombis merken (nur lesen), z. B. --name vorher / nachher / ruhe
    Merken {
        #[arg(long, default_value = "vorher")]
        name: String,
        /// Nur diesen Bereich lesen (sonst die üblichen Bereiche)
        #[arg(long, value_parser = parse_hex)]
        von: Option<u16>,
        #[arg(long, value_parser = parse_hex)]
        bis: Option<u16>,
    },
    /// Zwei gemerkte Stände vergleichen → Service-Kennungen und Rückstellwerte
    Vergleich {
        #[arg(long, default_value = "vorher")]
        vorher: String,
        #[arg(long, default_value = "nachher")]
        nachher: String,
        /// Stand ohne Rückstellung dazwischen: blendet Werte aus, die sich von selbst ändern
        #[arg(long)]
        ruhe: Option<String>,
        /// Ergebnis ins Fahrzeug übernehmen (fahrzeug.toml, [service])
        #[arg(long)]
        uebernehmen: bool,
    },
    /// Kennungen suchen, die die angezeigten Werte enthalten (Kombi-Anzeige ablesen)
    Suche {
        /// km bis Inspektion (überfällig: negativ)
        #[arg(long, allow_hyphen_values = true)]
        km: Option<f64>,
        /// Tage bis Inspektion
        #[arg(long, allow_hyphen_values = true)]
        tage: Option<f64>,
        /// km bis Ölwechsel
        #[arg(long, allow_hyphen_values = true)]
        oel_km: Option<f64>,
        /// Tage bis Ölwechsel
        #[arg(long, allow_hyphen_values = true)]
        oel_tage: Option<f64>,
        /// Gemerkten Stand durchsuchen statt neu zu lesen
        #[arg(long)]
        aus: Option<String>,
        /// Treffer als Service-Kennungen ins Fahrzeug übernehmen (danach geht `service zuruecksetzen`)
        #[arg(long)]
        einrichten: bool,
        /// Neues Intervall bei der Rückstellung: km,Tage Inspektion und km,Tage Ölwechsel (Serviceheft!)
        #[arg(long, default_value = "30000,730,15000,365")]
        intervall: String,
    },
    /// Service zurückstellen (schreibt ins Kombi; ohne --ja nur Vorschau)
    Zuruecksetzen {
        #[arg(long)]
        ja: bool,
        /// Ölwechsel NICHT im Wartungsplan eintragen
        #[arg(long)]
        ohne_wartung: bool,
    },
    /// Letzte Rückstellung rückgängig machen (alte Werte aus der Sicherung)
    Rueckgaengig {
        /// Sicherungsdatei (Standard: die neueste)
        #[arg(long)]
        datei: Option<std::path::PathBuf>,
        #[arg(long)]
        ja: bool,
    },
}

#[derive(Subcommand)]
enum Command {
    /// Geladene Label-Dateien anzeigen und prüfen
    Labels,
    /// Offline: Rohbytes mit einem Label dekodieren (zum Testen ohne Hardware)
    Decode {
        #[arg(long, value_parser = parse_hex)]
        did: u16,
        /// Rohbytes als Hex, z. B. "1180" (= 4480)
        #[arg(long)]
        bytes: String,
    },
    /// Erkennt automatisch, welche Diagnose-Welt das Fahrzeug spricht
    Detect,
    /// OBD-konforme Steuergeräte scannen (ISO 15765-4: 0x7E0..0x7E7)
    Scan,
    /// Passiv CAN-IDs mitschneiden, um die ECU-Adresskarte zu bauen
    Discover {
        /// Dauer in Sekunden
        #[arg(short, long, default_value_t = 5)]
        seconds: u64,
    },
    /// Fehlercodes (DTCs) eines Steuergeräts lesen
    Dtc {
        /// Request-CAN-ID (tester → ECU), z. B. 0x7E0
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        /// Response-CAN-ID (ECU → tester), z. B. 0x7E8
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
        /// Status-Maske (0xFF = alle)
        #[arg(long, value_parser = parse_hex, default_value = "0xFF")]
        mask: u16,
    },
    /// Fehlercodes löschen
    Clear {
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
    },
    /// CAN-Mitschnitt (candump) umwandeln: .pcap für Wireshark, .asc für Vector-/SavvyCAN-Werkzeuge
    LogExport {
        /// Mitschnitt im candump-Format
        #[arg(long)]
        log: PathBuf,
        /// Zieldatei, Endung bestimmt das Format (.pcap oder .asc)
        #[arg(long)]
        out: PathBuf,
    },
    /// CAN-Mitschnitt mit einer eigenen DBC-Datei entschlüsseln (Signale im Klartext)
    DbcDecode {
        #[arg(long)]
        dbc: PathBuf,
        #[arg(long)]
        log: PathBuf,
        /// Nur Signale, deren Name diesen Text enthält
        #[arg(long)]
        nur: Option<String>,
    },
    /// Anzahl der gespeicherten Fehler (UDS 0x19 0x01, nur lesen)
    DtcAnzahl {
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
        /// Statusmaske, Standard 0x08 = bestätigt/gespeichert
        #[arg(long, value_parser = parse_hex, default_value = "0x08")]
        maske: u16,
    },
    /// Steuergerät neu starten (UDS ECUReset 0x11) — nur am stehenden Auto, Zündung an; ohne --ja nur Hinweis
    EcuReset {
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
        /// 1 = hart (wie Strom weg), 2 = Zündung aus/an, 3 = weich (Software)
        #[arg(long, default_value_t = 1)]
        art: u8,
        #[arg(long)]
        ja: bool,
    },
    /// TP 2.0 (ältere CAN-Fahrzeuge): Kanal öffnen und KWP2000-Payload senden
    Tp20 {
        /// TP-2.0-Logikadresse, z. B. 0x01 (Motor), 0x03 (ABS); Golf 6: 0x1F Gateway, 0x20 Bordnetz (Auto-Scan zeigt alle)
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
        /// KWP2000-Request als Hex, z. B. "1a9b" (ReadEcuId) oder "1800ff00"; mehrere im selben Kanal mit ";" trennen, z. B. "1089;2101"
        #[arg(long)]
        request: String,
    },
    /// TP 2.0/KWP2000: Steuergerät identifizieren (Teilenummer, Bezeichnung)
    KwpIdent {
        /// TP-2.0-Logikadresse, z. B. 0x01 (Motor)
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
    },
    /// TP 2.0/KWP2000: Fehlerspeicher lesen
    KwpDtc {
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
    },
    /// TP 2.0/KWP2000: Fehlerspeicher löschen
    KwpClear {
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
    },
    /// TP 2.0/KWP2000: Messwertblöcke lesen (einmal oder fortlaufend, optional CSV)
    /// Steuerketten-Check: Phasenlage der Nockenwelle betriebswarm im Leerlauf, als Trend (nur lesen)
    Kette {
        /// Block/Feld der Phasenlage, z. B. 093/2 (wird je Fahrzeug gemerkt)
        #[arg(long)]
        feld: Option<String>,
        #[arg(long, default_value_t = 10)]
        proben: usize,
    },
    /// Ladedruck oder Kraftstoffdruck bei Volllast prüfen: Soll und Ist mitschreiben (nur lesen)
    Druckpruefung {
        /// ladedruck oder kraftstoff
        #[arg(long, default_value = "ladedruck")]
        art: String,
        /// Block/Feld Soll, z. B. 115/3 (wird je Fahrzeug gemerkt)
        #[arg(long, requires = "ist")]
        soll: Option<String>,
        /// Block/Feld Ist, z. B. 115/4
        #[arg(long, requires = "soll")]
        ist: Option<String>,
        #[arg(long, default_value_t = 20)]
        sekunden: u64,
    },
    /// Fahrt-Rekorder: Motor-Messwertblöcke laufend puffern, bei Ereignis die Sekunden davor speichern (Enter = merken, Strg-C = Ende)
    Rekorder {
        /// Blöcke des Motors, z. B. 1,2,3,115
        #[arg(long, value_delimiter = ',', default_value = "1,2,3")]
        bloecke: Vec<u8>,
        /// Grenze als Auslöser, z. B. "003/4>40" oder "001/2<70" (mehrfach möglich)
        #[arg(long)]
        grenze: Vec<String>,
        #[arg(long, default_value_t = 30)]
        vorlauf: u64,
        #[arg(long, default_value_t = 10)]
        nachlauf: u64,
    },
    /// Anpassungen sichern: alle lesbaren Kennungen der UDS- und TP-2.0-Steuergeräte (nur lesen, dauert einige Minuten)
    AnpassungSichern,
    /// Zwei Anpassungs-Sicherungen vergleichen (Standard: die neuesten zwei)
    AnpassungVergleich {
        #[arg(long, requires = "neu")]
        alt: Option<PathBuf>,
        #[arg(long, requires = "alt")]
        neu: Option<PathBuf>,
    },
    /// Anpassungen aus einer Sicherung zurückschreiben (z. B. nach Steuergeräte-Tausch). Ohne --ja wird nur verglichen.
    AnpassungZurueck {
        /// Sicherungsdatei (Standard: die neueste des Fahrzeugs)
        #[arg(long)]
        datei: Option<PathBuf>,
        /// Steuergerät (hex): UDS-Request-ID (z. B. 714) oder TP-2.0-Adresse (z. B. 20). Ohne: Liste der Steuergeräte
        #[arg(long, value_parser = parse_kennung)]
        geraet: Option<u16>,
        /// Kennung(en) zum Zurückschreiben (hex, z. B. 2243); mehrfach möglich
        #[arg(long, value_parser = parse_kennung)]
        kennung: Vec<u16>,
        /// Alle Kennungen, die anders sind und geschrieben werden dürfen
        #[arg(long, conflicts_with = "kennung")]
        alle: bool,
        /// Wirklich schreiben (ohne: nur anzeigen, was passieren würde)
        #[arg(long)]
        ja: bool,
    },
    /// Erweiterte Identifikation (UDS): alle Standard-Kennfelder eines Steuergeräts lesen und benennen (nur lesen)
    Ident {
        /// Steuergerät: UDS-Anfrage-ID (hex), z. B. 714
        #[arg(long, value_parser = parse_kennung)]
        geraet: u16,
        /// Antwort-ID (hex); ohne: OBD-Adressen +8, Hersteller-Adressen (0x700–0x795) +0x6A
        #[arg(long, value_parser = parse_kennung)]
        rx: Option<u16>,
    },
    /// Sicherer Schreibtest: eine Kennung mit ihrem jetzigen Inhalt zurückschreiben (ändert nichts) — prüft, ob Schreiben geht bzw. ein Login nötig ist
    AnpassungSchreibtest {
        /// Steuergerät (hex): UDS-Anfrage-ID (z. B. 714) oder TP-2.0-Adresse (z. B. 20)
        #[arg(long, value_parser = parse_kennung)]
        geraet: u16,
        /// Kennung (hex); ohne: die erste Kennung des Geräts
        #[arg(long, value_parser = parse_kennung)]
        kennung: Option<u16>,
        /// Wirklich schreiben (ohne: nur lesen, was getestet würde)
        #[arg(long)]
        ja: bool,
    },
    /// Grundeinstellungen-Katalog (Referenz): übliche Werkstattfunktionen je System — was es gibt, wozu, worauf achten (kein Eingriff)
    Grundeinstellung {
        /// nur ein System, z. B. Motor, Bremse, Kombi
        #[arg(long)]
        system: Option<String>,
        /// Suchtext (Name/Zweck)
        #[arg(long)]
        suche: Option<String>,
    },
    /// Login (SecurityAccess) für ein Steuergerät hinterlegen oder testen — damit geschützte Geräte das Schreiben erlauben
    Login {
        /// Steuergerät (hex): UDS-Anfrage-ID, z. B. 714. Ohne: alle hinterlegten Logins zeigen
        #[arg(long, value_parser = parse_kennung)]
        geraet: Option<u16>,
        /// SA2-Bytecode (hex) aus den Flash-Daten des Geräts hinterlegen
        #[arg(long)]
        sa2: Option<String>,
        /// Fester Key (hex) hinterlegen (statt SA2)
        #[arg(long, conflicts_with = "sa2")]
        key: Option<String>,
        /// TP-2.0-Login (KWP XorKey) hinterlegen — für ältere Geräte (Adresse als --geraet, z. B. 20)
        #[arg(long, conflicts_with_all = ["sa2", "key"])]
        kwp: bool,
        /// Tabellen-Index für --kwp (0–63), Standard 63
        #[arg(long, default_value_t = 63)]
        index: u8,
        /// Seed-Level (ungerade), Standard 3
        #[arg(long, default_value_t = 3)]
        level: u8,
        /// Den hinterlegten Login am Auto testen (meldet sich an, schreibt nichts)
        #[arg(long)]
        test: bool,
        /// Hinterlegten Login löschen
        #[arg(long)]
        loeschen: bool,
    },
    /// Codierung eines TP-2.0-Steuergeräts lesen (nur lesen): lang über Kennung 0x0600 oder Kurzcodierung
    KwpCoding {
        /// TP-2.0-Adresse (hex), z. B. 20 = Bordnetz beim Golf 6
        #[arg(long, value_parser = parse_can_id)]
        address: u16,
    },
    /// Alle Messwertblöcke eines TP-2.0-Steuergeräts abfragen (nur lesen) und je Feld die Art zeigen
    KwpBlockscan {
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
        #[arg(long, default_value_t = 1)]
        von: u8,
        #[arg(long, default_value_t = 255)]
        bis: u8,
    },
    /// Namensvorschläge für Messwertblöcke prüfen (nur wo die Formel am Auto passt); --ja übernimmt sie
    MesswertVorschlaege {
        #[arg(long, value_parser = parse_tp20, default_value = "1")]
        address: u16,
        /// Passende Vorschläge als eigene Messwert-Texte speichern (vorhandene bleiben)
        #[arg(long)]
        ja: bool,
    },
    KwpBlock {
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
        /// Blocknummer(n), dezimal, z. B. 1 oder 1,2,3
        #[arg(long, value_delimiter = ',', required = true)]
        block: Vec<u8>,
        /// Werte zusätzlich als CSV aufzeichnen (Trennzeichen ;, Dezimalkomma).
        /// Nur ein Dateiname + --fahrzeug → Fahrzeugordner/aufzeichnungen
        #[arg(long)]
        csv: Option<PathBuf>,
        /// Anzahl Lesungen (0 = endlos, Strg-C beendet)
        #[arg(long, default_value_t = 1)]
        count: u32,
        /// Pause zwischen Lesungen in ms
        #[arg(long, default_value_t = 250)]
        interval: u64,
    },
    /// OBD-II: VIN, MIL, Fehleranzahl und Readiness
    ObdInfo {
        #[arg(long, value_parser = parse_hex, default_value = "0x7E0")]
        tx: u16,
        #[arg(long, value_parser = parse_hex, default_value = "0x7E8")]
        rx: u16,
    },
    /// OBD-II: Live-Daten (PIDs) anzeigen, optional als CSV aufzeichnen
    ObdLive {
        #[arg(long, value_parser = parse_hex, default_value = "0x7E0")]
        tx: u16,
        #[arg(long, value_parser = parse_hex, default_value = "0x7E8")]
        rx: u16,
        /// PIDs (hex), z. B. 0C,0D,05; leer = gängige unterstützte
        #[arg(long, value_delimiter = ',', value_parser = parse_pid)]
        pids: Vec<u16>,
        /// Anzahl Lesungen (0 = endlos)
        #[arg(long, default_value_t = 1)]
        count: u32,
        #[arg(long, default_value_t = 500)]
        interval: u64,
        #[arg(long)]
        csv: Option<PathBuf>,
    },
    /// OBD-II: Fehlercodes lesen (gespeichert, sporadisch, permanent)
    ObdDtc {
        #[arg(long, value_parser = parse_hex, default_value = "0x7E0")]
        tx: u16,
        #[arg(long, value_parser = parse_hex, default_value = "0x7E8")]
        rx: u16,
    },
    /// OBD-II: Fehlercodes UND Readiness löschen
    ObdClear {
        #[arg(long, value_parser = parse_hex, default_value = "0x7E0")]
        tx: u16,
        #[arg(long, value_parser = parse_hex, default_value = "0x7E8")]
        rx: u16,
    },
    /// CAN-Mitschnitt (candump-Log) in Klartext übersetzen + TP-2.0-Prüfbericht
    DecodeLog {
        /// Logdatei (candump -L oder Bildschirmformat)
        file: PathBuf,
        /// Nur den Prüfbericht ausgeben
        #[arg(long)]
        report_only: bool,
    },
    /// CAN-Verkehr mitschneiden (candump -L-Format). Mit -t demo: Beispiel-Mitschnitt
    Record {
        /// Ausgabe (Standard: Fahrzeugordner/mitschnitte bei --fahrzeug, sonst mitschnitt.log)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Nur -t demo: Beispiel mit Werkstatt-Aktion (Leerlauf anheben, Codierung ändern)
        #[arg(long)]
        aktion: bool,
        #[arg(short, long, default_value_t = 30)]
        seconds: u64,
    },
    /// Data Identifier lesen (ReadDataByIdentifier)
    Rdid {
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
        /// DID, z. B. 0xF190 (VIN) oder 0xF187 (Teilenummer)
        #[arg(long, value_parser = parse_hex)]
        did: u16,
    },
    /// Bereich von Data Identifiern lesen (nur lesen) und alle lesbaren roh ausgeben,
    /// optional mit Suche nach einem Wert (z. B. Kilometerstand)
    RdidScan {
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
        /// Erste Kennung, z. B. 0x0100
        #[arg(long, value_parser = parse_hex)]
        von: u16,
        /// Letzte Kennung, z. B. 0x2FFF
        #[arg(long, value_parser = parse_hex)]
        bis: u16,
        /// Gesuchter Wert (z. B. Tacho-km); markiert Kennungen, die ihn enthalten könnten
        #[arg(long)]
        suche: Option<u32>,
        /// Diagnosesitzung (0x01 Standard, 0x03 erweitert — beide nur lesen)
        #[arg(long, value_parser = parse_hex, default_value = "0x01")]
        sitzung: u16,
    },
    /// (2) Auto-Scan: UDS (Labels + OBD-Bereich) und TP 2.0 (Adressen 0x01..0x7F)
    Autoscan {
        /// TP-2.0-Suche überspringen (schneller bei reinen UDS-Fahrzeugen)
        #[arg(long)]
        no_tp20: bool,
    },
    /// Verbauliste direkt aus dem Gateway (TP 2.0, nur lesen): welche Steuergeräte das Gateway als verbaut kennt und welche Fehler melden
    GatewayVerbauliste,
    /// Stellglieddiagnose (Funktion 03) eines TP-2.0-Steuergeräts — steuert Bauteile wirklich an! Enter = nächstes Bauteil, q = Ende
    KwpStellglied {
        /// TP-2.0-Adresse (hex), z. B. 22 = Tür Fahrer beim Golf 6
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
        /// Wirklich starten (ohne: nur Hinweis)
        #[arg(long)]
        ja: bool,
    },
    /// Kurzcodierung eines TP-2.0-Steuergeräts schreiben (z. B. Tür, Motor) — mit Kontrolle; ohne --ja nur Vorschau
    KwpCodierungSchreiben {
        /// TP-2.0-Adresse (hex), z. B. 22 = Tür Fahrer beim Golf 6
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
        /// Neue Kurzcodierung (dezimal, wie im Werkstattgerät), z. B. 180
        #[arg(long)]
        wert: u32,
        #[arg(long)]
        ja: bool,
    },
    /// Lange Codierung eines TP-2.0-Steuergeräts (z. B. Bordnetz, ABS, Gateway) lesen; mit --neu schreiben (ohne --ja nur Vorschau)
    KwpLangcodierung {
        /// TP-2.0-Adresse (hex), z. B. 20 = Bordnetz beim Golf 6
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
        /// Neue Codierung als Hex (gleiche Länge), z. B. 0F000A004108
        #[arg(long)]
        neu: Option<String>,
        #[arg(long)]
        ja: bool,
    },
    /// Einen Anpassungskanal (Funktion 10) eines TP-2.0-Steuergeräts ändern — mit Kontrolle; ohne --ja nur Vorschau
    KwpAnpassungSchreiben {
        /// TP-2.0-Adresse (hex), z. B. 20 = Bordnetz beim Golf 6
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
        /// Kanalnummer dezimal wie im Werkstattgerät, z. B. 23
        #[arg(long)]
        kanal: u8,
        /// Neuer Wert (dezimal)
        #[arg(long)]
        wert: u16,
        #[arg(long)]
        ja: bool,
    },
    /// Anpassungskanäle (Funktion 10) eines TP-2.0-Steuergeräts lesen (nur lesen)
    KwpAnpassung {
        /// TP-2.0-Adresse (hex), z. B. 20 = Bordnetz beim Golf 6
        #[arg(long, value_parser = parse_tp20)]
        address: u16,
        /// Kanalnummer(n) dezimal wie im Werkstattgerät, z. B. 1,3,17,23
        #[arg(long, value_delimiter = ',', required = true)]
        kanal: Vec<u8>,
    },
    /// (2) Verbauliste: alle verbauten Steuergeräte, nach System geordnet, mit Teilenummer und Status
    Verbauliste {
        /// TP-2.0-Suche überspringen (schneller bei reinen UDS-Fahrzeugen)
        #[arg(long)]
        no_tp20: bool,
        /// Nicht am Auto lesen, sondern den letzten gespeicherten Scan des Fahrzeugs nehmen (braucht --fahrzeug)
        #[arg(long)]
        gespeichert: bool,
        /// Liste zusätzlich als Textdatei speichern
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Fehlerspeicher-Protokoll aller Steuergeräte als PDF (oder .txt)
    Report {
        /// Ausgabedatei; Endung .pdf → PDF, sonst Text (Standard: Fahrzeugordner/berichte)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Notiz im Kopf, z. B. Kennzeichen oder Kilometerstand
        #[arg(long, default_value = "")]
        note: String,
        /// TP-2.0-Suche überspringen
        #[arg(long)]
        no_tp20: bool,
        /// Nicht in der Fahrzeug-Historie speichern
        #[arg(long)]
        no_history: bool,
        /// Voller Suchlauf statt Schnell-Scan (findet auch neu verbaute Steuergeräte)
        #[arg(long)]
        voll: bool,
    },
    /// Codierung aller Steuergeräte sichern
    CodingBackup {
        /// TP-2.0-Suche überspringen
        #[arg(long)]
        no_tp20: bool,
    },
    /// Codierungs-Sicherungen auflisten
    CodingList,
    /// Zwei Codierungs-Sicherungen vergleichen (Standard: die neuesten zwei)
    CodingCompare {
        #[arg(long, requires = "new")]
        old: Option<PathBuf>,
        #[arg(long, requires = "old")]
        new: Option<PathBuf>,
    },
    /// Codierung eines Steuergeräts aus einer Sicherung zurückschreiben
    CodingRestore {
        /// Sicherungsdatei (siehe coding-list)
        #[arg(long)]
        file: PathBuf,
        /// Request-CAN-ID des Steuergeräts, z. B. 0x7E0
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        /// Wirklich schreiben (ohne: nur anzeigen, was passieren würde)
        #[arg(long)]
        ja: bool,
    },
    /// Schnell-Check: alle Fehler, Readiness, Spannung, Wartung — mit Ampel und PDF
    Check,
    /// Batterie-/Starttest: Spannung messen, währenddessen Motor starten
    Batterie {
        /// Messdauer in Sekunden
        #[arg(long, default_value_t = 15)]
        seconds: u64,
    },
    /// Fahrzeugakte als PDF (Verlauf, Wartung, Codierung) — braucht --fahrzeug
    Akte {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Fahrzeugordner als ZIP sichern — braucht --fahrzeug
    AkteZip {
        #[arg(long)]
        out: PathBuf,
    },
    /// Datensicherung (alle Fahrzeuge + eigene Labels als ZIP): ohne Angaben jetzt sichern; mit --ziel/--tage/--behalten einrichten
    Datensicherung {
        /// Sicherungsordner (z. B. USB-Stick); "" schaltet die automatische Sicherung aus
        #[arg(long)]
        ziel: Option<String>,
        /// Automatisch alle N Tage (beim Start der GUI)
        #[arg(long)]
        tage: Option<u32>,
        /// So viele Sicherungen behalten
        #[arg(long)]
        behalten: Option<usize>,
    },
    /// Datensicherung einspielen (fehlende Dateien zurückholen)
    DatensicherungZurueck {
        datei: PathBuf,
        /// Auch vorhandene Dateien überschreiben
        #[arg(long)]
        ueberschreiben: bool,
    },
    /// Fahrzeug aus einer akte-zip-Sicherung wieder einspielen (z. B. nach Neuinstallation)
    FahrzeugImport {
        /// ZIP-Datei (von akte-zip)
        datei: PathBuf,
        /// Vorhandene Dateien überschreiben
        #[arg(long)]
        ueberschreiben: bool,
    },
    /// Wartungsplan des Fahrzeugs anzeigen — braucht --fahrzeug
    Wartung,
    /// Übliche Wartungsposten anlegen (Öl, Filter, Kerzen, Bremsflüssigkeit …)
    WartungVorlage,
    /// Wartungsposten als heute erledigt eintragen
    WartungErledigt {
        /// Name (oder Anfang davon), z. B. "Öl"
        #[arg(long)]
        name: String,
        /// Kilometerstand (sonst der zuletzt bekannte)
        #[arg(long)]
        km: Option<u32>,
    },
    /// Gewechselte Teile des Fahrzeugs anzeigen — braucht --fahrzeug
    Teile,
    /// Gewechseltes Teil eintragen (heißt es wie ein Wartungsposten, wird der mit eingetragen)
    Teil {
        /// z. B. "Bremsbeläge vorn"
        name: String,
        /// Kilometerstand (sonst der zuletzt bekannte)
        #[arg(long)]
        km: Option<u32>,
        /// Datum TT.MM.JJJJ (sonst heute)
        #[arg(long)]
        datum: Option<String>,
        #[arg(long, default_value = "")]
        teilenummer: String,
        #[arg(long, default_value = "")]
        notiz: String,
        /// Kosten in Euro, z. B. 89,90
        #[arg(long)]
        preis: Option<String>,
    },
    /// Tankfüllung eintragen, z. B.  tanken 42,5 --km 243100 --preis 71,40
    Tanken {
        /// Liter
        liter: String,
        /// Kilometerstand beim Tanken
        #[arg(long)]
        km: u32,
        /// Bezahlt gesamt in Euro
        #[arg(long)]
        preis: Option<String>,
        /// Datum TT.MM.JJJJ (sonst heute)
        #[arg(long)]
        datum: Option<String>,
        /// Nicht vollgetankt (dann zählt die Füllung erst bei der nächsten vollen)
        #[arg(long)]
        teil: bool,
        #[arg(long, default_value = "")]
        notiz: String,
    },
    /// Tankbuch mit Verbrauch anzeigen — braucht --fahrzeug
    Tankbuch,
    /// Kostenübersicht (Teile mit Preis + Tanken) — braucht --fahrzeug
    Kosten,
    /// Wartungstermine als Kalenderdatei (.ics) — braucht --fahrzeug
    Kalender {
        /// Ausgabedatei (Standard: Fahrzeugordner/berichte/wartung.ics)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Kilometerstand von Hand eintragen
    Km { wert: u32 },
    /// Reparatur-Notiz zum letzten (oder angegebenen) Scan eintragen
    Reparatur {
        text: String,
        /// Scan-Datei (Standard: letzter Scan des Fahrzeugs)
        #[arg(long)]
        datei: Option<PathBuf>,
    },
    /// Handy-Anzeige: Live-Werte (OBD-II) im Browser des Handys, gleiches WLAN
    Handy {
        /// Port des Webservers
        #[arg(long, default_value_t = 8088)]
        port: u16,
        /// PIDs (hex), Standard: Drehzahl, Geschwindigkeit, Kühlmittel, Last, Spannung
        #[arg(long, value_delimiter = ',', value_parser = parse_pid)]
        pids: Vec<u16>,
        /// Nach so vielen Sekunden beenden (0 = bis Strg-C)
        #[arg(long, default_value_t = 0)]
        seconds: u64,
    },
    /// Fahrer-Assistent: Wert (z. B. Drehzahl) in einem Zielbereich halten — du gibst Gas
    Ziel {
        /// OBD-PID (hex), Standard 0C = Drehzahl
        #[arg(long, value_parser = parse_hex, default_value = "0x0C")]
        pid: u16,
        /// halten:2500 | schwanken:2000:3000:10 | rampe:800:3000:20
        #[arg(long)]
        muster: String,
        /// erlaubte Abweichung (± in der Einheit des Werts)
        #[arg(long, default_value_t = 150.0)]
        toleranz: f64,
        /// Dauer (0 = bis Strg-C)
        #[arg(long, default_value_t = 0)]
        seconds: u64,
    },
    /// Steuergerät stellt den Wert selbst (Werkstattfunktion, nur im Stand!)
    Stellen {
        /// Name der Stellfunktion aus den Labels, oder "demo"
        #[arg(long)]
        funktion: String,
        /// halten:1200 | schwanken:900:1400:8 | rampe:800:1500:20
        #[arg(long)]
        muster: String,
        #[arg(long, default_value_t = 60)]
        seconds: u64,
        /// Wirklich ausführen (ohne: nur Plan und Sicherheitsgrenzen anzeigen)
        #[arg(long)]
        ja: bool,
    },
    /// OBD-II Mode 06: Prüfergebnisse der Abgas-Überwachung (wie knapp an der Grenze?)
    ObdTests,
    /// Gemisch-Assistent: Leerlauf vs. 2500 1/min → Falschluft, Kraftstoffdruck …
    Gemisch {
        /// Messdauer je Phase in Sekunden
        #[arg(long, default_value_t = 20)]
        seconds: u64,
    },
    /// Langzeit-Trends des Fahrzeugs (braucht --fahrzeug)
    Trends,
    /// Gebrauchtwagen-Check: gelöschte Fehler, Tacho, Fahrgestellnummern → PDF „Kaufberatung“
    Kaufcheck {
        #[arg(long)]
        out: Option<PathBuf>,
        /// Notiz (z. B. Verkäufer, Inserat)
        #[arg(long, default_value = "")]
        note: String,
        /// Ohne Auto: aus dem letzten gespeicherten Scan mit Abgasdaten erstellen (braucht --fahrzeug oder aktives Fahrzeug)
        #[arg(long)]
        aus_scan: bool,
    },
    /// Andere Marken (z. B. Mercedes): Steuergeräte suchen, Kennung + Fehler lesen (nur lesen) → PDF
    Fremd {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Komplett-Sicherung für den Notfall (nur lesen): Teilenummern, Software, Codierung, Anpassungen aller Steuergeräte + PDF „Ersatzteil-Pass“
    NotfallSicherung,
    /// Ersatzteil-Pass (PDF) aus den zuletzt gesicherten Daten neu erzeugen — liest das Auto nicht
    Ersatzteilpass,
    /// Prüfplan (geführte Fehlersuche) zu einem Fehlercode
    Pruefplan { code: String },
    /// Fehlerspeicher ALLER Steuergeräte löschen (auch OBD/Readiness!)
    FehlerLoeschen {
        #[arg(long)]
        ja: bool,
    },
    /// Nach der Reparatur: neu prüfen, mit letztem Scan vergleichen → Reparaturbericht (PDF)
    Reparaturbericht {
        /// Durchgeführte Arbeiten
        #[arg(long, default_value = "")]
        arbeiten: String,
    },
    /// Verbindungs-Assistent: Adapter, Interface, Bitrate, Steuergeräte prüfen
    Verbindung,
    /// Mitschnitt-Lernhilfe: Label-Einträge aus einem Werkstattgerät-Mitschnitt vorschlagen
    Lernen {
        /// Mitschnitt MIT der Aktion
        log: PathBuf,
        /// Mitschnitt OHNE die Aktion (gleicher Ablauf), optional
        #[arg(long)]
        vergleich: Option<PathBuf>,
        /// Als Label-Datei speichern
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, value_parser = parse_hex, default_value = "0x01")]
        adresse: u16,
        #[arg(long, default_value = "gelernt")]
        name: String,
    },
    /// Softwarestände aller Steuergeräte (+ Vergleich mit dem letzten Mal)
    Software,
    /// Nachrüst-Assistent: --phase vorher (vor dem Einbau), --phase nachher (danach)
    Nachruestung {
        #[arg(long)]
        name: String,
        /// vorher | nachher
        #[arg(long)]
        phase: String,
        #[arg(long, default_value = "")]
        notiz: String,
    },
    /// Partikelfilter (Diesel): Beladung, km seit Regeneration, Differenzdruck
    Dpf,
    /// Sprachen: anzeigen, wählen (--setzen en), Vorlage zum Übersetzen (--vorlage fr)
    Sprache {
        /// Diese Sprache dauerhaft wählen (z. B. en, de)
        #[arg(long)]
        setzen: Option<String>,
        /// Vorlage mit allen Texten für diese Sprache schreiben (eigene Sprache anlegen/ergänzen)
        #[arg(long)]
        vorlage: Option<String>,
        /// Zieldatei für --vorlage (Standard: <Datenordner>/sprachen/<code>.toml);
        /// bei --verwaist/--aufraeumen die zu prüfende Sprachdatei
        #[arg(long)]
        datei: Option<PathBuf>,
        /// Übersetzungen anzeigen, deren deutscher Text im Programm nicht mehr vorkommt (braucht --datei)
        #[arg(long)]
        verwaist: bool,
        /// Diese verwaisten Einträge aus der Datei entfernen (Sicherung .toml.bak; braucht --datei)
        #[arg(long)]
        aufraeumen: bool,
    },
    /// Firmenkopf für Berichte (Fußzeile jeder PDF-Seite): anzeigen, setzen oder löschen
    Firmenkopf {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        adresse: Option<String>,
        #[arg(long)]
        telefon: Option<String>,
        #[arg(long)]
        email: Option<String>,
        /// freie Zusatzzeile (z. B. Meisterbetrieb)
        #[arg(long)]
        zusatz: Option<String>,
        /// Firmenkopf entfernen
        #[arg(long)]
        loeschen: bool,
    },
    /// Fahrzeug-Datenbank (Hersteller, Steuergeräte, Fehler, CAN-Signale) — nur lokal
    Db {
        #[command(subcommand)]
        aktion: Option<DbAktion>,
        /// andere Datenbank-Datei (Standard: im Datenordner)
        #[arg(long)]
        datei: Option<PathBuf>,
    },
    /// Service-Anzeige (Inspektion/Ölwechsel): lesen, Kennungen lernen, zurückstellen
    Service {
        #[command(subcommand)]
        aktion: Option<ServiceAktion>,
    },
    /// Fehlerbilder-Bibliothek: Treffer zu einem Code (--code) oder eigenen Eintrag anlegen (--neu)
    Wissen {
        #[arg(long)]
        code: Option<String>,
        /// Neuer Eintrag: Text
        #[arg(long)]
        neu: Option<String>,
        /// Modell-Stichwort für neuen Eintrag (Standard: Fahrzeugbezeichnung)
        #[arg(long)]
        modell: Option<String>,
    },
    /// Genauer Kilometerstand aus dem Kombi (nur lesen). Mit --tacho einmal die Stelle suchen und merken
    KmGenau {
        /// Kilometerstand laut Tacho (genau ablesen)
        #[arg(long)]
        tacho: Option<u32>,
    },
    /// Testlauf am Fahrzeug (nur lesen): Verbindung, Scan, Motor-Messwerte, km-Suche im Kombi → Protokoll
    Testlauf {
        /// Kilometerstand laut Tacho (für die Suche im Kombi)
        #[arg(long)]
        km: Option<u32>,
        /// Ausgabedatei (Standard: Fahrzeugordner/berichte bzw. ./testlauf-….txt)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Eigene Fehlertexte: ohne Angaben alle zeigen, mit --code anzeigen, mit --text setzen ("" löscht)
    Fehlertext {
        #[arg(long)]
        code: Option<String>,
        #[arg(long)]
        text: Option<String>,
    },
    /// Alle möglichen Fehlercodes der UDS-Steuergeräte lesen (nur lesen) → Datei zum Ausfüllen der Klartexte
    FehlerGeruest {
        /// Ausgabedatei (Standard: Fahrzeugordner bzw. ./fehlertexte-geruest-….toml)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Auto-Scan-Protokolle (Textdateien) einlesen: Fehler mit km-Stand in den Verlauf, Fehlertexte übernehmen
    #[command(alias = "vcds-import")]
    AutoscanImport {
        /// Eine oder mehrere Protokolldateien
        #[arg(required = true)]
        dateien: Vec<PathBuf>,
        /// Nur anzeigen, was erkannt wird — nichts speichern
        #[arg(long)]
        probe: bool,
    },
    /// Ausgefülltes Fehlertext-Gerüst übernehmen (leere Einträge werden übersprungen)
    FehlertexteUebernehmen {
        datei: PathBuf,
    },
    /// Eigene Texte zu Messwertblöcken: ohne --text alle zeigen; mit --text setzen ("" löscht)
    Messwerttext {
        /// Steuergerät (TP-2.0-Adresse, hex), z. B. 01 = Motor
        #[arg(long, value_parser = parse_tp20)]
        adresse: Option<u16>,
        #[arg(long)]
        block: Option<u8>,
        /// Feldnummer 1–8; ohne = Name des ganzen Blocks
        #[arg(long)]
        feld: Option<u8>,
        #[arg(long)]
        text: Option<String>,
    },
    /// Eigene Texte zu Codier-Bytes/-Bits: ohne --text zeigen; mit --text setzen ("" löscht)
    Codiertext {
        /// Steuergerät (Request-CAN-ID, hex), z. B. 714 = Kombi
        #[arg(long, value_parser = parse_can_id)]
        tx: Option<u16>,
        /// Byte (0-basiert)
        #[arg(long)]
        byte: Option<usize>,
        /// Bit 0–7; ohne = Text zum ganzen Byte
        #[arg(long)]
        bit: Option<u8>,
        #[arg(long)]
        text: Option<String>,
    },
    /// Sicherheit & Assistenz: Airbag-, Brems- und Assistenz-Steuergeräte (nur lesen)
    Sicherheit,
    /// Ein Codier-Bit setzen (mit automatischer Sicherung vorher)
    CodierBit {
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
        /// Byte (0-basiert)
        #[arg(long)]
        byte: usize,
        /// Bit 0–7
        #[arg(long)]
        bit: u8,
        /// ein oder aus
        #[arg(long)]
        wert: String,
        /// Codier-DID (Standard 0x0600)
        #[arg(long, value_parser = parse_hex, default_value = "0x0600")]
        did: u16,
        #[arg(long)]
        ja: bool,
    },
    /// Alle Fahrzeuge mit Ordner und Inhalt
    Fahrzeuge,
    /// Neues Fahrzeug anlegen (Ordner nach VIN; Kennzeichen zum leichteren Finden)
    FahrzeugNeu {
        #[arg(long)]
        kennzeichen: String,
        #[arg(long, default_value = "")]
        vin: String,
        /// Bezeichnung, z. B. "Golf 6 1.4 TSI"
        #[arg(long, default_value = "")]
        name: String,
    },
    /// CSV-Aufzeichnung als PDF mit Diagrammen
    CsvPdf {
        /// CSV-Datei (von --csv oder aus der GUI)
        file: PathBuf,
        /// Ausgabe (Standard: gleicher Name mit .pdf)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Fahrzeug-Historie: alle Fahrzeuge, oder die Scans eines Fahrzeugs (--fahrzeug)
    History,
    /// Zwei Scans vergleichen: neueste zwei des Fahrzeugs (--fahrzeug) oder zwei Dateien
    Compare {
        /// Älterer Scan (Datei aus der Historie)
        #[arg(long, requires = "new")]
        old: Option<PathBuf>,
        /// Neuerer Scan
        #[arg(long, requires = "old")]
        new: Option<PathBuf>,
    },
    /// (1) Codierung lesen und über deine [[coding]]-Labels aufschlüsseln
    Coding {
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
        /// DID der Codierung (sonst coding_did aus dem Label)
        #[arg(long, value_parser = parse_hex)]
        did: u16,
        /// ECU-Adresse für die Label-Zuordnung
        #[arg(long, value_parser = parse_hex)]
        ecu: u16,
    },
    /// (1) Offline: Codier-Bytes über Labels aufschlüsseln (ohne Hardware)
    DecodeCoding {
        #[arg(long, value_parser = parse_hex)]
        ecu: Option<u16>,
        /// Steuergerät (Request-CAN-ID, hex) für deine eigenen Codiertexte
        #[arg(long, value_parser = parse_can_id)]
        tx: Option<u16>,
        /// Codier-Bytes als Hex
        #[arg(long)]
        bytes: String,
    },
    /// (1) Codierung/Anpassung schreiben (WriteDataByIdentifier 0x2E)
    CodeWrite {
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
        #[arg(long, value_parser = parse_hex)]
        did: u16,
        /// Neue Bytes als Hex
        #[arg(long)]
        bytes: String,
        /// Wirklich schreiben (ohne: nur anzeigen, was geschrieben würde)
        #[arg(long)]
        ja: bool,
    },
    /// (3) Grundeinstellung/Service-Routine (RoutineControl 0x31)
    Routine {
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
        /// Sub: 1=Start, 2=Stop, 3=Ergebnisse (hex oder dezimal)
        #[arg(long, value_parser = parse_hex, default_value = "1")]
        sub: u16,
        /// Routine-ID (16-bit)
        #[arg(long, value_parser = parse_hex)]
        id: u16,
        /// Optionale Daten als Hex
        #[arg(long, default_value = "")]
        data: String,
        /// Wirklich senden (ohne: nur anzeigen, was gesendet würde)
        #[arg(long)]
        ja: bool,
    },
    /// (3) Stellglieddiagnose (InputOutputControlByIdentifier 0x2F)
    Iocontrol {
        #[arg(long, value_parser = parse_hex)]
        tx: u16,
        #[arg(long, value_parser = parse_hex)]
        rx: u16,
        #[arg(long, value_parser = parse_hex)]
        did: u16,
        /// Steuerparameter (0x00=returnControl, 0x03=shortTermAdjustment …)
        #[arg(long, value_parser = parse_hex, default_value = "0x03")]
        control: u16,
        /// Stellgröße als Hex (ECU-abhängig)
        #[arg(long, default_value = "")]
        state: String,
        /// Wirklich senden (ohne: nur anzeigen, was gesendet würde)
        #[arg(long)]
        ja: bool,
    },
}

/// CAN-ID immer hexadezimal, mit oder ohne „0x“ (714 = 0x714).
fn parse_can_id(s: &str) -> Result<u16, String> {
    let s = s.trim();
    let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u16::from_str_radix(h, 16).map_err(|e| tr!("ungültige Zahl '{s}': {e}", s = s, e = e))
}

/// TP-2.0-Adresse: immer hexadezimal, mit oder ohne „0x“ (20 = 0x20 = Bordnetz).
fn parse_tp20(s: &str) -> Result<u16, String> {
    parse_can_id(s).and_then(|a| if a <= 0xFF { Ok(a) } else { Err(tr!("TP-2.0-Adresse '{s}' zu groß (00–FF)", s = s)) })
}

fn parse_hex(s: &str) -> Result<u16, String> {
    let s = s.trim();
    let v = if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u16::from_str_radix(h, 16)
    } else {
        s.parse::<u16>()
    };
    v.map_err(|e| tr!("ungültige Zahl '{s}': {e}", s = s, e = e))
}

/// OBD-PID: immer hexadezimal ("0C", "10" = 0x10, auch "0x0C").
fn parse_pid(s: &str) -> Result<u16, String> {
    let s = s.trim();
    let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    match u8::from_str_radix(h, 16) {
        Ok(v) => Ok(u16::from(v)),
        Err(e) => Err(tr!("ungültige PID '{s}' (hex, 00–FF): {e}", s = s, e = e)),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(s) = &cli.sprache {
        ecu_werkbank::i18n::set_sprache(s, false);
    }
    let labels_dir = cli.labels.clone().unwrap_or_else(ecu_werkbank::labels::default_dir);
    let db = LabelDb::load_dir(&labels_dir)?;
    let be = cli.backend();
    if let Some(spec) = &cli.demo_muster {
        ecu_werkbank::sim::apply_spec(spec)?;
    }
    match cli.cmd {
        Command::Labels => show_labels(&labels_dir, &db),
        Command::Decode { did, bytes } => {
            let raw = parse_hex_bytes(&bytes)?;
            print_value(did, &raw, &db);
            Ok(())
        }
        Command::Detect => detect(&be),
        Command::Scan => scan(&be),
        Command::Discover { seconds } => discover(&cli.iface, seconds),
        Command::Dtc { tx, rx, mask } => read_dtc(&be, tx, rx, mask as u8, &db),
        Command::Tp20 { address, request } => tp20_send(&be, address as u8, &request),
        Command::Clear { tx, rx } => clear_dtc(&be, tx, rx),
        Command::LogExport { log, out } => {
            let format = ecu_werkbank::spurexport::Format::aus_pfad(&out).context(tr!("Zieldatei braucht die Endung .pcap oder .asc"))?;
            let text = std::fs::read_to_string(&log).with_context(|| tr!("Log lesen: {a}", a = log.display()))?;
            let n = ecu_werkbank::spurexport::frames(&text).len();
            std::fs::write(&out, ecu_werkbank::spurexport::umwandeln(&text, format))?;
            println!("{}", tr!("{n} Frames geschrieben: {a}", n = n, a = out.display()));
            Ok(())
        }
        Command::DbcDecode { dbc, log, nur } => {
            let d = ecu_werkbank::dbc::Dbc::lesen(&std::fs::read_to_string(&dbc).with_context(|| tr!("DBC lesen: {a}", a = dbc.display()))?);
            let text = std::fs::read_to_string(&log).with_context(|| tr!("Log lesen: {a}", a = log.display()))?;
            println!("{}", tr!("{n} Botschaften in der DBC", n = d.botschaften.len()));
            for f in ecu_werkbank::spurexport::frames(&text) {
                let w: Vec<String> = d.entschluesseln(f.id, &f.data).iter().filter(|w| nur.as_ref().is_none_or(|n| w.signal.contains(n.as_str()))).map(|w| w.anzeige()).collect();
                if !w.is_empty() {
                    let name = d.botschaften.get(&f.id).map(|b| b.name.as_str()).unwrap_or("");
                    println!("{:>14.3}  {:>8X} {name}: {}", f.t.unwrap_or(0.0), f.id, w.join(", "));
                }
            }
            Ok(())
        }
        Command::DtcAnzahl { tx, rx, maske } => {
            let tp = be.open(tx, rx)?;
            let n = Uds::new(&*tp).dtc_anzahl(maske as u8)?;
            println!("{}", tr!("{n} Fehler (Statusmaske {a})", n = n, a = format!("{maske:#04x}")));
            Ok(())
        }
        Command::EcuReset { tx, rx, art, ja } => ecu_reset(&be, tx, rx, art, ja),
        Command::KwpIdent { address } => kwp_ident(&be, address as u8),
        Command::KwpDtc { address } => kwp_dtc(&be, address as u8, &db),
        Command::KwpClear { address } => kwp_clear(&be, address as u8),
        Command::KwpCoding { address } => kwp_coding(&be, address as u8, &db),
        Command::Kette { feld, proben } => kette(&be, feld.as_deref(), proben, cli.fahrzeug.as_deref()),
        Command::Druckpruefung { art, soll, ist, sekunden } => druckpruefung(&be, &art, soll.as_deref().zip(ist.as_deref()), sekunden, cli.fahrzeug.as_deref()),
        Command::Rekorder { bloecke, grenze, vorlauf, nachlauf } => rekorder(&be, &bloecke, &grenze, vorlauf, nachlauf, cli.fahrzeug.as_deref()),
        Command::AnpassungSichern => anpassung_sichern(&be, &db, cli.fahrzeug.as_deref()),
        Command::AnpassungVergleich { alt, neu } => anpassung_vergleich(alt.as_deref(), neu.as_deref(), cli.fahrzeug.as_deref()),
        Command::AnpassungZurueck { datei, geraet, kennung, alle, ja } => anpassung_zurueck(&be, datei.as_deref(), geraet, &kennung, alle, ja, cli.fahrzeug.as_deref()),
        Command::Ident { geraet, rx } => ident_cmd(&be, geraet, rx),
        Command::AnpassungSchreibtest { geraet, kennung, ja } => anpassung_schreibtest(&be, geraet, kennung, ja, cli.fahrzeug.as_deref()),
        Command::Grundeinstellung { system, suche } => grundeinstellung_cmd(system.as_deref(), suche.as_deref()),
        Command::Login { geraet, sa2, key, kwp, index, level, test, loeschen } => login_cmd(&be, geraet, sa2, key, kwp, index, level, test, loeschen, cli.fahrzeug.as_deref()),
        Command::KwpBlockscan { address, von, bis } => kwp_blockscan(&be, address as u8, von, bis),
        Command::MesswertVorschlaege { address, ja } => messwert_vorschlaege(&be, address as u8, ja),
        Command::KwpBlock { address, block, csv, count, interval } => {
            let csv = csv_target(csv, cli.fahrzeug.as_deref())?;
            kwp_block(&be, address as u8, &block, csv.as_deref(), count, interval, &db)
        }
        Command::ObdInfo { tx, rx } => obd_info(&be, tx, rx),
        Command::ObdLive { tx, rx, pids, count, interval, csv } => {
            let csv = csv_target(csv, cli.fahrzeug.as_deref())?;
            let pids: Vec<u8> = pids.into_iter().map(|p| p as u8).collect();
            obd_live(&be, tx, rx, &pids, count, interval, csv.as_deref())
        }
        Command::ObdDtc { tx, rx } => obd_dtc(&be, tx, rx),
        Command::ObdClear { tx, rx } => {
            let tp = be.open(tx, rx)?;
            ecu_werkbank::obd::Obd::new(&*tp).clear()?;
            println!("{}", tr!("OBD-Fehlercodes und Readiness gelöscht. Readiness muss jetzt neu erfahren werden."));
            Ok(())
        }
        Command::DecodeLog { file, report_only } => decode_log(&file, report_only),
        Command::Record { out, seconds, aktion } => {
            let out = match out {
                Some(p) => p,
                None => match cli.fahrzeug.as_deref().and_then(ecu_werkbank::vehicle::find) {
                    Some(v) => v.sub("mitschnitte")?.join(ecu_werkbank::vehicle::stamped("mitschnitt", "log")),
                    None => PathBuf::from("mitschnitt.log"),
                },
            };
            if aktion && matches!(be, Backend::Demo) {
                std::fs::write(&out, ecu_werkbank::canlog::demo_log_aktion())?;
                println!("{}", tr!("Beispiel-Mitschnitt mit Werkstatt-Aktion: {a}", a = out.display()));
                return Ok(());
            }
            record(&be, &cli.iface, &out, seconds)
        }
        Command::Rdid { tx, rx, did } => rdid(&be, tx, rx, did, &db),
        Command::RdidScan { tx, rx, von, bis, suche, sitzung } => rdid_scan(&be, tx, rx, von, bis, suche, sitzung as u8),
        Command::Autoscan { no_tp20 } => autoscan(&be, &db, !no_tp20),
        Command::GatewayVerbauliste => gateway_verbauliste(&be),
        Command::KwpStellglied { address, ja } => kwp_stellglied(&be, address as u8, ja),
        Command::KwpCodierungSchreiben { address, wert, ja } => kwp_codierung_schreiben(&be, address as u8, wert, ja),
        Command::KwpAnpassung { address, kanal } => kwp_anpassung(&be, address as u8, &kanal),
        Command::KwpLangcodierung { address, neu, ja } => kwp_langcodierung(&be, address as u8, neu.as_deref(), ja),
        Command::KwpAnpassungSchreiben { address, kanal, wert, ja } => kwp_anpassung_schreiben(&be, address as u8, kanal, wert, ja),
        Command::Verbauliste { no_tp20, gespeichert, out } => {
            verbauliste(&be, &db, !no_tp20, gespeichert, out.as_deref(), cli.fahrzeug.as_deref())
        }
        Command::Report { out, note, no_tp20, no_history, voll } => {
            report(&be, out.as_deref(), &note, &db, !no_tp20, !no_history, voll, cli.fahrzeug.as_deref())
        }
        Command::History => history(cli.fahrzeug.as_deref()),
        Command::Fahrzeuge => fahrzeuge(),
        Command::Software => software(&be, &db, cli.fahrzeug.as_deref()),
        Command::Nachruestung { name, phase, notiz } => nachruestung(&be, &db, &name, &phase, &notiz, cli.fahrzeug.as_deref()),
        Command::Firmenkopf { name, adresse, telefon, email, zusatz, loeschen } => {
            use ecu_werkbank::firmenkopf;
            let mut f = if loeschen { firmenkopf::Firmenkopf::default() } else { firmenkopf::laden() };
            let geaendert = loeschen || name.is_some() || adresse.is_some() || telefon.is_some() || email.is_some() || zusatz.is_some();
            for (feld, neu) in [(&mut f.name, name), (&mut f.adresse, adresse), (&mut f.telefon, telefon), (&mut f.email, email), (&mut f.zusatz, zusatz)] {
                if let Some(n) = neu {
                    *feld = n;
                }
            }
            if geaendert {
                firmenkopf::speichern(&f)?;
            }
            if f.ist_leer() {
                println!("{}", tr!("Kein Firmenkopf. Setzen z. B.: ecu-werkbank firmenkopf --name \"Kfz Muster\" --telefon \"0123 456\""));
            } else {
                println!("{}", tr!("Fußzeile in Berichten: {a}", a = f.zeile()));
            }
            Ok(())
        }
        Command::Db { aktion, datei } => db_cmd(aktion.unwrap_or(DbAktion::Status), datei, &labels_dir),
        Command::Sprache { setzen, vorlage, datei, verwaist, aufraeumen } => sprache_cmd(setzen, vorlage, datei, verwaist, aufraeumen),
        Command::Service { aktion } => service_cmd(&be, aktion.unwrap_or(ServiceAktion::Lesen), cli.fahrzeug.as_deref()),
        Command::Dpf => {
            let cfg = if matches!(be, Backend::Demo) {
                ecu_werkbank::dpf::demo_config()
            } else {
                need_vehicle(cli.fahrzeug.as_deref())?
                    .dpf
                    .context(tr!("Für dieses Fahrzeug sind keine Partikelfilter-Kennungen eingetragen (fahrzeug.toml, [dpf])"))?
            };
            let w = ecu_werkbank::dpf::read(&be, &cfg)?;
            for (st, t) in ecu_werkbank::dpf::bewerten(&w, &cfg) {
                println!("  [{}] {t}", st.text());
            }
            if let Some(v) = cli.fahrzeug.as_deref().and_then(ecu_werkbank::vehicle::find) {
                let mut tr = Vec::new();
                if let Some(x) = w.russ_g { tr.push(("DPF Rußbeladung".to_string(), x, "g".to_string())); }
                if let Some(x) = w.diff_mbar { tr.push(("DPF Differenzdruck".to_string(), x, "mbar".to_string())); }
                let _ = ecu_werkbank::trend::add(&v.key, v.km, &tr);
            }
            Ok(())
        }
        Command::KmGenau { tacho } => {
            if tacho.is_some() {
                println!("{}", tr!("Suche im Kombi — nur lesen, dauert bis zu einer Minute…"));
            }
            let e = ecu_werkbank::km::ermitteln(&be, tacho, |d, n| eprint!("\r{:<40}", tr!("… {d} ({n} lesbar)", d = format!("{:#06X}", d), n = n)))?;
            if tacho.is_some() {
                eprint!("\r{:<40}\r", "");
            }
            for z in &e.zeilen {
                println!("{z}");
            }
            let v = cli.fahrzeug.as_deref().and_then(ecu_werkbank::vehicle::find).or_else(|| e.vin.as_deref().and_then(ecu_werkbank::vehicle::find_by_vin));
            if let (Some(k), Some(mut v)) = (e.km, v.filter(|v| e.passt_zu(v))) {
                v.km = Some(k);
                v.km_datum = ecu_werkbank::vehicle::today();
                ecu_werkbank::vehicle::save(&v)?;
            }
            Ok(())
        }
        Command::Testlauf { km, out } => {
            println!("{}", tr!("Testlauf über {a} — nur lesen, dauert etwa eine Minute…", a = be.describe()));
            let s = ecu_werkbank::autotest::run(&be, &db, km, |p| eprint!("\r{p:<40}"));
            eprint!("\r{:<40}\r", "");
            let text = ecu_werkbank::autotest::to_text(&s, &be.describe());
            print!("{text}");
            let name = ecu_werkbank::vehicle::stamped("testlauf", "txt");
            let path = match out {
                Some(p) => p,
                None => match ecu_werkbank::profil::quick_vin(&be).and_then(|v| ecu_werkbank::vehicle::find_by_vin(&v)) {
                    Some(v) => v.sub("berichte")?.join(&name),
                    None => PathBuf::from(&name),
                },
            };
            std::fs::write(&path, &text)?;
            println!("{}", tr!("\nGespeichert: {a}", a = path.display()));
            Ok(())
        }
        Command::Messwerttext { adresse, block, feld, text } => {
            use ecu_werkbank::messwerttexte;
            match (adresse, block, text) {
                (Some(a), Some(b), Some(t)) => {
                    let a = u8::try_from(a).map_err(|_| anyhow::anyhow!(tr!("Adresse 01–7F")))?;
                    if feld.is_some_and(|f| !(1..=8).contains(&f)) {
                        anyhow::bail!(tr!("Feld 1–8"));
                    }
                    messwerttexte::setzen(a, b, feld, &t)?;
                    let k = messwerttexte::schluessel(a, b, feld);
                    if t.trim().is_empty() {
                        println!("{}", tr!("Text zu {a} gelöscht.", a = k));
                    } else {
                        println!("{}", tr!("Gespeichert: {a}  {b}", a = k, b = t.trim()));
                    }
                }
                (_, _, Some(_)) => anyhow::bail!(tr!("Zum Setzen --adresse und --block angeben")),
                _ => {
                    let m = messwerttexte::alle();
                    if m.is_empty() {
                        println!("{}", tr!("Noch keine eigenen Messwert-Texte. Beispiel: ecu-werkbank messwerttext --adresse 01 --block 1 --feld 2 --text \"Kühlmitteltemperatur\""));
                    }
                    for (k, t) in m {
                        println!("{k:<10}  {t}");
                    }
                }
            }
            Ok(())
        }
        Command::Codiertext { tx, byte, bit, text } => codiertext(tx, byte, bit, text),
        Command::FehlerGeruest { out } => fehler_geruest(&be, &db, out, cli.fahrzeug.as_deref()),
        Command::AutoscanImport { dateien, probe } => {
            for d in &dateien {
                let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if probe {
                    let b = std::fs::read(d).with_context(|| tr!("Lesen: {a}", a = d.display()))?;
                    match ecu_werkbank::vcds::lesen(&ecu_werkbank::vcds::dekodieren(&b)) {
                        Ok(p) => {
                            let n: usize = p.geraete.iter().map(|g| g.fehler.len()).sum();
                            println!("{name}: {} · {} km · {} Steuergeräte · {n} Fehler", p.datum, p.km.map(ecu_werkbank::report::group_thousands).unwrap_or_else(|| "?".into()), p.geraete.len());
                            for g in &p.geraete {
                                for f in &g.fehler {
                                    println!(
                                        "    {:02X} {:<7} {:<6} {:>9}  {:<10}  {}  | {}  | {}",
                                        g.adresse,
                                        f.code,
                                        f.alt.as_deref().unwrap_or(""),
                                        f.km.map(|k| format!("{} km", ecu_werkbank::report::group_thousands(k))).unwrap_or_default(),
                                        f.datum.as_deref().unwrap_or(""),
                                        f.text,
                                        f.art,
                                        f.status
                                    );
                                }
                            }
                        }
                        Err(e) => println!("{name}: {e:#}"),
                    }
                    continue;
                }
                match ecu_werkbank::vcds::importieren(d) {
                    Ok(e) => println!(
                        "{}",
                        tr!("{a}: {b}, {c} — {d} Fehler{e}, {f} neue Fehlertexte", a = name, b = e.fahrzeug, c = e.datum, d = e.fehler, e = if e.neu_im_verlauf { tr!(" (in den Verlauf übernommen)") } else { tr!(" (war schon im Verlauf)") }, f = e.neue_texte)
                    ),
                    Err(e) => println!("{name}: {e:#}"),
                }
            }
            Ok(())
        }
        Command::FehlertexteUebernehmen { datei } => {
            let inhalt = std::fs::read_to_string(&datei).with_context(|| tr!("Lesen: {a}", a = datei.display()))?;
            let (neu, geaendert, gleich) = ecu_werkbank::fehlergeruest::uebernehmen(&inhalt)?;
            println!("{}", tr!("Übernommen: {a} neu, {b} geändert, {c} unverändert. Gilt ab jetzt in allen Anzeigen und Berichten.", a = neu, b = geaendert, c = gleich));
            Ok(())
        }
        Command::Fehlertext { code, text } => {
            use ecu_werkbank::fehlertexte;
            match (code, text) {
                (Some(c), Some(t)) => {
                    fehlertexte::setzen(&c, &t)?;
                    if t.trim().is_empty() {
                        println!("{}", tr!("Fehlertext zu {a} gelöscht.", a = fehlertexte::norm(&c)));
                    } else {
                        println!("{}", tr!("Gespeichert: {a}  {b}", a = fehlertexte::norm(&c), b = t.trim()));
                    }
                }
                (Some(c), None) => match db.dtc_description(&c) {
                    Some(t) => println!("{}  {t}", fehlertexte::norm(&c)),
                    None => println!("{}", tr!("Kein Text zu {a}. Eintragen: ecu-werkbank fehlertext --code {b} --text \"…\"", a = fehlertexte::norm(&c), b = fehlertexte::norm(&c))),
                },
                (None, _) => {
                    let m = fehlertexte::alle();
                    if m.is_empty() {
                        println!("{}", tr!("Noch keine eigenen Fehlertexte."));
                    }
                    for (c, t) in m {
                        println!("{c:<8} {t}");
                    }
                }
            }
            Ok(())
        }
        Command::Wissen { code, neu, modell } => {
            let veh = cli.fahrzeug.as_deref().and_then(ecu_werkbank::vehicle::find);
            let model = modell.or_else(|| veh.as_ref().map(|v| v.bezeichnung.clone())).unwrap_or_default();
            match (code, neu) {
                (Some(c), Some(text)) => {
                    ecu_werkbank::wissen::hinzufuegen(ecu_werkbank::wissen::Eintrag { modell: model.clone(), code: c.clone(), text, datum: String::new() })?;
                    println!("{}", tr!("Gespeichert: {c} ({a}).", c = c, a = if model.is_empty() { "alle Modelle".into() } else { model }));
                }
                (Some(c), None) => {
                    let r = ecu_werkbank::wissen::suchen(&c, &model);
                    if r.is_empty() {
                        println!("{}", tr!("Noch keine Erfahrungen zu {c}.", c = c));
                    }
                    for (q, t) in r {
                        println!("  [{q}] {t}");
                    }
                }
                _ => {
                    for e in ecu_werkbank::wissen::eigene() {
                        println!("  {:<6} {:<20} {}  ({})", e.code, e.modell, e.text, e.datum);
                    }
                }
            }
            Ok(())
        }
        Command::Sicherheit => {
            let r = ecu_werkbank::report::scan(&be, &db, true, |_| {})?;
            let mut any = false;
            for m in r.modules.iter().filter(|m| ecu_werkbank::systems::sicherheit_modul(&m.protocol, m.address, &m.name)) {
                any = true;
                let a = m.address.unwrap_or(0);
                let name = if m.name.is_empty() && m.protocol != "TP 2.0" { ecu_werkbank::systems::name(a) } else { &m.name };
                println!("{}", tr!("{a} {name}  [{b}]  {c} Fehler", a = m.addr_text(), name = name, b = ecu_werkbank::systems::gruppe_modul(&m.protocol, m.address, &m.name), c = m.dtc_count()));
                for e in m.dtcs.iter().flatten() {
                    println!("    {}  {}", e.code, e.text);
                }
            }
            if !any {
                println!("{}", tr!("Keine Sicherheits-/Assistenz-Steuergeräte mit bekannter Adresse gefunden (Adressen über Labels zuordnen)."));
            }
            Ok(())
        }
        Command::CodierBit { tx, rx, byte, bit, wert, did, ja } => codier_bit(&be, tx, rx, did, byte, bit, &wert, ja, cli.fahrzeug.as_deref()),
        Command::Kaufcheck { out, note, aus_scan: true } => kaufcheck_aus_scan(out.as_deref(), &note, cli.fahrzeug.as_deref()),
        Command::Kaufcheck { out, note, aus_scan: false } => kaufcheck(&be, &db, out.as_deref(), &note, cli.fahrzeug.as_deref()),
        Command::NotfallSicherung => notfall_sicherung(&be, &db, cli.fahrzeug.as_deref()),
        Command::Ersatzteilpass => ersatzteilpass_neu(&be, cli.fahrzeug.as_deref()),
        Command::Fremd { out } => {
            let (mut r, s) = ecu_werkbank::fremd::scan(&be, &db, |p| eprint!("\r{p:<70}"))?;
            eprint!("\r{:<70}\r", "");
            if s.nur_obd {
                println!("{}", tr!("Nur OBD-Adressen gefragt ({a} CAN-IDs mit Verkehr gesehen).", a = s.verkehr));
            }
            for m in &r.modules {
                println!("{}  {}  {}", m.can, m.protocol, m.name);
                for i in &m.ident {
                    println!("    {i}");
                }
                match &m.dtcs {
                    None => println!("    {}", tr!("Fehlerspeicher nicht lesbar")),
                    Some(l) if l.is_empty() => println!("    {}", tr!("Keine Fehler gespeichert")),
                    Some(l) => {
                        for d in l {
                            println!("    {}  {}  [{}]", d.code, d.text, d.status);
                        }
                    }
                }
            }
            let v = r.assign_vehicle(cli.fahrzeug.as_deref())?;
            let out = match out {
                Some(p) => p,
                None => v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("fehlerspeicher", "pdf")),
            };
            std::fs::write(&out, r.to_pdf())?;
            let _ = ecu_werkbank::history::save(&r);
            println!("{}", tr!("PDF: {a}", a = out.display()));
            Ok(())
        }
        Command::Pruefplan { code } => {
            let p = ecu_werkbank::guide::plan_for(&code);
            let text = db.dtc_description(&code).or_else(|| db.dtc_description(&ecu_werkbank::guide::normalize(&code))).unwrap_or("");
            println!("{}", tr!("Prüfplan {code}: {a}{b}\n", code = code, a = p.titel, b = if text.is_empty() { String::new() } else { format!(" — {text}") }));
            for (i, st) in p.schritte.iter().enumerate() {
                let w = st.werkzeug.map(|w| format!("   [{}]", w.name())).unwrap_or_default();
                println!("  {}. {}{w}", i + 1, st.text);
            }
            Ok(())
        }
        Command::FehlerLoeschen { ja } => {
            let r = ecu_werkbank::report::scan(&be, &db, true, |_| {})?;
            let n = r.modules.iter().filter(|m| m.dtc_count() > 0).count();
            println!("{}", tr!("{n} Steuergerät(e) mit insgesamt {a} Fehler(n).", n = n, a = r.total_dtcs()));
            if !ja {
                println!("{}", tr!("Nichts gelöscht. Achtung: setzt auch die Readiness zurück (AU erst nach Fahrt wieder möglich). Mit --ja ausführen."));
                return Ok(());
            }
            for (m, res) in ecu_werkbank::repair::clear_all(&be, &r) {
                println!("  {m}: {}", match res { Ok(()) => tr!("gelöscht"), Err(e) => tr!("Fehler: {e}", e = e) });
            }
            Ok(())
        }
        Command::Reparaturbericht { arbeiten } => reparaturbericht(&be, &db, &arbeiten, cli.fahrzeug.as_deref()),
        Command::Verbindung => {
            println!("{}", tr!("Verbindung über {a}:\n", a = be.describe()));
            for (st, t) in ecu_werkbank::connect::check(&be) {
                println!("  [{}] {t}", st.text());
            }
            Ok(())
        }
        Command::Lernen { log, vergleich, out, adresse, name } => {
            let text = std::fs::read_to_string(&log).with_context(|| tr!("Lesen: {a}", a = log.display()))?;
            let cmp = vergleich.as_ref().map(std::fs::read_to_string).transpose()?;
            let v = ecu_werkbank::learn::lernen(&text, cmp.as_deref());
            if v.is_empty() {
                println!("{}", tr!("Keine Stellfunktionen, Schreibbefehle oder Messwerte gefunden."));
                return Ok(());
            }
            for x in &v {
                println!("• {}", x.titel);
            }
            let file = ecu_werkbank::learn::als_label_datei(&v, adresse, &name);
            match out {
                Some(p) => {
                    LabelDb::check_file(&file).context(tr!("erzeugte Label-Datei ist ungültig"))?;
                    std::fs::write(&p, &file)?;
                    println!("{}", tr!("\nLabel-Datei gespeichert: {a} — Namen, Einheiten und Grenzen prüfen!", a = p.display()));
                }
                None => println!("\n{file}"),
            }
            Ok(())
        }
        Command::ObdTests => obd_tests(&be),
        Command::Gemisch { seconds } => gemisch(&be, seconds, cli.fahrzeug.as_deref()),
        Command::Trends => {
            let v = need_vehicle(cli.fahrzeug.as_deref())?;
            let s = ecu_werkbank::trend::series(&v.key);
            if s.is_empty() {
                println!("{}", tr!("Noch keine Trend-Daten. Sie entstehen bei check, batterie und gemisch (mit -f)."));
            }
            for (name, pts) in &s {
                let vals: Vec<String> = pts.iter().rev().take(8).rev().map(|p| ecu_werkbank::csvlog::num(p.wert)).collect();
                println!("  {name:<38} {}  {}", vals.join(" → "), pts.last().map(|p| p.einheit.as_str()).unwrap_or(""));
            }
            let hints = ecu_werkbank::trend::analyze(&s);
            if !hints.is_empty() {
                println!();
            }
            for (st, t) in hints {
                println!("  [{}] {t}", st.text());
            }
            Ok(())
        }
        Command::Ziel { pid, muster, toleranz, seconds } => ziel(&be, pid as u8, &muster, toleranz, seconds),
        Command::Stellen { funktion, muster, seconds, ja } => stellen(&be, &db, &funktion, &muster, seconds, ja),
        Command::Handy { port, pids, seconds } => handy(&be, &db, port, &pids, seconds, cli.fahrzeug.as_deref()),
        Command::Check => quick_check(&be, &db, cli.fahrzeug.as_deref()),
        Command::Batterie { seconds } => battery_test(&be, seconds, cli.fahrzeug.as_deref()),
        Command::Akte { out } => {
            let v = need_vehicle(cli.fahrzeug.as_deref())?;
            let out = match out {
                Some(p) => p,
                None => v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("fahrzeugakte", "pdf")),
            };
            std::fs::write(&out, ecu_werkbank::akte::to_pdf(&v, &db))?;
            println!("{}", tr!("Fahrzeugakte: {a}", a = out.display()));
            Ok(())
        }
        Command::AkteZip { out } => {
            let v = need_vehicle(cli.fahrzeug.as_deref())?;
            let n = ecu_werkbank::zip::zip_dir(&v.dir(), &out, &v.key)?;
            println!("{}", tr!("{n} Datei(en) gesichert: {a}", n = n, a = out.display()));
            println!("{}", tr!("Wiederherstellen: ecu-werkbank fahrzeug-import {a}", a = out.display()));
            Ok(())
        }
        Command::Datensicherung { ziel, tage, behalten } => {
            use ecu_werkbank::datensicherung as ds;
            let mut e = ds::laden();
            if ziel.is_some() || tage.is_some() || behalten.is_some() {
                if let Some(z) = ziel {
                    e.ziel = z.trim().to_string();
                }
                e.tage = tage.unwrap_or(e.tage).max(1);
                e.behalten = behalten.unwrap_or(e.behalten).max(1);
                ds::speichern(&e)?;
                if e.ziel.is_empty() {
                    println!("{}", tr!("Automatische Datensicherung ausgeschaltet."));
                } else {
                    println!("{}", tr!("Datensicherung eingerichtet: alle {a} Tage nach {b}", a = e.tage, b = &e.ziel));
                }
                return Ok(());
            }
            if e.ziel.trim().is_empty() {
                anyhow::bail!(tr!("Noch kein Sicherungsordner eingerichtet:  ecu-werkbank datensicherung --ziel /media/…/ecu-werkbank"));
            }
            let (p, n) = ds::sichern(std::path::Path::new(e.ziel.trim()), e.behalten)?;
            println!("{}", tr!("Datensicherung: {a} Dateien gesichert in {b}", a = n, b = p.display()));
            Ok(())
        }
        Command::DatensicherungZurueck { datei, ueberschreiben } => {
            let (ein, ueber) = ecu_werkbank::datensicherung::zurueckspielen(&datei, ueberschreiben)?;
            println!("{}", tr!("Eingespielt: {a} Datei(en), {b} schon vorhanden (unverändert).", a = ein, b = ueber));
            Ok(())
        }
        Command::FahrzeugImport { datei, ueberschreiben } => {
            let root = ecu_werkbank::vehicle::root();
            let dateien = ecu_werkbank::zip::unzip_to(&datei, &root, ueberschreiben)?;
            // betroffene Fahrzeuge = oberste Ordnerebene der Einträge
            let mut keys: Vec<String> = dateien.iter().filter_map(|p| p.split('/').next().map(str::to_string)).collect();
            keys.sort();
            keys.dedup();
            println!("{}", tr!("{a} Datei(en) eingespielt nach {b}", a = dateien.len(), b = root.display()));
            for k in &keys {
                let label = ecu_werkbank::vehicle::load(k).map(|v| v.label()).unwrap_or_else(|| k.clone());
                println!("  {label}  ({k})");
            }
            if dateien.is_empty() {
                println!("{}", tr!("Nichts eingespielt — ist das eine akte-zip-Datei?"));
            }
            Ok(())
        }
        Command::Wartung => {
            let v = need_vehicle(cli.fahrzeug.as_deref())?;
            if v.wartung.is_empty() {
                println!("{}", tr!("Kein Wartungsplan. Vorlage anlegen mit:  ecu-werkbank -f \"{a}\" wartung-vorlage", a = v.label()));
            }
            let heute = ecu_werkbank::vehicle::today();
            println!("{}", tr!("{a}  —  km-Stand: {b}", a = v.label(), b = v.km.map(|k| format!("{k} km ({})", v.km_datum)).unwrap_or_else(|| "unbekannt".into())));
            for w in &v.wartung {
                let f = w.faellig(v.km, &heute);
                println!("  [{:<7}] {:<34} {}", f.stufe.text(), w.name, f.text);
            }
            Ok(())
        }
        Command::WartungVorlage => {
            let mut v = need_vehicle(cli.fahrzeug.as_deref())?;
            let mut n = 0;
            for w in ecu_werkbank::vehicle::wartung_vorlage() {
                if !v.wartung.iter().any(|x| x.name == w.name) {
                    v.wartung.push(w);
                    n += 1;
                }
            }
            ecu_werkbank::vehicle::save(&v)?;
            println!("{}", tr!("{n} Posten angelegt. Letzte Durchführung eintragen mit:  wartung-erledigt --name … --km …", n = n));
            Ok(())
        }
        Command::WartungErledigt { name, km } => {
            let mut v = need_vehicle(cli.fahrzeug.as_deref())?;
            let full = v
                .wartung
                .iter()
                .find(|w| w.name.to_lowercase().starts_with(&name.to_lowercase()))
                .map(|w| w.name.clone())
                .with_context(|| tr!("Kein Wartungsposten beginnt mit '{name}'", name = name))?;
            if let Some(k) = km {
                v.note_km(k);
            }
            v.wartung_erledigt(&full, km);
            ecu_werkbank::vehicle::save(&v)?;
            println!("{}", tr!("Eingetragen: {full} am {a}", full = full, a = ecu_werkbank::vehicle::today()));
            Ok(())
        }
        Command::Teile => {
            let v = need_vehicle(cli.fahrzeug.as_deref())?;
            if v.teile.is_empty() {
                println!("{}", tr!("Noch keine Teile eingetragen. Eintragen mit:  ecu-werkbank -f \"{a}\" teil \"Bremsbeläge vorn\" --km …", a = v.label()));
            }
            for t in &v.teile {
                let km = t.km.map(ecu_werkbank::report::group_thousands).unwrap_or_else(|| "–".into());
                let extra = [t.teilenummer.as_str(), t.notiz.as_str()].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join(" · ");
                let preis = t.preis.map(ecu_werkbank::kosten::euro).unwrap_or_default();
                println!("  {:<10} {:>9} km  {:<34} {:>12}  {}", t.datum, km, t.name, preis, extra);
            }
            Ok(())
        }
        Command::Tanken { liter, km, preis, datum, teil, notiz } => {
            let mut v = need_vehicle(cli.fahrzeug.as_deref())?;
            let datum = datum.unwrap_or_default();
            if !datum.trim().is_empty() && ecu_werkbank::vehicle::days_of(&datum).is_none() {
                anyhow::bail!(tr!("Datum bitte als TT.MM.JJJJ eingeben."));
            }
            let l = ecu_werkbank::kosten::zahl_lesen(&liter).filter(|&l| l > 0.0).with_context(|| tr!("Liter bitte als Zahl eingeben, z. B. 42,5"))?;
            let preis = preis_lesen(preis.as_deref())?;
            v.tanken_eintragen(ecu_werkbank::vehicle::Tankung { datum, km, liter: l, preis, voll: !teil, notiz });
            ecu_werkbank::vehicle::save(&v)?;
            let vb = ecu_werkbank::kosten::verbrauch(&v.tankbuch);
            match vb.abschnitte.iter().find(|a| a.km == km) {
                Some(a) => println!("{}", tr!("Getankt: {a} l — {b} l/100 km seit der letzten vollen Füllung.", a = ecu_werkbank::kosten::komma(l, 2), b = ecu_werkbank::kosten::komma(a.l_100km, 1))),
                None => println!("{}", tr!("Getankt: {a} l", a = ecu_werkbank::kosten::komma(l, 2))),
            }
            Ok(())
        }
        Command::Tankbuch => {
            use ecu_werkbank::kosten::{euro, komma};
            let v = need_vehicle(cli.fahrzeug.as_deref())?;
            if v.tankbuch.is_empty() {
                println!("{}", tr!("Noch keine Tankfüllung. Eintragen mit:  ecu-werkbank -f \"{a}\" tanken 42,5 --km … --preis …", a = v.label()));
                return Ok(());
            }
            let vb = ecu_werkbank::kosten::verbrauch(&v.tankbuch);
            for t in &v.tankbuch {
                let l100 = vb.abschnitte.iter().find(|a| a.km == t.km).map(|a| format!("{} l/100", komma(a.l_100km, 1))).unwrap_or_default();
                let art = if t.voll { String::new() } else { tr!(" (teil)") };
                println!("  {:<10} {:>9} km  {:>7} l{:<7} {:>11}  {:>10}  {}", t.datum, ecu_werkbank::report::group_thousands(t.km), komma(t.liter, 2), art, t.preis.map(euro).unwrap_or_default(), l100, t.notiz);
            }
            if let Some(s) = vb.schnitt {
                println!("{}", tr!("Verbrauch im Schnitt: {a} l/100 km", a = komma(s, 1)));
            }
            if let Some(p) = vb.euro_pro_liter {
                println!("{}", tr!("im Schnitt {a} €/l", a = komma(p, 3)));
            }
            Ok(())
        }
        Command::Kosten => {
            use ecu_werkbank::kosten::{euro, komma};
            let v = need_vehicle(cli.fahrzeug.as_deref())?;
            let k = ecu_werkbank::kosten::kosten(&v, &ecu_werkbank::vehicle::today());
            if k.gesamt <= 0.0 {
                println!("{}", tr!("Noch keine Preise eingetragen — bei Teilen und Tankfüllungen den Preis angeben."));
                return Ok(());
            }
            println!("{}", tr!("Seit {a}: {b} gesamt (Teile {c}, Tanken {d})", a = &k.von, b = euro(k.gesamt), c = euro(k.teile), d = euro(k.tanken)));
            if let Some(m) = k.je_monat {
                println!("  {}", tr!("{a} pro Monat", a = euro(m)));
            }
            if let Some(t) = k.je_1000km {
                println!("  {}", tr!("{a} pro 1000 km", a = euro(t)));
            }
            if let Some(s) = k.strecke_km {
                println!("  {}", tr!("{a} km gefahren", a = ecu_werkbank::report::group_thousands(s)));
            }
            for (j, w) in k.jahre.iter().rev() {
                println!("  {j}:  {}", tr!("Teile {a}, Tanken {b} ({c} l), gesamt {d}", a = euro(w.teile), b = euro(w.tanken), c = komma(w.liter, 0), d = euro(w.teile + w.tanken)));
            }
            Ok(())
        }
        Command::Kalender { out } => {
            let v = need_vehicle(cli.fahrzeug.as_deref())?;
            let t = ecu_werkbank::kalender::termine(&v, &ecu_werkbank::vehicle::today());
            if t.is_empty() {
                anyhow::bail!(tr!("Keine Termine: Beim Wartungsplan erst die letzte Durchführung eintragen."));
            }
            for x in &t {
                println!("  {:<10}{} {:<34} {}", x.datum, if x.geschaetzt { "~" } else { " " }, x.posten, x.hinweis);
            }
            let pfad = match out {
                Some(p) => p,
                None => v.sub("berichte")?.join("wartung.ics"),
            };
            std::fs::write(&pfad, ecu_werkbank::kalender::ics(&v, &t)).with_context(|| tr!("Schreiben: {a}", a = pfad.display()))?;
            println!("{}", tr!("{a} Termine gespeichert: {b} — Datei aufs Handy schicken oder im Kalender importieren.", a = t.len(), b = pfad.display()));
            Ok(())
        }
        Command::Teil { name, km, datum, teilenummer, notiz, preis } => {
            let mut v = need_vehicle(cli.fahrzeug.as_deref())?;
            let datum = datum.unwrap_or_default();
            if !datum.trim().is_empty() && ecu_werkbank::vehicle::days_of(&datum).is_none() {
                anyhow::bail!(tr!("Datum bitte als TT.MM.JJJJ eingeben."));
            }
            let preis = preis_lesen(preis.as_deref())?;
            let km = km.or(v.km);
            let posten = v.teil_eintragen(ecu_werkbank::vehicle::Teil { datum, km, name: name.clone(), teilenummer, notiz, preis });
            ecu_werkbank::vehicle::save(&v)?;
            println!("{}", tr!("Eingetragen: {a}", a = name.trim()));
            if let Some(p) = posten {
                println!("{}", tr!("Wartungsposten „{p}“ mit aktualisiert.", p = p));
            }
            Ok(())
        }
        Command::Km { wert } => {
            let mut v = need_vehicle(cli.fahrzeug.as_deref())?;
            v.km = Some(wert);
            v.km_datum = ecu_werkbank::vehicle::today();
            ecu_werkbank::vehicle::save(&v)?;
            println!("{}", tr!("{a}: {wert} km eingetragen.", a = v.label(), wert = wert));
            Ok(())
        }
        Command::Reparatur { text, datei } => {
            let path = match datei {
                Some(p) => p,
                None => {
                    let v = need_vehicle(cli.fahrzeug.as_deref())?;
                    ecu_werkbank::history::list(&v.key).pop().map(|(p, _)| p).context(tr!("Noch kein Scan für dieses Fahrzeug"))?
                }
            };
            ecu_werkbank::history::set_repair_note(&path, &text)?;
            println!("{}", tr!("Notiz gespeichert in {a}", a = path.display()));
            Ok(())
        }
        Command::FahrzeugNeu { kennzeichen, vin, name } => {
            let v = ecu_werkbank::vehicle::create(&kennzeichen, &vin, &name)?;
            println!("{}", tr!("Angelegt: {a}  →  {b}", a = v.label(), b = v.dir().display()));
            Ok(())
        }
        Command::CsvPdf { file, out } => csv_pdf(&file, out.as_deref(), cli.fahrzeug.as_deref()),
        Command::CodingBackup { no_tp20 } => coding_backup(&be, &db, !no_tp20, cli.fahrzeug.as_deref()),
        Command::CodingList => coding_list(),
        Command::CodingCompare { old, new } => coding_compare(old.as_deref(), new.as_deref(), &db),
        Command::CodingRestore { file, tx, ja } => coding_restore(&be, &file, tx, ja),
        Command::Compare { old, new } => compare(cli.fahrzeug.as_deref(), old.as_deref(), new.as_deref()),
        Command::Coding { tx, rx, did, ecu } => coding_read(&be, tx, rx, did, ecu, &db),
        Command::DecodeCoding { ecu, tx, bytes } => {
            let raw = parse_hex_bytes(&bytes)?;
            print_coding(ecu, tx, &raw, &db);
            Ok(())
        }
        Command::CodeWrite { tx, rx, did, bytes, ja } => {
            let raw = parse_hex_bytes(&bytes)?;
            ecu_werkbank::km::schreiben_erlaubt(did, tx)?;
            let plan = tr!("Kennung {did} an {tx}/{rx} schreiben: {d}", did = format!("{:#06x}", did), tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx), d = bytes.trim());
            if !ja {
                println!("{}", tr!("Vorschau (nichts gesendet): {a}", a = plan));
                println!("{}", tr!("Wirklich senden mit --ja. Nur am stehenden Auto."));
                return Ok(());
            }
            println!("{}", tr!("Sende: {a}", a = plan));
            let tp = be.open(tx, rx)?;
            let uds = Uds::new(&*tp);
            uds.diagnostic_session(ecu_werkbank::uds::SESSION_EXTENDED)?;
            uds.write_data_by_id(did, &raw)?;
            println!("{}", tr!("Codierung/Anpassung {did} geschrieben ({a} Byte).", did = format!("{:#06x}", did), a = raw.len()));
            Ok(())
        }
        Command::Routine { tx, rx, sub, id, data, ja } => {
            let sub = ecu_werkbank::uds::routine_sub_pruefen(sub)?;
            let raw = parse_hex_bytes(&data)?;
            let plan = tr!(
                "Routine {id} {was} (Sub {sub}) an {tx}/{rx}, Daten: {d}",
                id = format!("{:#06x}", id),
                was = ecu_werkbank::uds::routine_sub_text(sub),
                sub = sub,
                tx = format!("{:#05x}", tx),
                rx = format!("{:#05x}", rx),
                d = if raw.is_empty() { "—".to_string() } else { data.clone() }
            );
            if !ja {
                println!("{}", tr!("Vorschau (nichts gesendet): {a}", a = plan));
                println!("{}", tr!("Wirklich senden mit --ja. Nur am stehenden Auto."));
                return Ok(());
            }
            println!("{}", tr!("Sende: {a}", a = plan));
            let tp = be.open(tx, rx)?;
            let uds = Uds::new(&*tp);
            uds.diagnostic_session(ecu_werkbank::uds::SESSION_EXTENDED)?;
            let resp = uds.routine_control(sub, id, &raw)?;
            let hex: Vec<String> = resp.iter().map(|b| format!("{b:02X}")).collect();
            println!("{}", tr!("RoutineControl {id} sub={sub}: Antwort [{a}]", id = format!("{:#06x}", id), sub = sub, a = hex.join(" ")));
            Ok(())
        }
        Command::Iocontrol { tx, rx, did, control, state, ja } => {
            let control = u8::try_from(control).map_err(|_| anyhow::anyhow!(tr!("Control ungültig (0x00–0xFF)")))?;
            let raw = parse_hex_bytes(&state)?;
            let plan = tr!(
                "Stellglied {did} ansteuern (Control {c}) an {tx}/{rx}, Wert: {d}",
                did = format!("{:#06x}", did),
                c = format!("{:#04x}", control),
                tx = format!("{:#05x}", tx),
                rx = format!("{:#05x}", rx),
                d = if raw.is_empty() { "—".to_string() } else { state.clone() }
            );
            if !ja {
                println!("{}", tr!("Vorschau (nichts gesendet): {a}", a = plan));
                println!("{}", tr!("Wirklich senden mit --ja. Nur am stehenden Auto."));
                return Ok(());
            }
            println!("{}", tr!("Sende: {a}", a = plan));
            let tp = be.open(tx, rx)?;
            let uds = Uds::new(&*tp);
            uds.diagnostic_session(ecu_werkbank::uds::SESSION_EXTENDED)?;
            let resp = uds.io_control(did, control, &raw)?;
            let hex: Vec<String> = resp.iter().map(|b| format!("{b:02X}")).collect();
            println!("{}", tr!("IOControl {did} ctrl={control}: Antwort [{a}]", did = format!("{:#06x}", did), control = format!("{:#04x}", control), a = hex.join(" ")));
            Ok(())
        }
    }
}

/// (2) Auto-Scan über die im Label hinterlegten CAN-IDs plus den OBD-Bereich.
fn autoscan(be: &Backend, db: &LabelDb, with_tp20: bool) -> Result<()> {
    println!("{}", tr!("Auto-Scan über {a}…\n", a = be.describe()));
    // Ziele sammeln: ECUs mit CAN-IDs aus den Labels + OBD 0x7E0..0x7E7.
    let targets = ecu_werkbank::report::uds_targets(be, db);

    let mut found = 0;
    for (addr, tx, rx) in targets {
        let probe = (|| -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
            let tp = be.open(tx, rx)?;
            let uds = Uds::new(&*tp);
            uds.diagnostic_session(ecu_werkbank::uds::SESSION_DEFAULT)?;
            // Teilenummer (0xF187) als Lebenszeichen versuchen.
            let d = uds.read_data_by_id(0xF187).or_else(|_| uds.read_data_by_id(0xF190))?;
            let sys = uds.read_data_by_id(0xF197).unwrap_or_default();
            Ok((d, sys))
        })();
        if let Ok((data, sys)) = probe {
            found += 1;
            let sys = String::from_utf8_lossy(&sys).trim().to_string();
            let name = addr
                .and_then(|a| db.ecu_name(a).map(|n| n.to_string()))
                .unwrap_or_else(|| {
                    let n = ecu_werkbank::kwp::tp20_name(0, std::slice::from_ref(&sys));
                    if n.is_empty() { sys.clone() } else { n }
                });
            let info = if data.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
                String::from_utf8_lossy(&data).to_string()
            } else {
                data.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join("")
            };
            let addr_s = addr.map(|a| format!("{a:#04x} ")).unwrap_or_else(|| "UDS ".into());
            println!("{}", tr!("  ● {addr_s}{name}  tx={tx} rx={rx}  [{info}]", addr_s = addr_s, name = name, tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx), info = info));
        }
    }
    if with_tp20 && be.supports_tp20() {
        println!("\n{}", tr!("TP 2.0 (KWP2000), Adressen 0x01..0x7F…"));
        for info in kwp::sweep(be, |_| {})? {
            let addr = info.address;
            found += 1;
            let _ = db;
            println!("{}", tr!("  ● TP2.0 {addr} {a}  [{b}]", addr = format!("{:#04x}", addr), a = info.name, b = info.ident.join(" | ")));
        }
    }
    if found == 0 {
        println!("{}", tr!("Keine Steuergeräte erreichbar. Hardware/Bitrate prüfen."));
    } else {
        println!("{}", tr!("\n{found} Steuergerät(e) gefunden.", found = found));
    }
    Ok(())
}

/// Verbauliste: alle verbauten Steuergeräte, nach System geordnet, mit
/// Teilenummer und Status. Live vom Auto (voller Suchlauf) oder aus dem
/// letzten gespeicherten Scan (`--gespeichert`). Nur lesend.
fn verbauliste(be: &Backend, db: &LabelDb, with_tp20: bool, gespeichert: bool, out: Option<&std::path::Path>, fahrzeug: Option<&str>) -> Result<()> {
    let r = if gespeichert {
        let key = active_vehicle(fahrzeug)?.context(tr!("--gespeichert braucht --fahrzeug <Kennzeichen/VIN>"))?;
        let scans = ecu_werkbank::history::list(&key);
        scans.into_iter().next_back().map(|(_, r)| r).with_context(|| tr!("Keine gespeicherten Scans für {key}.", key = key))?
    } else {
        println!("{}", tr!("Verbauliste über {a}…\n", a = be.describe()));
        let r = ecu_werkbank::report::scan_voll(be, db, with_tp20, |p| eprint!("\r{p:<40}"))?;
        eprintln!();
        r
    };
    let gruppen = ecu_werkbank::verbauliste::aus_scan(&r);
    let t = ecu_werkbank::verbauliste::text(&gruppen);
    print!("{t}");
    if let Some(p) = out {
        std::fs::write(p, &t).with_context(|| tr!("Schreiben: {a}", a = p.display()))?;
        println!("{}", tr!("Gespeichert: {a}", a = p.display()));
    }
    Ok(())
}

/// Fehlerspeicher-Protokoll: alle Steuergeräte abfragen, anzeigen und speichern.
/// `--csv name.csv` (ohne Ordner) mit `--fahrzeug` → Fahrzeugordner/aufzeichnungen/name.csv
fn csv_target(csv: Option<PathBuf>, fahrzeug: Option<&str>) -> Result<Option<PathBuf>> {
    let Some(p) = csv else { return Ok(None) };
    let bare = p.parent().is_none_or(|d| d.as_os_str().is_empty());
    match (bare, fahrzeug) {
        (true, Some(q)) => {
            let v = ecu_werkbank::vehicle::find(q).with_context(|| tr!("Fahrzeug '{q}' unbekannt", q = q))?;
            Ok(Some(v.sub("aufzeichnungen")?.join(p)))
        }
        _ => Ok(Some(p)),
    }
}

fn active_vehicle(q: Option<&str>) -> Result<Option<String>> {
    match q {
        None => Ok(None),
        Some(q) => match ecu_werkbank::vehicle::find(q) {
            Some(v) => Ok(Some(v.key)),
            None => anyhow::bail!(tr!("Fahrzeug '{q}' unbekannt — anlegen mit: ecu-werkbank fahrzeug-neu --kennzeichen \"{q}\"", q = q)),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn report(
    be: &Backend,
    out: Option<&std::path::Path>,
    note: &str,
    db: &LabelDb,
    with_tp20: bool,
    keep: bool,
    voll: bool,
    fahrzeug: Option<&str>,
) -> Result<()> {
    use ecu_werkbank::history;
    let active = active_vehicle(fahrzeug)?;
    let mut r = ecu_werkbank::report::scan_opts(be, db, with_tp20, voll, |p| eprint!("\r{p:<40}"))?;
    eprint!("\r{:<40}\r", "");
    r.note = note.to_string();
    let v = r.assign_vehicle(active.as_deref())?;
    r.comparison = history::compare_with_previous(&r);
    print!("{}", r.to_text());
    let out = match out {
        Some(p) => p.to_path_buf(),
        None => v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("fehlerspeicher", "pdf")),
    };
    r.save(&out)?;
    println!("{}", tr!("\nGespeichert: {a}", a = out.display()));
    if keep && !r.modules.is_empty() {
        let p = history::save(&r)?;
        println!("{}", tr!("In Historie: {a}", a = p.display()));
    }
    Ok(())
}

fn fahrzeuge() -> Result<()> {
    let list = ecu_werkbank::vehicle::list();
    if list.is_empty() {
        println!("{}", tr!("Noch keine Fahrzeuge. Anlegen mit:  ecu-werkbank fahrzeug-neu --kennzeichen \"B-XY 123\" --name \"Golf 6\""));
        println!("{}", tr!("(Ein Scan mit lesbarer Fahrgestellnummer legt das Fahrzeug auch automatisch an.)"));
    }
    for v in list {
        let st: Vec<String> = ecu_werkbank::vehicle::stats(&v).into_iter().filter(|(_, n)| *n > 0).map(|(s, n)| format!("{n} {s}")).collect();
        let vin = if v.vin.is_empty() { String::new() } else { format!("  VIN {}", v.vin) };
        println!("{}{vin}", v.label());
        println!("    {}  {}", v.dir().display(), if st.is_empty() { "(leer)".into() } else { st.join(", ") });
    }
    Ok(())
}

fn csv_pdf(file: &std::path::Path, out: Option<&std::path::Path>, fahrzeug: Option<&str>) -> Result<()> {
    let cols = ecu_werkbank::csvlog::read(file)?;
    let mut meta = vec![
        ("Datum".to_string(), ecu_werkbank::report::now_local()),
        ("Quelle".to_string(), file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
    ];
    let veh = fahrzeug.and_then(ecu_werkbank::vehicle::find);
    if let Some(v) = &veh {
        meta.insert(0, (tr!("Fahrzeug"), v.label()));
    }
    let out = match (out, &veh) {
        (Some(p), _) => p.to_path_buf(),
        (None, Some(v)) => v.sub("scope")?.join(file.with_extension("pdf").file_name().unwrap_or_default()),
        (None, None) => file.with_extension("pdf"),
    };
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out, ecu_werkbank::chart::to_pdf(ecu_werkbank::trs!("Messwert-Aufzeichnung"), &meta, &cols))?;
    println!("{}", tr!("{a} Kurve(n) → {b}", a = cols.len(), b = out.display()));
    Ok(())
}

fn history(fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::history;
    match active_vehicle(fahrzeug)? {
        None => {
            let v = history::vehicles();
            if v.is_empty() {
                println!("{}", tr!("Noch keine Scans gespeichert. Ein Scan landet automatisch dort mit:  ecu-werkbank report"));
            }
            for (veh, n, last) in v {
                println!("{}", tr!("{a} {n} Scan(s), zuletzt {last}", a = format!("{:<32}", veh.label()), n = format!("{:>3}", n), last = last));
            }
        }
        Some(key) => {
            let scans = history::list(&key);
            if scans.is_empty() {
                println!("{}", tr!("Keine Scans für {key}.", key = key));
            }
            for (path, r) in scans {
                println!("{}", tr!("{a}  {b} Steuergerät(e)  {c} Fehler  {d}", a = r.created, b = format!("{:>2}", r.modules.len()), c = format!("{:>2}", r.total_dtcs()), d = path.display()));
            }
        }
    }
    Ok(())
}

fn compare(fahrzeug: Option<&str>, old: Option<&std::path::Path>, new: Option<&std::path::Path>) -> Result<()> {
    use ecu_werkbank::history;
    let (a, b, earlier) = match (old, new) {
        (Some(o), Some(n)) => (history::load(o)?, history::load(n)?, Vec::new()),
        _ => {
            let key = match active_vehicle(fahrzeug)? {
                Some(k) => k,
                None => match history::vehicles().as_slice() {
                    [(v, _, _)] => v.key.clone(),
                    [] => anyhow::bail!(tr!("Noch keine Scans in der Historie.")),
                    _ => anyhow::bail!(tr!("Mehrere Fahrzeuge — bitte --fahrzeug angeben (siehe `ecu-werkbank history`).")),
                },
            };
            let mut all: Vec<_> = history::list(&key).into_iter().map(|(_, r)| r).collect();
            if all.len() < 2 {
                anyhow::bail!(tr!("Für {key} gibt es erst {a} Scan(s) — zum Vergleich braucht es zwei.", key = key, a = all.len()));
            }
            let b = all.pop().expect("≥2");
            let a = all.pop().expect("≥2");
            (a, b, all)
        }
    };
    let c = history::Comparison { since: a.created.clone(), diff: history::diff(&a, &b, &earlier) };
    println!("{}", tr!("Scan vom {a}  →  Scan vom {b}\n", a = a.created, b = b.created));
    print!("{}", c.to_text());
    Ok(())
}

fn coding_read(be: &Backend, tx: u16, rx: u16, did: u16, ecu: u16, db: &LabelDb) -> Result<()> {
    let tp = be.open(tx, rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(ecu_werkbank::uds::SESSION_DEFAULT)?;
    let bytes = uds.read_data_by_id(did)?;
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    println!("{}", tr!("Codierung {did} = {a}", did = format!("{:#06x}", did), a = hex.join(" ")));
    print_coding(Some(ecu), Some(tx), &bytes, db);
    Ok(())
}

/// Codier-Bytes aufschlüsseln: alle Bytes als Bits, dazu die Namen aus deinen
/// Codiertexten (über `tx`) und `[[coding]]`-Labels (über `ecu`).
fn print_coding(ecu: Option<u16>, tx: Option<u16>, bytes: &[u8], db: &LabelDb) {
    let labels = ecu.and_then(|e| db.coding(e)).unwrap_or(&[]);
    // Ohne CAN-ID: Labels allein (eigene Texte hängen an der CAN-ID)
    let tx_key = tx.unwrap_or(u16::MAX);
    let texte = if tx.is_some() { ecu_werkbank::codiertexte::Texte::laden() } else { Default::default() };
    println!("{}", tr!("Aufschlüsselung:"));
    for z in texte.aufschluesseln(tx_key, labels, bytes) {
        println!("  {z}");
    }
    if labels.is_empty() && texte.anzahl(tx_key) == 0 {
        println!("{}", tr!("Noch keine Bit-Namen. Eintragen mit:  ecu-werkbank codiertext --tx … --byte … --bit … --text \"…\"  (oder in der GUI: Seite „Codierung“)"));
    }
}

/// Eigene Codiertexte zeigen oder setzen.
fn codiertext(tx: Option<u16>, byte: Option<usize>, bit: Option<u8>, text: Option<String>) -> Result<()> {
    use ecu_werkbank::codiertexte;
    if bit.is_some_and(|b| b > 7) {
        anyhow::bail!(tr!("Bit 0–7"));
    }
    match (tx, byte, text) {
        (Some(tx), Some(byte), Some(t)) => {
            codiertexte::setzen(tx, byte, bit, &t)?;
            let k = codiertexte::schluessel(tx, byte, bit);
            if t.trim().is_empty() {
                println!("{}", tr!("Text zu {a} gelöscht.", a = k));
            } else {
                println!("{}", tr!("Gespeichert: {a}  {b}", a = k, b = t.trim()));
            }
        }
        (_, _, Some(_)) => anyhow::bail!(tr!("Zum Setzen --tx und --byte angeben")),
        (Some(tx), _, None) => {
            let t = codiertexte::Texte::laden();
            let v = t.fuer(tx);
            if v.is_empty() {
                println!("{}", tr!("Noch keine Texte für Steuergerät {a}.", a = format!("{tx:03X}")));
            }
            for (byte, bit, text) in v {
                let wo = match bit {
                    Some(b) => tr!("Byte {a} Bit {b}", a = byte, b = b),
                    None => tr!("Byte {a}", a = byte),
                };
                println!("  {wo:<14}  {text}");
            }
        }
        _ => {
            let m = codiertexte::alle();
            if m.is_empty() {
                println!("{}", tr!("Noch keine eigenen Codiertexte. Beispiel: ecu-werkbank codiertext --tx 714 --byte 0 --bit 3 --text \"Tagfahrlicht\""));
            }
            for (k, t) in m {
                println!("{k:<10}  {t}");
            }
        }
    }
    Ok(())
}

/// Probiert die CAN-Diagnosewelten durch und meldet, welche das Fahrzeug spricht.
/// Hat das Steuergerät geantwortet? Auch eine Ablehnung (NRC) ist eine Antwort.
fn hat_geantwortet<T>(r: &Result<T>) -> bool {
    match r {
        Ok(_) => true,
        Err(e) => format!("{e:#}").contains("NRC"),
    }
}

fn detect(be: &Backend) -> Result<()> {
    println!("{}", tr!("Erkenne Diagnose-Generation über {a}…\n", a = be.describe()));

    // 1) UDS-Motor: Session auf den OBD-IDs (moderne Autos, ~2008+).
    print!("  UDS Motor (0x7E0)… ");
    let uds_motor = be.open(0x7E0, 0x7E8).map(|tp| hat_geantwortet(&Uds::new(&*tp).diagnostic_session(0x01))).unwrap_or(false);
    println!("{}", if uds_motor { tr!("antwortet ✓") } else { tr!("keine Antwort") });

    // 2) UDS-Kombi (0x714): am Golf 6 die zuverlässige UDS-Quelle.
    print!("  UDS Kombi (0x714)… ");
    let uds_kombi = be.open(0x714, 0x77E).map(|tp| hat_geantwortet(&Uds::new(&*tp).read_data_by_id(0xF190))).unwrap_or(false);
    println!("{}", if uds_kombi { tr!("antwortet ✓") } else { tr!("keine Antwort") });

    // 3) OBD-II (Mode 09 VIN): zeigt, dass überhaupt CAN/OBD da ist.
    print!("  OBD-II (Mode 09, VIN)… ");
    let vin = be.open(ecu_werkbank::obd::DEFAULT_TX, ecu_werkbank::obd::DEFAULT_RX).ok().and_then(|tp| ecu_werkbank::obd::Obd::new(&*tp).vin().ok()).filter(|v| v.len() == 17);
    println!("{}", match &vin { Some(v) => tr!("{a} ✓", a = v), None => tr!("keine Antwort") });

    // 4) TP 2.0 (~2004–2010): Kanal zu einer von mehreren Adressen (Motor, Bordnetz, Gateway).
    print!("  {} ", tr!("TP 2.0 (KWP2000, Kanäle 0x01/0x20/0x1f)…"));
    let tp20_ok = [0x01u8, 0x20, 0x1f].iter().any(|&a| be.open_tp20(a, Duration::from_millis(1000)).is_ok());
    println!("{}", if tp20_ok { tr!("Kanal offen ✓") } else { tr!("kein Kanal") });

    let uds = uds_motor || uds_kombi;
    println!();
    match (uds, tp20_ok, vin.is_some()) {
        // Golf-6-typisch: UDS-Gateway/Kombi + TP-2.0-Steuergeräte nebeneinander
        (true, true, _) => println!("{}", tr!("→ Gemischt (UDS + TP 2.0), typisch ~2009–2013. Nutze `autoscan` (deckt beide ab), dann `check`.")),
        (true, false, _) => println!("{}", tr!("→ Fahrzeug spricht UDS. Nutze `scan`, `autoscan`, `dtc`, `rdid`.")),
        (false, true, _) => println!("{}", tr!("→ Fahrzeug spricht TP 2.0. Nutze `autoscan`, `kwp-ident`, `kwp-dtc`, `kwp-block`.")),
        (false, false, true) => println!("{}", tr!("→ Nur OBD-II antwortet (Motor OBD-only). Nutze `obd-info`, `scan`, `autoscan`.")),
        (false, false, false) => {
            println!("{}", tr!("→ Keine CAN-Diagnose erkannt. Prüfe: Zündung an, Bitrate (meist"));
            println!("{}", tr!("  500 kbit/s), Verkabelung. Vor ~2004 ist es evtl. K-Line (nicht"));
            println!("{}", tr!("  über SocketCAN erreichbar)."));
        }
    }
    Ok(())
}

fn scan(be: &Backend) -> Result<()> {
    println!("{}", tr!("Scanne OBD-Steuergeräte über {a} (ISO 15765-4)…\n", a = be.describe()));
    let mut found = 0;
    for i in 0..8u16 {
        let tx = 0x7E0 + i;
        let rx = 0x7E8 + i;
        let tp = be.open(tx, rx)?;
        // Jede Antwort zählt — auch eine Absage (0x7F) heißt: hier ist ein Steuergerät.
        // Erst OBD (01 00, muss jedes OBD-Steuergerät können), dann UDS (10 01).
        // Golf 6: Motor antwortet hier nur auf OBD, UDS spricht er nicht.
        let antwort = |anfrage: &[u8], ok: u8| -> bool {
            tp.send(anfrage).is_ok() && tp.recv().is_ok_and(|r| r.first().is_some_and(|&b| b == ok || b == 0x7F))
        };
        let obd = antwort(&[0x01, 0x00], 0x41);
        let uds = antwort(&[0x10, 0x01], 0x50);
        if obd || uds {
            let art = match (obd, uds) {
                (true, true) => "OBD + UDS",
                (true, false) => "OBD",
                _ => "UDS",
            };
            println!("{}", tr!("  ✓ ECU bei tx={tx} rx={rx} antwortet ({art})", tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx), art = art));
            found += 1;
        }
    }
    if found == 0 {
        println!("{}", tr!("Keine Steuergeräte gefunden. Ist der Bus aktiv (Zündung an) und das"));
        println!("{}", tr!("Interface auf die richtige Bitrate gesetzt (meist 500 kbit/s)?"));
    } else {
        println!("{}", tr!("\n{found} Steuergerät(e) gefunden.", found = found));
    }
    Ok(())
}

fn discover(iface: &str, seconds: u64) -> Result<()> {
    println!("{}", tr!("Höre {seconds}s passiv auf {iface} und sammle CAN-IDs…\n", seconds = seconds, iface = iface));
    let sock = CanSocket::open(iface)?;
    sock.set_read_timeout(Duration::from_millis(200))?;
    let mut ids: BTreeSet<u16> = BTreeSet::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(seconds) {
        if let Ok(frame) = sock.read_frame() {
            if let Id::Standard(s) = frame.id() {
                ids.insert(s.as_raw());
            }
        }
    }
    println!("{}", tr!("Gesehene 11-bit CAN-IDs ({a}):", a = ids.len()));
    for id in &ids {
        println!("  {id:#05x}");
    }
    println!("{}", tr!("\nTipp: Diagnose-Antwort-IDs liegen meist im Bereich 0x7xx."));
    println!("{}", tr!("Daraus lässt sich deine eigene ECU-Adresskarte aufbauen."));
    Ok(())
}

fn show_labels(dir: &std::path::Path, db: &LabelDb) -> Result<()> {
    let (ecus, dtcs, meas, coding) = db.len();
    println!("{}", tr!("Label-Ordner: {a}", a = dir.display()));
    println!("{}", tr!("Geladen: {ecus} Steuergerät(e), {dtcs} DTC-Text(e), {meas} Messwert(e), \
         {coding} Anpassung/Codierung.", ecus = ecus, dtcs = dtcs, meas = meas, coding = coding));
    if ecus + dtcs + meas + coding == 0 {
        println!("{}", tr!("\nNoch keine Labels. Lege z. B. labels/beispiel.toml an (siehe README)."));
    }
    Ok(())
}

fn read_dtc(be: &Backend, tx: u16, rx: u16, mask: u8, db: &LabelDb) -> Result<()> {
    let tp = be.open(tx, rx)?;
    let uds = Uds::new(&*tp).mit_tx(tx);
    uds.diagnostic_session(0x01)?;
    let dtcs = uds.read_dtcs(mask)?;
    if dtcs.is_empty() {
        println!("{}", tr!("Keine Fehlercodes gespeichert."));
    } else {
        println!("{}", tr!("{a} Fehlercode(s):", a = dtcs.len()));
        for d in dtcs {
            let code = d.code();
            match db.dtc_description(&code) {
                Some(text) => println!("{}", tr!("  {code}  {text}  (status {a})", code = code, text = text, a = format!("{:#04x}", d.status))),
                None => println!("{}", tr!("  {code}  (status {a})", code = code, a = format!("{:#04x}", d.status))),
            }
        }
    }
    Ok(())
}

fn clear_dtc(be: &Backend, tx: u16, rx: u16) -> Result<()> {
    let tp = be.open(tx, rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(0x01)?;
    uds.clear_dtcs()?;
    println!("{}", tr!("Fehlercodes gelöscht."));
    Ok(())
}

fn ecu_reset(be: &Backend, tx: u16, rx: u16, art: u8, ja: bool) -> Result<()> {
    let name = match art {
        1 => tr!("hart"),
        2 => tr!("Zündung aus/an"),
        3 => tr!("weich"),
        _ => anyhow::bail!(tr!("Reset-Art {a} gibt es nicht — nur 1, 2 oder 3.", a = art)),
    };
    if !ja {
        println!("{}", tr!("Würde Steuergerät {a} neu starten ({b}). Nur am stehenden Auto — dann mit --ja.", a = format!("{tx:#05x}"), b = name));
        return Ok(());
    }
    let tp = be.open(tx, rx)?;
    Uds::new(&*tp).ecu_reset(art)?;
    println!("{}", tr!("Steuergerät {a} startet neu ({b}).", a = format!("{tx:#05x}"), b = name));
    Ok(())
}

/// Kennung immer hex, mit oder ohne „0x“ (wie in den Sicherungsdateien).
fn parse_kennung(s: &str) -> Result<u16, String> {
    let h = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u16::from_str_radix(h, 16).map_err(|e| tr!("ungültige Kennung '{s}': {e}", s = s, e = e))
}

fn parse_hex_bytes(s: &str) -> Result<Vec<u8>> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !clean.len().is_multiple_of(2) {
        anyhow::bail!(tr!("Hex-String braucht gerade Länge: '{s}'", s = s));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).map_err(|e| anyhow::anyhow!("{e}")))
        .collect()
}

fn tp20_send(be: &Backend, address: u8, request: &str) -> Result<()> {
    // mehrere Anfragen im selben Kanal: "1089;2101" (z. B. erst Sitzung, dann lesen)
    let reqs: Vec<Vec<u8>> = request.split(';').filter(|r| !r.trim().is_empty()).map(parse_hex_bytes).collect::<Result<_>>()?;
    println!("{}", tr!("Öffne TP2.0-Kanal zu ECU {address} über {a}…", address = format!("{:#04x}", address), a = be.describe()));
    let mut ch = be.open_tp20(address, Duration::from_millis(1000))?;
    println!("{}", tr!("Kanal offen: tx={a} rx={b}", a = format!("{:#05x}", ch.tx_id()), b = format!("{:#05x}", ch.rx_id())));
    let hex = |d: &[u8]| d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
    for kwp in &reqs {
        println!("{}", tr!("Sende KWP2000: {a}", a = hex(kwp)));
        let mut resp = ch.request(kwp);
        // „Antwort folgt“ (7F xx 78): auf die eigentliche Antwort warten
        for _ in 0..10 {
            match &resp {
                Ok(r) if r.len() >= 3 && r[0] == 0x7F && r[2] == 0x78 => {
                    println!("{}", tr!("  Antwort folgt ({a})", a = hex(r)));
                    resp = ch.recv();
                }
                _ => break,
            }
        }
        match resp {
            Ok(r) => println!("{}", tr!("  Antwort ({a} B): {b}", a = r.len(), b = hex(&r))),
            Err(e) => println!("{}", tr!("  Fehler: {e}", e = format!("{:#}", e))),
        }
    }
    Ok(())
}

fn rdid_scan(be: &Backend, tx: u16, rx: u16, von: u16, bis: u16, suche: Option<u32>, sitzung: u8) -> Result<()> {
    if bis < von {
        anyhow::bail!(tr!("--bis muss größer oder gleich --von sein"));
    }
    let tp = be.open(tx, rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(sitzung)?;
    let t0 = std::time::Instant::now();
    let (mut lesbar, mut treffer) = (0, 0);
    for did in von..=bis {
        if (did - von).is_multiple_of(256) {
            eprintln!("{}", tr!("… {did} ({lesbar} lesbar)", did = format!("{:#06X}", did), lesbar = lesbar));
            // Sitzung halten (TesterPresent)
            let _ = uds.tester_present();
        }
        let Ok(d) = uds.read_data_by_id(did) else { continue };
        lesbar += 1;
        let hex: Vec<String> = d.iter().map(|b| format!("{b:02X}")).collect();
        let txt: String = d.iter().map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '.' }).collect();
        let mut zeile = format!("{did:#06X}  ({:3} B)  {}  |{txt}|", d.len(), hex.join(" "));
        if let Some(s) = suche {
            let hits: Vec<String> = ecu_werkbank::autotest::km_kandidaten(&d)
                .into_iter()
                .filter(|(v, w)| v.abs_diff(u64::from(s)) <= if w.contains("×10") { 10 } else { 2 })
                .map(|(v, w)| format!("{v} ({w})"))
                .collect();
            if !hits.is_empty() {
                treffer += 1;
                zeile += &format!("   <== {}", hits.join(", "));
            }
        }
        println!("{zeile}");
    }
    eprintln!("{}", tr!("{lesbar} Kennungen lesbar{a}, Dauer {b} s", lesbar = lesbar, a = suche.map(|_| tr!(", {treffer} mit passendem Wert", treffer = treffer)).unwrap_or_default(), b = format!("{:.0}", t0.elapsed().as_secs_f64())));
    Ok(())
}

fn rdid(be: &Backend, tx: u16, rx: u16, did: u16, db: &LabelDb) -> Result<()> {
    let tp = be.open(tx, rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(0x01)?;
    let data = uds.read_data_by_id(did)?;
    print_value(did, &data, db);
    Ok(())
}

/// Gemeinsame Darstellung über die Library-Funktion.
fn print_value(did: u16, data: &[u8], db: &LabelDb) {
    println!("{}", format_value(db, did, data));
}

// ---------------------------------------------------------------------------
// TP 2.0 / KWP2000
// ---------------------------------------------------------------------------

fn kwp_open(be: &Backend, address: u8) -> Result<tp20::Channel> {
    kwp::open_session(be, address)
}

fn kwp_dtc_line(d: &kwp::KwpDtc, address: u8, db: &LabelDb) -> String {
    let obd = d.obd_code(Some(address));
    let text = obd
        .as_deref()
        .and_then(|c| db.dtc_description(c))
        .or_else(|| db.dtc_description(&d.vag_code()))
        .unwrap_or("");
    let obd_s = obd.map(|c| format!(" ({c})")).unwrap_or_default();
    format!("{}{obd_s}  {text}  ({})", d.vag_code(), d.status_text())
}

fn kwp_ident(be: &Backend, address: u8) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    let (tx, rx) = (ch.tx_id(), ch.rx_id());
    let mut k = Kwp::new(&mut ch);
    let (data, lines) = match k.read_ecu_id(kwp::ECU_ID_VAG) {
        Ok(d) => {
            // fester VW-Aufbau (Teilenummer, SW, Name ab Byte 26), sonst Textstücke
            let l = kwp::ident_9b(&d).unwrap_or_else(|| kwp::ident_strings(&d));
            (d, l)
        }
        // neuere Steuergeräte: 0x22 statt 0x1A
        Err(e) => {
            let l = kwp::ident_22(&mut k);
            if l.is_empty() {
                return Err(e);
            }
            (Vec::new(), l)
        }
    };
    let name = kwp::tp20_name(address, &lines);
    println!("{}", tr!("Steuergerät {address} {name} (tx={tx} rx={rx}):", address = format!("{:#04x}", address), name = name, tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx)));
    for s in &lines {
        println!("  {s}");
    }
    if data.is_empty() {
        return Ok(());
    }
    let hex: Vec<String> = data.iter().map(|b| format!("{b:02X}")).collect();
    println!("{}", tr!("  Rohdaten: {a}", a = hex.join(" ")));
    Ok(())
}

fn kwp_dtc(be: &Backend, address: u8, db: &LabelDb) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    let dtcs = Kwp::new(&mut ch).read_dtcs_mit_umgebung()?;
    if dtcs.is_empty() {
        println!("{}", tr!("Keine Fehlercodes gespeichert."));
    } else {
        println!("{}", tr!("{a} Fehlercode(s):", a = dtcs.len()));
        for (d, env) in &dtcs {
            println!("  {}", kwp_dtc_line(d, address, db));
            if let Some(e) = env {
                println!("{}", tr!("      Umgebungsdaten (roh): {a}", a = kwp::umgebung_hex(e)));
            }
        }
    }
    Ok(())
}

fn kwp_clear(be: &Backend, address: u8) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    Kwp::new(&mut ch).clear_dtcs()?;
    println!("{}", tr!("Fehlerspeicher von {address} gelöscht.", address = format!("{:#04x}", address)));
    Ok(())
}

/// In der GUI zuletzt gewähltes Fahrzeug (<Datenordner>/aktives-fahrzeug).
fn load_active() -> Option<String> {
    let key = std::fs::read_to_string(ecu_werkbank::history::data_dir().join("aktives-fahrzeug")).ok()?;
    ecu_werkbank::vehicle::load(key.trim()).map(|v| v.key)
}

/// Fahrzeug für Motor-Checks: --fahrzeug, sonst über die VIN (OBD), sonst das aktive.
/// Optionaler Preis „45,90“ → Zahl (Fehler bei Unsinn).
fn preis_lesen(p: Option<&str>) -> Result<Option<f64>> {
    match p.map(str::trim).filter(|p| !p.is_empty()) {
        None => Ok(None),
        Some(p) => ecu_werkbank::kosten::zahl_lesen(p).map(Some).with_context(|| tr!("Preis bitte als Zahl eingeben, z. B. 45,90")),
    }
}

fn fehler_geruest(be: &Backend, db: &LabelDb, out: Option<PathBuf>, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::fehlergeruest;
    println!("{}", tr!("Mögliche Fehlercodes lesen über {a} (nur lesen) …", a = be.describe()));
    let g = fehlergeruest::lesen(be, db, |t| {
        use std::io::Write;
        eprint!("\r  {t} …");
        let _ = std::io::stderr().flush();
    })?;
    eprintln!("\r          ");
    if g.is_empty() {
        anyhow::bail!(tr!("Kein UDS-Steuergerät hat seine Fehlercodes gemeldet. (TP 2.0 / KWP2000 kennt diese Abfrage nicht.)"));
    }
    let v = fahrzeug_fuer(be, fahrzeug).ok();
    let name = v.as_ref().map(|v| if v.bezeichnung.is_empty() { v.key.clone() } else { v.bezeichnung.clone() }).unwrap_or_default();
    let jetzt = ecu_werkbank::report::now_local();
    let text = fehlergeruest::geruest(&g, db, &name, &jetzt);
    let datei = format!("fehlertexte-geruest-{}.toml", ecu_werkbank::report::unix_now());
    let pfad = out.unwrap_or_else(|| match &v {
        Some(v) => ecu_werkbank::vehicle::dir(&v.key).join(&datei),
        None => PathBuf::from(&datei),
    });
    if let Some(p) = pfad.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&pfad, &text).with_context(|| tr!("Schreiben: {a}", a = pfad.display()))?;
    let n: usize = g.iter().map(|x| x.codes.len()).sum();
    for x in &g {
        println!("{}", tr!("  {a} {b}  {c} mögliche Fehler", a = format!("{:<28}", x.name), b = format!("{:#05x}", x.tx), c = x.codes.len()));
    }
    println!("{}", tr!("{a} Steuergerät(e), {b} Fehlercodes → {c}", a = g.len(), b = n, c = pfad.display()));
    println!("{}", tr!("Klartexte eintragen, dann: ecu-werkbank fehlertexte-uebernehmen \"{a}\"", a = pfad.display()));
    Ok(())
}

fn fahrzeug_fuer(be: &Backend, fahrzeug: Option<&str>) -> Result<ecu_werkbank::vehicle::Vehicle> {
    let active = active_vehicle(fahrzeug)?;
    if active.is_some() {
        return ecu_werkbank::vehicle::resolve(None, active.as_deref());
    }
    let vin = ecu_werkbank::profil::quick_vin(be);
    ecu_werkbank::vehicle::resolve(vin.as_deref(), load_active().as_deref())
}

fn kette(be: &Backend, feld: Option<&str>, proben: usize, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::motorcheck::{self, Feld};
    let v = fahrzeug_fuer(be, fahrzeug)?;
    let mut e = motorcheck::laden(&v.key);
    if let Some(f) = feld {
        e.kette = Some(Feld::parse(f).with_context(|| tr!("--feld {a}: Block/Feld, z. B. 093/2", a = f))?);
        motorcheck::speichern(&v.key, &e)?;
    }
    let Some(feld) = e.kette else {
        println!("{}", tr!("Noch kein Feld für die Phasenlage festgelegt. Suche mit:"));
        println!("  ecu-werkbank kwp-blockscan --address 01 --von 80 --bis 100");
        println!("{}", tr!("Felder mit Art „~Winkel“ oder „~Zündwinkel“ sind Kandidaten. Dann:  ecu-werkbank kette --feld 093/2"));
        return Ok(());
    };
    println!("{}", tr!("Steuerketten-Check ({a}, {b}) — Motor betriebswarm im Leerlauf, nichts tun …", a = v.label(), b = feld.text()));
    motorcheck::demo_vorbereiten(be, false);
    let m = motorcheck::kette_messen(be, feld, proben, |_| {});
    motorcheck::demo_ende(be);
    let m = m?;
    let frueher: Vec<f64> = ecu_werkbank::trend::series(&v.key).get(motorcheck::TREND_KETTE).map(|p| p.iter().map(|x| x.wert).collect()).unwrap_or_default();
    println!("{}", tr!("Phasenlage {a} {u} (Spanne {b}, {c} Proben, {d} 1/min{e})", a = format!("{:.1}", m.wert), u = m.einheit, b = format!("{:.1}", m.streuung), c = m.proben, d = format!("{:.0}", m.drehzahl), e = m.kuehlmittel.map(|t| format!(", {t:.0} °C")).unwrap_or_default()));
    for (st, t) in motorcheck::kette_bewerten(&m, &frueher) {
        println!("  [{}] {t}", st.text());
    }
    if m.kuehlmittel.is_none_or(|t| t >= 80.0) {
        ecu_werkbank::trend::add(&v.key, v.km, &[(motorcheck::TREND_KETTE.to_string(), m.wert, m.einheit.clone())])?;
        println!("{}", tr!("(In den Trends von {a} gespeichert.)", a = v.label()));
    }
    Ok(())
}

fn druckpruefung(be: &Backend, art: &str, paar: Option<(&str, &str)>, sekunden: u64, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::motorcheck::{self, Druck, Feld};
    let art = match art.to_lowercase().as_str() {
        "ladedruck" | "lade" | "turbo" => Druck::Ladedruck,
        "kraftstoff" | "kraftstoffdruck" | "rail" => Druck::Kraftstoff,
        x => anyhow::bail!(tr!("--art {a}: ladedruck oder kraftstoff", a = x)),
    };
    let v = fahrzeug_fuer(be, fahrzeug)?;
    let mut e = motorcheck::laden(&v.key);
    if let Some((s, i)) = paar {
        let p = [Feld::parse(s).with_context(|| tr!("--soll {a}: Block/Feld, z. B. 115/3", a = s))?, Feld::parse(i).with_context(|| tr!("--ist {a}: Block/Feld, z. B. 115/4", a = i))?];
        match art {
            Druck::Ladedruck => e.ladedruck = Some(p),
            Druck::Kraftstoff => e.kraftstoff = Some(p),
        }
        motorcheck::speichern(&v.key, &e)?;
    }
    let paar = match art {
        Druck::Ladedruck => e.ladedruck.or(Some([Feld { block: 115, feld: 3 }, Feld { block: 115, feld: 4 }])),
        Druck::Kraftstoff => e.kraftstoff,
    };
    let Some(paar) = paar else {
        println!("{}", tr!("Noch keine Felder für Soll/Ist festgelegt. Suche mit `kwp-blockscan --address 01`, dann z. B.:  ecu-werkbank druckpruefung --art kraftstoff --soll 103/3 --ist 103/4"));
        return Ok(());
    };
    println!("{}", tr!("{a}-Prüfung ({b} / {c}): Motor warm, 3. Gang, ab ca. 1500 1/min voll beschleunigen. Nur wo es sicher und erlaubt ist! Aufzeichnung {d} s läuft …", a = art.name(), b = paar[0].text(), c = paar[1].text(), d = sekunden));
    motorcheck::demo_vorbereiten(be, true);
    let r = motorcheck::druck_aufzeichnen(be, paar, sekunden as f64, |_| true);
    motorcheck::demo_ende(be);
    let (proben, einheit) = r?;
    let (erg, bew) = motorcheck::druck_bewerten(art, &proben, &einheit);
    for (st, t) in bew {
        println!("  [{}] {t}", st.text());
    }
    if let Some(r) = erg {
        let name = tr!("{a} größte Lücke", a = art.name());
        ecu_werkbank::trend::add(&v.key, v.km, &[(name, r.luecke, einheit)])?;
        println!("{}", tr!("(In den Trends von {a} gespeichert.)", a = v.label()));
    }
    Ok(())
}

fn rekorder(be: &Backend, bloecke: &[u8], grenzen: &[String], vorlauf: u64, nachlauf: u64, fahrzeug: Option<&str>) -> Result<()> {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use ecu_werkbank::rekorder::{self, Grenze, Meldung};
    let g: Vec<Grenze> = grenzen.iter().map(|x| Grenze::parse(x).with_context(|| tr!("--grenze {a}: z. B. \"003/4>40\"", a = x))).collect::<Result<_>>()?;
    let v = fahrzeug_fuer(be, fahrzeug)?;
    let ordner = v.sub("rekorder")?;
    let stop = AtomicBool::new(false);
    let merken = Arc::new(AtomicBool::new(false));
    {
        let m = merken.clone();
        std::thread::spawn(move || {
            let mut l = String::new();
            while std::io::stdin().read_line(&mut l).is_ok_and(|n| n > 0) {
                m.store(true, std::sync::atomic::Ordering::Relaxed);
                l.clear();
            }
        });
    }
    println!("{}", tr!("Fahrt-Rekorder läuft (Blöcke {a}) — Enter = Moment merken, Strg-C = Ende. Ablage: {b}", a = bloecke.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","), b = ordner.display()));
    rekorder::laufen(be, bloecke, &g, vorlauf as f64, nachlauf as f64, &ordner, &stop, &merken, |m| match m {
        Meldung::Laeuft { spalten: Some(s), .. } => println!("{}", tr!("Spalten: {a}", a = s.join(" | "))),
        Meldung::Ausgeloest(t) => println!("{}", tr!("⏺ Ausgelöst: {a} — nehme noch {b} s auf …", a = t, b = nachlauf)),
        Meldung::Gespeichert(p, t) => println!("{}", tr!("✓ Gespeichert ({a}): {b}", a = t, b = p.display())),
        _ => {}
    })
}

fn anpassung_sichern(be: &Backend, db: &LabelDb, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::anpassung;
    println!("{}", tr!("Anpassungen sichern über {a} (nur lesen, je Steuergerät etwa 30–60 s) …", a = be.describe()));
    let mut zuletzt = String::new();
    let mut s = anpassung::sichern(be, db, true, |g, _| {
        if g != zuletzt {
            println!("  {g}");
            zuletzt = g.to_string();
        }
    })?;
    s.vehicle = fahrzeug_fuer(be, fahrzeug)?.key;
    let p = anpassung::speichern(&s)?;
    let n: usize = s.geraete.iter().map(|g| g.kennungen.len()).sum();
    println!("{}", tr!("{a} Steuergerät(e), {b} Kennungen gesichert: {c}", a = s.geraete.len(), b = n, c = p.display()));
    Ok(())
}

fn anpassung_vergleich(alt: Option<&std::path::Path>, neu: Option<&std::path::Path>, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::anpassung;
    let (a, b) = match (alt, neu) {
        (Some(a), Some(b)) => (anpassung::laden(a)?, anpassung::laden(b)?),
        _ => {
            // ohne --fahrzeug: das Fahrzeug mit der neuesten Sicherung
            let l = match active_vehicle(fahrzeug)? {
                Some(k) => anpassung::liste(&k),
                None => ecu_werkbank::vehicle::list().iter().map(|v| anpassung::liste(&v.key)).max_by_key(|l| l.first().map(|x| x.1.timestamp)).unwrap_or_default(),
            };
            // nur komplette Sicherungen (nicht die Werte vor einem Zurückschreiben)
            let l: Vec<_> = l.into_iter().filter(|x| x.1.vollstaendig()).collect();
            if l.len() < 2 {
                anyhow::bail!(tr!("Zum Vergleich braucht es zwei Sicherungen (vorhanden: {a}).", a = l.len()));
            }
            (l[1].1.clone(), l[0].1.clone())
        }
    };
    println!("{}", tr!("Anpassungen vom {a}  →  {b}\n", a = a.created, b = b.created));
    let d = anpassung::vergleichen(&a, &b);
    if d.is_empty() {
        println!("{}", tr!("Keine Kennung geändert."));
    }
    for (geraet, aend) in d {
        println!("{geraet}");
        for x in aend {
            println!("  {}", x.zeile());
        }
    }
    Ok(())
}

fn grundeinstellung_cmd(system: Option<&str>, suche: Option<&str>) -> Result<()> {
    use ecu_werkbank::grundeinstellung as ge;
    let treffer = ge::filtern(system, suche);
    if treffer.is_empty() {
        println!("{}", tr!("Nichts gefunden. Systeme: {a}", a = ge::systeme().join(", ")));
        return Ok(());
    }
    println!("{}", tr!("Grundeinstellungen/Werkstattfunktionen (Referenz, {a}):", a = treffer.len()));
    let mut letztes = "";
    for e in treffer {
        if e.system != letztes {
            println!("\n{}", e.system);
            letztes = e.system;
        }
        println!("  {a} {b}  [{c}]", a = e.stufe.ampel(), b = e.name, c = e.stufe.text());
        println!("      {}", e.zweck);
        println!("      ⚠ {}", e.hinweis);
    }
    println!("{}", tr!("\nReines Nachschlagen — ausgeführt wird nichts. Service-Rückstellung: `service`. Stellglieder: `stellen` (nur im Stand)."));
    Ok(())
}



fn ident_cmd(be: &Backend, geraet: u16, rx: Option<u16>) -> Result<()> {
    use ecu_werkbank::swinfo;
    let rx = rx.unwrap_or_else(|| ecu_werkbank::uds::antwort_id(geraet));
    println!("{}", tr!("Erweiterte Identifikation {a} über {b} (nur lesen) …", a = format!("{geraet:#05x}"), b = be.describe()));
    let tp = be.open(geraet, rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(ecu_werkbank::uds::SESSION_DEFAULT).context(tr!("Steuergerät antwortet nicht (Zündung an?)"))?;
    let mut gelesen = 0;
    for (did, name) in swinfo::STD_DIDS {
        match uds.read_data_by_id(*did) {
            Ok(d) if !d.is_empty() => {
                println!("  {a:<32} {b}", a = name, b = swinfo::ident_wert(*did, &d));
                gelesen += 1;
            }
            _ => {}
        }
    }
    // lange Codierung (0x0600), falls vorhanden
    if let Ok(d) = uds.read_data_by_id(ecu_werkbank::backup::VAG_CODING_DID) {
        if !d.is_empty() {
            println!("  {a:<32} {b}", a = tr!("Codierung (lang)"), b = ecu_werkbank::backup::hex(&d));
            gelesen += 1;
        }
    }
    if gelesen == 0 {
        println!("{}", tr!("Keine Kennfelder gelesen — spricht das Gerät UDS? (TP 2.0: kwp-ident nutzen)"));
    }
    Ok(())
}

fn anpassung_schreibtest(be: &Backend, geraet: u16, kennung: Option<u16>, ja: bool, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::anpassung;
    let l = match active_vehicle(fahrzeug)? {
        Some(k) => anpassung::liste(&k),
        None => ecu_werkbank::vehicle::list().iter().map(|v| anpassung::liste(&v.key)).max_by_key(|l| l.first().map(|x| x.1.timestamp)).unwrap_or_default(),
    };
    let s = l.into_iter().map(|x| x.1).find(|s| s.vollstaendig()).context(tr!("Keine Anpassungs-Sicherung gefunden (erst: anpassung-sichern oder notfall-sicherung)."))?;
    let g = anpassung::geraet_finden(&s, geraet).with_context(|| tr!("Steuergerät {a} ist nicht in dieser Sicherung", a = format!("{geraet:#x}")))?;
    let did = match kennung {
        Some(d) => d,
        None => g.kennungen.keys().next().and_then(|k| u16::from_str_radix(k, 16).ok()).context(tr!("Dieses Steuergerät hat keine Kennungen."))?,
    };
    println!("{}", tr!("Schreibtest an {a}, Kennung {b} — es wird der jetzige Inhalt unverändert zurückgeschrieben.", a = g.titel(), b = format!("{did:#06X}")));
    if !ja {
        let r = anpassung::schreibtest(be, g, &s.vehicle, did, false)?;
        println!("{}", tr!("Jetziger Inhalt: {a}", a = ecu_werkbank::backup::hex(&r.vorher)));
        println!("{}", tr!("Nichts geschrieben. Zum echten Test denselben Befehl mit --ja ausführen (Zündung an)."));
        return Ok(());
    }
    let r = anpassung::schreibtest(be, g, &s.vehicle, did, true)?;
    println!("{}", tr!("Jetziger Inhalt: {a}", a = ecu_werkbank::backup::hex(&r.vorher)));
    match &r.ergebnis {
        Ok(()) => println!("{}", tr!("Schreiben und Kontroll-Lesen erfolgreich ✓ — der Schreibweg funktioniert (nichts verändert).")),
        Err(t) => println!("{}", tr!("Nicht geschrieben: {a}", a = t)),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn login_cmd(be: &Backend, geraet: Option<u16>, sa2: Option<String>, key: Option<String>, kwp: bool, index: u8, level: u8, test: bool, loeschen: bool, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::login::{self, Methode};
    let v = fahrzeug_fuer(be, fahrzeug)?;
    let Some(id) = geraet else {
        let l = login::laden(&v.key);
        if l.geraete.is_empty() {
            println!("{}", tr!("Keine Logins hinterlegt. Beispiel: ecu-werkbank login --geraet 714 --sa2 <hex> --level 3"));
        }
        for (k, m) in &l.geraete {
            println!("  {k}  {}", m.beschreibung());
        }
        return Ok(());
    };
    // TP 2.0 = Adresse < 0x100; UDS = CAN-Anfrage-ID
    let ist_tp20 = id < 0x100;
    if loeschen {
        let mut l = login::laden(&v.key);
        if l.geraete.remove(&format!("{id:#05x}")).is_some() {
            let d = ecu_werkbank::vehicle::dir(&v.key).join("logins.toml");
            std::fs::write(&d, toml::to_string(&l)?)?;
            println!("{}", tr!("Login für {a} gelöscht.", a = format!("{id:#05x}")));
        }
        return Ok(());
    }
    if kwp {
        login::setzen(&v.key, id, Methode::KwpXor { ecu_index: index })?;
        println!("{}", tr!("KWP-Login (XorKey, Index {b}) für {a} hinterlegt.", a = format!("{id:#05x}"), b = index));
    } else if let Some(bc) = sa2 {
        let roh = ecu_werkbank::backup::parse_hex(&bc).context(tr!("SA2-Bytecode ist kein gültiges Hex"))?;
        login::setzen(&v.key, id, Methode::Sa2 { level, bytecode: ecu_werkbank::backup::hex(&roh) })?;
        println!("{}", tr!("SA2-Login für {a} hinterlegt (Level {b}, {c} Byte).", a = format!("{id:#05x}"), b = level, c = roh.len()));
    } else if let Some(k) = key {
        let roh = ecu_werkbank::backup::parse_hex(&k).context(tr!("Key ist kein gültiges Hex"))?;
        login::setzen(&v.key, id, Methode::FesterKey { level, key: ecu_werkbank::backup::hex(&roh) })?;
        println!("{}", tr!("Fester-Key-Login für {a} hinterlegt (Level {b}).", a = format!("{id:#05x}"), b = level));
    }
    if test {
        let m = login::fuer(&v.key, id).with_context(|| tr!("Für {a} ist kein Login hinterlegt.", a = format!("{id:#05x}")))?;
        println!("{}", tr!("Teste Login an {a} ({b}) — es wird nur angemeldet, nichts geschrieben …", a = format!("{id:#05x}"), b = m.beschreibung()));
        let erg = if ist_tp20 {
            let mut ch = kwp_open(be, id as u8)?;
            login::anmelden_kwp(&mut ecu_werkbank::kwp::Kwp::new(&mut ch), &m)
        } else {
            let tp = be.open(id, ecu_werkbank::uds::antwort_id(id))?;
            let uds = ecu_werkbank::uds::Uds::new(&*tp);
            uds.diagnostic_session(ecu_werkbank::uds::SESSION_EXTENDED).context(tr!("erweiterte Diagnosesitzung (0x03) abgelehnt"))?;
            login::anmelden_uds(&uds, &m)
        };
        match erg {
            Ok(()) => println!("{}", tr!("Login erfolgreich ✓ — das Steuergerät erlaubt jetzt das Schreiben.")),
            Err(e) => println!("{}", tr!("Login fehlgeschlagen: {e}", e = format!("{e:#}"))),
        }
    }
    Ok(())
}

fn anpassung_zurueck(be: &Backend, datei: Option<&std::path::Path>, geraet: Option<u16>, kennung: &[u16], alle: bool, ja: bool, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::anpassung::{self, Zustand};
    let s = match datei {
        Some(p) => anpassung::laden(p)?,
        None => {
            let l = match active_vehicle(fahrzeug)? {
                Some(k) => anpassung::liste(&k),
                None => ecu_werkbank::vehicle::list().iter().map(|v| anpassung::liste(&v.key)).max_by_key(|l| l.first().map(|x| x.1.timestamp)).unwrap_or_default(),
            };
            // die neueste komplette (nicht die Werte vor einem Zurückschreiben)
            l.into_iter().map(|x| x.1).find(|s| s.vollstaendig()).context(tr!("Keine Anpassungs-Sicherung gefunden (erst: anpassung-sichern oder notfall-sicherung)."))?
        }
    };
    println!("{}", tr!("Sicherung vom {a} ({b})", a = s.created, b = s.vehicle));
    if !s.notiz.is_empty() {
        println!("  {}", s.notiz);
    }
    let Some(id) = geraet else {
        println!("{}", tr!("Steuergeräte in der Sicherung (Auswahl mit --geraet):"));
        for g in &s.geraete {
            let id = g.tp20.map(|a| format!("{a:02x}")).unwrap_or_else(|| format!("{:03x}", g.tx));
            println!("{}", tr!("  --geraet {a} {b}  ({c} Kennungen)", a = format!("{id:<6}"), b = g.titel(), c = g.kennungen.len()));
        }
        return Ok(());
    };
    let g = anpassung::geraet_finden(&s, id).with_context(|| tr!("Steuergerät {a} ist nicht in dieser Sicherung", a = format!("{id:#x}")))?;
    anpassung::fahrzeug_pruefen(be, &s)?;
    println!("{}", tr!("{a}: vergleiche {b} Kennungen mit dem Auto (nur lesen) …", a = g.titel(), b = g.kennungen.len()));
    let punkte = anpassung::pruefen(be, g, &s.vehicle, |_, _| {})?;
    let gleich = punkte.iter().filter(|p| p.zustand == Zustand::Gleich).count();
    println!("{}", tr!("{a} gleich, {b} anders:", a = gleich, b = punkte.len() - gleich));
    for p in punkte.iter().filter(|p| p.zustand != Zustand::Gleich) {
        println!("  {}", p.zeile());
    }
    let ziel: Vec<u16> = if alle {
        punkte.iter().filter(|p| p.zustand == Zustand::Anders).map(|p| p.did).collect()
    } else {
        kennung.to_vec()
    };
    if ziel.is_empty() {
        if punkte.iter().any(|p| p.zustand == Zustand::Anders) {
            println!("{}", tr!("\nZum Zurückschreiben: --kennung <hex> (einzeln) oder --alle, dann mit --ja."));
        }
        return Ok(());
    }
    println!("{}", tr!("\nZurückschreiben: {a}", a = ziel.iter().map(|d| format!("{d:04X}")).collect::<Vec<_>>().join(", ")));
    if !ja {
        println!("{}", tr!("Nichts geschrieben. Zum Schreiben denselben Befehl mit --ja ausführen (Zündung an, Motor aus, Ladegerät empfohlen)."));
        return Ok(());
    }
    let r = anpassung::zurueckschreiben(be, g, &s.vehicle, &ziel)?;
    for e in &r {
        match &e.ergebnis {
            Ok(()) => println!("{}", tr!("  {a}  geschrieben und kontrolliert ✓", a = format!("{:#06X}", e.did))),
            Err(t) => println!("{}", tr!("  {a}  nicht geschrieben: {b}", a = format!("{:#06X}", e.did), b = t)),
        }
    }
    if let Some(p) = anpassung::vorher_sichern(&s.vehicle, g, &r)? {
        println!("{}", tr!("Die Werte davor sind gesichert: {a}\n(Rückgängig: anpassung-zurueck --datei {a} --geraet {b} --alle --ja)", a = p.display(), b = format!("{id:x}")));
    }
    Ok(())
}

/// Codierung eines TP-2.0-Steuergeräts lesen und aufschlüsseln.
fn kwp_coding(be: &Backend, address: u8, db: &LabelDb) -> Result<()> {
    use ecu_werkbank::kwp::TpCodierung;
    let mut ch = kwp_open(be, address)?;
    let mut k = Kwp::new(&mut ch);
    let ident_raw = k.read_ecu_id(kwp::ECU_ID_VAG).unwrap_or_default();
    let key = ecu_werkbank::codiertexte::tp20_key(address);
    match kwp::codierung_lesen(&mut k, &ident_raw) {
        Some(TpCodierung::Lang(b)) => {
            println!("{}", tr!("Codierung {a} (TP 2.0) = {b}", a = format!("{address:02X}"), b = ecu_werkbank::backup::hex(&b)));
            println!("{}", tr!("Eigene Texte dazu: codiertext --tx {a} …", a = format!("{key:03X}")));
            print_coding(None, Some(key), &b, db);
        }
        Some(TpCodierung::Kurz { wert, roh }) => {
            println!("{}", tr!("Kurzcodierung {a}: {b}", a = format!("{address:02X}"), b = format!("{wert:07}")));
            if let Some((w, i, h)) = roh.get(4..10).and_then(ecu_werkbank::swinfo::betriebsnummer) {
                println!("{}", tr!("Betriebsnummer der letzten Codierung: Werkstatt {w} · Importeur {i} · Händler {h}", w = format!("{w:05}"), i = format!("{i:03}"), h = format!("{h:05}")));
            }
        }
        None => anyhow::bail!(tr!("Keine Codierung lesbar (weder in der Identifikation noch über Kennung 0x0600).")),
    }
    Ok(())
}

/// Alle Blöcke von `von` bis `bis` lesen; nicht vorhandene still überspringen.
fn kwp_blockscan(be: &Backend, address: u8, von: u8, bis: u8) -> Result<()> {
    let texte = ecu_werkbank::messwerttexte::Texte::laden();
    println!("{}", tr!("Blockscan Steuergerät {a}, Block {b}–{c} (nur lesen) …", a = format!("{address:02X}"), b = von, c = bis));
    let gefunden = kwp::blockscan(be, address, von, bis, |_| {})?;
    for (block, v) in &gefunden {
        let name = texte.block(address, *block).map(|n| format!("  [{n}]")).unwrap_or_default();
        println!("{block:03}{name}: {}", texte.zeile(address, *block, v));
    }
    println!("{}", tr!("{a} Block/Blöcke mit Werten. Felder mit „~“ haben noch keinen eigenen Namen (messwerttext oder ✏ in der GUI).", a = gefunden.len()));
    Ok(())
}

/// Namensvorschläge gegen die echten Blöcke prüfen und auf Wunsch übernehmen.
fn messwert_vorschlaege(be: &Backend, address: u8, ja: bool) -> Result<()> {
    use ecu_werkbank::messwerttexte;
    use ecu_werkbank::mwvorschlag::{self, Pruefung};
    let vorschlaege = mwvorschlag::fuer(address);
    if vorschlaege.is_empty() {
        anyhow::bail!(tr!("Für Steuergerät {a} gibt es noch keine Vorschläge (bisher nur Motor 01).", a = format!("{address:02X}")));
    }
    let mut ch = kwp_open(be, address)?;
    let texte = messwerttexte::Texte::laden();
    let (mut neu, mut passend) = (0, 0);
    for b in vorschlaege {
        let werte = match Kwp::new(&mut ch).read_block(b.block) {
            Ok(w) => w,
            Err(_) => {
                println!("{}", tr!("Block {a} ({b}): gibt es hier nicht", a = format!("{:03}", b.block), b = b.name));
                continue;
            }
        };
        println!("{}", tr!("Block {a} ({b}):", a = format!("{:03}", b.block), b = b.name));
        let mut block_passt = false;
        for (f, p) in mwvorschlag::pruefen(b, &werte) {
            let (zeichen, info) = match &p {
                Pruefung::Passt => ("✓", String::new()),
                Pruefung::Anders(x) => ("✗", tr!("  (Formel {a} passt nicht — anders belegt)", a = format!("{x:#04x}"))),
                Pruefung::Fehlt => ("–", tr!("  (Feld fehlt)")),
            };
            let eigen = texte.eigen(address, b.block, f.feld);
            let schon = eigen.map(|t| tr!("  — schon benannt: {a}", a = t)).unwrap_or_default();
            println!("  {zeichen} {}{info}{schon}", tr!("Feld {a}: {b}", a = f.feld, b = f.name));
            if p == Pruefung::Passt {
                passend += 1;
                block_passt = true;
                if ja && eigen.is_none() {
                    messwerttexte::setzen(address, b.block, Some(f.feld), f.name)?;
                    neu += 1;
                }
            }
        }
        if ja && block_passt && texte.block(address, b.block).is_none() {
            messwerttexte::setzen(address, b.block, None, b.name)?;
        }
    }
    if ja {
        println!("{}", tr!("{a} Feldname(n) übernommen. Ändern jederzeit mit ✏ in der GUI oder `messwerttext`.", a = neu));
    } else {
        println!("{}", tr!("{a} Vorschlag/Vorschläge passen. Übernehmen mit --ja (vorhandene eigene Texte bleiben).", a = passend));
    }
    Ok(())
}

fn kwp_block(
    be: &Backend,
    address: u8,
    blocks: &[u8],
    csv: Option<&std::path::Path>,
    count: u32,
    interval: u64,
    db: &LabelDb,
) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    let texte = ecu_werkbank::messwerttexte::Texte::laden();
    for &block in blocks {
        let name = texte.block(address, block).map(str::to_string).or_else(|| db.measuring(u16::from(address), u16::from(block)).map(|m| m.name.clone()).filter(|n| !n.is_empty()));
        if let Some(n) = name {
            println!("{}", tr!("Block {block}: {a}", block = format!("{:03}", block), a = n));
        }
    }
    let mut log = csv.map(ecu_werkbank::csvlog::CsvLog::create).transpose()?;
    let mut n = 0u32;
    loop {
        let mut cols = Vec::new();
        for &block in blocks {
            let values = Kwp::new(&mut ch).read_block(block)?;
            println!("{block:03}: {}", texte.zeile(address, block, &values));
            cols.extend(block_columns(block, &values));
        }
        if let Some(l) = log.as_mut() {
            l.write(&cols)?;
        }
        n += 1;
        if count != 0 && n >= count {
            break;
        }
        std::thread::sleep(Duration::from_millis(interval));
    }
    if let Some(l) = log {
        println!("{}", tr!("{a} Zeile(n) in {b} gespeichert.", a = l.rows(), b = l.path().display()));
    }
    Ok(())
}

/// CSV-Spalten eines Messwertblocks: "B001.1 [1/min]" → Wert.
fn block_columns(block: u8, values: &[kwp::BlockValue]) -> Vec<(String, String)> {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| match v.decode() {
            Some((x, unit)) => (format!("B{block:03}.{} [{unit}]", i + 1), ecu_werkbank::csvlog::num(x)),
            None => (tr!("B{block}.{a} [roh]", block = format!("{:03}", block), a = i + 1), format!("{:02X}{:02X}{:02X}", v.formula, v.a, v.b)),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// OBD-II
// ---------------------------------------------------------------------------

fn obd_info(be: &Backend, tx: u16, rx: u16) -> Result<()> {
    use ecu_werkbank::obd::Obd;
    let tp = be.open(tx, rx)?;
    let obd = Obd::new(&*tp);
    match obd.vin() {
        Ok(v) => println!("{}", tr!("VIN:          {v}", v = v)),
        Err(e) => println!("{}", tr!("VIN:          nicht lesbar ({e})", e = e)),
    }
    let r = obd.readiness()?;
    println!("{}", tr!("MIL (Motorkontrollleuchte): {a}", a = if r.mil { "AN" } else { "aus" }));
    println!("{}", tr!("Gespeicherte Fehler:        {a}", a = r.dtc_count));
    println!("{}", tr!("Motor:                      {a}", a = if r.compression { "Diesel" } else { "Benziner" }));
    println!("{}", tr!("\nReadiness:"));
    for m in r.monitors.iter().filter(|m| m.available) {
        println!("  {:<24} {}", m.name, if m.complete { tr!("fertig ✓") } else { tr!("NICHT fertig") });
    }
    println!(
        "\n→ {}",
        if r.all_complete() { tr!("Alle Tests abgeschlossen (z. B. bereit für AU/HU).") } else { tr!("Noch nicht alle Tests abgeschlossen — weiterfahren.") }
    );
    Ok(())
}

fn obd_live(
    be: &Backend,
    tx: u16,
    rx: u16,
    pids: &[u8],
    count: u32,
    interval: u64,
    csv: Option<&std::path::Path>,
) -> Result<()> {
    use ecu_werkbank::obd::{self, Obd};
    let tp = be.open(tx, rx)?;
    let o = Obd::new(&*tp);
    let pids: Vec<u8> = if pids.is_empty() {
        let sup = o.supported_pids()?;
        [0x0C, 0x0D, 0x05, 0x04, 0x11, 0x10, 0x0F, 0x42]
            .into_iter()
            .filter(|p| sup.contains(p))
            .collect()
    } else {
        pids.to_vec()
    };
    let mut log = csv.map(ecu_werkbank::csvlog::CsvLog::create).transpose()?;
    let mut n = 0u32;
    loop {
        let mut cols = Vec::new();
        let mut shown = Vec::new();
        for &pid in &pids {
            let unit = obd::pid_info(pid).map_or("", |(_, u)| u);
            let name = obd::pid_info(pid).map_or_else(|| format!("PID {pid:02X}"), |(n, _)| n.to_string());
            match o.read_value(pid) {
                Ok(v) => {
                    shown.push(format!("{name} {v:.1} {unit}"));
                    cols.push((format!("{name} [{unit}]"), ecu_werkbank::csvlog::num(v)));
                }
                Err(_) => {
                    shown.push(format!("{name} —"));
                    cols.push((format!("{name} [{unit}]"), String::new()));
                }
            }
        }
        println!("{}", shown.join("  |  "));
        if let Some(l) = log.as_mut() {
            l.write(&cols)?;
        }
        n += 1;
        if count != 0 && n >= count {
            break;
        }
        std::thread::sleep(Duration::from_millis(interval));
    }
    if let Some(l) = log {
        println!("{}", tr!("{a} Zeile(n) in {b} gespeichert.", a = l.rows(), b = l.path().display()));
    }
    Ok(())
}

fn obd_dtc(be: &Backend, tx: u16, rx: u16) -> Result<()> {
    use ecu_werkbank::obd::{self, Obd};
    let tp = be.open(tx, rx)?;
    let o = Obd::new(&*tp);
    for (svc, what) in [(obd::DTC_STORED, ecu_werkbank::trs!("Gespeichert")), (obd::DTC_PENDING, ecu_werkbank::trs!("Sporadisch")), (obd::DTC_PERMANENT, ecu_werkbank::trs!("Permanent"))] {
        match o.read_dtcs(svc) {
            Ok(c) if c.is_empty() => println!("{what:<12} {}", tr!("keine")),
            Ok(c) => println!("{what:<12} {}", c.join(", ")),
            Err(_) => println!("{}", tr!("{what} nicht unterstützt", what = format!("{:<12}", what))),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Mitschnitte
// ---------------------------------------------------------------------------

fn decode_log(file: &std::path::Path, report_only: bool) -> Result<()> {
    let text = std::fs::read_to_string(file).with_context(|| tr!("Log lesen: {a}", a = file.display()))?;
    let d = ecu_werkbank::canlog::Decoder::decode_text(&text);
    if !report_only {
        for l in d.lines() {
            println!("{l}");
        }
        println!();
    }
    print!("{}", d.pruefbericht());
    Ok(())
}

fn record(be: &Backend, iface: &str, out: &std::path::Path, seconds: u64) -> Result<()> {
    use std::io::Write;
    if let Backend::Demo = be {
        std::fs::write(out, ecu_werkbank::canlog::demo_log())?;
        println!("{}", tr!("Beispiel-Mitschnitt aus dem Demo-Fahrzeug: {a}", a = out.display()));
        println!("{}", tr!("Ansehen mit:  ecu-werkbank decode-log {a}", a = out.display()));
        return Ok(());
    }
    let Backend::SocketCan { .. } = be else {
        anyhow::bail!(tr!("Mitschneiden geht nur mit SocketCAN (oder -t demo für ein Beispiel)"));
    };
    let sock = CanSocket::open(iface).with_context(|| tr!("CAN-Interface '{iface}' öffnen", iface = iface))?;
    sock.set_read_timeout(Duration::from_millis(200))?;
    let mut w = std::io::BufWriter::new(std::fs::File::create(out)?);
    println!("{}", tr!("Schneide {seconds}s auf {iface} mit → {a} (Strg-C bricht ab)…", seconds = seconds, iface = iface, a = out.display()));
    let start = Instant::now();
    let mut n = 0usize;
    while start.elapsed() < Duration::from_secs(seconds) {
        let Ok(frame) = sock.read_frame() else { continue };
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let id = match frame.id() {
            Id::Standard(s) => u32::from(s.as_raw()),
            Id::Extended(e) => e.as_raw(),
        };
        writeln!(w, "{}", ecu_werkbank::canlog::format_line(t, iface, id, frame.data()))?;
        n += 1;
        if n.is_multiple_of(100) {
            w.flush()?;
        }
    }
    w.flush()?;
    println!("{}", tr!("{n} Frames gespeichert. Auswerten mit:  ecu-werkbank decode-log {a}", n = n, a = out.display()));
    Ok(())
}

// ---------------------------------------------------------------------------
// Codierungs-Sicherung
// ---------------------------------------------------------------------------

fn coding_backup(be: &Backend, db: &LabelDb, with_tp20: bool, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::backup;
    let active = active_vehicle(fahrzeug)?;
    let mut b = backup::backup(be, db, with_tp20, |_| {})?;
    b.vehicle = ecu_werkbank::vehicle::resolve(b.vin.as_deref(), active.as_deref())?.key;
    if b.entries.is_empty() {
        anyhow::bail!(tr!("Keine Steuergeräte erreichbar — nichts gesichert."));
    }
    for e in &b.entries {
        let data = if e.data_hex.is_empty() { e.note.clone() } else { e.data_hex.clone() };
        println!("  {:<34} {data}", e.title());
    }
    let p = backup::save(&b)?;
    println!("{}", tr!("\n{a} Steuergerät(e) gesichert: {b}", a = b.entries.len(), b = p.display()));
    Ok(())
}

fn coding_list() -> Result<()> {
    let all = ecu_werkbank::backup::list_all();
    if all.is_empty() {
        println!("{}", tr!("Noch keine Sicherungen. Anlegen mit:  ecu-werkbank coding-backup"));
    }
    for (p, b) in all {
        println!("{}", tr!("{a}  {b} {c} Steuergerät(e)  {d}", a = b.created, b = format!("{:<18}", b.vin.as_deref().unwrap_or(ecu_werkbank::trs!("ohne VIN"))), c = format!("{:>2}", b.entries.len()), d = p.display()));
    }
    Ok(())
}

fn coding_compare(old: Option<&std::path::Path>, new: Option<&std::path::Path>, db: &LabelDb) -> Result<()> {
    use ecu_werkbank::backup;
    let (a, b) = match (old, new) {
        (Some(o), Some(n)) => (backup::load(o)?, backup::load(n)?),
        _ => {
            let all = backup::list_all();
            if all.len() < 2 {
                anyhow::bail!(tr!("Zum Vergleich braucht es zwei Sicherungen (vorhanden: {a}).", a = all.len()));
            }
            (all[1].1.clone(), all[0].1.clone())
        }
    };
    println!("{}", tr!("Sicherung vom {a}  →  {b}\n", a = a.created, b = b.created));
    let changes = backup::compare(&a, &b, db);
    if changes.is_empty() {
        println!("{}", tr!("Keine Codierung geändert."));
    }
    for c in changes {
        println!("{}", c.module);
        let zeig = |h: &str| if h.trim().is_empty() { tr!("(nicht lesbar)") } else { h.to_string() };
        println!("  {}", tr!("vorher: {a}", a = zeig(&c.old_hex)));
        println!("  {}", tr!("jetzt:  {a}", a = zeig(&c.new_hex)));
        for (byte, bit, x, y, name) in c.bits {
            let name = if name.is_empty() { String::new() } else { format!("  {name}") };
            println!("{}", tr!("    Byte {byte} Bit {bit}: {a} → {b}{name}", byte = byte, bit = bit, a = if x { tr!("ein") } else { tr!("aus") }, b = if y { tr!("ein") } else { tr!("aus") }, name = name));
        }
    }
    Ok(())
}

fn coding_restore(be: &Backend, file: &std::path::Path, tx: u16, ja: bool) -> Result<()> {
    use ecu_werkbank::backup;
    let b = backup::load(file)?;
    let e = b
        .entries
        .iter()
        .find(|e| e.tx == tx)
        .with_context(|| tr!("Steuergerät {tx} ist nicht in dieser Sicherung", tx = format!("{:#05x}", tx)))?;
    if !e.can_restore() {
        anyhow::bail!(tr!("{a}: Zurückschreiben nicht möglich ({b})", a = e.title(), b = e.note));
    }
    // Aktuellen Stand zeigen
    let tp = be.open(e.tx, e.rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(ecu_werkbank::uds::SESSION_DEFAULT)?;
    let now = uds.read_data_by_id(e.did.expect("geprüft"))?;
    drop(tp);
    println!("{}", e.title());
    println!("  aktuell:      {}", backup::hex(&now));
    println!("{}", tr!("  Sicherung:    {a}  (vom {b})", a = e.data_hex, b = b.created));
    if now == e.bytes() {
        println!("{}", tr!("\nIdentisch — nichts zu tun."));
        return Ok(());
    }
    if !ja {
        println!("{}", tr!("\nNichts geschrieben. Zum Zurückschreiben denselben Befehl mit --ja ausführen."));
        return Ok(());
    }
    let login = ecu_werkbank::login::fuer(&b.vehicle, e.tx);
    backup::restore(be, e, login.as_ref())?;
    println!("{}", tr!("\nZurückgeschrieben und kontrolliert ✓"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Schnell-Check, Batterie
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Service-Anzeige
// ---------------------------------------------------------------------------

fn sprache_cmd(setzen: Option<String>, vorlage: Option<String>, datei: Option<PathBuf>, verwaist: bool, aufraeumen: bool) -> Result<()> {
    use ecu_werkbank::i18n;
    if verwaist || aufraeumen {
        let p = datei.context(tr!("Bitte mit --datei die Sprachdatei angeben, z. B. --datei lang/en.toml"))?;
        let text = std::fs::read_to_string(&p).with_context(|| tr!("Lesen: {a}", a = p.display()))?;
        let liste = i18n::verwaist(&text);
        if liste.is_empty() {
            println!("{}", tr!("{a}: keine verwaisten Einträge.", a = p.display()));
            return Ok(());
        }
        println!("{}", tr!("{a}: {n} verwaiste Einträge (deutscher Text kommt im Programm nicht mehr vor):", a = p.display(), n = liste.len()));
        for k in &liste {
            println!("  {k}");
        }
        if aufraeumen {
            let (neu, n) = i18n::aufraeumen(&text);
            std::fs::copy(&p, p.with_extension("toml.bak"))?;
            std::fs::write(&p, neu)?;
            println!("{}", tr!("{n} Einträge entfernt. Sicherung: {a}", n = n, a = p.with_extension("toml.bak").display()));
        } else {
            println!("{}", tr!("Entfernen mit --aufraeumen."));
        }
        return Ok(());
    }
    if let Some(c) = setzen {
        i18n::set_sprache(&c, true);
        println!("{}", tr!("Sprache: {a}", a = i18n::sprache()));
    }
    if let Some(c) = vorlage {
        let p = datei.unwrap_or_else(|| ecu_werkbank::history::data_dir().join("sprachen").join(format!("{c}.toml")));
        if p.exists() {
            // vorhandene Datei sichern, ihre Übersetzungen stecken schon in der Vorlage
            std::fs::copy(&p, p.with_extension("toml.bak"))?;
        }
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, i18n::vorlage(&c))?;
        let (da, alle) = i18n::abdeckung(&c);
        println!("{}", tr!("Vorlage geschrieben: {a}  ({da} von {alle} Texten übersetzt)", a = p.display(), da = da, alle = alle));
        println!("{}", tr!("Rechts die Übersetzung eintragen; leere Zeilen bleiben deutsch. Wirkt beim nächsten Start."));
        return Ok(());
    }
    let akt = i18n::sprache();
    for (c, n) in i18n::verfuegbar() {
        let (da, alle) = i18n::abdeckung(&c);
        let anteil = if c == "de" { "Grundsprache".to_string() } else { tr!("{da}/{alle} Texte", da = da, alle = alle) };
        println!("{} {c:<4} {n:<14} {anteil}", if c == akt { "●" } else { " " });
    }
    println!("{}", tr!("\nWählen: ecu-werkbank sprache --setzen en   ·   Neue Sprache: ecu-werkbank sprache --vorlage fr"));
    Ok(())
}

/// Fahrzeug (falls angegeben) und Service-Einstellungen.
fn service_cfg(be: &Backend, fz: Option<&str>) -> Result<(Option<ecu_werkbank::vehicle::Vehicle>, service::Config)> {
    let v = match fz {
        Some(q) => Some(ecu_werkbank::vehicle::find(q).with_context(|| tr!("Fahrzeug '{q}' unbekannt", q = q))?),
        // am echten Auto sonst das in der Oberfläche gewählte Fahrzeug
        None if !matches!(be, Backend::Demo) => load_active().and_then(|k| ecu_werkbank::vehicle::load(&k)),
        None => None,
    };
    let cfg = service::config_fuer(be, v.as_ref());
    Ok((v, cfg))
}

fn service_ordner(be: &Backend, v: Option<&ecu_werkbank::vehicle::Vehicle>) -> Result<std::path::PathBuf> {
    service::ordner(be, v).context(tr!("Bitte das Fahrzeug angeben: -f \"B XY 123\" (die Daten werden im Fahrzeugordner gespeichert)"))
}

use service::merk_pfad;

fn service_cmd(be: &Backend, a: ServiceAktion, fz: Option<&str>) -> Result<()> {
    let (v, cfg) = service_cfg(be, fz)?;
    match a {
        ServiceAktion::Lesen => {
            if cfg.werte.is_empty() {
                println!("{}", tr!("Für dieses Fahrzeug sind noch keine Service-Kennungen bekannt."));
                println!("{}", tr!("So lernt ECUWerkbank sie (nur lesen, bis auf die Tasten-Rückstellung):"));
                println!("{}", tr!("  1. ecu-werkbank -f <Fahrzeug> service merken --name vorher"));
                println!("{}", tr!("  2. Service mit den Tasten am Kombi zurückstellen (ecu-werkbank service anleitung)"));
                println!("{}", tr!("  3. ecu-werkbank -f <Fahrzeug> service merken --name nachher"));
                println!("{}", tr!("  4. ecu-werkbank -f <Fahrzeug> service vergleich --uebernehmen"));
                println!("{}", tr!("Oder ohne Tasten-Rückstellung: Anzeige im Kombi ablesen und `service suche --km … --tage … --einrichten`."));
                return Ok(());
            }
            println!("{}", tr!("Service-Anzeige (Kombi {a}):", a = format!("{:#05x}", cfg.tx)));
            for g in service::lesen(be, &cfg)? {
                println!("  [{}] {}", g.stufe().text(), g.zeile());
            }
            if !cfg.kann_zuruecksetzen() {
                println!("{}", tr!("\nRückstellung über ECUWerkbank noch nicht eingerichtet (keine Rückstellwerte)."));
            }
        }
        ServiceAktion::Anleitung => {
            for z in service::anleitung_tasten(v.as_ref().map(|v| v.bezeichnung.as_str()).unwrap_or("golf")) {
                println!("{z}");
            }
        }
        ServiceAktion::Merken { name, von, bis } => {
            let ordner = service_ordner(be, v.as_ref())?;
            let bereiche: Vec<(u16, u16)> = match (von, bis) {
                (Some(a), Some(b)) => vec![(a, b)],
                (None, None) => service::bereiche(be),
                _ => anyhow::bail!(tr!("--von und --bis bitte zusammen angeben")),
            };
            let mut alle = service::Schnappschuss::new();
            for (a, b) in bereiche {
                let s = service::schnappschuss(be, cfg.tx, cfg.rx, a, b, |d, n| eprintln!("{}", tr!("… {d} ({n} lesbar)", d = format!("{:#06X}", d), n = n)))?;
                alle.extend(s);
            }
            let p = merk_pfad(&ordner, &name);
            service::speichern(&alle, &p)?;
            println!("{}", tr!("{a} Kennungen gemerkt: {b}", a = alle.len(), b = p.display()));
        }
        ServiceAktion::Vergleich { vorher, nachher, ruhe, uebernehmen } => {
            let ordner = service_ordner(be, v.as_ref())?;
            let a = service::laden(&merk_pfad(&ordner, &vorher))?;
            let b = service::laden(&merk_pfad(&ordner, &nachher))?;
            let r = ruhe.map(|n| service::laden(&merk_pfad(&ordner, &n))).transpose()?;
            let d = service::vergleich(&a, &b, r.as_ref());
            if d.is_empty() {
                println!("{}", tr!("Keine Kennung hat sich geändert — wurde der Service zwischen den beiden Ständen zurückgestellt?"));
                return Ok(());
            }
            println!("{}", tr!("{a} geänderte Kennung(en):", a = d.len()));
            for x in &d {
                println!("  {}", x.zeile());
            }
            let vor = service::vorschlag(&d);
            println!("{}", tr!("\nVorschlag (Art geraten — bitte mit der Anzeige im Kombi vergleichen):"));
            for w in &vor {
                println!("{}", tr!("  {a}  {b}  Rückstellwert {c}", a = format!("{:#06X}", w.did), b = format!("{:<22}", format!("{:?}", w.art)), c = w.reset));
            }
            if d.len() > 8 {
                println!("{}", tr!("\nViele Änderungen — besser mit --ruhe <Stand ohne Rückstellung> filtern."));
            }
            if uebernehmen {
                let mut v = v.context(tr!("Zum Übernehmen bitte das Fahrzeug angeben (-f)"))?;
                v.service = Some(service::Config { werte: vor, ..cfg });
                ecu_werkbank::vehicle::save(&v)?;
                println!("{}", tr!("\nIn fahrzeug.toml übernommen. Prüfen mit: ecu-werkbank -f {a} service", a = v.key));
            } else {
                println!("{}", tr!("\nÜbernehmen mit --uebernehmen."));
            }
        }
        ServiceAktion::Suche { km, tage, oel_km, oel_tage, aus, einrichten, intervall } => {
            use service::Art;
            let ziele: Vec<(Art, f64)> = [(Art::KmBisInspektion, km), (Art::TageBisInspektion, tage), (Art::KmBisOelwechsel, oel_km), (Art::TageBisOelwechsel, oel_tage)]
                .into_iter()
                .filter_map(|(a, w)| w.map(|w| (a, w)))
                .collect();
            if ziele.is_empty() {
                anyhow::bail!(tr!("Bitte mindestens einen angezeigten Wert angeben, z. B. --km 4800 --tage 212"));
            }
            let s = match aus {
                Some(n) => service::laden(&merk_pfad(&service_ordner(be, v.as_ref())?, &n))?,
                None => {
                    let bereiche = service::bereiche(be);
                    let mut alle = service::Schnappschuss::new();
                    for (a, b) in bereiche {
                        alle.extend(service::schnappschuss(be, cfg.tx, cfg.rx, a, b, |d, n| eprintln!("{}", tr!("… {d} ({n} lesbar)", d = format!("{:#06X}", d), n = n)))?);
                    }
                    alle
                }
            };
            let k = service::suche(&s, &ziele);
            if k.is_empty() {
                println!("{}", tr!("Keine passende Kennung gefunden ({a} gelesen). Tipp: Vorher/Nachher-Vergleich (service merken).", a = s.len()));
            } else {
                println!("{}", tr!("{a} Kandidat(en) aus {b} Kennungen:", a = k.len(), b = s.len()));
                for c in &k {
                    println!("{}", tr!("  {a}  Byte {b} ({c} B{d}, ×{f})  = {g}  → {h} {i}", a = format!("{:#06X}", c.wert.did), b = c.wert.offset, c = c.wert.laenge, d = if c.wert.vorzeichen { ecu_werkbank::trs!(", mit Vorzeichen") } else { "" }, f = c.wert.faktor, g = format!("{:.0}", c.gelesen), h = c.art.text(), i = c.art.einheit()));
                }
                println!("{}", tr!("\nNur eine Kennung je Wert ist richtig. Sicher wird es mit dem Vorher/Nachher-Vergleich."));
                if einrichten {
                    let zahlen: Vec<f64> = intervall.split(',').map(|x| x.trim().parse::<f64>()).collect::<Result<_, _>>().map_err(|e| anyhow::anyhow!(tr!("--intervall: {e}", e = e)))?;
                    if zahlen.len() != 4 {
                        anyhow::bail!(tr!("--intervall braucht 4 Zahlen: km,Tage Inspektion, km,Tage Ölwechsel"));
                    }
                    let iv: Vec<(Art, f64)> = service::INTERVALL_VORSCHLAG.iter().zip(&zahlen).map(|((a, _), n)| (*a, *n)).collect();
                    // je Wert nur, wenn genau ein Treffer — sonst ist unklar, welche Kennung stimmt
                    let mut gewaehlt = Vec::new();
                    for (a, _) in &ziele {
                        let t: Vec<_> = k.iter().filter(|c| c.art == *a).collect();
                        match t.as_slice() {
                            [eins] => gewaehlt.push((*eins).clone()),
                            [] => println!("{}", tr!("  {a}: kein Treffer — wird nicht eingerichtet", a = a.text())),
                            _ => println!("{}", tr!("  {a}: {n} Treffer — nicht eindeutig, wird nicht eingerichtet (in der GUI auswählen)", a = a.text(), n = t.len())),
                        }
                    }
                    let Some(mut v) = v else { anyhow::bail!(tr!("Bitte ein Fahrzeug wählen (-f)")) };
                    let c = service::aus_suche(&cfg, &gewaehlt, &iv)?;
                    println!("{}", tr!("\nEingerichtet — bei der Rückstellung wird geschrieben:"));
                    for z in c.vorschau() {
                        println!("  {z}");
                    }
                    v.service = Some(c);
                    ecu_werkbank::vehicle::save(&v)?;
                    println!("{}", tr!("\nZurückstellen mit: ecu-werkbank -f {a} service zuruecksetzen", a = v.key));
                }
            }
        }
        ServiceAktion::Zuruecksetzen { ja, ohne_wartung } => {
            if !cfg.kann_zuruecksetzen() {
                println!("{}", tr!("Rückstellung über ECUWerkbank ist für dieses Fahrzeug noch nicht eingerichtet."));
                println!("{}", tr!("Sicherer Weg — mit den Tasten am Kombi:\n"));
                for z in service::anleitung_tasten(v.as_ref().map(|v| v.bezeichnung.as_str()).unwrap_or("golf")) {
                    println!("{z}");
                }
                return Ok(());
            }
            println!("{}", tr!("Aktuell:"));
            for g in service::lesen(be, &cfg)? {
                println!("  {}", g.zeile());
            }
            println!("{}", tr!("\nWird geschrieben (Kombi {a}):", a = format!("{:#05x}", cfg.tx)));
            for z in cfg.vorschau() {
                println!("  {z}");
            }
            if !ja {
                println!("{}", tr!("\nNichts geschrieben. Zündung an, Motor aus — dann denselben Befehl mit --ja ausführen."));
                return Ok(());
            }
            let ordner = service_ordner(be, v.as_ref()).ok();
            let r = service::zuruecksetzen(be, &cfg, ordner.as_deref())?;
            for z in &r.zeilen {
                println!("  {z}");
            }
            if let Some(p) = &r.sicherung {
                println!("{}", tr!("Alte Werte gesichert: {a}", a = p.display()));
            }
            if r.ok {
                println!("{}", tr!("\nService zurückgestellt ✓"));
                for g in service::lesen(be, &cfg)? {
                    println!("  {}", g.zeile());
                }
                match v {
                    Some(mut v) if !ohne_wartung => {
                        let n = service::wartung_eintragen(&mut v);
                        ecu_werkbank::vehicle::save(&v)?;
                        println!("{}", tr!("Im Wartungsplan als heute erledigt eingetragen: {a}", a = n.join(", ")));
                    }
                    _ => {}
                }
            } else {
                anyhow::bail!(tr!("Rückstellung nicht vollständig — Anzeige im Kombi prüfen; rückgängig mit `service rueckgaengig --ja`"));
            }
        }
        ServiceAktion::Rueckgaengig { datei, ja } => {
            let ordner = service_ordner(be, v.as_ref())?;
            let p = match datei {
                Some(p) => p,
                None => service::neueste_sicherung(&ordner).context(tr!("Keine Service-Sicherung gefunden"))?,
            };
            let s = service::laden(&p)?;
            println!("{}", tr!("Sicherung {a}:", a = p.display()));
            for (d, b) in &s {
                println!("  {d:#06X}  ← {}", b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" "));
            }
            if !ja {
                println!("{}", tr!("\nNichts geschrieben. Zum Zurückschreiben mit --ja ausführen."));
                return Ok(());
            }
            for z in service::sicherung_zurueck(be, &cfg, &p)? {
                println!("  {z}");
            }
            println!("{}", tr!("Alte Werte zurückgeschrieben ✓"));
        }
    }
    Ok(())
}

fn need_vehicle(q: Option<&str>) -> Result<ecu_werkbank::vehicle::Vehicle> {
    let q = q.context(tr!("Bitte das Fahrzeug angeben: -f \"B XY 123\" (siehe `ecu-werkbank fahrzeuge`)"))?;
    ecu_werkbank::vehicle::find(q).with_context(|| tr!("Fahrzeug '{q}' unbekannt", q = q))
}

fn quick_check(be: &Backend, db: &LabelDb, fahrzeug: Option<&str>) -> Result<()> {
    let active = active_vehicle(fahrzeug)?;
    println!("{}", tr!("Schnell-Check über {a} …\n", a = be.describe()));
    let res = ecu_werkbank::check::run(be, db, active.as_deref(), |_| {})?;
    let r = &res.report;
    for (st, t) in &r.check {
        let mark = match st {
            ecu_werkbank::limits::Stufe::Gut => "🟢",
            ecu_werkbank::limits::Stufe::Hinweis => "🟡",
            ecu_werkbank::limits::Stufe::Warnung => "🔴",
        };
        println!("  {mark} {t}");
    }
    let v = ecu_werkbank::vehicle::load(&r.vehicle).context(tr!("Fahrzeugordner fehlt"))?;
    let out = v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("schnellcheck", "pdf"));
    r.save(&out)?;
    println!("{}", tr!("\nGesamt: {a}   PDF: {b}", a = res.gesamt.text(), b = out.display()));
    Ok(())
}

fn battery_test(be: &Backend, seconds: u64, fahrzeug: Option<&str>) -> Result<()> {
    use std::io::Write;
    use ecu_werkbank::battery;
    println!("{}", tr!("Batterietest: Zündung AN, Motor AUS. Messung beginnt in 3 s —"));
    println!("{}", tr!("sobald „JETZT STARTEN“ erscheint, den Motor starten und laufen lassen.\n"));
    std::thread::sleep(Duration::from_secs(3));
    let mut shown = false;
    let pts = battery::measure(be, seconds as f64, |t, v| {
        if t > 2.0 && !shown {
            println!("{}", tr!("\n>>> JETZT STARTEN <<<\n"));
            shown = true;
        }
        print!("\r  {t:5.1} s   {v:5.2} V   ");
        let _ = std::io::stdout().flush();
    })?;
    println!("\n");
    let r = battery::analyze(&pts);
    for (st, t) in &r.bewertung {
        println!("  [{}] {t}", st.text());
    }
    println!("{}", tr!("\nGesamt: {a}", a = r.gesamt().text()));
    let veh = match fahrzeug {
        Some(q) => ecu_werkbank::vehicle::find(q),
        None => None,
    };
    let mut meta = vec![("Datum".to_string(), ecu_werkbank::report::now_local()), ("Ergebnis".to_string(), r.gesamt().text().to_string())];
    if let Some(v) = &veh {
        meta.insert(0, (tr!("Fahrzeug"), v.label()));
    }
    for (i, (_, t)) in r.bewertung.iter().enumerate() {
        meta.push((tr!("Bewertung {a}", a = i + 1), t.clone()));
    }
    if let Some(v) = &veh {
        let mut tr = Vec::new();
        if let Some(x) = r.ruhe { tr.push(("Ruhespannung".to_string(), x, "V".to_string())); }
        if let Some(x) = r.minimum { tr.push(("Startspannung min".to_string(), x, "V".to_string())); }
        if let Some(x) = r.laden { tr.push(("Ladespannung".to_string(), x, "V".to_string())); }
        let _ = ecu_werkbank::trend::add(&v.key, v.km, &tr);
    }
    let col = ecu_werkbank::csvlog::Column {
        name: "Bordspannung".into(),
        unit: "V".into(),
        points: pts,
        soll: Some(ecu_werkbank::limits::Range { min: Some(9.6), max: Some(14.9) }),
    };
    let dir = match &veh {
        Some(v) => v.sub("berichte")?,
        None => ecu_werkbank::vehicle::resolve(None, None)?.sub("berichte")?,
    };
    let out = dir.join(ecu_werkbank::vehicle::stamped("batterietest", "pdf"));
    std::fs::write(&out, ecu_werkbank::chart::to_pdf(ecu_werkbank::trs!("Batterie- und Starttest"), &meta, &[col]))?;
    println!("{}", tr!("PDF: {a}", a = out.display()));
    Ok(())
}

fn handy(be: &Backend, db: &LabelDb, port: u16, pids: &[u16], seconds: u64, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::{limits::Stufe, obd, web};
    let pids: Vec<u8> = if pids.is_empty() { vec![0x0C, 0x0D, 0x05, 0x04, 0x42] } else { pids.iter().map(|p| *p as u8).collect() };
    let data: web::Shared = Default::default();
    let srv = web::serve(port, data.clone())?;
    let title = fahrzeug.and_then(ecu_werkbank::vehicle::find).map(|v| v.label()).unwrap_or_else(|| "ECUWerkbank".into());
    println!("{}", tr!("Handy-Anzeige läuft. Im Handy-Browser öffnen:\n\n    {a}\n", a = srv.url));
    println!("{}", tr!("(Handy und Rechner im gleichen WLAN/Hotspot. Strg-C beendet.)"));
    let start = Instant::now();
    loop {
        let tiles: Vec<web::Tile> = match be.open(obd::DEFAULT_TX, obd::DEFAULT_RX) {
            Ok(tp) => {
                let o = obd::Obd::new(&*tp);
                pids.iter()
                    .map(|&pid| {
                        let (name, unit) = obd::pid_info(pid).unwrap_or(("PID", ""));
                        let value = o.read_value(pid).ok();
                        let state = match value.and_then(|v| db.sollwert("obd", None, u16::from(pid), None).map(|r| r.check(v))) {
                            Some(Stufe::Gut) => "gut",
                            Some(Stufe::Hinweis) => "hinweis",
                            Some(Stufe::Warnung) => "warnung",
                            None => "",
                        };
                        web::Tile { name: name.into(), unit: unit.into(), value, state: state.into() }
                    })
                    .collect()
            }
            Err(_) => Vec::new(),
        };
        {
            let mut d = data.lock().unwrap();
            d.title = title.clone();
            d.alarm = tiles
                .iter()
                .filter(|t| t.state == "warnung")
                .map(|t| format!("⚠ {} {} {}", t.name, ecu_werkbank::csvlog::num(t.value.unwrap_or(0.0)), t.unit))
                .collect::<Vec<_>>()
                .join("  ");
            d.tiles = tiles;
        }
        if seconds > 0 && start.elapsed() >= Duration::from_secs(seconds) {
            break;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    srv.stop();
    Ok(())
}

// ---------------------------------------------------------------------------
// Zielbereich / Stellfunktion
// ---------------------------------------------------------------------------

static STOP_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_sigint(_: libc::c_int) {
    STOP_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Strg-C abfangen, damit wir sauber aufräumen können.
fn catch_ctrl_c() {
    // SAFETY: Der Handler setzt nur ein AtomicBool (async-signal-sicher).
    unsafe {
        libc::signal(libc::SIGINT, on_sigint as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
}

fn stop_requested() -> bool {
    STOP_REQUESTED.load(std::sync::atomic::Ordering::SeqCst)
}

/// Balken: Ziel in der Mitte, ● = Istwert.
fn band_bar(ist: f64, ziel: f64, tol: f64) -> String {
    let width = 31i64;
    let span = tol * 4.0;
    let pos = (((ist - ziel) / span + 0.5) * (width - 1) as f64).round().clamp(0.0, (width - 1) as f64) as i64;
    let lo = (((-tol) / span + 0.5) * (width - 1) as f64).round() as i64;
    let hi = (((tol) / span + 0.5) * (width - 1) as f64).round() as i64;
    (0..width)
        .map(|i| if i == pos { '●' } else if i >= lo && i <= hi { '=' } else { '-' })
        .collect()
}

fn ziel(be: &Backend, pid: u8, muster: &str, toleranz: f64, seconds: u64) -> Result<()> {
    use std::io::Write;
    use ecu_werkbank::control::{bewerten, Muster, Richtung};
    let m = Muster::parse(muster)?;
    let (name, unit) = ecu_werkbank::obd::pid_info(pid).unwrap_or(("Wert", ""));
    catch_ctrl_c();
    println!("{}", tr!("Fahrer-Assistent: {name} {a} (±{b} {unit}). Strg-C beendet.\n", name = name, a = m.text(), b = ecu_werkbank::csvlog::num(toleranz), unit = unit));
    let tp = be.open(ecu_werkbank::obd::DEFAULT_TX, ecu_werkbank::obd::DEFAULT_RX)?;
    let o = ecu_werkbank::obd::Obd::new(&*tp);
    let start = Instant::now();
    let (mut n, mut inside) = (0u32, 0u32);
    while !stop_requested() && (seconds == 0 || start.elapsed() < Duration::from_secs(seconds)) {
        let t = start.elapsed().as_secs_f64();
        let soll = m.ziel(t);
        if let Ok(ist) = o.read_value(pid) {
            let (r, _) = bewerten(ist, soll, toleranz);
            n += 1;
            if r == Richtung::Passt {
                inside += 1;
            }
            let hint = match r {
                Richtung::Mehr => ecu_werkbank::trs!("▲ mehr Gas"),
                Richtung::Weniger => ecu_werkbank::trs!("▼ weniger Gas"),
                Richtung::Passt => ecu_werkbank::trs!("✓ im Bereich"),
            };
            print!("\r  {ist:7.0} {unit:<5} [{}]  {hint:<14} {}   ", band_bar(ist, soll, toleranz), tr!("Ziel {a}", a = format!("{soll:.0}")));
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    println!("{}", tr!("\n\nIm Bereich: {a} % der Zeit ({n} Messungen).", a = format!("{:.0}", if n > 0 { f64::from(inside) * 100.0 / f64::from(n) } else { 0.0 }), n = n));
    Ok(())
}

fn stellen(be: &Backend, db: &LabelDb, funktion: &str, muster: &str, seconds: u64, ja: bool) -> Result<()> {
    use ecu_werkbank::control::{demo_stellfunktion, Muster, Regler};
    let m = Muster::parse(muster)?;
    let f = if funktion.eq_ignore_ascii_case("demo") {
        if !matches!(be, Backend::Demo) {
            anyhow::bail!(tr!("Die Demo-Stellfunktion gibt es nur mit -t demo"));
        }
        demo_stellfunktion()
    } else {
        db.stellfunktionen()
            .iter()
            .find(|f| f.name.to_lowercase().contains(&funktion.to_lowercase()))
            .cloned()
            .with_context(|| {
                let names: Vec<&str> = db.stellfunktionen().iter().map(|f| f.name.as_str()).collect();
                tr!("Stellfunktion '{funktion}' nicht in den Labels (vorhanden: {a})", funktion = funktion, a = if names.is_empty() { tr!("keine") } else { names.join(", ") })
            })?
    };
    println!("{}", tr!("Stellfunktion: {a}", a = f.name));
    println!("{}", tr!("  Steuergerät {a}/{b}, {c} {d}", a = format!("{:#05x}", f.tx), b = format!("{:#05x}", f.rx), c = f.art, d = format!("{:#06x}", f.id)));
    println!("{}", tr!("  Muster:      {a}  (begrenzt auf {b}–{c} {d})", a = m.text(), b = f.min, c = f.max, d = f.einheit));
    println!("{}", tr!("  Dauer:       {seconds} s (Zeitlimit Steuergerät-Funktion: {a} s)", seconds = seconds, a = f.max_dauer_s));
    println!("{}", tr!("  Sicherheit:  nur im Stand; Abbruch bei Fahrt, Überschreitung, Fehlern, Strg-C"));
    if !ja {
        println!("{}", tr!("\nNichts ausgeführt. Fahrzeug steht, Handbremse angezogen, Gang raus/P? Dann mit --ja starten."));
        return Ok(());
    }
    catch_ctrl_c();
    let mut r = Regler::start(be, f)?;
    println!("{}", tr!("\nLäuft — Strg-C bricht ab und gibt die Kontrolle zurück.\n"));
    let result = (|| -> Result<()> {
        while !stop_requested() && r.laufzeit() < seconds as f64 {
            let soll = m.ziel(r.laufzeit());
            match r.schritt(soll) {
                Ok(x) => println!("{}", tr!("  {a} s  Soll {b}  Ist {c}", a = format!("{:5.1}", r.laufzeit()), b = format!("{:6.0}", x.soll), c = format!("{:>6}", x.ist.map(|v| format!("{v:.0}")).unwrap_or_else(|| "—".into())))),
                Err(e) if ecu_werkbank::control::ist_abbruch(&e) => return Err(e),
                Err(e) => println!("{}", tr!("  (Fehler, wird wiederholt: {e})", e = e)),
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Ok(())
    })();
    r.stop();
    println!("{}", tr!("\nKontrolle ans Steuergerät zurückgegeben."));
    result
}

fn obd_tests(be: &Backend) -> Result<()> {
    use ecu_werkbank::obd::{self, Obd};
    let tp = be.open(obd::DEFAULT_TX, obd::DEFAULT_RX)?;
    let tests = Obd::new(&*tp).test_results()?;
    if tests.is_empty() {
        println!("{}", tr!("Keine Prüfergebnisse (Mode 06) verfügbar."));
        return Ok(());
    }
    println!("{}", tr!("{a} {b} {c} {d}  Ergebnis", a = format!("{:<52}", "Test"), b = format!("{:>10}", "Wert"), c = format!("{:>10}", "min"), d = format!("{:>10}", "max")));
    for t in &tests {
        let res = if !t.has_limits() {
            "—".to_string()
        } else {
            format!("{}{}", t.stufe().text(), t.reserve().map(|r| tr!(" (Reserve {r} %)", r = format!("{:.0}", r))).unwrap_or_default())
        };
        println!("{:<52} {:>10.2} {:>10.2} {:>10.2}  {res}  {}", t.name(), t.value, t.min, t.max, t.unit);
    }
    let sum = obd::test_summary(&tests);
    println!();
    if sum.is_empty() {
        println!("{}", tr!("Alle Tests ohne Auffälligkeit."));
    }
    for (st, t) in sum {
        println!("  [{}] {t}", st.text());
    }
    Ok(())
}

fn gemisch(be: &Backend, seconds: u64, fahrzeug: Option<&str>) -> Result<()> {
    use std::io::Write;
    use ecu_werkbank::mixture;
    let demo = matches!(be, Backend::Demo);
    let tp = be.open(ecu_werkbank::obd::DEFAULT_TX, ecu_werkbank::obd::DEFAULT_RX)?;
    let o = ecu_werkbank::obd::Obd::new(&*tp);
    let veh = fahrzeug.and_then(ecu_werkbank::vehicle::find);
    let run_phase = |title: &str, target: Option<f64>| -> Result<Vec<mixture::Sample>> {
        println!("\n{title}");
        if demo {
            let rpm = target.unwrap_or(800.0);
            ecu_werkbank::sim::set_muster(ecu_werkbank::sim::Param::Drehzahl, Some(ecu_werkbank::control::Muster::Halten(rpm)));
        }
        let start = Instant::now();
        let mut v = Vec::new();
        while start.elapsed() < Duration::from_secs(seconds) {
            let s = mixture::sample(&o)?;
            let hint = match target {
                Some(t) if s.rpm < t - 250.0 => "▲ mehr Gas",
                Some(t) if s.rpm > t + 250.0 => "▼ weniger Gas",
                Some(_) => "✓ halten",
                None => "",
            };
            print!("\r  {:4.0} s  {:5.0} 1/min  kurz {:+5.1} %  lang {:+5.1} %  {hint}      ", start.elapsed().as_secs_f64(), s.rpm, s.stft, s.ltft);
            let _ = std::io::stdout().flush();
            if target.is_none_or(|t| (s.rpm - t).abs() < 400.0) {
                v.push(s);
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        println!();
        Ok(v)
    };
    println!("{}", tr!("Gemisch-Assistent — Motor betriebswarm, alle Verbraucher aus, Fahrzeug steht."));
    let idle = run_phase(&tr!("Phase 1: Leerlauf ({seconds} s) — nichts tun.", seconds = seconds), None)?;
    let high = run_phase(&tr!("Phase 2: Drehzahl ca. 2500 1/min halten ({seconds} s).", seconds = seconds), Some(2500.0))?;
    if demo {
        ecu_werkbank::sim::clear_muster();
    }
    if idle.is_empty() || high.is_empty() {
        anyhow::bail!(tr!("Zu wenige Messwerte — Phase 2 bei ca. 2500 1/min wiederholen."));
    }
    let (a, b) = (mixture::summarize(&idle), mixture::summarize(&high));
    println!("{}", tr!("\nErgebnis:"));
    for (st, t) in mixture::analyze(&a, &b, veh.as_ref().and_then(|v| v.hubraum_l)) {
        println!("  [{}] {t}", st.text());
    }
    if let Some(v) = veh {
        let mut tr = vec![("Gemisch Leerlauf".to_string(), a.total(), "%".to_string()), ("Gemisch 2500".to_string(), b.total(), "%".to_string())];
        if let Some(m) = a.maf {
            tr.push(("Luftmasse Leerlauf".into(), m, "g/s".into()));
        }
        ecu_werkbank::trend::add(&v.key, v.km, &tr)?;
        println!("{}", tr!("\n(In den Trends von {a} gespeichert.)", a = v.label()));
    }
    Ok(())
}

fn kaufcheck(be: &Backend, db: &LabelDb, out: Option<&std::path::Path>, note: &str, fahrzeug: Option<&str>) -> Result<()> {
    let active = active_vehicle(fahrzeug)?;
    println!("{}", tr!("Gebrauchtwagen-Check über {a} …\n", a = be.describe()));
    let mut e = ecu_werkbank::usedcar::run(be, db, |_| {})?;
    e.report.note = note.to_string();
    let v = e.report.assign_vehicle(active.as_deref())?;
    for x in &e.punkte {
        println!("  [{:<7}] {:<20} {}", x.stufe.text(), x.bereich, x.text);
    }
    println!("\n→ {}", e.urteil());
    let out = match out {
        Some(p) => p.to_path_buf(),
        None => v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("kaufcheck", "pdf")),
    };
    std::fs::write(&out, e.to_pdf())?;
    let _ = ecu_werkbank::history::save(&e.report);
    println!("{}", tr!("PDF: {a}", a = out.display()));
    Ok(())
}

/// Gebrauchtwagen-Check ohne Auto aus dem letzten gespeicherten Scan mit Abgasdaten.
fn kaufcheck_aus_scan(out: Option<&std::path::Path>, note: &str, fahrzeug: Option<&str>) -> Result<()> {
    // ohne --fahrzeug: das in der GUI gewählte Fahrzeug
    let gewaehlt = || std::fs::read_to_string(ecu_werkbank::history::data_dir().join("aktives-fahrzeug")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let key = active_vehicle(fahrzeug)?.or_else(gewaehlt).context(tr!("Kein Fahrzeug gewählt — mit --fahrzeug angeben"))?;
    let v = ecu_werkbank::vehicle::load(&key).context(tr!("Fahrzeug nicht gefunden"))?;
    let r = ecu_werkbank::usedcar::letzter_scan(&v.key).context(tr!("Für dieses Fahrzeug ist noch kein Scan gespeichert"))?;
    let mut e = ecu_werkbank::usedcar::aus_scan(r.clone());
    if !note.is_empty() {
        e.report.note = note.to_string();
    }
    println!("{}", tr!("Gebrauchtwagen-Check aus dem Scan vom {a} …\n", a = r.created));
    for x in &e.punkte {
        println!("  [{:<7}] {:<20} {}", x.stufe.text(), x.bereich, x.text);
    }
    println!("\n→ {}", e.urteil());
    let out = match out {
        Some(p) => p.to_path_buf(),
        None => v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("kaufcheck", "pdf")),
    };
    std::fs::write(&out, e.to_pdf())?;
    println!("{}", tr!("PDF: {a}", a = out.display()));
    Ok(())
}

fn reparaturbericht(be: &Backend, db: &LabelDb, arbeiten: &str, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::history;
    let active = active_vehicle(fahrzeug)?;
    let mut after = ecu_werkbank::report::scan(be, db, true, |_| {})?;
    let v = after.assign_vehicle(active.as_deref())?;
    let before = history::list(&v.key).pop().map(|(_, r)| r).context(tr!("Kein früherer Scan als „vorher“ — vor der Reparatur `report` oder `check` ausführen"))?;
    after.reparatur = arbeiten.trim().to_string();
    after.comparison = history::compare_with_previous(&after);
    let offen: Vec<String> = be
        .open(ecu_werkbank::obd::DEFAULT_TX, ecu_werkbank::obd::DEFAULT_RX)
        .ok()
        .and_then(|tp| ecu_werkbank::obd::Obd::new(&*tp).readiness().ok())
        .map(|r| r.monitors.iter().filter(|m| m.available && !m.complete).map(|m| m.name.to_string()).collect())
        .unwrap_or_default();
    let out = v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("reparaturbericht", "pdf"));
    std::fs::write(&out, ecu_werkbank::repair::bericht_pdf(&before, &after, arbeiten, &offen))?;
    history::save(&after)?;
    let d = history::diff(&before, &after, &[]);
    println!("{}", tr!("Vorher {a} Fehler ({b}), nachher {c} Fehler.", a = before.total_dtcs(), b = before.created, c = after.total_dtcs()));
    println!("{}", tr!("Behoben: {a}, noch da: {b}, neu: {c}", a = d.gone.len(), b = d.still.len(), c = d.new.len()));
    if !offen.is_empty() {
        println!("{}", tr!("Readiness offen: {a}", a = offen.join(", ")));
    }
    println!("{}", tr!("PDF: {a}", a = out.display()));
    Ok(())
}

fn software(be: &Backend, db: &LabelDb, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::swinfo;
    let st = swinfo::read(be, db, true, |_| {})?;
    println!("{:<10} {:<22} {:<14} {:<8} {:<14} {:<6} Seriennr.", "Adresse", "System", "Teilenummer", "SW", "HW-Nr.", "HW");
    for m in &st.module {
        let sys = if m.system.is_empty() { &m.name } else { &m.system };
        println!("{:<10} {:<22} {:<14} {:<8} {:<14} {:<6} {}", m.adresse, sys, m.teilenummer, m.software, m.hardware_nr, m.hardware_ver, m.seriennummer);
    }
    let active = active_vehicle(fahrzeug)?;
    let v = ecu_werkbank::vehicle::resolve(st.vin.as_deref(), active.as_deref())?;
    if let Some(prev) = swinfo::list(&v.key).pop() {
        let d = swinfo::compare(&prev, &st);
        println!("{}", tr!("\nSeit {a}: {b}", a = prev.created, b = if d.is_empty() { tr!("keine Änderung") } else { tr!("{n} Änderung(en)", n = d.len()) }));
        for (m, t) in d {
            println!("  {m}: {t}");
        }
    }
    let p = swinfo::save(&v.key, &st)?;
    println!("{}", tr!("\nGespeichert: {a}", a = p.display()));
    Ok(())
}

fn nachruestung(be: &Backend, db: &LabelDb, name: &str, phase: &str, notiz: &str, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::retrofit;
    let v = need_vehicle(fahrzeug)?;
    let dir = v.sub("nachruestung")?.join(ecu_werkbank::vehicle::plate_key(name).to_lowercase());
    std::fs::create_dir_all(&dir)?;
    match phase {
        "vorher" => {
            let st = retrofit::aufnehmen(be, db, |_| {})?;
            std::fs::write(dir.join("vorher-scan.toml"), toml::to_string(&st.scan)?)?;
            std::fs::write(dir.join("vorher-codierung.toml"), toml::to_string(&st.codierung)?)?;
            println!("{}", tr!("Vorher-Stand gesichert ({a} Steuergeräte, {b} Fehler). Jetzt einbauen, dann --phase nachher.", a = st.scan.modules.len(), b = st.scan.total_dtcs()));
        }
        "nachher" => {
            let vorher = retrofit::Stand {
                scan: ecu_werkbank::history::load(&dir.join("vorher-scan.toml")).context(tr!("Erst --phase vorher ausführen"))?,
                codierung: ecu_werkbank::backup::load(&dir.join("vorher-codierung.toml"))?,
            };
            let mut nachher = retrofit::aufnehmen(be, db, |_| {})?;
            nachher.scan.assign_vehicle(Some(&v.key))?;
            nachher.scan.reparatur = tr!("Nachrüstung {name}{a}", name = name, a = if notiz.is_empty() { String::new() } else { format!(": {notiz}") });
            let r = retrofit::vergleich(&vorher, &nachher, db);
            for (st, t) in &r {
                println!("  [{}] {t}", st.text());
            }
            let out = v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("nachruestung", "pdf"));
            std::fs::write(&out, retrofit::pdf(name, &vorher, &nachher, &r, notiz))?;
            ecu_werkbank::history::save(&nachher.scan)?;
            nachher.codierung.vehicle = v.key.clone();
            ecu_werkbank::backup::save(&nachher.codierung)?;
            println!("{}", tr!("PDF: {a}", a = out.display()));
        }
        x => anyhow::bail!(tr!("--phase {x}: erlaubt sind vorher | nachher", x = x)),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn codier_bit(be: &Backend, tx: u16, rx: u16, did: u16, byte: usize, bit: u8, wert: &str, ja: bool, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::backup;
    let on = match wert.to_lowercase().as_str() {
        "ein" | "1" | "an" => true,
        "aus" | "0" => false,
        x => anyhow::bail!(tr!("--wert {x}: ein oder aus", x = x)),
    };
    let tp = be.open(tx, rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(ecu_werkbank::uds::SESSION_DEFAULT)?;
    let now = uds.read_data_by_id(did)?;
    drop(tp);
    let mut new = now.clone();
    if !ecu_werkbank::set_coding_bit(&mut new, byte, bit, on) {
        anyhow::bail!(tr!("Byte {byte} gibt es nicht (Codierung hat {a} Byte)", byte = byte, a = now.len()));
    }
    if let Some(n) = ecu_werkbank::codiertexte::Texte::laden().eigen(tx, byte, Some(bit)) {
        println!("{}", tr!("Bit: {a}", a = n));
    }
    println!("{}", tr!("Codierung {did} an {tx}:\n  jetzt: {a}\n  neu:   {b}", did = format!("{:#06x}", did), tx = format!("{:#05x}", tx), a = backup::hex(&now), b = backup::hex(&new)));
    if now == new {
        println!("{}", tr!("Bit ist bereits so gesetzt."));
        return Ok(());
    }
    if !ja {
        println!("{}", tr!("Nichts geschrieben. Mit --ja ausführen (vorher wird automatisch gesichert)."));
        return Ok(());
    }
    // Sicherung des Ist-Stands
    let active = active_vehicle(fahrzeug)?;
    let mk = |data: &[u8]| backup::CodingEntry { protocol: "UDS".into(), address: None, name: String::new(), tx, rx, did: Some(did), part: String::new(), data_hex: backup::hex(data), note: String::new() };
    let b = backup::CodingBackup {
        created: ecu_werkbank::report::now_local(),
        timestamp: ecu_werkbank::report::unix_now(),
        vin: None,
        transport: be.describe(),
        vehicle: ecu_werkbank::vehicle::resolve(None, active.as_deref())?.key,
        entries: vec![mk(&now)],
    };
    let p = backup::save(&b)?;
    println!("{}", tr!("Sicherung: {a}", a = p.display()));
    let login = ecu_werkbank::login::fuer(&b.vehicle, tx);
    backup::restore(be, &mk(&new), login.as_ref())?;
    println!("{}", tr!("Geschrieben und kontrolliert ✓  (Rückgängig: coding-restore --file {a} --tx {tx} --ja)", a = p.display(), tx = format!("{:#05x}", tx)));
    Ok(())
}


fn ersatzteilpass_neu(be: &Backend, fahrzeug: Option<&str>) -> Result<()> {
    let v = fahrzeug_fuer(be, fahrzeug)?;
    let pdf = ecu_werkbank::notfall::pass_neu(&v)?;
    println!("{}", tr!("Ersatzteil-Pass neu erzeugt: {a}", a = pdf.display()));
    Ok(())
}

fn notfall_sicherung(be: &Backend, db: &LabelDb, fahrzeug: Option<&str>) -> Result<()> {
    let v = fahrzeug_fuer(be, fahrzeug)?;
    println!("{}", tr!("Komplett-Sicherung für {a} über {b} — nur lesen, dauert etwa 10–20 Minuten. Zündung an lassen, Ladegerät an die Batterie.", a = v.label(), b = be.describe()));
    let mut zuletzt = String::new();
    let e = ecu_werkbank::notfall::sichern(be, db, &v, |t| {
        if t != zuletzt {
            println!("  {t}");
            zuletzt = t.to_string();
        }
    })?;
    let n: usize = e.anpassung.geraete.iter().map(|g| g.kennungen.len()).sum();
    println!("{}", tr!("Fertig: {a} Steuergeräte, {b} Codierungen, {c} Anpassungswerte.", a = e.software.module.len(), b = e.codierung.entries.len(), c = n));
    for d in &e.dateien {
        println!("  {}", d.display());
    }
    println!("{}", tr!("Ersatzteil-Pass: {a}", a = e.pdf.display()));
    Ok(())
}

/// Verbauliste aus dem Gateway anzeigen (nur lesen).
fn gateway_verbauliste(be: &Backend) -> Result<()> {
    println!("{}", tr!("Verbauliste aus dem Gateway über {a} (nur lesen) …", a = be.describe()));
    let l = kwp::gateway_verbauliste(be)?;
    let mut verbaut: Vec<_> = l.iter().filter(|e| e.verbaut()).collect();
    verbaut.sort_by_key(|e| e.adresse);
    for e in &verbaut {
        let name = ecu_werkbank::systems::name(u16::from(e.adresse));
        let status = if e.fehler() { tr!("Fehler gespeichert") } else { tr!("i.O.") };
        println!("  {a:02X}  {b:<28} {c}", a = e.adresse, b = name, c = status);
    }
    let fehler = verbaut.iter().filter(|e| e.fehler()).count();
    println!("{}", tr!("\n{a} verbaut, davon {b} mit Fehler; {c} weitere Adressen kennt das Gateway als nicht verbaut.", a = verbaut.len(), b = fehler, c = l.len() - verbaut.len()));
    Ok(())
}

/// Anpassungskanäle eines TP-2.0-Steuergeräts anzeigen (nur lesen).
fn kwp_anpassung(be: &Backend, address: u8, kanaele: &[u8]) -> Result<()> {
    println!("{}", tr!("Anpassung Steuergerät {a} (nur lesen) …", a = format!("{address:02X}")));
    for r in kwp::anpassung_lesen(be, address, kanaele)? {
        match r {
            Ok(k) => {
                let felder: Vec<String> = k.felder.iter().filter(|v| !v.is_leer()).map(|v| v.display()).collect();
                println!("{}", tr!("  Kanal {a}: Wert {b}   [{c}]", a = format!("{:03}", k.kanal), b = k.wert, c = felder.join("  |  ")));
            }
            Err(e) => println!("  {e}"),
        }
    }
    Ok(())
}

/// Stellglieddiagnose im Terminal: Enter = nächstes Bauteil, q = Ende.
fn kwp_stellglied(be: &Backend, address: u8, ja: bool) -> Result<()> {
    println!("{}", tr!("Stellglieddiagnose Steuergerät {a}: Bauteile werden dabei WIRKLICH angesteuert (Fensterheber, Verriegelung, Lampen …).", a = format!("{address:02X}")));
    if !ja {
        println!("{}", tr!("Nichts gestartet. Fahrzeug sichern (Hände weg von Fenstern/Türen), dann mit --ja starten."));
        return Ok(());
    }
    println!("{}", tr!("Enter = nächstes Bauteil, q + Enter = beenden."));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut zeile = String::new();
        while std::io::stdin().read_line(&mut zeile).is_ok() {
            let ende = zeile.trim().eq_ignore_ascii_case("q") || zeile.is_empty();
            if tx.send(if ende { kwp::StellBefehl::Ende } else { kwp::StellBefehl::Weiter }).is_err() || ende {
                break;
            }
            zeile.clear();
        }
    });
    kwp::stellglieddiagnose(be, address, |s| {
        if let Some(s) = s {
            println!("  {}", s.text());
        }
        rx.try_recv().unwrap_or(kwp::StellBefehl::Bleiben)
    })?;
    println!("{}", tr!("Stellglieddiagnose beendet."));
    Ok(())
}

/// Kurzcodierung schreiben (TP 2.0) — erst Vorschau, mit --ja wirklich.
fn kwp_codierung_schreiben(be: &Backend, address: u8, wert: u32, ja: bool) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    let raw = Kwp::new(&mut ch).read_ecu_id(kwp::ECU_ID_VAG)?;
    drop(ch);
    let alt = match kwp::kurzcodierung_9b(&raw) {
        Some(kwp::TpCodierung::Kurz { wert, .. }) => wert,
        _ => anyhow::bail!(tr!("Steuergerät {a} hat keine Kurzcodierung (Aufbau 03 xx xx xx).", a = format!("{address:02X}"))),
    };
    println!("{}", tr!("Steuergerät {a}: Kurzcodierung jetzt {b}, neu {c}.", a = format!("{address:02X}"), b = format!("{alt:07}"), c = format!("{wert:07}")));
    if !ja {
        println!("{}", tr!("Nichts geschrieben. Den alten Wert notieren, dann mit --ja schreiben."));
        return Ok(());
    }
    let (vorher, danach) = kwp::kurzcodierung_schreiben(be, address, wert)?;
    if danach == wert {
        println!("{}", tr!("Geschrieben und kontrolliert: {a} → {b}.", a = format!("{vorher:07}"), b = format!("{danach:07}")));
    } else {
        println!("{}", tr!("Achtung: danach gelesen {a}, erwartet {b}. Alter Wert war {c}.", a = format!("{danach:07}"), b = format!("{wert:07}"), c = format!("{vorher:07}")));
    }
    Ok(())
}

/// Ergebnis eines TP-2.0-Schreibversuchs ausgeben.
fn schreib_ergebnis(r: &kwp::Schreiben, zeige: impl Fn(&[u8]) -> String) {
    match r {
        kwp::Schreiben::Probelauf { anfrage, alt } => {
            println!("{}", tr!("PROBELAUF — nichts gesendet. Dieser Schreibweg ist am Auto noch nicht mit einem Mitschnitt bestätigt."));
            println!("{}", tr!("Jetzt: {a}", a = zeige(alt)));
            println!("{}", tr!("Würde senden: {a}", a = ecu_werkbank::backup::hex(anfrage)));
        }
        kwp::Schreiben::Geschrieben { alt, danach } => {
            println!("{}", tr!("Geschrieben. Vorher {a}, danach gelesen {b}.", a = zeige(alt), b = zeige(danach)));
        }
    }
}

fn kwp_langcodierung(be: &Backend, address: u8, neu: Option<&str>, ja: bool) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    let (art, alt) = kwp::langcodierung_lesen(&mut Kwp::new(&mut ch)).ok_or_else(|| anyhow::anyhow!(tr!("Steuergerät {a} hat keine lange Codierung.", a = format!("{address:02X}"))))?;
    drop(ch);
    let wo = match art {
        kwp::LangArt::Kennung0600 => "0x0600",
        kwp::LangArt::Gateway9A(_) => "0x9A",
    };
    println!("{}", tr!("Steuergerät {a}: lange Codierung ({b} Byte, {c})", a = format!("{address:02X}"), b = alt.len(), c = wo));
    println!("  {}", ecu_werkbank::backup::hex(&alt));
    let Some(neu) = neu else { return Ok(()) };
    let neu: Vec<u8> = parse_hex_bytes(neu)?;
    let anfrage = kwp::langcodierung_anfrage(&art, &alt, &neu)?;
    for (i, (a, b)) in alt.iter().zip(&neu).enumerate() {
        for bit in 0..8 {
            if (a ^ b) >> bit & 1 == 1 {
                println!("  {}", tr!("Byte {i} Bit {bit}: {a} → {b}", i = i, bit = bit, a = a >> bit & 1, b = b >> bit & 1));
            }
        }
    }
    if !ja {
        println!("{}", tr!("Nichts geschrieben (Anfrage wäre {a}). Den alten Stand notieren, dann mit --ja schreiben.", a = ecu_werkbank::backup::hex(&anfrage)));
        return Ok(());
    }
    let r = kwp::langcodierung_schreiben(be, address, &neu)?;
    schreib_ergebnis(&r, ecu_werkbank::backup::hex);
    if let kwp::Schreiben::Geschrieben { danach, .. } = &r {
        if *danach != neu {
            println!("{}", tr!("Achtung: danach gelesen ist nicht der neue Stand."));
        }
    }
    Ok(())
}

fn kwp_anpassung_schreiben(be: &Backend, address: u8, kanal: u8, wert: u16, ja: bool) -> Result<()> {
    let wert_text = |b: &[u8]| if b.len() == 2 { u16::from_be_bytes([b[0], b[1]]).to_string() } else { ecu_werkbank::backup::hex(b) };
    if !ja {
        let alt = match kwp::anpassung_lesen(be, address, &[kanal])?.pop() {
            Some(Ok(a)) => a.wert,
            Some(Err(e)) => anyhow::bail!(e),
            None => anyhow::bail!(tr!("Kanal {a}: keine Antwort", a = kanal)),
        };
        println!("{}", tr!("Steuergerät {a}, Kanal {b}: jetzt {c}, neu {d}.", a = format!("{address:02X}"), b = format!("{kanal:03}"), c = alt, d = wert));
        println!("{}", tr!("Nichts geschrieben. Den alten Wert notieren, dann mit --ja schreiben."));
        return Ok(());
    }
    let r = kwp::anpassung_schreiben(be, address, kanal, wert)?;
    schreib_ergebnis(&r, wert_text);
    if let kwp::Schreiben::Geschrieben { danach, .. } = &r {
        if *danach != wert.to_be_bytes() {
            println!("{}", tr!("Achtung: danach gelesen ist nicht der neue Wert."));
        }
    }
    Ok(())
}


// ---------------------------------------------------------------------------
// Fahrzeug-Datenbank
// ---------------------------------------------------------------------------

fn db_cmd(aktion: DbAktion, datei: Option<PathBuf>, labels_dir: &std::path::Path) -> Result<()> {
    use ecu_werkbank::db::{darf_weitergeben, standard_pfad, Db};
    let pfad = datei.unwrap_or_else(standard_pfad);
    let db = Db::oeffnen(&pfad)?;
    match aktion {
        DbAktion::Status => {
            println!("{}", tr!("Datenbank: {a}", a = pfad.display()));
            println!("{}", tr!("Schema-Version: {a}", a = db.schema_version()?));
            for (t, n) in db.statistik()? {
                println!("  {t:<20} {n:>9}");
            }
            if db.quellen()?.is_empty() {
                println!("{}", tr!("\nNoch leer. Start:  ecu-werkbank db eingebaut   (dann z. B. db labels, db dbc --datei …)"));
            }
        }
        DbAktion::Quellen => {
            let q = db.quellen()?;
            if q.is_empty() {
                println!("{}", tr!("Keine Quellen."));
            }
            for q in q {
                let teilen = if darf_weitergeben(q.art, &q.lizenz) { "" } else { " [bleibt lokal]" };
                println!("{:>3}  {:<9} {}  {}  Lizenz: {}  {} Einträge  ({}){teilen}", q.id, q.art.text(), q.name, q.version, if q.lizenz.is_empty() { "–" } else { &q.lizenz }, q.zeilen, q.importiert);
                if !q.herkunft.is_empty() {
                    println!("       {}", q.herkunft);
                }
            }
        }
        DbAktion::Eingebaut => {
            db.normdaten_eintragen()?;
            println!("{}", tr!("Normdaten (UDS-Dienste, Sitzungen) eingetragen."));
            for (name, e) in db.community_uebernehmen()? {
                println!("{}", tr!("Community „{a}“: {b}", a = name, b = e.text()));
            }
        }
        DbAktion::Paket { datei, community } => {
            use ecu_werkbank::db::{paket::Datenpaket, QuellArt};
            let p = Datenpaket::lesen(&datei)?;
            if community && p.quelle.lizenz.trim().is_empty() {
                anyhow::bail!(tr!("Community-Daten brauchen eine Lizenz ([quelle] lizenz = …)."));
            }
            let e = db.paket_importieren(&p, if community { QuellArt::Community } else { QuellArt::User })?;
            println!("{}", tr!("„{a}“ übernommen: {b} (Quelle {c}).", a = p.quelle.name, b = e.text(), c = e.quelle_id));
        }
        DbAktion::Csv { datei, lizenz } => {
            let e = db.csv_importieren(&datei, &lizenz)?;
            println!("{}", tr!("Übernommen: {a} (Quelle {b}).", a = e.text(), b = e.quelle_id));
        }
        DbAktion::Export { ziel, lizenz, name } => {
            let (p, weg) = db.paket_exportieren(&name, &lizenz)?;
            p.schreiben(&ziel)?;
            let n: usize = p.hersteller.iter().map(|h| h.steuergeraete.len()).sum();
            println!("{}", tr!("Geschrieben: {a} ({n} Steuergeräte, {f} allgemeine Fehlertexte).", a = ziel.display(), n = n, f = p.fehler.len()));
            for w in weg {
                println!("{}", tr!("  nicht enthalten (bleibt lokal): {a}", a = w));
            }
        }
        DbAktion::Labels { ordner } => {
            let ordner = ordner.unwrap_or_else(|| labels_dir.to_path_buf());
            let vw = db.hersteller("Volkswagen", &["WVW", "WV1", "WV2", "WV3"], None)?;
            let e = db.labels_importieren(&ordner, vw)?;
            println!("{}", tr!("Aus {a} übernommen: {b} (Quelle {c}).", a = ordner.display(), b = e.text(), c = e.quelle_id));
        }
        DbAktion::Dbc { datei, lizenz, hersteller } => {
            let text = std::fs::read(&datei).with_context(|| tr!("DBC lesen: {a}", a = datei.display()))?;
            let dbc = ecu_werkbank::dbc::Dbc::lesen(&String::from_utf8_lossy(&text));
            if dbc.botschaften.is_empty() {
                anyhow::bail!(tr!("Keine Botschaften in {a} gefunden.", a = datei.display()));
            }
            let h = hersteller.map(|h| db.hersteller(&h, &[], None)).transpose()?;
            let name = datei.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let e = db.dbc_importieren(&dbc, &name, &lizenz, &datei.display().to_string(), h)?;
            println!("{}", tr!("Übernommen: {a} (Quelle {b}).", a = e.text(), b = e.quelle_id));
            if lizenz.is_empty() {
                println!("{}", tr!("Ohne Lizenzangabe bleiben die Daten nur in deiner Datenbank."));
            }
        }
        DbAktion::Entfernen { id, ja } => {
            let Some(q) = db.quellen()?.into_iter().find(|q| q.id == id) else {
                anyhow::bail!(tr!("Keine Quelle mit der Nummer {a}.", a = id));
            };
            if !ja {
                println!("{}", tr!("Würde entfernen: {a} „{b}“ mit {c} Einträgen. Wirklich: --ja", a = q.art.text(), b = q.name, c = q.zeilen));
                return Ok(());
            }
            db.quelle_entfernen(id)?;
            println!("{}", tr!("Entfernt: {a} „{b}“.", a = q.art.text(), b = q.name));
        }
        DbAktion::Sichern { ziel } => {
            db.sichern(&ziel)?;
            println!("{}", tr!("Kopie gespeichert: {a}", a = ziel.display()));
        }
    }
    Ok(())
}
