//! ECUWerkbank — grafische Oberfläche (egui/eframe).
//!
//! Teilt sich die gesamte Logik mit dem CLI über die Library `ecu_werkbank`.
//! Ohne Adapter nutzbar: der Reiter „Messwerte" dekodiert Rohbytes offline über
//! deine Labels. Scan/DTC sprechen echte Hardware an, sobald ein CAN-Interface
//! verfügbar ist.

// Windows: ohne zusätzliches Konsolenfenster starten
#![cfg_attr(windows, windows_subsystem = "windows")]

// Verschachtelte Typen sind hier gewollt; die Ergebnis-Meldungen (große
// Varianten) werden nur einmal je Vorgang verschickt.
#![allow(clippy::type_complexity, clippy::large_enum_variant)]

use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;
use ecu_werkbank::{tr, trn};
use ecu_werkbank::transport::Backend;
use ecu_werkbank::labels::LabelDb;
use ecu_werkbank::uds::Uds;
use ecu_werkbank::kwp::{self, Kwp, Tp20Found};
use ecu_werkbank::tp20::Channel;
use ecu_werkbank::csvlog::CsvLog;
use ecu_werkbank::labels::LabelFile;
use ecu_werkbank::obd::{self, Obd};
use std::sync::mpsc;

#[path = "gui/motor.rs"]
mod motor;
#[path = "gui/obd_tab.rs"]
mod obd_tab;
#[path = "gui/fahrzeuge.rs"]
mod fahrzeuge;
#[path = "gui/verbindung.rs"]
mod verbindung;
#[path = "gui/werkzeuge.rs"]
mod werkzeuge;
#[path = "gui/tp20_tab.rs"]
mod tp20_tab;
#[path = "gui/editor.rs"]
mod editor;
#[path = "gui/pdf_tab.rs"]
mod pdf_tab;
#[path = "gui/historie.rs"]
mod historie;
#[path = "gui/codiersicherung.rs"]
mod codiersicherung;
#[path = "gui/diagnose.rs"]
mod diagnose;
#[path = "gui/kern.rs"]
mod kern;
#[path = "gui/hintergrund.rs"]
mod hintergrund;
#[path = "gui/canmonitor_tab.rs"]
mod canmonitor_tab;
#[path = "gui/ablegen.rs"]
mod ablegen;
#[path = "gui/reiter.rs"]
mod reiter;
#[path = "gui/scope.rs"]
mod scope;
#[path = "gui/batterie.rs"]
mod batterie;
#[path = "gui/ziel.rs"]
mod ziel;
#[path = "gui/start.rs"]
mod start;
#[path = "gui/mitschnitt.rs"]
mod mitschnitt;

/// Meldungen des PDF-Protokolls (Scan im Hintergrund-Thread).
enum PdfMsg {
    Progress(String),
    Done(Result<ecu_werkbank::report::ScanReport, String>),
}

use ecu_werkbank::limits::Stufe;

/// Meldungen des Gebrauchtwagen-Checks.
/// Meldungen der Suche bei anderen Marken.
enum FmMsg {
    Progress(String),
    Done(Result<(ecu_werkbank::report::ScanReport, ecu_werkbank::fremd::Suche), String>),
}

enum KcMsg {
    Progress(String),
    Done(Result<ecu_werkbank::usedcar::Ergebnis, String>),
}

/// Meldungen des Schnell-Checks.
enum ChkMsg {
    Progress(String),
    Done(Result<ecu_werkbank::check::CheckResult, String>),
}

/// Meldungen des Batterietests: (t, U) oder Ende.
enum BatMsg {
    Sample(f64, f64),
    Done(Result<Vec<[f64; 2]>, String>),
}

/// Farbiger Punkt (die GUI-Schrift hat kein ●).
/// In einer umbrechenden Zeile (`horizontal_wrapped`) eine neue Zeile beginnen,
/// wenn `breite` nicht mehr passt. Nötig vor Klappmenüs, Rahmen und Gruppen:
/// die brechen selbst nicht um, laufen über den Rand und machen sonst die
/// ganze Leiste breiter als das Fenster.
fn platz(ui: &mut egui::Ui, breite: f32) {
    if ui.available_size_before_wrap().x < breite {
        ui.end_row();
    }
}

fn dot(ui: &mut egui::Ui, c: egui::Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 5.0, c);
}

fn stufe_color(s: Stufe) -> egui::Color32 {
    match s {
        Stufe::Gut => egui::Color32::from_rgb(80, 190, 105),
        Stufe::Hinweis => egui::Color32::from_rgb(225, 165, 40),
        Stufe::Warnung => egui::Color32::from_rgb(235, 85, 70),
    }
}

/// Fehlercode in einer Textzeile finden (P0300, U0100, 16684 …).
fn code_in(line: &str) -> Option<String> {
    line.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')' || c == ':')
        .find(|t| {
            let b = t.as_bytes();
            (t.len() == 5 && matches!(b[0], b'P' | b'C' | b'B' | b'U') && t[1..].chars().all(|c| c.is_ascii_hexdigit()))
                || (t.len() == 5 && t.chars().all(|c| c.is_ascii_digit()))
        })
        .map(str::to_string)
}

/// Websuche zu einem Fehlercode (+ Fahrzeugmodell) im Browser öffnen.
fn open_code_search(code: &str, model: &str, status: &mut String) {
    let q = tr!("{code} {model} Fehlercode", code = code, model = model);
    let enc: String = q
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    open_path(std::path::Path::new(&format!("https://duckduckgo.com/?q={enc}")), status);
}

/// Alle Fehlercodes in einer Zeile (z. B. „08851 (P2293)“ → beide).
fn codes_in(line: &str) -> Vec<String> {
    let mut rest = line;
    let mut out = Vec::new();
    while let Some(c) = code_in(rest) {
        let i = rest.find(&c).map(|i| i + c.len()).unwrap_or(rest.len());
        out.push(c);
        rest = &rest[i..];
    }
    out
}

/// Meldungen der Codierungs-Sicherung (Hintergrund-Thread).
/// Komplett-Sicherung für den Notfall.
enum NfMsg {
    Progress(String),
    /// (Fahrzeug, PDF, Zusammenfassung)
    Done(Result<(String, PathBuf, String), String>),
}

enum BackupMsg {
    Progress(String),
    Done(Result<ecu_werkbank::backup::CodingBackup, String>),
}

/// Ergebnis der Fahrzeug-Erkennung im Hintergrund.
enum ErkMsg {
    /// Versuch übersprungen (Bus ruhig, kein Grund zu senden)
    Ausgelassen,
    /// Suche: gefunden oder nicht; dazu die Zähler des Adapters danach
    Gesucht(Option<ecu_werkbank::erkennung::Fund>, Option<(u64, u64)>),
    /// Nachschauen: Auto noch da?
    NochDa(bool),
}

/// Meldungen der TP-2.0-Suche (läuft im Hintergrund-Thread).
enum SvzMsg {
    Fortschritt(u16, usize),
    Fertig(Result<ecu_werkbank::service::Schnappschuss, String>),
}

enum SweepMsg {
    Progress(u8),
    Done(Result<Vec<Tp20Found>, String>),
}

/// Meldungen des Blockscans (Hintergrund-Thread).
enum ScanMsg {
    Block(u8),
    Fertig(Result<Vec<(u8, Vec<kwp::BlockValue>)>, String>),
}

/// Gängige VAG-Steuergeräte-Adressen (genormte Fakten) mit eigenen
/// Kurzbezeichnungen. Keine fremden Label-Texte — reine Adressliste.
const COMMON_MODULES: &[(u16, &str)] = &[
    (0x01, trn!("Motor")),
    (0x02, trn!("Getriebe")),
    (0x03, trn!("Bremsen (ABS/ESP)")),
    (0x08, trn!("Klimaanlage")),
    (0x09, trn!("Bordnetz")),
    (0x15, trn!("Airbag")),
    (0x16, trn!("Lenkradelektronik")),
    (0x17, trn!("Kombiinstrument")),
    (0x19, trn!("Gateway")),
    (0x25, trn!("Wegfahrsperre")),
    (0x42, trn!("Türelektronik FS")),
    (0x44, trn!("Lenkhilfe")),
    (0x46, trn!("Komfortsystem")),
    (0x52, trn!("Türelektronik BS")),
    (0x55, trn!("Leuchtweite")),
    (0x56, trn!("Radio")),
];

