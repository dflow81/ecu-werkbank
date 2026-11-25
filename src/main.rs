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
    /// Aufräumen: Anpassungskanäle ohne echten Namen („Kanal 4“) löschen
    Aufraeumen,
    /// Was die Datenbank zu einer Teilenummer weiß (Anpassung, Codierung, Messwerte, Fehler)
    Teil {
        /// z. B. 5K0937086 oder "1K0 907 379 CD"
        teilenummer: String,
    },
    /// Katalog zu einem Fehlercode abfragen, optional mit UDS-Fehlerart
    Fehler {
        code: String,
        #[arg(long)]
        art: Option<String>,
    },
    /// Normdaten und die mitgelieferten Community-Daten übernehmen/aktualisieren
    Eingebaut,
    /// Beliebige Datei übernehmen — die Art wird erkannt (DBC, Datenpaket, Label, CSV, ODX/PDX)
    Import {
        #[arg(long)]
        datei: Vec<PathBuf>,
        #[arg(long, default_value = "Volkswagen")]
        hersteller: String,
        #[arg(long, default_value = "")]
        lizenz: String,
    },
    /// Datenpaket (TOML oder JSON) übernehmen; gleiche Quelle wird ersetzt
    Paket {
        #[arg(long)]
        datei: PathBuf,
        /// als Community-Daten (braucht eine Lizenz im Paket); Standard: eigene Daten
        #[arg(long)]
        community: bool,
    },
    /// ODX- oder PDX-Datei übernehmen (nur rechtmäßig erworbene Daten) — bleibt immer lokal
    Odx {
        #[arg(long)]
        datei: PathBuf,
        /// Hersteller, zu dem die Steuergeräte gehören
        #[arg(long)]
        hersteller: String,
    },
    /// CSV übernehmen: Fehlertexte (code;text), DIDs (did;name;einheit;faktor;offset), Fehlerarten — auch gemischt
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
    /// Service-Kennungen des Fahrzeugs in die Fahrzeug-Datenbank übernehmen (für alle Kombis mit derselben Teilenummer)
    InDatenbank,
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
    /// Mitschnitt abspielen — nur auf ein virtuelles CAN-Interface (vcan), nie aufs Auto
    Replay {
        #[arg(long)]
        log: PathBuf,
        /// virtuelles Interface, z. B. vcan0 (echte Adapter werden abgelehnt)
        #[arg(long, default_value = "vcan0")]
        iface: String,
        /// 2.0 = doppelt so schnell
        #[arg(long, default_value_t = 1.0)]
        tempo: f64,
        /// immer wieder von vorn (Strg-C beendet)
        #[arg(long)]
        schleife: bool,
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
    /// Steuergeräte-Speicher auslesen (UDS ReadMemoryByAddress, nur lesen) und als .bin sichern
    SpeicherLesen {
        /// Anfrage-ID, z. B. 0x7E0
        #[arg(long, value_parser = parse_hex)]
        geraet: u16,
        /// Antwort-ID (Standard: passend zur Anfrage-ID)
        #[arg(long, value_parser = parse_hex)]
        rx: Option<u16>,
        /// Startadresse (hex), z. B. 0x0
        #[arg(long, value_parser = parse_hex32)]
        von: u32,
        /// Anzahl Bytes (hex oder dezimal), z. B. 0x1000
        #[arg(long, value_parser = parse_hex32)]
        laenge: u32,
        /// Bytes je Anfrage
        #[arg(long, default_value_t = 128)]
        block: u16,
        /// Diagnosesitzung vorher, z. B. 0x03 (erweitert); ohne = Standardsitzung
        #[arg(long, value_parser = parse_hex)]
        sitzung: Option<u16>,
        /// Ausgabe (.bin); Standard: Fahrzeugordner/speicher
        #[arg(long)]
        out: Option<PathBuf>,
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
    /// Eigene Texte für Messwerte mit Textnummer (F25) aus einem Block-Export des eigenen
    /// Werkstattgeräts lernen (bleiben nur auf diesem Rechner)
    WertetexteLernen {
        /// CSV-Export der Messwertblöcke (Werkstattgerät, gleiches Steuergerät)
        #[arg(long)]
        blockmap: PathBuf,
        /// eigene Ausgabe von kwp-blockscan (Textdatei) …
        #[arg(long)]
        scan: Option<PathBuf>,
        /// … oder jetzt am Auto lesen: TP-2.0-Adresse (hex)
        #[arg(long, value_parser = parse_tp20)]
        address: Option<u16>,
        /// nur zeigen, nicht speichern
        #[arg(long)]
        nur_zeigen: bool,
    },
    /// Eigene Messwert-Texte zeigen oder setzen, z. B. --code F25:02FD --text normal
    Wertetext {
        #[arg(long)]
        code: Option<String>,
        /// leer = löschen
        #[arg(long)]
        text: Option<String>,
    },
    /// Skript (Rhai) ausführen — nur lesende Funktionen am Auto (siehe beispiele/*.rhai)
    Skript {
        datei: PathBuf,
    },
    /// REST-Schnittstelle auf die Fahrzeug-Datenbank: nur lesen, nur dieser Rechner, mit Schlüssel (Strg-C beendet)
    Api {
        #[arg(long, default_value_t = 8787)]
        port: u16,
        /// Zusätzlich Live-Werte vom Auto unter /api/v1/live/obd?pids=0C,0D (OBD-II, nur lesen)
        #[arg(long)]
        live: bool,
        /// andere Datenbank-Datei (Standard: im Datenordner)
        #[arg(long)]
        datei: Option<PathBuf>,
    },
    /// Ganze Konfiguration vergleichen (Codierung, Anpassungen, Software): vorher/nachher oder zwei Fahrzeuge
    KonfigVergleich {
        /// Fahrzeug (Kennzeichen, VIN oder Ordner); Standard: aktives Fahrzeug
        #[arg(long)]
        a: Option<String>,
        /// zweites Fahrzeug; ohne: neueste gegen vorherige Sicherungen von A
        #[arg(long)]
        b: Option<String>,
    },
    /// Firmenkopf für Berichte (oben auf jeder PDF-Seite, mit Logo): anzeigen, setzen oder löschen
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
        /// Firmenlogo (PNG oder JPG); leer ("") entfernt das Logo
        #[arg(long)]
        logo: Option<PathBuf>,
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

fn parse_hex32(s: &str) -> Result<u32, String> {
    let t = s.trim();
    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16).map_err(|e| e.to_string()),
        None => t.parse::<u32>().map_err(|e| e.to_string()),
    }
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
        Command::Replay { log, iface, tempo, schleife } => {
            let text = ecu_werkbank::ablage::lesen(&log).with_context(|| tr!("Log lesen: {a}", a = log.display()))?;
            let frames: Vec<_> = text.lines().filter_map(ecu_werkbank::canlog::parse_line).collect();
            if frames.is_empty() {
                anyhow::bail!(tr!("Keine Frames in {a}.", a = log.display()));
            }
            ecu_werkbank::replay::nur_vcan(&iface)?;
            loop {
                println!("{}", tr!("Spiele {n} Frames auf {i} ab (Tempo {t})…", n = frames.len(), i = iface, t = tempo));
                let n = ecu_werkbank::replay::auf_vcan(&iface, &frames, tempo, || true)?;
                println!("{}", tr!("{n} Frames gesendet.", n = n));
                if !schleife {
                    break;
                }
            }
            Ok(())
        }
        Command::LogExport { log, out } => {
            let format = ecu_werkbank::spurexport::Format::aus_pfad(&out).context(tr!("Zieldatei braucht die Endung .pcap oder .asc"))?;
            let text = ecu_werkbank::ablage::lesen(&log).with_context(|| tr!("Log lesen: {a}", a = log.display()))?;
            let n = ecu_werkbank::spurexport::frames(&text).len();
            ecu_werkbank::ablage::schreiben(&out, ecu_werkbank::spurexport::umwandeln(&text, format))?;
            println!("{}", tr!("{n} Frames geschrieben: {a}", n = n, a = out.display()));
            Ok(())
        }
        Command::DbcDecode { dbc, log, nur } => {
            let d = ecu_werkbank::dbc::Dbc::lesen(&ecu_werkbank::ablage::lesen(&dbc).with_context(|| tr!("DBC lesen: {a}", a = dbc.display()))?);
            let text = ecu_werkbank::ablage::lesen(&log).with_context(|| tr!("Log lesen: {a}", a = log.display()))?;
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
        Command::SpeicherLesen { geraet, rx, von, laenge, block, sitzung, out } => {
            use ecu_werkbank::speicher;
            let a = speicher::Auftrag { tx: geraet, rx: rx.unwrap_or_else(|| ecu_werkbank::uds::antwort_id(geraet)), von, laenge, block, sitzung: sitzung.map(|s| s as u8) };
            println!("{}", tr!("Speicher lesen {a} ab {b}, {c} Byte (nur lesen) …", a = format!("{geraet:#05x}"), b = format!("{von:#010x}"), c = laenge));
            let z = speicher::lesen(&be, &a, |n, g| eprint!("\r  {n}/{g} Byte"))?;
            eprintln!();
            for (adr, n, g) in &z.luecken {
                println!("{}", tr!("  nicht lesbar: {a}–{b}: {c}", a = format!("{adr:#010x}"), b = format!("{:#010x}", adr + n - 1), c = g));
            }
            let out = match out {
                Some(p) => p,
                None => {
                    let name = ecu_werkbank::vehicle::stamped(&format!("speicher-{geraet:03x}-{von:08x}"), "bin");
                    match active_vehicle(cli.fahrzeug.as_deref())?.and_then(|k| ecu_werkbank::vehicle::load(&k)) {
                        Some(v) => v.dir().join("speicher").join(name),
                        None => PathBuf::from(name),
                    }
                }
            };
            let meta = speicher::speichern(&z, &out)?;
            println!("{}", z.zusammenfassung());
            println!("{}", tr!("Gespeichert: {a} (Beschreibung: {b})", a = out.display(), b = meta.display()));
            Ok(())
        }
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
                ecu_werkbank::ablage::schreiben(&out, ecu_werkbank::canlog::demo_log_aktion())?;
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
        Command::KwpLangcodierung { address, neu, ja } => kwp_langcodierung(&be, address as u8, neu.as_deref(), ja, &db),
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
        Command::WertetexteLernen { blockmap, scan, address, nur_zeigen } => {
            use ecu_werkbank::wertetexte;
            let csv = std::fs::read(&blockmap).with_context(|| tr!("{a} lesen", a = blockmap.display()))?;
            let anzeige = wertetexte::anzeige_aus_csv(&csv);
            let roh = match (scan, address) {
                (Some(p), _) => wertetexte::roh_aus_blockscan(&ecu_werkbank::ablage::lesen(&p).with_context(|| tr!("{a} lesen", a = p.display()))?),
                (None, Some(a)) => {
                    println!("{}", tr!("Lese alle Messwertblöcke von {a} (nur lesen) …", a = format!("{a:02X}")));
                    ecu_werkbank::kwp::blockscan(&be, a as u8, 1, 255, |_| {})?
                        .iter()
                        .flat_map(|(b, v)| v.iter().enumerate().map(move |(i, w)| (*b, i as u8 + 1, w.formula, w.a, w.b)).collect::<Vec<_>>())
                        .collect()
                }
                (None, None) => anyhow::bail!(tr!("--scan <datei> oder --address <adresse> angeben")),
            };
            let g = wertetexte::lernen(&roh, &anzeige);
            for (k, t) in &g.eindeutig {
                println!("  {k}  →  {t}");
            }
            for (k, t) in &g.widerspruch {
                println!("{}", tr!("  {k}  nicht eindeutig: {a} (nicht übernommen)", k = k, a = t.iter().cloned().collect::<Vec<_>>().join(" / ")));
            }
            if nur_zeigen {
                println!("{}", tr!("{a} Texte gefunden, nichts gespeichert (--nur-zeigen).", a = g.eindeutig.len()));
            } else {
                let liste: Vec<(String, String)> = g.eindeutig.into_iter().collect();
                wertetexte::mehrere_setzen(&liste)?;
                println!("{}", tr!("{a} Texte gespeichert in {b} (nur auf diesem Rechner).", a = liste.len(), b = wertetexte::pfad().display()));
            }
            Ok(())
        }
        Command::Wertetext { code, text } => {
            use ecu_werkbank::wertetexte;
            match (code, text) {
                (Some(c), Some(t)) => {
                    let (f, a, b) = wertetexte::schluessel_lesen(&c).context(tr!("Code wie F25:02FD angeben"))?;
                    let k = wertetexte::schluessel(f, a, b);
                    wertetexte::mehrere_setzen(&[(k.clone(), t.clone())])?;
                    println!("{}", if t.trim().is_empty() { tr!("{k} gelöscht.", k = k) } else { tr!("{k} → {t}", k = k, t = t.trim()) });
                }
                (Some(c), None) => {
                    let (f, a, b) = wertetexte::schluessel_lesen(&c).context(tr!("Code wie F25:02FD angeben"))?;
                    println!("{}  {}", wertetexte::schluessel(f, a, b), wertetexte::text(f, a, b).unwrap_or_else(|| tr!("(kein Text)")));
                }
                (None, _) => {
                    let alle = wertetexte::alle();
                    if alle.is_empty() {
                        println!("{}", tr!("Noch keine eigenen Messwert-Texte. Lernen: ecu-werkbank wertetexte-lernen --blockmap … --scan …"));
                    }
                    for (k, t) in alle {
                        println!("  {k}  {t}");
                    }
                }
            }
            Ok(())
        }
        Command::Skript { datei } => {
            let code = ecu_werkbank::ablage::lesen(&datei).with_context(|| tr!("{a} lesen", a = datei.display()))?;
            let stopp = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            ecu_werkbank::skript::ausfuehren(be.clone(), std::sync::Arc::new(db), &code, |s| println!("{s}"), stopp)
        }
        Command::Api { port, datei, live } => {
            use ecu_werkbank::db;
            let pfad = datei.unwrap_or_else(db::standard_pfad);
            if !pfad.exists() {
                anyhow::bail!(tr!("Noch keine Datenbank ({a}). Erst: ecu-werkbank db eingebaut", a = pfad.display()));
            }
            let schluessel = db::api::schluessel();
            db::api::dienen(&pfad, port, &schluessel, live.then_some(&be), |url| {
                println!("{}", tr!("Schnittstelle läuft (nur lesen, nur dieser Rechner):"));
                println!("  {url}status");
                if live {
                    println!("  {url}live/obd?pids=0C,0D,05,42");
                }
                println!("{}", tr!("Die Adresse enthält den Schlüssel — nicht weitergeben. Strg-C beendet."));
            })
        }
        Command::KonfigVergleich { a, b } => {
            use ecu_werkbank::konfigvergleich;
            let a = match active_vehicle(a.as_deref().or(cli.fahrzeug.as_deref()))? {
                Some(k) => k,
                None => ecu_werkbank::vehicle::list().into_iter().next().map(|v| v.key).context(tr!("Kein Fahrzeug gespeichert."))?,
            };
            let (x, y) = match b {
                Some(b) => {
                    let b = active_vehicle(Some(&b))?.context(tr!("Fahrzeug B nicht gefunden."))?;
                    println!("{}", tr!("Vergleich {a}  →  {b}\n", a = a, b = b));
                    (konfigvergleich::laden(&a, 0), konfigvergleich::laden(&b, 0))
                }
                None => {
                    println!("{}", tr!("{a}: vorherige → neueste Sicherungen\n", a = a));
                    (konfigvergleich::laden(&a, 1), konfigvergleich::laden(&a, 0))
                }
            };
            print!("{}", konfigvergleich::text(&x, &y, &db));
            Ok(())
        }
        Command::Firmenkopf { name, adresse, telefon, email, zusatz, logo, loeschen } => {
            use ecu_werkbank::firmenkopf;
            let mut f = if loeschen { firmenkopf::Firmenkopf::default() } else { firmenkopf::laden() };
            let geaendert = loeschen || name.is_some() || adresse.is_some() || telefon.is_some() || email.is_some() || zusatz.is_some() || logo.is_some();
            match logo {
                Some(p) if p.as_os_str().is_empty() => f.logo.clear(),
                Some(p) => {
                    let roh = std::fs::read(&p).with_context(|| tr!("{a} lesen", a = p.display()))?;
                    let b = f.logo_setzen(&roh)?;
                    println!("{}", tr!("Logo: {a} × {b} Pixel", a = b.breite, b = b.hoehe));
                }
                None => {}
            }
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
                println!("{}", tr!("Kopf in Berichten: {a}{b}", a = f.zeilen().join("  ·  "), b = if f.logo.is_empty() { String::new() } else { tr!(" (mit Logo)") }));
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
            ecu_werkbank::ablage::schreiben(&path, &text)?;
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
            let inhalt = ecu_werkbank::ablage::lesen(&datei).with_context(|| tr!("Lesen: {a}", a = datei.display()))?;
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
            ecu_werkbank::ablage::schreiben(&out, r.to_pdf())?;
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
            let e = ecu_werkbank::history::erfahrung(&code);
            if !e.zeilen().is_empty() {
                println!();
                for z in e.zeilen() {
                    println!("{z}");
                }
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
            let text = ecu_werkbank::ablage::lesen(&log).with_context(|| tr!("Lesen: {a}", a = log.display()))?;
            let cmp = vergleich.as_ref().map(ecu_werkbank::ablage::lesen).transpose()?;
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
                    ecu_werkbank::ablage::schreiben(&p, &file)?;
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
            ecu_werkbank::ablage::schreiben(&out, ecu_werkbank::akte::to_pdf(&v, &db))?;
            println!("{}", tr!("Fahrzeugakte: {a}", a = out.display()));
            Ok(())
        }
        Command::AkteZip { out } => {
            let v = need_vehicle(cli.fahrzeug.as_deref())?;
            let n = ecu_werkbank::ablage::ordner_zippen(&v.dir(), &out, &v.key)?;
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
            // erst entpacken, dann einspielen: Daten in die Datenbank, Dokumente als Dateien
            let tmp = std::env::temp_dir().join(format!("ecu-werkbank-import-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp)?;
            let r = ecu_werkbank::zip::unzip_to(&datei, &tmp, true).and_then(|_| Ok(ecu_werkbank::ablage::einspielen(&tmp, &root, ueberschreiben)?));
            let _ = std::fs::remove_dir_all(&tmp);
            let dateien = r?.2;
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
            ecu_werkbank::ablage::schreiben(&pfad, ecu_werkbank::kalender::ics(&v, &t)).with_context(|| tr!("Schreiben: {a}", a = pfad.display()))?;
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
                    ecu_werkbank::history::letzter(&v.key).map(|(p, _)| p).context(tr!("Noch kein Scan für dieses Fahrzeug"))?
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
            print_coding(ecu, tx, &raw, &db, None);
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
            if let Some(v) = ecu_werkbank::spannung::vor_dem_schreiben(&be)? {
                println!("{}", tr!("Bordspannung {a} V — in Ordnung.", a = format!("{v:.1}")));
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
            if let Some(v) = ecu_werkbank::spannung::vor_dem_schreiben(&be)? {
                println!("{}", tr!("Bordspannung {a} V — in Ordnung.", a = format!("{v:.1}")));
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
            if let Some(v) = ecu_werkbank::spannung::vor_dem_schreiben(&be)? {
                println!("{}", tr!("Bordspannung {a} V — in Ordnung.", a = format!("{v:.1}")));
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
        ecu_werkbank::ablage::schreiben(p, &t).with_context(|| tr!("Schreiben: {a}", a = p.display()))?;
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
    ecu_werkbank::ablage::schreiben(&out, ecu_werkbank::chart::to_pdf(ecu_werkbank::trs!("Messwert-Aufzeichnung"), &meta, &cols))?;
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
            // normale Scans nur untereinander vergleichen (nicht mit „Andere Marken“)
            if all.iter().any(|r| !history::ist_fremd(r)) {
                all.retain(|r| !history::ist_fremd(r));
            }
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
    print_coding(Some(ecu), Some(tx), &bytes, db, None);
    Ok(())
}

/// Codier-Bytes aufschlüsseln: alle Bytes als Bits, dazu die Namen aus deinen
/// Codiertexten (über `tx`) und `[[coding]]`-Labels (über `ecu`).
fn print_coding(ecu: Option<u16>, tx: Option<u16>, bytes: &[u8], db: &LabelDb, teil: Option<&str>) {
    let mut labels: Vec<ecu_werkbank::labels::Coding> = ecu.and_then(|e| db.coding(e)).map(|c| c.to_vec()).unwrap_or_default();
    // Sonst Codier-Bits aus der Fahrzeug-Datenbank über die Teilenummer
    if labels.is_empty() {
        if let Some(w) = teil.and_then(ecu_werkbank::db::teile::wissen_falls_vorhanden).filter(|w| !w.codierung.is_empty()) {
            println!("{}", tr!("Bit-Namen aus der Fahrzeug-Datenbank ({a})", a = w.teilenummer));
            labels = w.codierung.into_iter().map(|c| ecu_werkbank::labels::Coding { byte: c.byte, bit: c.bit, name: c.name, benoetigt: c.benoetigt, schliesst_aus: c.schliesst_aus }).collect();
        }
    }
    let labels = &labels[..];
    // Ohne CAN-ID: Labels allein (eigene Texte hängen an der CAN-ID)
    let tx_key = tx.unwrap_or(u16::MAX);
    let texte = if tx.is_some() { ecu_werkbank::codiertexte::Texte::laden() } else { Default::default() };
    println!("{}", tr!("Aufschlüsselung:"));
    for z in texte.aufschluesseln(tx_key, labels, bytes) {
        println!("  {z}");
    }
    for h in ecu_werkbank::codiertexte::regeln_pruefen(labels, bytes) {
        println!("  ⚠ {h}");
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
    println!("{}", tr!("Eigene Labels: Fahrzeug-Datenbank {a} (Label-Ordner {b} wurde einmal übernommen)", a = ecu_werkbank::db::standard_pfad().display(), b = dir.display()));
    println!("{}", tr!("Geladen: {ecus} Steuergerät(e), {dtcs} DTC-Text(e), {meas} Messwert(e), \
         {coding} Anpassung/Codierung.", ecus = ecus, dtcs = dtcs, meas = meas, coding = coding));
    if ecus + dtcs + meas + coding == 0 {
        println!("{}", tr!("\nNoch keine Labels. In der GUI: Werkzeuge → Label-Editor (oder eine Label-Datei ins Fenster ziehen)."));
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

// Befehle nach Bereichen in src/cli/ (Roadmap 27)
#[path = "cli/tp20.rs"]
mod cli_tp20;
#[allow(unused_imports)]
use cli_tp20::*;
#[path = "cli/obd.rs"]
mod cli_obd;
#[allow(unused_imports)]
use cli_obd::*;
#[path = "cli/mitschnitt.rs"]
mod cli_mitschnitt;
#[allow(unused_imports)]
use cli_mitschnitt::*;
#[path = "cli/codierung.rs"]
mod cli_codierung;
#[allow(unused_imports)]
use cli_codierung::*;
#[path = "cli/pruefen.rs"]
mod cli_pruefen;
#[allow(unused_imports)]
use cli_pruefen::*;
#[path = "cli/werkstatt.rs"]
mod cli_werkstatt;
#[allow(unused_imports)]
use cli_werkstatt::*;
#[path = "cli/daten.rs"]
mod cli_daten;
#[allow(unused_imports)]
use cli_daten::*;