/// Programmversion aus Cargo.toml (wird oben in der Oberfläche und im Fenstertitel gezeigt).
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> eframe::Result<()> {
    // Fenstergröße, z. B. VAG_FENSTER=1100x780 (für Bildschirmfotos im Handbuch)
    let groesse = std::env::var("VAG_FENSTER")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some([w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?])
        })
        .unwrap_or([900.0, 620.0]);
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(groesse)
        .with_min_inner_size([560.0, 400.0])
        .with_title(format!("ECUWerkbank {VERSION}"))
        // passt zu ecu-werkbank.desktop, damit Taskleiste/Startmenü dasselbe Icon zeigen
        .with_app_id("ecu-werkbank");
    match eframe::icon_data::from_png_bytes(include_bytes!("../../packaging/ecu-werkbank.png")) {
        Ok(icon) => viewport = viewport.with_icon(std::sync::Arc::new(icon)),
        Err(e) => eprintln!("Icon nicht lesbar: {e}"),
    }
    let opts = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "ecu-werkbank",
        opts,
        Box::new(|cc| {
            install_fallback_fonts(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

/// Ersatzschriften aus dem System, damit Zeichen wie → ✓ ✗ ≤ ≥ nicht als
/// Kästchen erscheinen (die eingebaute Schrift kennt sie nicht). Fehlen die
/// Dateien, bleibt alles wie es ist.
fn install_fallback_fonts(ctx: &egui::Context) {
    let candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf",
    ];
    let mut fonts = egui::FontDefinitions::default();
    let mut added = false;
    for (i, path) in candidates.iter().enumerate() {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let name = format!("fallback{i}");
        fonts.font_data.insert(name.clone(), egui::FontData::from_owned(bytes));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(fam).or_default().push(name.clone());
        }
        added = true;
    }
    if added {
        ctx.set_fonts(fonts);
    }
}

/// Meldungen der Stellglieddiagnose aus dem Hintergrund
enum SgMsg {
    Stand(String),
    Fertig(Result<(), String>),
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Tab {
    Start,
    Steuergeraete,
    Messwerte,
    Scope,
    Codierung,
    Service,
    Tp20,
    Obd,
    Fahrzeuge,
    Batterie,
    Ziel,
    Diagnose,
    Software,
    Nachruestung,
    Dpf,
    ServiceAnzeige,
    Wissen,
    Testlauf,
    Sicherheit,
    Mitschnitt,
    CanMonitor,
    Historie,
    Sicherung,
    Namen,
    Editor,
    Motor,
    Rekorder,
    Fremd,
    Ident,
    GrundLexikon,
}

/// VAG_FILM: nimmt eine Bildfolge auf, während die Demo läuft (für ein bewegtes Video).
struct FilmState {
    dir: PathBuf,
    total: usize,
    interval: std::time::Duration,
    warten: std::time::Duration,
    start: std::time::Instant,
    naechste: std::time::Instant,
    zaehler: usize,
    wartet_bild: bool,
}

struct App {
    labels_dir: PathBuf,
    db: LabelDb,
    iface: String,
    transport: String, // "socketcan" | "elm327" | "wifi"
    port: String,
    baud: u32,
    tab: Tab,
    dark: bool,
    /// Andere Marken: laufende Suche, Fortschritt, Ergebnis, PDF, Lösch-Bestätigung
    fm_job: Option<mpsc::Receiver<FmMsg>>,
    fm_progress: String,
    fm_result: Option<(ecu_werkbank::report::ScanReport, ecu_werkbank::fremd::Suche)>,
    fm_pdf: Option<PathBuf>,
    nf_job: Option<mpsc::Receiver<NfMsg>>,
    nf_progress: String,
    nf_pdf: Option<PathBuf>,
    fm_confirm: Option<usize>,
    /// Fahrzeug-Erkennung im Hintergrund an?
    erk_an: bool,
    erk_job: Option<mpsc::Receiver<ErkMsg>>,
    erk_zuletzt: Option<std::time::Instant>,
    erk_zaehler: Option<(u64, u64)>,
    /// Gerade verbundenes Auto (None = keins erkannt)
    erk_fund: Option<ecu_werkbank::erkennung::Fund>,
    /// Für welchen Adapter die Erkennung gilt
    erk_fuer: String,
    selected_ecu: Option<u16>,

    scan_results: Vec<(u16, u16)>,

    dtc_tx: String,
    dtc_rx: String,
    dtc_results: Vec<String>,
    /// Hinweis zur letzten Fehlerabfrage (Weg, „keine Fehler“, Fehlermeldung)
    dtc_info: String,
    /// Steuergeräte des Fahrzeugs: gewähltes Modul (Index im letzten Scan) + Ergebnis
    fz_sel: Option<usize>,
    fz_lines: Vec<String>,
    fz_info: String,
    fz_clear_confirm: bool,
    /// Adapter-Zustand für die Ampel oben (alle 2 s aktualisiert)
    ad_cache: std::cell::RefCell<Option<(std::time::Instant, String, (Stufe, String, String))>>,

    // (c) Live-Dekodierung: pro (ecu,id) ein Rohbyte-Eingabefeld
    meas_inputs: HashMap<(u16, u16), String>,

    // Schnell-Eingabe eigener Messwert-/Feldnamen
    qn_addr: String,
    qn_block: String,
    qn_feld: String,
    qn_name: String,

    // (1) Codierung (Long Coding Helper, offline)
    code_ecu: Option<u16>,
    code_tx: String,
    code_rx: String,
    code_read: Option<Vec<u8>>,
    code_confirm: bool,
    /// Ausgangsstand für „geändert“: (woher, Bytes)
    code_quelle: Option<(String, Vec<u8>)>,
    /// Neueste Sicherung: (Anzeige, Datum, UDS-Einträge); None = noch nicht geladen
    code_bk: Option<(String, String, Vec<ecu_werkbank::backup::CodingEntry>)>,
    code_texte: ecu_werkbank::codiertexte::Texte,
    /// Offener Text-Editor: (Byte, Bit oder None = ganzes Byte, Text)
    code_edit: Option<(usize, Option<u8>, String)>,
    code_nur_benannt: bool,
    /// TP-2.0-Steuergerät (Adresse) statt UDS: nur lesen und benennen
    code_tp20: Option<u8>,
    /// Kurzcodierung (TP 2.0): gelesener Wert, Eingabe neuer Wert, Bestätigung
    code_kurz: Option<u32>,
    code_kurz_neu: String,
    code_kurz_ok: bool,

    // Softwarestände
    sw_last: Option<ecu_werkbank::swinfo::Stand>,
    sw_diff: Vec<(String, String)>,
    // Erweiterte Identifikation (nur lesen)
    ident_tx: String,
    ident_result: Option<(String, Vec<(String, String)>)>,
    // Grundeinstellung-Katalog (Referenz)
    gek_system: String,
    gek_suche: String,
    // Nachrüstung
    nr_name: String,
    nr_notiz: String,
    nr_vorher: Option<ecu_werkbank::retrofit::Stand>,
    nr_result: Vec<(Stufe, String)>,
    nr_job: Option<(u8, mpsc::Receiver<Result<ecu_werkbank::retrofit::Stand, String>>)>,
    // Partikelfilter
    dpf_vals: Option<(ecu_werkbank::dpf::Werte, Vec<(Stufe, String)>)>,
    svz_vals: Vec<ecu_werkbank::service::Gelesen>,
    svz_lines: Vec<String>,
    svz_ziel: [String; 4],
    svz_kandidaten: Vec<(ecu_werkbank::service::Kandidat, bool)>,
    svz_neu: [String; 4],
    svz_job: Option<(mpsc::Receiver<SvzMsg>, String)>,
    svz_fortschritt: (u16, usize),
    svz_vorschlag: Vec<ecu_werkbank::service::Wert>,
    svz_bestaetigen: bool,
    svz_wartung: bool,
    /// Für welches Fahrzeug die Werte schon automatisch gelesen wurden
    svz_gelesen_fuer: Option<Option<String>>,
    /// Fehlertext direkt in der Fehlerliste eintragen: (Code, Entwurf)
    ft_inline: Option<(String, String)>,
    dpf_did: [String; 4],
    dpf_fak: [f64; 4],
    dpf_tx: String,
    dpf_grenze: f64,
    dpf_loaded_for: Option<String>,
    // Fehlerbilder
    ws_code: String,
    ws_modell: String,
    ws_text: String,
    /// eigene Fehlertexte: Code + Text
    ft_code: String,
    ft_text: String,
    /// Datensicherung: Einstellungen (Eingabe), laufender Job, beim Start schon geprüft?
    ds: ecu_werkbank::datensicherung::Einstellung,
    ds_job: Option<mpsc::Receiver<Result<(PathBuf, usize), String>>>,
    ds_geprueft: bool,
    /// Fehlertext-Gerüst: laufende Abfrage, Fortschritt, Ergebnis, Eingaben je Code
    fg_job: Option<mpsc::Receiver<FgMsg>>,
    fg_progress: String,
    fg_geraete: Vec<ecu_werkbank::fehlergeruest::Geraet>,
    fg_texte: std::collections::BTreeMap<String, String>,
    fg_nur_leer: bool,
    // Sicherheit & Assistenz
    si_job: Option<mpsc::Receiver<Result<ecu_werkbank::report::ScanReport, String>>>,
    // Verbauliste direkt aus dem Gateway (TP 2.0)
    gw_vb_job: Option<mpsc::Receiver<Result<Vec<ecu_werkbank::kwp::VerbauEintrag>, String>>>,
    gw_vb: Option<Result<Vec<ecu_werkbank::kwp::VerbauEintrag>, String>>,
    /// Testlauf: Tachostand, laufender Job (Fortschritt / fertiger Text + Pfad), Ergebnis
    tl_km: String,
    tl_job: Option<mpsc::Receiver<TlMsg>>,
    tl_progress: String,
    tl_text: String,
    si_report: Option<ecu_werkbank::report::ScanReport>,
    code_bytes: String,
    /// Codier-Helfer: frühere Stände der Hex-Codierung (Rückgängig), neuester zuletzt
    code_undo: Vec<String>,

    // Scope (Live-Grafik, mehrere Kurven)
    scope_did: String,
    scope_tx: String,
    scope_rx: String,
    scope_running: bool,
    scope_series: Vec<Series>,
    scope_start: std::time::Instant,
    scope_last: std::time::Instant,
    scope_interval_ms: u64,
    scope_window: f64,
    scope_normalize: bool,
    scope_add_obd: u8,
    scope_add_kwp_addr: String,
    scope_add_kwp_block: String,
    scope_kwp_ch: HashMap<u8, Channel>,
    scope_csv: Option<CsvLog>,
    scope_file: Option<String>,
    scope_gauges: bool,
    scope_alarm_on: bool,
    scope_alarm_text: String,
    scope_alarm_until: Option<std::time::Instant>,
    trip_on: bool,
    trip_active: bool,
    trip_last_check: std::time::Instant,
    trip_low_since: Option<std::time::Instant>,
    web_server: Option<ecu_werkbank::web::Server>,
    web_data: ecu_werkbank::web::Shared,

    // (3) Service: Routine / Stellglied
    svc_tx: String,
    svc_rx: String,
    routine_id: String,
    routine_sub: String,
    routine_data: String,
    io_did: String,
    io_control: String,
    io_state: String,
    svc_result: String,
    /// Wartet auf Bestätigung, bevor etwas ans Auto geht
    svc_plan: Option<SvcPlan>,

    // TP 2.0 / KWP2000 (ältere Fahrzeuge)
    tp_addr: String,
    tp_block: String,
    tp_live: bool,
    tp_channel: Option<(u8, Channel)>,
    tp_csv: Option<CsvLog>,

    // OBD-II
    obd_vin: String,
    obd_readiness: Option<obd::Readiness>,
    /// Readiness-Assistent: diese Fahrt + Fortschritt seit dem Löschen
    obd_stand: Option<ecu_werkbank::readiness::Stand>,
    obd_dtcs: Vec<String>,
    obd_supported: Vec<u8>,
    obd_selected: std::collections::BTreeSet<u8>,
    obd_live: bool,
    obd_values: Vec<(String, String, Option<Stufe>)>,
    obd_ready_live: bool,
    obd_ready_last: std::time::Instant,
    obd_freeze: Option<Option<obd::FreezeFrame>>,
    obd_last: std::time::Instant,
    obd_csv: Option<CsvLog>,
    obd_confirm_clear: bool,

    // Mitschnitt
    log_path: String,
    log_lines: Vec<String>,
    log_report: String,
    log_filter: String,
    log_text: String,
    log_cmp: Option<(String, String)>,
    learn: Vec<ecu_werkbank::learn::Vorschlag>,
    learn_addr: String,
    learn_name: String,

    // Verbindungs-Assistent
    con_result: Option<Vec<(Stufe, String)>>,

    // Mitschnitt aufzeichnen
    rec_job: Option<(std::sync::Arc<std::sync::atomic::AtomicBool>, std::sync::Arc<std::sync::atomic::AtomicUsize>, PathBuf, std::time::Instant)>,
    // Alle Fehler löschen (Steuergeräte-Reiter)
    clr_confirm: bool,

    // Gebrauchtwagen-Check
    kc_job: Option<mpsc::Receiver<KcMsg>>,
    kc_progress: String,
    kc_result: Option<(Vec<ecu_werkbank::usedcar::Punkt>, Stufe, String, Option<PathBuf>)>,
    kc_note: String,

    // Geführte Fehlersuche
    gf_code: String,
    gf_done: Vec<bool>,
    gf_for: String,
    gf_note: String,

    // Reparatur-Modus
    rp_before: Option<ecu_werkbank::report::ScanReport>,
    rp_arbeiten: String,
    /// Schritt, Ergebnis: Scan und (nach der Reparatur) noch offene Readiness-Tests
    rp_job: Option<(u8, mpsc::Receiver<Result<(ecu_werkbank::report::ScanReport, Vec<String>), String>>)>,
    rp_confirm_clear: bool,
    rp_cleared: Vec<(String, Result<(), String>)>,

    // Fehlerspeicher-Protokoll (PDF)
    /// Ziel = None » automatisch im Fahrzeugordner (berichte/)
    pdf_job: Option<(Option<PathBuf>, mpsc::Receiver<PdfMsg>)>,

    // Frühere Reparaturen je Code (Cache für das aktive Fahrzeug)
    rep_cache: HashMap<String, Vec<String>>,
    rep_cache_for: Option<String>,

    // Diagnose: Mode 06 + Gemisch
    m6_results: Vec<obd::TestResult>,
    m6_summary: Vec<(Stufe, String)>,
    gm_phase: u8,
    gm_seconds: u32,
    gm_start: std::time::Instant,
    gm_last: std::time::Instant,
    gm_idle: Vec<ecu_werkbank::mixture::Sample>,
    gm_high: Vec<ecu_werkbank::mixture::Sample>,
    gm_now: Option<ecu_werkbank::mixture::Sample>,
    gm_result: Vec<(Stufe, String)>,

    // Historie: Trend-Auswahl
    tr_sel: String,

    // Zielbereich: Assistent
    zl_pid: u8,
    zl_m: MusterEdit,
    zl_tol: f64,
    zl_running: bool,
    zl_start: std::time::Instant,
    zl_last: std::time::Instant,
    zl_ist: Vec<[f64; 2]>,
    zl_soll: Vec<[f64; 2]>,
    zl_n: u32,
    zl_in: u32,
    zl_beep: bool,
    zl_out_since: Option<std::time::Instant>,
    zl_last_beep: std::time::Instant,
    zl_now: Option<(f64, f64)>,
    // Zielbereich: Steuergerät stellt
    st_sel: usize,
    st_m: MusterEdit,
    st_seconds: u32,
    st_confirm: bool,
    st_regler: Option<ecu_werkbank::control::Regler>,
    st_last: std::time::Instant,
    st_ist: Vec<[f64; 2]>,
    st_soll: Vec<[f64; 2]>,
    // Zielbereich: Simulation
    sim_param: usize,
    sim_m: MusterEdit,

    // Schnell-Check
    chk_job: Option<mpsc::Receiver<ChkMsg>>,
    chk_progress: String,
    chk_result: Option<(Vec<(Stufe, String)>, Stufe, Option<PathBuf>)>,
    pdf_km: String,

    // Batterietest
    bat_job: Option<mpsc::Receiver<BatMsg>>,
    bat_points: Vec<[f64; 2]>,
    bat_result: Option<ecu_werkbank::battery::BatteryResult>,
    bat_seconds: u32,

    // Historie: Reparatur-Notiz des gewählten Scans
    hist_note: String,
    hist_note_for: Option<usize>,

    // Wartung (Fahrzeuge-Reiter)
    wt_new_name: String,
    wt_new_km: String,
    wt_new_mon: String,
    // Gewechselte Teile: Eingabezeile
    teil_name: String,
    teil_nr: String,
    teil_km: String,
    teil_datum: String,
    teil_notiz: String,
    teil_preis: String,
    /// Tankbuch: Eingabezeile
    tk_km: String,
    tk_liter: String,
    tk_preis: String,
    tk_datum: String,
    tk_voll: bool,
    tk_notiz: String,
    veh_km: String,
    km_job: Option<mpsc::Receiver<Result<ecu_werkbank::km::Ergebnis, String>>>,
    km_lines: Vec<String>,

    // Fahrzeuge
    active_vehicle: Option<String>,
    veh_sel: Option<String>,
    veh_edit: ecu_werkbank::vehicle::Vehicle,
    veh_new_plate: String,
    veh_new_name: String,
    veh_new_vin: String,
    pdf_progress: String,
    pdf_note: String,
    pdf_last: Option<PathBuf>,

    // Historie
    hist_vehicle: String,
    hist_scans: Vec<(PathBuf, ecu_werkbank::report::ScanReport)>,
    hist_a: Option<usize>,
    hist_b: Option<usize>,

    // Codierungs-Sicherung
    bk_job: Option<mpsc::Receiver<BackupMsg>>,
    bk_progress: String,
    bk_list: Vec<(PathBuf, ecu_werkbank::backup::CodingBackup)>,
    /// Für welches Fahrzeug `bk_list` geladen ist
    bk_fuer: Option<Option<String>>,
    bk_sel: Option<usize>,
    bk_cmp: Option<usize>,
    bk_confirm: Option<usize>,

    // Label-Editor
    ed_path: Option<PathBuf>,
    ed_file: LabelFile,
    ed_addr: String,
    ed_tx: String,
    ed_rx: String,
    ed_new_name: String,
    tp_last_read: std::time::Instant,
    tp_lines: Vec<String>,
    tp_block_values: Vec<(String, Option<Stufe>)>,
    tp_block_raw: Vec<kwp::BlockValue>,
    /// Motor-Checks, Fahrt-Rekorder, Anpassungen (siehe gui/motor.rs)
    motor: motor::State,
    /// Blockscan: laufender Job, aktueller Block, Ergebnis (Adresse, Blöcke)
    bs_job: Option<mpsc::Receiver<ScanMsg>>,
    bs_at: u8,
    bs_result: Option<(u8, Vec<(u8, Vec<kwp::BlockValue>)>)>,
    /// Eingaben beim Benennen aller Blöcke (Schlüssel wie messwerttexte)
    bs_namen: std::collections::BTreeMap<String, String>,
    /// Stellglieddiagnose (TP 2.0): Bestätigung, laufender Job (Befehle hin, Meldungen zurück), Verlauf
    sg_bestaetigt: bool,
    sg_job: Option<(mpsc::Sender<kwp::StellBefehl>, mpsc::Receiver<SgMsg>)>,
    sg_log: Vec<String>,
    /// Stellglied-Bereich beim nächsten Anzeigen aufklappen (Sprung von „Steuergeräte“)
    sg_zeigen: bool,
    /// Anpassung (TP 2.0): Kanäle als Eingabe, Ergebniszeilen, aufklappen
    an_kanaele: String,
    an_result: Vec<String>,
    an_zeigen: bool,
    /// Anpassung schreiben: Kanal, neuer Wert, Bestätigung
    an_kanal: String,
    an_wert: String,
    an_ok: bool,
    /// Benennen-Tabelle: nur Felder ohne Namen, Suchtext, Rohwerte ausblenden
    bn_nur_offen: bool,
    bn_suche: String,
    bn_roh_aus: bool,
    /// „Eigene Namen“: Steuergerät, dessen Blöcke gelesen werden
    bn_addr: String,
    tp_block_key: (u8, u8),
    mw_texte: ecu_werkbank::messwerttexte::Texte,
    /// Grundeinstellung (TP 2.0): Gruppe, gelernter-aber-noch-nicht-übernommener Ablauf,
    /// Rückfrage offen, laufender Ablauf (Stopp-Schalter + Meldungen), letzte Werte/Status
    ge_gruppe: String,
    ge_neu: Option<ecu_werkbank::kwp_grund::Ablauf>,
    ge_frage: bool,
    ge_job: Option<(std::sync::Arc<std::sync::atomic::AtomicBool>, mpsc::Receiver<ecu_werkbank::kwp_grund::Meldung>)>,
    ge_laeuft: bool,
    ge_werte: Vec<String>,
    ge_status: String,
    /// Bildschirmfoto: Abschnitt „Grundeinstellung“ beim Start aufgeklappt
    ge_zeigen: bool,
    /// VAG_FOTO: Ziel-Datei, Wartezeit, Startzeit, schon angefordert
    foto: Option<(PathBuf, std::time::Duration, std::time::Instant, bool)>,
    /// VAG_FILM: Bildfolge für ein bewegtes Video (nur Demo/Screenshots)
    film: Option<FilmState>,
    /// Favoriten-Messwerte (Steuergerät + Block), im Speicher gehalten
    favoriten: Vec<ecu_werkbank::favoriten::Favorit>,
    mw_edit: Option<(Option<u8>, String)>,
    tp_sweep: Option<mpsc::Receiver<SweepMsg>>,
    tp_sweep_at: u8,
    tp_found: Vec<Tp20Found>,
    /// Zwischenspeicher der bekannten TP-2.0-Steuergeräte (Suche + letzter Scan), alle paar Sekunden neu
    tp20_bekannt: Option<(std::time::Instant, Vec<(u8, String)>)>,
    /// Voller Suchlauf statt Schnell-Scan mit Fahrzeugprofil
    voll_scan: bool,
    /// Demo/Screenshot: Suche beim Start auslösen (VAG_TAB=tp20)
    tp_autostart: bool,
    /// Demo/Screenshot: Reiter beim Start befüllen (VAG_TAB=obd|mitschnitt|editor)
    demo_autostart: String,
    /// Bus-Arbeit im Hintergrund (ein Thread, Aufträge der Reihe nach)
    bus: hintergrund::BusArbeiter,
    /// Live-CAN-Monitor (Seite „CAN-Monitor“)
    cm: canmonitor_tab::CanMonitorSeite,

    status: String,
}


impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.hintergrund_tick(ctx);
        self.ablegen_tick(ctx);
        self.erkennung_tick(ctx);
        self.datensicherung_tick(ctx);
        match std::mem::take(&mut self.demo_autostart).as_str() {
            "obd" => {
                self.run_obd_info();
                self.run_obd_dtc();
                self.obd_live = true;
            }
            "mitschnitt" => {
                let d = ecu_werkbank::canlog::Decoder::decode_text(&ecu_werkbank::canlog::demo_log());
                self.log_lines = d.lines().iter().map(|l| gui_text(l)).collect();
                self.log_report = gui_text(&d.report.text());
                self.log_path = tr!("(Beispiel)");
            }
            "editor" => self.ed_open(self.labels_dir.join("beispiel.toml")),
            "historie" => self.hist_refresh(),
            "blockscan" => {
                self.tp_addr = "0x01".into();
                if let Ok(v) = kwp::blockscan(&self.backend(), 0x01, 1, 255, |_| {}) {
                    self.bs_result = Some((0x01, v));
                }
            }
            "fehlertexte" => {
                if let Ok(g) = ecu_werkbank::fehlergeruest::lesen(&self.backend(), &self.db, |_| {}) {
                    self.fg_geraete = g;
                    self.fg_texte_neu_laden();
                }
            }
            "fahrzeuge" => self.veh_sel = self.active_vehicle.clone(),
            "check" => self.start_check(),
            "diagnose" => self.run_mode06(),
            "verbindung" => self.verbindung_pruefen(),
            t @ ("canmonitor" | "canmonitor-grafik") => {
                if let Ok(l) = ecu_werkbank::canmonitor::Lauf::starten(ecu_werkbank::canmonitor::Quelle::Demo) {
                    l.demo_aktion.store(true, std::sync::atomic::Ordering::Relaxed);
                    self.cm_demo_start(l);
                }
                // Bildschirmfoto: eigene DBC (VAG_DBC) laden, Signale in die Live-Grafik
                if let Ok(p) = std::env::var("VAG_DBC") {
                    self.cm_dbc_laden(std::path::Path::new(&p));
                }
                if t == "canmonitor-grafik" {
                    let signale: Vec<(u32, String, String, String)> = self
                        .cm_dbc()
                        .map(|d| d.botschaften.values().flat_map(|b| b.signale.iter().map(|s| (b.id, b.name.clone(), s.name.clone(), s.einheit.clone()))).collect())
                        .unwrap_or_default();
                    for (id, b, s, e) in signale {
                        self.scope_series.push(Series::new(&format!("{b}.{s}"), &e, ChanKind::Can { id, signal: s }));
                    }
                    self.tab = Tab::Scope;
                    self.scope_start_stop();
                }
            }
            "kaufcheck" => {
                ecu_werkbank::sim::set_manipuliert(true);
                self.start_kaufcheck();
            }
            "lernen" => {
                self.log_text = ecu_werkbank::canlog::demo_log_aktion();
                self.log_cmp = Some((tr!("(Beispiel ohne Aktion)"), ecu_werkbank::canlog::demo_log()));
                self.learn = ecu_werkbank::learn::lernen(&self.log_text, self.log_cmp.as_ref().map(|c| c.1.as_str()));
            }
            "gemisch" => {
                ecu_werkbank::sim::set_fehlerbild(ecu_werkbank::sim::Fehlerbild::Falschluft);
                self.gm_seconds = 3;
                self.gm_begin();
            }
            "ziel" => {
                ecu_werkbank::sim::set_muster(ecu_werkbank::sim::Param::Drehzahl, Some(ecu_werkbank::control::Muster::Schwanken { min: 2100.0, max: 3000.0, periode: 6.0 }));
                self.zl_toggle();
            }
            "stellen" => {
                ecu_werkbank::sim::set_muster(ecu_werkbank::sim::Param::Geschwindigkeit, Some(ecu_werkbank::control::Muster::Halten(0.0)));
                self.st_m = MusterEdit { mode: 1, a: 900.0, b: 1600.0, c: 5.0 };
                self.st_confirm = true;
                self.st_start();
            }
            "batterie" => self.start_battery(),
            "sicherung" => self.start_backup(),
            "ident" => {
                self.ident_tx = "0x714".into();
                self.ident_lesen();
            }
            "tp20-block" | "favoriten" => {
                self.tp_addr = "0x01".into();
                self.tp_block = "2".into();
                self.block_lesen();
            }
            "sicherheit" => self.si_starten(),
            "steuergeraete-wahl" => {
                // erstes älteres Steuergerät aus dem letzten Scan wählen (für Bilder)
                if let Some(k) = self.active_vehicle.clone() {
                    if let Some((_, r)) = ecu_werkbank::history::list(&k).into_iter().last() {
                        self.fz_sel = r.modules.iter().position(|m| m.protocol == "TP 2.0");
                    }
                }
            }
            "namen-scan" => {
                self.bn_addr = "0x01".into();
                self.bs_start(0x01);
            }
            "anpassung" => {
                self.tp_addr = "0x20".into();
                self.an_kanaele = "1,3,17,23".into();
                self.an_zeigen = true;
                self.an_kanal = "23".into();
                self.an_wert = "15".into();
                if let Ok(l) = ecu_werkbank::kwp::anpassung_lesen(&self.backend(), 0x20, &[1, 3, 17, 23]) {
                    self.an_result = l.into_iter().map(|r| r.map(|k| tr!("Kanal {a}: Wert {b}", a = format!("{:03}", k.kanal), b = k.wert)).unwrap_or_else(|e| e)).collect();
                }
            }
            "grundeinstellung" => {
                self.tp_addr = "0x01".into();
                self.ge_gruppe = "60".into();
                self.ge_zeigen = true;
            }
            "service-frage" => {
                self.svc_tx = "0x7E0".into();
                self.svc_rx = "0x7E8".into();
                self.routine_id = "0x0203".into();
                self.routine_sub = "1".into();
                self.svc_plan = self.svc_plan_bauen(true).ok();
            }
            "editor-scan" => {
                self.ed_aus_scan();
                if let Some(p) = self.label_files().into_iter().find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("20-"))) {
                    self.ed_open(p);
                }
            }
            "scope" | "armaturen" => {
                self.scope_add_kwp_block_series();
                self.scope_start_stop();
            }
            "pdf" => {
                if let Ok(out) = std::env::var("VAG_PDF_OUT") {
                    self.pdf_note = tr!("Demo");
                    self.start_pdf_to(Some(PathBuf::from(out)));
                }
            }
            _ => {}
        }
        self.foto_tick(ctx);
        self.film_tick(ctx);
        self.poll_tp20(ctx);
        self.poll_grund(ctx);
        self.poll_obd(ctx);
        self.poll_pdf(ctx);
        self.poll_scope(ctx);
        self.poll_backup(ctx);
        self.poll_check(ctx);
        self.poll_battery(ctx);
        self.poll_readiness(ctx);
        self.check_window(ctx);
        self.connect_window(ctx);
        self.poll_kaufcheck(ctx);
        self.poll_repair(ctx);
        self.poll_nachruestung(ctx);
        self.poll_sicherheit(ctx);
        self.poll_testlauf(ctx);
        self.poll_fehlergeruest(ctx);
        self.poll_ziel(ctx);
        self.poll_gemisch(ctx);
        self.poll_stellen(ctx);
        ctx.set_visuals(if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.heading("ECUWerkbank");
                ui.weak(format!("v{VERSION}")).on_hover_text(tr!("Programmversion"));
                ui.separator();
                platz(ui, 150.0);
                egui::ComboBox::from_id_source("transport")
                    .selected_text(match self.transport.as_str() {
                        "elm327" => tr!("ELM327 (USB/BT)"),
                        "wifi" => tr!("ELM327 WiFi"),
                        "demo" => tr!("🚗 Demo-Fahrzeug"),
                        _ => tr!("SocketCAN"),
                    })
                    .show_ui(ui, |ui| {
                        if ecu_werkbank::can::VERFUEGBAR {
                            ui.selectable_value(&mut self.transport, "socketcan".into(), tr!("SocketCAN"));
                        }
                        ui.selectable_value(&mut self.transport, "elm327".into(), tr!("ELM327 (USB/BT)"));
                        ui.selectable_value(&mut self.transport, "wifi".into(), tr!("ELM327 WiFi"));
                        ui.selectable_value(&mut self.transport, "demo".into(), tr!("🚗 Demo-Fahrzeug"));
                    });
                match self.transport.as_str() {
                    "socketcan" => {
                        ui.label("iface:");
                        ui.add(egui::TextEdit::singleline(&mut self.iface).desired_width(64.0));
                        let (st, kurz, lang) = self.adapter_state();
                        dot(ui, stufe_color(st));
                        if ui.add(egui::Label::new(egui::RichText::new(kurz).small()).extend().sense(egui::Sense::click())).on_hover_text(lang).clicked() {
                            self.verbindung_pruefen();
                        }
                        self.erkennung_anzeige(ui);
                    }
                    "wifi" => {
                        ui.label("host:port:");
                        ui.add(egui::TextEdit::singleline(&mut self.port).desired_width(130.0));
                    }
                    "demo" => {
                        let cur = ecu_werkbank::sim::modell();
                        platz(ui, 130.0);
                        egui::ComboBox::from_id_source("sim_modell").selected_text(cur.name()).show_ui(ui, |ui| {
                            for m in ecu_werkbank::sim::Modell::ALL {
                                if ui.selectable_label(cur == m, m.name()).clicked() && cur != m {
                                    ecu_werkbank::sim::set_modell(m);
                                    self.tp_found.clear();
                                    self.status = tr!("Simulation: {a} — Fehlerspeicher und Werte zurückgesetzt.", a = m.name());
                                }
                            }
                        })
                        .response
                        .on_hover_text(tr!("Welches Fahrzeug simuliert wird. „Golf 6“ ist nach dem echten Scan nachgebaut (Steuergeräte, Kennungen, Fehler)."));
                        ui.add(egui::Label::new(egui::RichText::new(tr!("simuliert, ohne Adapter")).weak()).extend());
                        self.erkennung_anzeige(ui);
                    }
                    _ => {
                        ui.label(tr!("Port:"));
                        ui.add(egui::TextEdit::singleline(&mut self.port).desired_width(120.0));
                    }
                }
                ui.separator();
                let label = self
                    .active_vehicle
                    .as_deref()
                    .and_then(ecu_werkbank::vehicle::load)
                    .map(|v| v.label())
                    .unwrap_or_else(|| tr!("— kein Fahrzeug —"));
                let mut pick: Option<Option<String>> = None;
                platz(ui, 215.0);
                egui::ComboBox::from_id_source("activeveh")
                    .selected_text(format!("🚘 {label}"))
                    .width(190.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(self.active_vehicle.is_none(), tr!("— kein Fahrzeug —")).clicked() {
                            pick = Some(None);
                        }
                        for v in ecu_werkbank::vehicle::list() {
                            if ui.selectable_label(self.active_vehicle.as_deref() == Some(v.key.as_str()), v.label()).clicked() {
                                pick = Some(Some(v.key.clone()));
                            }
                        }
                        ui.separator();
                        if ui.button(tr!("+ Fahrzeug anlegen / bearbeiten…")).clicked() {
                            self.tab = Tab::Fahrzeuge;
                        }
                    })
                    .response
                    .on_hover_text(tr!("Aktives Fahrzeug: PDFs, Scans, Sicherungen und Aufzeichnungen landen in seinem Ordner. Wird eine Fahrgestellnummer gelesen, ordnet ecu-werkbank automatisch zu."));
                if let Some(p) = pick {
                    self.set_active_vehicle(p);
                }
                if ui.button(tr!("🔌 Verbindung")).on_hover_text(tr!("Adapter, Interface, Bitrate und Steuergeräte prüfen")).clicked() {
                    self.verbindung_pruefen();
                }
                if ui
                    .add_enabled(self.chk_job.is_none(), egui::Button::new(tr!("✅ Schnell-Check")))
                    .on_hover_text(tr!("Alle Fehlerspeicher, Readiness, Bordspannung, Vergleich und Wartung — mit Ampel und PDF"))
                    .clicked()
                {
                    self.start_check();
                }
                if self.chk_job.is_some() {
                    ui.spinner();
                    ui.add(egui::Label::new(egui::RichText::new(&self.chk_progress).weak()).extend());
                }
                let busy = self.pdf_job.is_some();
                if ui
                    .add_enabled(!busy, egui::Button::new(tr!("📄 Fehler-PDF")))
                    .on_hover_text(tr!("Alle Steuergeräte abfragen und den Fehlerspeicher als PDF speichern"))
                    .clicked()
                {
                    self.start_pdf();
                }
                if busy {
                    ui.spinner();
                    ui.add(egui::Label::new(egui::RichText::new(&self.pdf_progress).weak()).extend());
                }
                // Sprache + Hell/Dunkel rechtsbündig — passt es nicht mehr, in die nächste Zeile
                platz(ui, 200.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let icon = if self.dark { tr!("☀ Hell") } else { tr!("🌙 Dunkel") };
                    if ui.button(icon).clicked() {
                        self.dark = !self.dark;
                    }
                    // Sprache: Deutsch + eingebaute + eigene Dateien im Datenordner (sprachen/)
                    let aktuell = ecu_werkbank::i18n::sprache();
                    let liste = ecu_werkbank::i18n::verfuegbar();
                    let name = liste.iter().find(|(c, _)| *c == aktuell).map(|(_, n)| n.clone()).unwrap_or_else(|| aktuell.clone());
                    egui::ComboBox::from_id_source("sprache").selected_text(format!("🌐 {name}")).width(110.0).show_ui(ui, |ui| {
                        for (c, n) in &liste {
                            if ui.selectable_label(*c == aktuell, n).clicked() && *c != aktuell {
                                ecu_werkbank::i18n::set_sprache(c, true);
                            }
                        }
                    });
                });
            });
            ui.add_space(4.0);
            // Gruppen + Unterseiten
            let group = group_of(self.tab);
            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(self.tab == Tab::Start, egui::RichText::new(tr!("🏠 Start")).size(15.0)).clicked() {
                    self.tab = Tab::Start;
                }
                for (gi, (gname, pages)) in GROUPS.iter().enumerate() {
                    let sel = group == Some(gi);
                    if ui.selectable_label(sel, egui::RichText::new(ecu_werkbank::i18n::t(gname)).size(15.0).strong()).clicked() && !sel {
                        self.tab = pages[0].0;
                    }
                }
            });
            if let Some(gi) = group {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(8.0);
                    for (t, name) in GROUPS[gi].1 {
                        ui.selectable_value(&mut self.tab, *t, ecu_werkbank::i18n::t(name));
                    }
                });
            }
            ui.add_space(4.0);
        });

        // (b) Modul-Seitenleiste — nur dort, wo Steuergeräte gewählt werden
        if matches!(self.tab, Tab::Messwerte | Tab::Codierung) {
        egui::SidePanel::left("module").resizable(true).default_width(200.0).show(ctx, |ui| {
            ui.add_space(6.0);
            ui.strong(tr!("Steuergeräte"));
            ui.separator();
            let ecus = self.db.ecus();
            if ecus.is_empty() {
                ui.weak(tr!("Keine Labels geladen."));
            }
            let mut clear = false;
            for (addr, name) in &ecus {
                let sel = self.selected_ecu == Some(*addr);
                let label = format!("{addr:#04x}  {name}");
                if ui.selectable_label(sel, label).clicked() {
                    self.selected_ecu = if sel { None } else { Some(*addr) };
                    clear = true;
                }
            }
            if clear {
                self.tab = Tab::Messwerte;
            }
            ui.add_space(10.0);
            ui.separator();
            ui.weak(tr!("Scan-Treffer (Hardware):"));
            if self.scan_results.is_empty() {
                ui.weak("—");
            }
            for (tx, rx) in &self.scan_results {
                ui.monospace(tr!("● tx={tx} rx={rx}", tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx)));
            }
            if !self.tp_found.is_empty() {
                ui.add_space(6.0);
                ui.weak(tr!("TP 2.0 (ältere):"));
                let mut pick = None;
                for f in &self.tp_found {
                    let text = format!("● {:#04x} {}", f.address, if f.name.is_empty() { f.ident.first().map(String::as_str).unwrap_or("") } else { &f.name });
                    if ui.selectable_label(false, egui::RichText::new(text).monospace()).clicked() {
                        pick = Some(f.address);
                    }
                }
                if let Some(a) = pick {
                    self.tp_addr = format!("{a:#04x}");
                    self.tab = Tab::Tp20;
                }
            }
        });

        }

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                self.ui_hintergrund(ui);
                if self.status == READY {
                    let t = match self.transport.as_str() {
                        "demo" => tr!("{a} aktiv — alles simuliert, kein Adapter nötig.", a = ecu_werkbank::sim::modell().name()),
                        "socketcan" => {
                            let (st, _, lang) = self.adapter_state();
                            if st == Stufe::Gut { tr!("Bereit. {lang}", lang = lang) } else { tr!("{lang}  (Ohne Adapter ausprobieren: oben „Demo-Fahrzeug“ wählen.)", lang = lang) }
                        }
                        _ => tr!("Bereit."),
                    };
                    ui.add(egui::Label::new(t).truncate());
                } else {
                    // lange Meldungen (Pfade) kürzen, ganz beim Drüberfahren; Platz für die PDF-Knöpfe lassen
                    let rest = ui.available_width() - if self.pdf_last.is_some() { 170.0 } else { 0.0 };
                    ui.allocate_ui(egui::vec2(rest.max(80.0), 18.0), |ui| ui.add(egui::Label::new(&self.status).truncate()))
                        .inner
                        .on_hover_text(&self.status);
                }
                if let Some(p) = self.pdf_last.clone() {
                    if ui.small_button(tr!("📄 PDF öffnen")).clicked() {
                        open_path(&p, &mut self.status);
                    }
                    if let Some(dir) = p.parent() {
                        if ui.small_button(tr!("📂 Ordner")).clicked() {
                            open_path(dir, &mut self.status);
                        }
                    }
                }
            });
            ui.add_space(2.0);
        });

        // Jeder Reiter scrollt (senkrecht und, wenn etwas zu breit ist, waagerecht).
        // Innen bleibt die Fenstergröße der Rahmen: Text bricht um, Grafiken
        // füllen weiter den restlichen Platz.
        let tab = self.tab;
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both().id_source(("reiter", tab)).auto_shrink([false, false]).show(ui, |ui| self.reiter_zeigen(ui))
        });
    }
}


fn parse_hex(s: &str) -> Option<u16> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u16::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn parse_hex_bytes(s: &str) -> anyhow::Result<Vec<u8>> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !clean.len().is_multiple_of(2) {
        anyhow::bail!(tr!("gerade Anzahl Hex-Ziffern nötig"));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).map_err(|e| anyhow::anyhow!("{e}")))
        .collect()
}

/// Was im Reiter „Grundeinstellung / Stellglied“ gesendet werden soll —
/// erst nach ausdrücklicher Bestätigung.
#[derive(Debug, Clone, PartialEq)]
enum SvcPlan {
    Routine { tx: u16, rx: u16, sub: u8, id: u16, data: Vec<u8> },
    Io { tx: u16, rx: u16, did: u16, ctrl: u8, state: Vec<u8> },
}

impl SvcPlan {
    /// Klartext für die Rückfrage.
    fn text(&self) -> String {
        let hex = |d: &[u8]| if d.is_empty() { "—".to_string() } else { d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ") };
        match self {
            SvcPlan::Routine { tx, rx, sub, id, data } => tr!(
                "Routine {id} {was} (Sub {sub}) an Steuergerät {tx}/{rx}, Daten: {d}",
                id = format!("{id:#06x}"),
                was = ecu_werkbank::uds::routine_sub_text(*sub),
                sub = format!("{sub:#04x}"),
                tx = format!("{tx:#05x}"),
                rx = format!("{rx:#05x}"),
                d = hex(data)
            ),
            SvcPlan::Io { tx, rx, did, ctrl, state } => tr!(
                "Stellglied {did} ansteuern (Control {c}) an Steuergerät {tx}/{rx}, Wert: {d}",
                did = format!("{did:#06x}"),
                c = format!("{ctrl:#04x}"),
                tx = format!("{tx:#05x}"),
                rx = format!("{rx:#05x}"),
                d = hex(state)
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// TP 2.0 / KWP2000
// ---------------------------------------------------------------------------

/// Die GUI-Schrift hat kein ✓/✗ — für die Anzeige ersetzen.
fn gui_text(s: &str) -> String {
    s.replace('✓', "OK").replace('✗', "!!").replace('→', "»")
}

/// CSV-Aufzeichnung mit Zeitstempel-Namen im Ordner `dir` anlegen.
fn auto_csv(dir: Option<PathBuf>, prefix: &str, status: &mut String) -> Option<CsvLog> {
    let Some(dir) = dir else {
        *status = tr!("Kein Ordner für die Aufzeichnung verfügbar.");
        return None;
    };
    let path = dir.join(ecu_werkbank::vehicle::stamped(prefix, "csv"));
    match CsvLog::create(&path) {
        Ok(l) => {
            *status = tr!("Aufzeichnung läuft: {a}", a = path.display());
            Some(l)
        }
        Err(e) => {
            *status = tr!("CSV-Fehler: {e}", e = format!("{:#}", e));
            None
        }
    }
}

/// Datei oder Ordner mit dem Standardprogramm öffnen.
/// Fehler-Übersicht: neu / behoben / weiterhin seit dem letzten Scan, dazu häufige Fehler.
fn fehler_uebersicht(ui: &mut egui::Ui, u: &ecu_werkbank::history::Uebersicht) {
    ui.add_space(8.0);
    egui::CollapsingHeader::new(egui::RichText::new(tr!("Fehler-Übersicht")).strong())
        .id_source("fehler_uebersicht")
        .default_open(true)
        .show(ui, |ui| {
            let d = &u.diff;
            match &u.seit {
                Some(seit) => ui.weak(tr!("Letzter Scan {a} verglichen mit {seit} ({b} Scans gespeichert)", a = u.letzter, seit = seit, b = u.scans)),
                None => ui.weak(tr!("Erster Scan {a} — beim nächsten Scan siehst du hier, was neu oder behoben ist.", a = u.letzter)),
            };
            let zeile = |ui: &mut egui::Ui, st: Stufe, was: &str, it: &ecu_werkbank::history::DiffItem, extra: &str| {
                ui.horizontal_wrapped(|ui| {
                    dot(ui, stufe_color(st));
                    ui.label(egui::RichText::new(ecu_werkbank::i18n::t(was)).strong());
                    ui.monospace(&it.code);
                    if !it.text.is_empty() {
                        ui.label(&it.text);
                    }
                    ui.weak(format!("[{}]{}", it.module, ecu_werkbank::i18n::t(extra)));
                });
            };
            if d.new.is_empty() && d.gone.is_empty() && d.still.is_empty() {
                ui.horizontal(|ui| {
                    dot(ui, stufe_color(Stufe::Gut));
                    ui.label(tr!("Keine Fehler gespeichert."));
                });
            }
            for it in &d.new {
                zeile(ui, Stufe::Warnung, trn!("neu"), it, if it.recurring { trn!("  — war früher schon einmal da") } else { "" });
            }
            for it in &d.still {
                zeile(ui, Stufe::Hinweis, if u.seit.is_some() { trn!("weiterhin") } else { trn!("gespeichert") }, it, "");
            }
            for it in &d.gone {
                zeile(ui, Stufe::Gut, trn!("weg"), it, "");
            }
            let oft: Vec<_> = u.haeufig.iter().filter(|(_, n)| *n >= 3).collect();
            if !oft.is_empty() {
                ui.add_space(4.0);
                ui.weak(tr!("Kommt immer wieder:"));
                for (it, n) in oft.iter().take(6) {
                    zeile(ui, Stufe::Hinweis, &format!("{n}×"), it, "");
                }
            }
        });
}

enum FgMsg {
    Progress(String),
    Done(Result<Vec<ecu_werkbank::fehlergeruest::Geraet>, String>),
}

enum TlMsg {
    Progress(String),
    /// Protokolltext, gespeicherter Pfad oder Fehler
    Done(String, Result<String, String>),
}

/// Statuszeile ohne besondere Meldung (wird je nach Verbindung ersetzt).
const READY: &str = "Bereit.";

/// Programm, das Dateien und Ordner mit der Standard-Anwendung öffnet.
const OEFFNER: &str = if cfg!(windows) {
    "explorer"
} else if cfg!(target_os = "macos") {
    "open"
} else {
    "xdg-open"
};

fn open_path(p: &std::path::Path, status: &mut String) {
    if let Err(e) = std::process::Command::new(OEFFNER).arg(p).spawn() {
        *status = tr!("Konnte {a} nicht öffnen: {e}", a = p.display(), e = e);
    }
}

/// Zuletzt aktives Fahrzeug (gemerkt in <Datenordner>/aktives-fahrzeug).
fn load_active_vehicle() -> Option<String> {
    if let Ok(v) = std::env::var("VAG_FAHRZEUG") {
        return ecu_werkbank::vehicle::find(&v).map(|v| v.key);
    }
    let key = std::fs::read_to_string(ecu_werkbank::history::data_dir().join("aktives-fahrzeug")).ok()?;
    let key = key.trim().to_string();
    ecu_werkbank::vehicle::load(&key).map(|v| v.key)
}

// ---------------------------------------------------------------------------
// Mitschnitt
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Label-Editor
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Fehlerspeicher-Protokoll (PDF)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Scope: Live-Grafik mit mehreren Kurven
// ---------------------------------------------------------------------------

/// Datenquelle einer Kurve.
#[derive(Clone, PartialEq)]
enum ChanKind {
    Obd(u8),
    Kwp { addr: u8, block: u8, field: usize },
    Did { tx: u16, rx: u16, did: u16 },
    /// DBC-Signal aus dem laufenden CAN-Monitor (nur zuhören)
    Can { id: u32, signal: String },
    /// Aus einer CSV-Datei geladen (keine Live-Abfrage)
    File,
}

struct Series {
    name: String,
    unit: String,
    kind: ChanKind,
    points: std::collections::VecDeque<[f64; 2]>,
    visible: bool,
    error: Option<String>,
    /// Sollwert (aus Labels / eingebaut); None = keiner
    soll: Option<ecu_werkbank::limits::Range>,
    soll_checked: bool,
    alarmed: bool,
}

impl Series {
    fn new(name: &str, unit: &str, kind: ChanKind) -> Self {
        Self {
            name: name.into(),
            unit: unit.into(),
            kind,
            points: Default::default(),
            visible: true,
            error: None,
            soll: None,
            soll_checked: false,
            alarmed: false,
        }
    }

    fn label(&self) -> String {
        if self.unit.is_empty() { self.name.clone() } else { format!("{} [{}]", self.name, self.unit) }
    }

    fn state(&self) -> Option<Stufe> {
        let v = self.points.back()?[1];
        self.soll.map(|r| r.check(v))
    }

    fn push(&mut self, t: f64, v: f64) {
        self.points.push_back([t, v]);
        while self.points.len() > 20_000 {
            self.points.pop_front();
        }
    }
}

/// CSV-Aufzeichnung als Kurven laden.
fn read_csv_series(path: &std::path::Path) -> anyhow::Result<Vec<Series>> {
    Ok(ecu_werkbank::csvlog::read(path)?
        .into_iter()
        .map(|c| {
            let mut s = Series::new(&c.name, &c.unit, ChanKind::File);
            s.points = c.points.into_iter().collect();
            s
        })
        .collect())
}


// ---------------------------------------------------------------------------
// Historie
// ---------------------------------------------------------------------------

/// Umgebungsbedingungen eines Fehlers zum Aufklappen (nichts, wenn keine da sind).
fn umgebung_zeigen(ui: &mut egui::Ui, id: impl std::hash::Hash, umgebung: &[(String, String)]) {
    if umgebung.is_empty() {
        return;
    }
    ui.indent(egui::Id::new(&id).with("einzug"), |ui| {
        egui::CollapsingHeader::new(tr!("Umgebungsbedingungen ({a})", a = umgebung.len())).id_source(egui::Id::new(id)).show(ui, |ui| {
            egui::Grid::new(ui.id().with("umg")).striped(true).spacing([14.0, 1.0]).show(ui, |ui| {
                for (k, v) in umgebung {
                    ui.weak(k);
                    ui.monospace(v);
                    ui.end_row();
                }
            });
        });
    });
}

// ---------------------------------------------------------------------------
// Codierungs-Sicherung
// ---------------------------------------------------------------------------

/// Anzeige einer Codierungs-Sicherung: Datum + Fahrzeug.
fn backup_label(b: &ecu_werkbank::backup::CodingBackup) -> String {
    let who = ecu_werkbank::vehicle::load(&b.vehicle)
        .map(|v| v.label())
        .or_else(|| b.vin.clone())
        .unwrap_or_else(|| tr!("ohne Fahrzeug"));
    format!("{}  {who}", b.created)
}

// ---------------------------------------------------------------------------
// Fahrzeuge (Fahrzeugakte)
// ---------------------------------------------------------------------------

trait IfEmpty {
    fn if_empty(self, other: &str) -> String;
}
impl IfEmpty for String {
    fn if_empty(self, other: &str) -> String {
        if self.is_empty() { other.to_string() } else { self }
    }
}


/// Desktop-Benachrichtigung + Ton (wenn verfügbar).
fn notify(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send").args(["-a", "ECUWerkbank", title, body]).spawn();
    let _ = std::process::Command::new("canberra-gtk-play").args(["-i", "dialog-warning"]).spawn().or_else(|_| {
        std::process::Command::new("paplay").arg("/usr/share/sounds/freedesktop/stereo/dialog-warning.oga").spawn()
    });
}

/// Rundinstrument für eine Kurve.
fn draw_gauge(ui: &mut egui::Ui, s: &Series) {
    use egui::{pos2, vec2, Color32, Stroke};
    let (rect, _) = ui.allocate_exact_size(vec2(190.0, 170.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let dark = ui.visuals().dark_mode;
    p.rect_filled(rect.shrink(3.0), 12.0, if dark { Color32::from_gray(30) } else { Color32::from_gray(238) });
    let c = rect.center() + vec2(0.0, 12.0);
    let r = 64.0;
    // Bereich: Sollwert (± 25 %) oder beobachtete Werte
    let (omin, omax) = s.points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q[1]), b.max(q[1])));
    let (mut lo, mut hi) = match s.soll {
        Some(ecu_werkbank::limits::Range { min: Some(a), max: Some(b) }) => {
            let pad = (b - a).abs() * 0.25;
            (a - pad, b + pad)
        }
        _ if omin.is_finite() => {
            let pad = ((omax - omin).abs() * 0.15).max(1.0);
            (if omin >= 0.0 { 0.0f64.max(omin - pad) } else { omin - pad }, omax + pad)
        }
        _ => (0.0, 1.0),
    };
    if let Some(rg) = s.soll {
        if let Some(m) = rg.max {
            hi = hi.max(m * 1.05);
        }
        if let Some(m) = rg.min {
            lo = lo.min(m - (m.abs() * 0.05));
        }
    }
    let frac = |v: f64| ((v - lo) / (hi - lo).max(1e-9)).clamp(0.0, 1.0) as f32;
    let ang = |f: f32| std::f32::consts::PI * (1.25 - 1.5 * f);
    let pt = |a: f32, rr: f32| pos2(c.x + rr * a.cos(), c.y - rr * a.sin());
    let arc = |f0: f32, f1: f32, col: Color32, w: f32| {
        let n = 40;
        let pts: Vec<egui::Pos2> = (0..=n).map(|i| pt(ang(f0 + (f1 - f0) * i as f32 / n as f32), r)).collect();
        p.add(egui::Shape::line(pts, Stroke::new(w, col)));
    };
    arc(0.0, 1.0, Color32::from_gray(if dark { 70 } else { 190 }), 9.0);
    if let Some(rg) = s.soll {
        let (a, b) = rg.as_pair();
        arc(frac(a.max(lo)), frac(b.min(hi)), Color32::from_rgb(60, 150, 85), 9.0);
    }
    let cur = s.points.back().map(|q| q[1]);
    let col = s.state().map(stufe_color).unwrap_or(if dark { Color32::from_rgb(130, 190, 255) } else { Color32::from_rgb(30, 90, 170) });
    if let Some(v) = cur {
        p.line_segment([c, pt(ang(frac(v)), r - 6.0)], Stroke::new(3.0_f32, col));
    }
    p.circle_filled(c, 5.0, col);
    let txt = if dark { Color32::WHITE } else { Color32::BLACK };
    p.text(pos2(rect.center().x, rect.top() + 14.0), egui::Align2::CENTER_CENTER, &s.name, egui::FontId::proportional(13.0), txt);
    let value = cur.map(|v| if v.abs() >= 100.0 { format!("{v:.0}") } else { format!("{v:.1}") }).unwrap_or_else(|| "—".into());
    p.text(c + vec2(0.0, 34.0), egui::Align2::CENTER_CENTER, value, egui::FontId::proportional(26.0), col);
    p.text(c + vec2(0.0, 56.0), egui::Align2::CENTER_CENTER, &s.unit, egui::FontId::proportional(12.0), Color32::GRAY);
    let fmt = |v: f64| if v.abs() >= 100.0 { format!("{v:.0}") } else { format!("{v:.1}") };
    p.text(pt(ang(0.0), r + 2.0) + vec2(8.0, 12.0), egui::Align2::CENTER_CENTER, fmt(lo), egui::FontId::proportional(10.0), Color32::GRAY);
    p.text(pt(ang(1.0), r + 2.0) + vec2(-8.0, 12.0), egui::Align2::CENTER_CENTER, fmt(hi), egui::FontId::proportional(10.0), Color32::GRAY);
}

// ---------------------------------------------------------------------------
// Zielbereich: Fahrer-Assistent, Steuergerät-Stellfunktion, Simulation
// ---------------------------------------------------------------------------

/// Eingabe eines Musters (halten / schwanken / Rampe).
#[derive(Clone, Copy)]
struct MusterEdit {
    mode: u8,
    a: f64,
    b: f64,
    c: f64,
}

impl MusterEdit {
    fn muster(&self) -> ecu_werkbank::control::Muster {
        use ecu_werkbank::control::Muster;
        match self.mode {
            1 => Muster::Schwanken { min: self.a.min(self.b), max: self.a.max(self.b), periode: self.c.max(0.5) },
            2 => Muster::Rampe { von: self.a, bis: self.b, dauer: self.c.max(0.5) },
            _ => Muster::Halten(self.a),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, id: &str, unit: &str) {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_source(id)
                .selected_text(ecu_werkbank::i18n::t([trn!("halten"), trn!("schwanken"), trn!("Rampe")][self.mode as usize]))
                .width(100.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.mode, 0, tr!("halten"));
                    ui.selectable_value(&mut self.mode, 1, tr!("schwanken"));
                    ui.selectable_value(&mut self.mode, 2, tr!("Rampe"));
                });
            let speed = (self.a.abs() / 100.0).max(0.1);
            match self.mode {
                0 => {
                    ui.label(tr!("Wert:"));
                    ui.add(egui::DragValue::new(&mut self.a).speed(speed).suffix(format!(" {unit}")));
                }
                1 => {
                    ui.label(tr!("zwischen"));
                    ui.add(egui::DragValue::new(&mut self.a).speed(speed));
                    ui.label(tr!("und"));
                    ui.add(egui::DragValue::new(&mut self.b).speed(speed).suffix(format!(" {unit}")));
                    ui.label(tr!("Periode"));
                    ui.add(egui::DragValue::new(&mut self.c).speed(0.1).range(0.5..=600.0).suffix(" s"));
                }
                _ => {
                    ui.label(tr!("von"));
                    ui.add(egui::DragValue::new(&mut self.a).speed(speed));
                    ui.label(tr!("bis"));
                    ui.add(egui::DragValue::new(&mut self.b).speed(speed).suffix(format!(" {unit}")));
                    ui.label(tr!("in"));
                    ui.add(egui::DragValue::new(&mut self.c).speed(0.1).range(0.5..=600.0).suffix(" s"));
                }
            }
        });
    }
}

/// Ton ohne Desktop-Meldung (wenn verfügbar).
fn beep() {
    let _ = std::process::Command::new("canberra-gtk-play").args(["-i", "bell"]).spawn().or_else(|_| {
        std::process::Command::new("paplay").arg("/usr/share/sounds/freedesktop/stereo/bell.oga").spawn()
    });
}

/// Großer Balken mit grüner Zielzone und Istwert-Markierung + Richtungspfeil.
fn draw_band(ui: &mut egui::Ui, ist: f64, soll: f64, tol: f64, unit: &str) {
    use egui::{pos2, vec2, Color32, Stroke};
    use ecu_werkbank::control::{bewerten, Richtung};
    let (r, st) = bewerten(ist, soll, tol);
    let col = stufe_color(st);
    let w = ui.available_width().min(640.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, 150.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let dark = ui.visuals().dark_mode;
    let txt = if dark { Color32::WHITE } else { Color32::BLACK };
    // Istwert groß
    p.text(pos2(rect.left() + 10.0, rect.top() + 30.0), egui::Align2::LEFT_CENTER, format!("{ist:.0} {unit}"), egui::FontId::proportional(40.0), col);
    // Richtung
    let (label, up) = match r {
        Richtung::Mehr => (tr!("MEHR GAS"), Some(true)),
        Richtung::Weniger => (tr!("WENIGER GAS"), Some(false)),
        Richtung::Passt => (tr!("IM BEREICH"), None),
    };
    let ax = rect.right() - 210.0;
    let ay = rect.top() + 30.0;
    match up {
        Some(true) => {
            p.add(egui::Shape::convex_polygon(vec![pos2(ax, ay + 16.0), pos2(ax + 18.0, ay - 16.0), pos2(ax + 36.0, ay + 16.0)], col, Stroke::NONE));
        }
        Some(false) => {
            p.add(egui::Shape::convex_polygon(vec![pos2(ax, ay - 16.0), pos2(ax + 36.0, ay - 16.0), pos2(ax + 18.0, ay + 16.0)], col, Stroke::NONE));
        }
        None => {
            p.circle_filled(pos2(ax + 18.0, ay), 15.0, col);
        }
    }
    p.text(pos2(ax + 46.0, ay), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(22.0), col);
    // Balken: Bereich ±4·Toleranz um das Ziel
    let bar = egui::Rect::from_min_size(pos2(rect.left() + 10.0, rect.top() + 75.0), vec2(w - 20.0, 30.0));
    p.rect_filled(bar, 6.0, Color32::from_gray(if dark { 55 } else { 210 }));
    let span = tol * 4.0;
    let x = |v: f64| bar.left() + (((v - soll) / span + 0.5).clamp(0.0, 1.0) as f32) * bar.width();
    let zone = egui::Rect::from_min_max(pos2(x(soll - tol), bar.top()), pos2(x(soll + tol), bar.bottom()));
    p.rect_filled(zone, 4.0, Color32::from_rgb(55, 140, 75));
    p.line_segment([pos2(x(soll), bar.top() - 4.0), pos2(x(soll), bar.bottom() + 4.0)], Stroke::new(2.0_f32, txt));
    let mx = x(ist);
    p.add(egui::Shape::convex_polygon(vec![pos2(mx - 9.0, bar.top() - 12.0), pos2(mx + 9.0, bar.top() - 12.0), pos2(mx, bar.top() + 2.0)], col, Stroke::NONE));
    p.line_segment([pos2(mx, bar.top()), pos2(mx, bar.bottom())], Stroke::new(4.0_f32, col));
    p.text(pos2(bar.center().x, bar.bottom() + 16.0), egui::Align2::CENTER_CENTER, tr!("Ziel {soll} ± {tol} {unit}", soll = format!("{:.0}", soll), tol = format!("{:.0}", tol), unit = unit), egui::FontId::proportional(14.0), Color32::GRAY);
}


// ---------------------------------------------------------------------------
// Diagnose: Prüfergebnisse (Mode 06), Gemisch-Assistent; Trends; Reparaturen
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Verbindung, Gebrauchtwagen-Check, geführte Fehlersuche, Reparatur-Modus, Lernhilfe
// ---------------------------------------------------------------------------


/// Verläufe eines Fahrzeugs: Kilometerstand und Fehler je Scan (aus der Historie).
fn verlauf_diagramme(ui: &mut egui::Ui, v: &ecu_werkbank::vehicle::Vehicle) {
    use ecu_werkbank::blick::datum_von_tag;
    let b = ecu_werkbank::blick::blick(v);
    let plot = |ui: &mut egui::Ui, id: &str, titel: &str, pts: Vec<[f64; 2]>, einheit: &'static str, ab_null: bool| {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.vertical(|ui| {
            ui.set_width(380.0);
            ui.weak(ecu_werkbank::i18n::t(titel));
            if pts.len() < 2 {
                ui.weak(tr!("Ab zwei Scans siehst du hier den Verlauf."));
                return;
            }
            let max = pts.iter().map(|p| p[1]).fold(0.0, f64::max);
            let mut pl = egui_plot::Plot::new(id);
            if ab_null {
                pl = pl.include_y(0.0).include_y(max + 1.0).y_axis_formatter(|m, _| if m.value.fract() == 0.0 { format!("{:.0}", m.value) } else { String::new() });
            } else {
                pl = pl.y_axis_formatter(|m, _| ecu_werkbank::report::group_thousands(m.value.max(0.0) as u32));
            }
            pl
                .height(110.0)
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .show_axes([true, true])
                .x_axis_formatter(|m, _| datum_von_tag(m.value.round() as i64).get(3..).unwrap_or("").to_string())
                .label_formatter(move |_, p| format!("{}\n{:.0} {einheit}", datum_von_tag(p.x.round() as i64), p.y))
                .show(ui, |pu| {
                    pu.line(egui_plot::Line::new(egui_plot::PlotPoints::from(pts.clone())).width(2.0_f32));
                    pu.points(egui_plot::Points::new(egui_plot::PlotPoints::from(pts)).radius(3.0_f32));
                });
            });
        });
    };
    if b.km_verlauf.len() >= 2 || b.fehler_verlauf.len() >= 2 {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            plot(ui, "blick_km", trn!("Kilometerverlauf"), b.km_verlauf.iter().map(|&(t, k)| [t as f64, f64::from(k)]).collect(), "km", false);
            plot(ui, "blick_fehler", trn!("Fehler je Scan"), b.fehler_verlauf.iter().map(|&(t, n)| [t as f64, n as f64]).collect(), trn!("Fehler"), true);
        });
    }
}

/// Gruppen der Oberfläche mit ihren Unterseiten.
const GROUPS: &[(&str, &[(Tab, &str)])] = &[
    // Diagnose: wie beim Werkstattgerät — Steuergerät wählen, dann Funktion
    (trn!("🔧 Diagnose"), &[
        (Tab::Steuergeraete, trn!("Steuergeräte")),
        (Tab::Tp20, trn!("Ältere Steuergeräte (TP 2.0)")),
        (Tab::Obd, trn!("OBD-II")),
        (Tab::Codierung, trn!("Codierung")),
        (Tab::Ident, trn!("Erweiterte Identifikation")),
        (Tab::Diagnose, trn!("Prüfen & Fehlersuche")),
        (Tab::Sicherheit, trn!("Sicherheit & Assistenz")),
        (Tab::Fremd, trn!("Andere Marken")),
    ]),
    (trn!("📈 Messen"), &[
        (Tab::Scope, trn!("Live-Grafik")),
        (Tab::Ziel, trn!("Zielbereich")),
        (Tab::Batterie, trn!("Batterie")),
        (Tab::Dpf, trn!("Partikelfilter")),
        (Tab::Motor, trn!("Motor-Checks")),
        (Tab::Rekorder, trn!("Fahrt-Rekorder")),
        (Tab::Messwerte, trn!("Rohwerte umrechnen")),
    ]),
    (trn!("🚘 Fahrzeug"), &[
        (Tab::Fahrzeuge, trn!("Fahrzeuge & Wartung")),
        (Tab::ServiceAnzeige, trn!("Service-Anzeige")),
        (Tab::Historie, trn!("Historie & Trends")),
        (Tab::Software, trn!("Softwarestände")),
        (Tab::Sicherung, trn!("Codier-Sicherung")),
        (Tab::Nachruestung, trn!("Nachrüstung")),
    ]),
    (trn!("🛠 Werkzeuge"), &[
        (Tab::Testlauf, trn!("Testlauf")),
        (Tab::CanMonitor, trn!("CAN-Monitor")),
        (Tab::Mitschnitt, trn!("Mitschnitt & Lernhilfe")),
        (Tab::Namen, trn!("Eigene Namen")),
        (Tab::Editor, trn!("Label-Editor")),
        (Tab::Wissen, trn!("Fehlerbilder")),
        (Tab::GrundLexikon, trn!("Grundeinstellungen (Referenz)")),
        (Tab::Service, trn!("Experten: Routine & Stellglied (UDS)")),
    ]),
];

fn group_of(t: Tab) -> Option<usize> {
    GROUPS.iter().position(|(_, pages)| pages.iter().any(|(p, _)| *p == t))
}


// ---------------------------------------------------------------------------
// Softwarestände, Nachrüstung, Partikelfilter, Fehlerbilder, Sicherheit
// ---------------------------------------------------------------------------
