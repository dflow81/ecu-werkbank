//! GUI: App-Kern (Aufbau, Backend, Scan/DTC-Läufe, Fahrzeug-Steuergeräteliste, Service senden)

use super::*;

impl App {
    pub(super) fn new() -> Self {
        let labels_dir = ecu_werkbank::labels::default_dir();
        let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
        // Optionaler Start-Reiter (nur für Demo/Screenshots): VAG_TAB=messwerte|labels
        let tabsel = std::env::var("VAG_TAB").unwrap_or_default();
        let (tab, _vorbelegen) = match tabsel.as_str() {
            "messwerte" => (Tab::Scope, true),
            "scope" | "armaturen" => (Tab::Scope, false),
            "codierung" => (Tab::Codierung, false),
            "service" => (Tab::Service, false),
            "tp20" | "blockscan" => (Tab::Tp20, false),
            "obd" => (Tab::Obd, false),
            "mitschnitt" => (Tab::Mitschnitt, false),
            "canmonitor" | "canmonitor-grafik" => (Tab::CanMonitor, false),
            "skripte" => (Tab::Skripte, false),
            "datenbank" | "datenbank-vorschau" | "datenbank-inhalt" => (Tab::Datenbank, false),
            "editor" => (Tab::Datenbank, false),
            "fahrzeuge" => (Tab::Fahrzeuge, false),
            "batterie" => (Tab::Batterie, false),
            "motor" => (Tab::Motor, false),
            "rekorder" => (Tab::Rekorder, false),
            "software" => (Tab::Software, false),
            "nachruestung" => (Tab::Nachruestung, false),
            "dpf" => (Tab::Dpf, false),
            "service-anzeige" | "inspektion" | "serviceanzeige" => (Tab::ServiceAnzeige, false),
            "wissen" | "fehlertexte" => (Tab::Wissen, false),
            "testlauf" => (Tab::Testlauf, false),
            "sicherheit" => (Tab::Sicherheit, false),
            "ziel" | "stellen" => (Tab::Ziel, false),
            "diagnose" | "gemisch" | "kaufcheck" => (Tab::Diagnose, false),
            "lernen" => (Tab::Mitschnitt, false),
            "historie" => (Tab::Historie, false),
            "sicherung" => (Tab::Sicherung, false),
            "labels" | "namen" => (Tab::Datenbank, false),
            "ident" => (Tab::Ident, false),
            "lexikon" | "grundlexikon" => (Tab::Wissen, false),
            "tp20-block" | "favoriten" | "grundeinstellung" | "anpassung" => (Tab::Tp20, false),
            "service-frage" => (Tab::Service, false),
            "editor-scan" => (Tab::Datenbank, false),
            "namen-scan" => (Tab::Tp20, false),
            "steuergeraete-wahl" => (Tab::Steuergeraete, false),
            "steuergeraete" => (Tab::Steuergeraete, false),
            _ => (Tab::Start, false),
        };
        let mut startup_status = READY.to_string();
        if let Some(v) = load_active_vehicle().and_then(|k| ecu_werkbank::vehicle::load(&k)) {
            let due: Vec<String> = v
                .wartung_offen()
                .into_iter()
                .filter(|(_, f)| f.stufe == Stufe::Warnung)
                .map(|(w, f)| format!("{} ({})", w.name, f.text))
                .collect();
            if !due.is_empty() {
                startup_status = tr!("⚠ Wartung fällig für {a}: {b}", a = v.label(), b = due.join("; "));
            }
        }
        let (code_ecu, code_bytes) = if tabsel == "codierung" {
            (Some(0x09u16), "110007".to_string())
        } else {
            (None, String::new())
        };
        let demo_ecu = if tabsel == "module" { Some(0x09u16) } else { None };
        let scope_on = tabsel == "scope" || tabsel == "armaturen";
        // Für Demo-Aufnahmen (Scope/Armaturen): schwankende Werte, damit sich etwas bewegt.
        if scope_on {
            let _ = ecu_werkbank::sim::apply_spec(
                "drehzahl=schwanken:820:4600:5;geschwindigkeit=schwanken:0:135:7;kuehlmittel=schwanken:82:94:22;last=schwanken:12:88:4;spannung=schwanken:13.8:14.5:6",
            );
        }
        Self {
            labels_dir,
            db,
            iface: std::env::var("VAG_IFACE").unwrap_or_else(|_| "can0".into()),
            transport: match std::env::var("VAG_TRANSPORT").unwrap_or_else(|_| ecu_werkbank::transport::STANDARD_TRANSPORT.into()) {
                // VAG_TRANSPORT=golf6 → Simulation Golf 6
                t if t != "demo" && ecu_werkbank::sim::Modell::parse(&t).is_some() => {
                    ecu_werkbank::sim::set_modell(ecu_werkbank::sim::Modell::parse(&t).unwrap_or_default());
                    "demo".into()
                }
                t => t,
            },
            port: ecu_werkbank::transport::STANDARD_PORT.into(),
            baud: 38400,
            tab,
            dark: std::env::var("VAG_HELL").is_err(), // VAG_HELL=1: helles Design (Handbuch-Bilder)
            fm_job: None,
            fm_progress: String::new(),
            fm_result: None,
            fm_pdf: None,
            nf_job: None,
            nf_progress: String::new(),
            nf_pdf: None,
            fm_confirm: None,
            erk_an: !ecu_werkbank::ablage::existiert(ecu_werkbank::history::data_dir().join("erkennung-aus")),
            erk_job: None,
            erk_zuletzt: None,
            erk_zaehler: None,
            erk_fund: None,
            erk_fuer: String::new(),
            selected_ecu: demo_ecu,
            scan_results: Vec::new(),
            dtc_tx: "0x7E0".into(),
            dtc_rx: "0x7E8".into(),
            dtc_results: Vec::new(),
            dtc_info: String::new(),
            fz_sel: None,
            fz_lines: Vec::new(),
            fz_info: String::new(),
            fz_clear_confirm: false,
            ad_cache: std::cell::RefCell::new(None),
            code_ecu,
            code_bytes,
            code_tx: "0x7E0".into(),
            code_rx: "0x7E8".into(),
            code_read: None,
            code_confirm: false,
            code_undo: Vec::new(),
            code_quelle: None,
            code_bk: None,
            code_texte: ecu_werkbank::codiertexte::Texte::laden(),
            code_edit: None,
            code_nur_benannt: false,
            code_tp20: None,
            code_kurz: None,
            code_kurz_neu: String::new(),
            code_kurz_ok: false,
            sw_last: None,
            sw_fahrzeug: None,
            sw_diff: Vec::new(),
            ident_tx: "0x7E0".into(),
            ident_result: None,
            gek_system: String::new(),
            gek_suche: String::new(),
            nr_name: String::new(),
            nr_notiz: String::new(),
            nr_vorher: None,
            nr_result: Vec::new(),
            nr_job: None,
            dpf_vals: None,
            svz_vals: Vec::new(),
            svz_lines: Vec::new(),
            svz_ziel: Default::default(),
            svz_kandidaten: Vec::new(),
            svz_neu: ecu_werkbank::service::INTERVALL_VORSCHLAG.map(|(_, n)| format!("{n:.0}")),
            svz_job: None,
            svz_fortschritt: (0, 0),
            svz_vorschlag: Vec::new(),
            svz_bestaetigen: false,
            svz_wartung: true,
            svz_gelesen_fuer: None,
            ft_inline: None,
            dpf_did: Default::default(),
            dpf_fak: [1.0; 4],
            dpf_tx: "0x7E0".into(),
            dpf_grenze: 24.0,
            dpf_loaded_for: None,
            ws_code: String::new(),
            ws_modell: String::new(),
            ws_text: String::new(),
            ds: ecu_werkbank::datensicherung::laden(),
            ds_job: None,
            ds_geprueft: false,
            fg_job: None,
            fg_progress: String::new(),
            fg_geraete: Vec::new(),
            fg_texte: Default::default(),
            fg_nur_leer: false,
            si_job: None,
            gw_vb_job: None,
            gw_vb: None,
            tl_km: String::new(),
            tl_job: None,
            tl_progress: String::new(),
            tl_text: String::new(),
            si_report: None,
            scope_did: "0xF40C".into(),
            scope_tx: "0x7E0".into(),
            scope_rx: "0x7E8".into(),
            scope_running: false,
            scope_series: if scope_on {
                vec![Series::new("Drehzahl", "1/min", ChanKind::Obd(0x0C)), Series::new("Geschwindigkeit", "km/h", ChanKind::Obd(0x0D))]
            } else {
                Vec::new()
            },
            scope_start: std::time::Instant::now(),
            scope_last: std::time::Instant::now(),
            scope_interval_ms: if scope_on { 120 } else { 250 },
            scope_window: 60.0,
            scope_normalize: false,
            scope_add_obd: 0x0C,
            scope_add_kwp_addr: "0x01".into(),
            scope_add_kwp_block: "1".into(),
            scope_kwp_ch: HashMap::new(),
            scope_csv: None,
            scope_file: None,
            scope_gauges: tabsel == "armaturen",
            ansicht_name: String::new(),
            scope_alarm_on: true,
            scope_alarm_text: String::new(),
            scope_alarm_until: None,
            trip_on: false,
            trip_active: false,
            trip_last_check: std::time::Instant::now(),
            trip_low_since: None,
            web_server: None,
            web_data: Default::default(),
            svc_tx: "0x7E0".into(),
            svc_rx: "0x7E8".into(),
            routine_id: "0x0203".into(),
            routine_sub: "1".into(),
            routine_data: String::new(),
            io_did: "0x1234".into(),
            io_control: "0x03".into(),
            io_state: "01".into(),
            svc_result: String::new(),
            svc_plan: None,
            tp_addr: "0x01".into(),
            tp_block: "1".into(),
            tp_live: false,
            tp_channel: None,
            tp_csv: None,
            obd_vin: String::new(),
            obd_readiness: None,
            obd_stand: None,
            obd_dtcs: Vec::new(),
            obd_supported: Vec::new(),
            obd_selected: [0x0C, 0x0D, 0x05, 0x04].into_iter().collect(),
            obd_live: false,
            obd_values: Vec::new(),
            obd_ready_live: false,
            obd_ready_last: std::time::Instant::now(),
            obd_freeze: None,
            obd_last: std::time::Instant::now(),
            obd_csv: None,
            obd_confirm_clear: false,
            log_path: String::new(),
            log_lines: Vec::new(),
            log_report: String::new(),
            log_filter: String::new(),
            log_text: String::new(),
            log_cmp: None,
            learn: Vec::new(),
            learn_addr: "0x01".into(),
            learn_name: tr!("Motor (gelernt)"),
            con_result: None,
            rec_job: None,
            clr_confirm: false,
            kc_job: None,
            kc_progress: String::new(),
            kc_result: None,
            kc_note: String::new(),
            gf_code: String::new(),
            gf_done: Vec::new(),
            gf_for: String::new(),
            gf_note: String::new(),
            rp_before: None,
            rp_arbeiten: String::new(),
            rp_job: None,
            rp_confirm_clear: false,
            rp_cleared: Vec::new(),
            pdf_job: None,
            rep_cache: HashMap::new(),
            rep_cache_for: None,
            m6_results: Vec::new(),
            m6_summary: Vec::new(),
            gm_phase: 0,
            gm_seconds: 20,
            gm_start: std::time::Instant::now(),
            gm_last: std::time::Instant::now(),
            gm_idle: Vec::new(),
            gm_high: Vec::new(),
            gm_now: None,
            gm_result: Vec::new(),
            tr_sel: String::new(),
            zl_pid: 0x0C,
            zl_m: MusterEdit { mode: 0, a: 2500.0, b: 3000.0, c: 10.0 },
            zl_tol: 150.0,
            zl_running: false,
            zl_start: std::time::Instant::now(),
            zl_last: std::time::Instant::now(),
            zl_ist: Vec::new(),
            zl_soll: Vec::new(),
            zl_n: 0,
            zl_in: 0,
            zl_beep: true,
            zl_out_since: None,
            zl_last_beep: std::time::Instant::now(),
            zl_now: None,
            st_sel: 0,
            st_m: MusterEdit { mode: 0, a: 1200.0, b: 1500.0, c: 8.0 },
            st_seconds: 60,
            st_confirm: false,
            st_regler: None,
            st_last: std::time::Instant::now(),
            st_ist: Vec::new(),
            st_soll: Vec::new(),
            sim_param: 0,
            sim_m: MusterEdit { mode: 1, a: 2000.0, b: 3000.0, c: 10.0 },
            chk_job: None,
            chk_progress: String::new(),
            chk_result: None,
            pdf_km: String::new(),
            bat_job: None,
            bat_points: Vec::new(),
            bat_result: None,
            bat_seconds: 15,
            hist_note: String::new(),
            hist_note_for: None,
            wt_new_name: String::new(),
            wt_new_km: String::new(),
            wt_new_mon: String::new(),
            teil_name: String::new(),
            teil_nr: String::new(),
            teil_km: String::new(),
            teil_datum: String::new(),
            teil_notiz: String::new(),
            teil_preis: String::new(),
            tk_km: String::new(),
            tk_liter: String::new(),
            tk_preis: String::new(),
            tk_datum: String::new(),
            tk_voll: true,
            tk_notiz: String::new(),
            veh_km: String::new(),
            km_job: None,
            km_lines: Vec::new(),
            active_vehicle: load_active_vehicle(),
            veh_sel: None,
            veh_edit: Default::default(),
            veh_new_plate: String::new(),
            veh_new_name: String::new(),
            veh_new_vin: String::new(),
            pdf_progress: String::new(),
            pdf_note: String::new(),
            pdf_last: None,
            hist_vehicle: String::new(),
            hist_scans: Vec::new(),
            hist_a: None,
            hist_b: None,
            bk_job: None,
            bk_progress: String::new(),
            bk_list: Vec::new(),
            bk_fuer: None,
            bk_sel: None,
            bk_cmp: None,
            bk_confirm: None,
            tp_last_read: std::time::Instant::now(),
            tp_lines: Vec::new(),
            tp_block_values: Vec::new(),
            tp_block_raw: Vec::new(),
            motor: motor::State::default(),
            bs_job: None,
            bs_at: 0,
            bs_result: None,
            bs_namen: Default::default(),
            sg_bestaetigt: false,
            sg_job: None,
            sg_log: Vec::new(),
            sg_zeigen: false,
            an_kanaele: "1".into(),
            an_result: Vec::new(),
            an_wissen: None,
            diesel_cache: Default::default(),
            code_wissen: None,
            an_zeigen: false,
            an_kanal: String::new(),
            an_wert: String::new(),
            an_ok: false,
            bn_nur_offen: false,
            bn_suche: String::new(),
            bn_roh_aus: true,
            tp_block_key: (0, 0),
            mw_texte: ecu_werkbank::messwerttexte::Texte::laden(),
            favoriten: ecu_werkbank::favoriten::alle(),
            ge_gruppe: String::new(),
            ge_neu: None,
            ge_frage: false,
            ge_job: None,
            ge_laeuft: false,
            ge_werte: Vec::new(),
            ge_status: String::new(),
            ge_zeigen: false,
            foto: std::env::var("VAG_FOTO").ok().map(|p| {
                let warten = std::env::var("VAG_FOTO_WARTEN").ok().and_then(|s| s.parse::<f64>().ok()).unwrap_or(3.0);
                (PathBuf::from(p), std::time::Duration::from_secs_f64(warten), std::time::Instant::now(), false)
            }),
            film: std::env::var("VAG_FILM").ok().and_then(|s| {
                // Format: "ordner:anzahl:intervall_ms"
                let teile: Vec<&str> = s.rsplitn(3, ':').collect(); // [intervall, anzahl, ordner]
                let ordner = teile.get(2)?;
                let total = teile.get(1)?.parse().ok()?;
                let ms: u64 = teile.first()?.parse().ok()?;
                let warten = std::env::var("VAG_FOTO_WARTEN").ok().and_then(|s| s.parse::<f64>().ok()).unwrap_or(2.0);
                let jetzt = std::time::Instant::now();
                Some(super::FilmState {
                    dir: PathBuf::from(*ordner),
                    total,
                    interval: std::time::Duration::from_millis(ms),
                    warten: std::time::Duration::from_secs_f64(warten),
                    start: jetzt,
                    naechste: jetzt,
                    zaehler: 0,
                    wartet_bild: false,
                })
            }),
            mw_edit: None,
            tp_sweep: None,
            tp_sweep_at: 0,
            tp_found: Vec::new(),
            tp20_bekannt: None,
            voll_scan: false,
            tp_autostart: tabsel == "tp20",
            demo_autostart: tabsel.clone(),
            bus: Default::default(),
            cm: Default::default(),
            sk: Default::default(),
            dbs: Default::default(),
            firmenkopf: None,
            status: startup_status,
        }
    }

    pub(super) fn backend(&self) -> Backend {
        match self.transport.as_str() {
            "elm327" => Backend::Elm327Serial {
                port: self.port.clone(),
                baud: self.baud,
            },
            "wifi" => Backend::Elm327Tcp {
                addr: self.port.clone(),
            },
            "demo" => Backend::Demo,
            _ => Backend::SocketCan {
                iface: self.iface.clone(),
            },
        }
    }

    pub(super) fn reload_labels(&mut self) {
        self.db = LabelDb::load_dir(&self.labels_dir).unwrap_or_default();
        ecu_werkbank::dtc_db::fehlerarten_neu_laden();
        self.an_wissen = None;
        self.code_wissen = None;
        *self.diesel_cache.borrow_mut() = None;
        self.dbs_zaehler_neu();
    }

    /// Die 8 OBD-Motor-Adressen nach UDS-Antwort abfragen (im Hintergrund).
    pub(super) fn run_scan(&mut self) {
        self.scan_starten(Vec::new(), tr!("Suche Steuergeräte"));
    }

    /// (2) Auto-Scan: Label-ECUs mit CAN-IDs + OBD-Bereich.
    pub(super) fn run_autoscan(&mut self) {
        let targets: Vec<(u16, u16)> = self.db.ecus_with_can().into_iter().map(|(_, tx, rx)| (tx, rx)).collect();
        self.scan_starten(targets, tr!("Auto-Scan"));
    }

    fn scan_starten(&mut self, mut targets: Vec<(u16, u16)>, text: String) {
        let auto = !targets.is_empty();
        for i in 0..8u16 {
            targets.push((0x7E0 + i, 0x7E8 + i));
        }
        let be = self.backend();
        let iface = self.iface.clone();
        self.im_hintergrund(text, move || {
            let mut gefunden: Vec<(u16, u16)> = Vec::new();
            for (tx, rx) in targets {
                let ok = be.open(tx, rx).and_then(|tp| Uds::new(&*tp).diagnostic_session(0x01).map(|_| ())).is_ok();
                if ok && !gefunden.contains(&(tx, rx)) {
                    gefunden.push((tx, rx));
                }
            }
            Box::new(move |app: &mut App| {
                app.status = if auto {
                    tr!("Auto-Scan: {a} Steuergerät(e).", a = gefunden.len())
                } else if gefunden.is_empty() {
                    tr!("Keine ECU auf {a}. Zündung an? Bitrate 500k?", a = iface)
                } else {
                    tr!("{a} Steuergerät(e) gefunden.", a = gefunden.len())
                };
                app.scan_results = gefunden;
            })
        });
    }

    /// UDS-Fehler als Anzeigezeilen (Code, Art, Text aus den Labels, Status).
    pub(super) fn uds_dtc_zeilen(&self, dtcs: &[ecu_werkbank::uds::Dtc]) -> Vec<String> {
        dtcs.iter()
            .map(|d| {
                let code = d.code();
                let text = self.db.dtc_description(&code).unwrap_or("");
                let art = ecu_werkbank::dtc_db::fehlerart_kurz(d.fehlerart()).map(|t| format!(" ({t})")).unwrap_or_default();
                format!("{code}{art}  {text}  ({})", ecu_werkbank::report::uds_status_text(d.status))
            })
            .collect()
    }

    pub(super) fn run_dtc(&mut self) {
        let (tx, rx) = match (parse_hex(&self.dtc_tx), parse_hex(&self.dtc_rx)) {
            (Some(t), Some(r)) => (t, r),
            _ => {
                self.status = tr!("Ungültige TX/RX-ID.");
                return;
            }
        };
        self.dtc_results.clear();
        self.dtc_info.clear();
        let be = self.backend();
        let tp20 = if be.supports_tp20() { self.tp20_addr_for(self.selected_ecu) } else { None };
        enum Antwort {
            Uds(Vec<ecu_werkbank::uds::Dtc>),
            Tp20(u8, Vec<(kwp::KwpDtc, Option<Vec<u8>>)>),
            Keine { uds: String, tp20: Option<String> },
        }
        self.im_hintergrund(tr!("Fehlerspeicher lesen"), move || {
            // 1. UDS (neuere Steuergeräte)
            let uds_res = (|| -> anyhow::Result<Vec<ecu_werkbank::uds::Dtc>> {
                let tp = be.open(tx, rx)?;
                let uds = Uds::new(&*tp).mit_tx(tx);
                uds.diagnostic_session(0x01)?;
                uds.read_dtcs(0xFF)
            })();
            let antwort = match uds_res {
                Ok(d) => Antwort::Uds(d),
                Err(e) => {
                    let uds = format!("{e:#}");
                    // 2. TP 2.0 (ältere VAG, z. B. Golf 6 Motor)
                    match tp20 {
                        Some(a) => match kwp::open_session(&be, a).and_then(|mut ch| Kwp::new(&mut ch).read_dtcs_mit_umgebung()) {
                            Ok(d) => Antwort::Tp20(a, d),
                            Err(e) => Antwort::Keine { uds, tp20: Some(format!("{e:#}")) },
                        },
                        None => Antwort::Keine { uds, tp20: None },
                    }
                }
            };
            Box::new(move |app: &mut App| match antwort {
                Antwort::Uds(d) => {
                    let lines = app.uds_dtc_zeilen(&d);
                    app.dtc_info = tr!("UDS ({tx}/{rx}): {a}", tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx), a = if lines.is_empty() { tr!("keine Fehler gespeichert.") } else { tr!("gelesen.") });
                    app.dtc_results = lines;
                    app.status = tr!("Fehlerspeicher gelesen.");
                }
                Antwort::Tp20(a, dtcs) => {
                    app.dtc_results = app.tp_dtc_lines(&dtcs, a);
                    app.dtc_info = tr!("Über TP 2.0 (Adresse {a}) gelesen — das Steuergerät spricht kein UDS.{b}", a = format!("{:#04x}", a), b = if dtcs.is_empty() { ecu_werkbank::trs!(" Keine Fehler gespeichert.") } else { "" });
                    app.status = tr!("Fehlerspeicher gelesen.");
                }
                Antwort::Keine { uds, tp20: Some(e) } => {
                    app.dtc_info = tr!("Keine Antwort — weder UDS ({uds_err}) noch TP 2.0 ({e}). Zündung an? Verbindung prüfen.", uds_err = uds, e = e);
                    app.status = tr!("Fehlerspeicher nicht lesbar.");
                }
                Antwort::Keine { uds, tp20: None } => {
                    app.dtc_info = tr!("Keine Antwort über UDS: {uds_err}. Ältere Steuergeräte: Auto-Scan starten (findet TP 2.0) oder Seite „TP 2.0“.", uds_err = uds);
                    app.status = tr!("Fehlerspeicher nicht lesbar.");
                }
            })
        });
    }

    /// Adapter-Ampel: (Stufe, Kurztext, Erklärung). Liest den Zustand höchstens alle 2 s.
    pub(super) fn adapter_state(&self) -> (Stufe, String, String) {
        let iface = self.iface.trim().to_string();
        if let Some((t, i, v)) = self.ad_cache.borrow().as_ref() {
            if *i == iface && t.elapsed() < std::time::Duration::from_secs(2) {
                return v.clone();
            }
        }
        let v = if !std::path::Path::new(&format!("/sys/class/net/{iface}")).exists() {
            match ecu_werkbank::connect::dfu_device() {
                Some(serial) => (Stufe::Warnung, tr!("Update-Modus").to_string(), tr!("Adapter steckt im Update-Modus (Seriennr. {serial}). Zurückholen: sudo dfu-util -a 0 -s 0x08000000:leave — Klick: Verbindungs-Assistent", serial = serial)),
                None => (Stufe::Warnung, tr!("kein Adapter").to_string(), tr!("'{iface}' nicht gefunden — Adapter anstecken. Klick: Verbindungs-Assistent", iface = iface)),
            }
        } else {
            let up = ecu_werkbank::ablage::lesen(format!("/sys/class/net/{iface}/flags"))
                .ok()
                .and_then(|f| u32::from_str_radix(f.trim().trim_start_matches("0x"), 16).ok())
                .is_some_and(|f| f & 1 != 0);
            if !up {
                (Stufe::Warnung, "aus".to_string(), tr!("'{iface}' ist ausgeschaltet. Einschalten: sudo ip link set {iface} type can bitrate 500000 && sudo ip link set {iface} up", iface = iface))
            } else {
                match ecu_werkbank::connect::can_state(&iface).as_deref() {
                    Some("BUS-OFF") => (Stufe::Warnung, tr!("Bus-Off").to_string(), tr!("Adapter hat sich abgeschaltet — CAN-High/-Low vertauscht? Wieder einschalten: sudo ip link set {iface} down && sudo ip link set {iface} up", iface = iface)),
                    Some("ERROR-PASSIVE") | Some("ERROR-WARNING") => (Stufe::Hinweis, tr!("keine Verbindung").to_string(), tr!("Niemand bestätigt die Nachrichten: Stecker, Leitungen oder Zündung prüfen.").to_string()),
                    _ => (Stufe::Gut, "bereit".to_string(), tr!("Adapter '{iface}' ist eingeschaltet (Bitrate {a} kbit/s).", iface = iface, a = ecu_werkbank::connect::bitrate(&iface).map(|b| b / 1000).unwrap_or(0))),
                }
            }
        };
        *self.ad_cache.borrow_mut() = Some((std::time::Instant::now(), iface, v.clone()));
        v
    }

    /// Steuergeräte des aktiven Fahrzeugs aus dem letzten Scan (egal ob UDS
    /// oder TP 2.0): Klick » Fehler / Identifikation lesen.
    pub(super) fn ui_fahrzeug_module(&mut self, ui: &mut egui::Ui) {
        let Some(key) = self.active_vehicle.clone() else { return };
        let Some((_, last)) = ecu_werkbank::history::list(&key).pop() else {
            ui.heading(tr!("Steuergeräte dieses Fahrzeugs"));
            ui.label(tr!("Noch kein Scan gespeichert. Einmal alle Steuergeräte auslesen — danach stehen sie hier, und ein Klick auf eines zeigt alle Funktionen (Fehler, Messwerte, Codierung …)."));
            if ui.add_enabled(self.pdf_job.is_none(), egui::Button::new(egui::RichText::new(tr!("🔎 Alle Steuergeräte auslesen")).size(16.0))).clicked() {
                self.start_pdf();
            }
            ui.add_space(6.0);
            ui.separator();
            return;
        };
        ui.heading(tr!("Steuergeräte dieses Fahrzeugs"));
        ui.weak(tr!("Aus dem Scan vom {a} — {b} Steuergeräte. Klick wählt eines aus; die Fehler werden dann live gelesen.", a = last.created, b = last.modules.len()));
        ui.add_space(4.0);
        // --- Gruppierte Verbauliste (nach System) + Export ---
        let gruppen = ecu_werkbank::verbauliste::aus_scan(&last);
        let zus = ecu_werkbank::verbauliste::zusammenfassung(&gruppen);
        let mut vb_export = false;
        egui::CollapsingHeader::new(tr!("📋 Verbauliste nach System ({a} Steuergeräte, {b} mit Fehler)", a = zus.anzahl, b = zus.mit_fehler))
            .id_source("verbauliste")
            .show(ui, |ui| {
                for g in &gruppen {
                    ui.strong(&g.name);
                    for z in &g.zeilen {
                        let (st, stext) = match z.fehler {
                            Some(0) => (Stufe::Gut, tr!("i.O.")),
                            Some(n) => (Stufe::Warnung, tr!("{n} Fehler", n = n)),
                            None => (Stufe::Hinweis, tr!("Speicher nicht lesbar")),
                        };
                        ui.horizontal_wrapped(|ui| {
                            dot(ui, stufe_color(st));
                            let teil = if z.teil.is_empty() { String::new() } else { format!("  ·  {}", z.teil) };
                            ui.label(format!("{}  ({} {}){}  —  {}", z.name, z.protokoll, z.adresse, teil, stext));
                        });
                    }
                    ui.add_space(4.0);
                }
                if ui.button(tr!("💾 Verbauliste als Textdatei speichern")).clicked() {
                    vb_export = true;
                }
                ui.add_space(6.0);
                self.ui_gateway_verbauliste(ui, Some(&last));
            });
        if vb_export {
            let txt = ecu_werkbank::verbauliste::text(&gruppen);
            match ecu_werkbank::vehicle::load(&key).and_then(|v| v.sub("berichte").ok()) {
                Some(dir) => {
                    let p = dir.join(ecu_werkbank::vehicle::stamped("verbauliste", "txt"));
                    match ecu_werkbank::ablage::schreiben(&p, &txt) {
                        Ok(()) => {
                            self.status = tr!("Verbauliste gespeichert: {a}", a = p.display());
                            let _ = std::process::Command::new(OEFFNER).arg(&dir).spawn();
                        }
                        Err(e) => self.status = tr!("Speichern fehlgeschlagen: {e}", e = format!("{:#}", e)),
                    }
                }
                None => self.status = tr!("Kein Fahrzeugordner zum Speichern gefunden."),
            }
        }
        let mut pick = None;
        egui::ScrollArea::vertical().id_source("fzmods").max_height(260.0).show(ui, |ui| {
            egui::Grid::new("fzgrid").striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
                for h in ["", trn!("Steuergerät"), trn!("Adresse"), trn!("Teilenummer"), trn!("Fehler")] {
                    ui.strong(ecu_werkbank::i18n::t(h));
                }
                ui.end_row();
                for (i, m) in last.modules.iter().enumerate() {
                    let n = m.dtc_count();
                    dot(ui, stufe_color(if n > 0 { Stufe::Warnung } else { Stufe::Gut }));
                    let name = if m.name.is_empty() { tr!("(unbekannt)").to_string() } else { m.name.clone() };
                    if ui.selectable_label(self.fz_sel == Some(i), name).clicked() {
                        pick = Some(i);
                    }
                    ui.weak(format!("{} {}", m.protocol, m.addr_text()));
                    ui.monospace(m.ident.first().cloned().unwrap_or_default());
                    ui.label(match &m.dtcs {
                        None => "?".to_string(),
                        Some(_) => n.to_string(),
                    });
                    ui.end_row();
                }
            });
        });
        if let Some(i) = pick {
            self.fz_sel = Some(i);
            self.fz_lines.clear();
            self.fz_info.clear();
            self.fz_clear_confirm = false;
        }
        if let Some(m) = self.fz_sel.and_then(|i| last.modules.get(i)).cloned() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("{}  ({} {})", m.title(), m.protocol, m.addr_text()));
                if ui.button(tr!("🛠 Fehler lesen")).clicked() {
                    self.fz_read(&m, false);
                }
                if ui.button(tr!("🔎 Identifikation")).clicked() {
                    self.fz_read(&m, true);
                }
                // Weitere Funktionen dieses Steuergeräts — springt mit vorbelegter Adresse
                // auf die passende Seite (wie „Steuergerät wählen » Funktion“ beim Werkstattgerät)
                if m.protocol == "TP 2.0" {
                    if let Some(a) = m.address.map(|a| a as u8) {
                        let adr = format!("{a:#04x}");
                        if ui.button(tr!("📊 Messwerte")).clicked() {
                            self.tp_addr = adr.clone();
                            self.tp_block = "1".into();
                            self.tab = Tab::Tp20;
                        }
                        if ui.button(tr!("🔢 Codierung")).clicked() {
                            self.code_kurz = None;
                            self.code_tp20 = Some(a);
                            self.code_quelle = None;
                            self.code_read = None;
                            self.code_confirm = false;
                            self.tab = Tab::Codierung;
                        }
                        if ui.button(tr!("🔧 Anpassung")).clicked() {
                            self.tp_addr = adr.clone();
                            self.an_zeigen = true;
                            self.tab = Tab::Tp20;
                        }
                        if ui.button(tr!("⚙ Grundeinstellung")).clicked() {
                            self.tp_addr = adr.clone();
                            self.ge_zeigen = true;
                            self.tab = Tab::Tp20;
                        }
                        if ui.button(tr!("▶ Stellglieddiagnose")).clicked() {
                            self.tp_addr = adr;
                            self.sg_zeigen = true;
                            self.tab = Tab::Tp20;
                        }
                    }
                } else if let Some((tx, rx)) = ecu_werkbank::profil::parse_can(&m.can) {
                    if ui.button(tr!("🔢 Codierung")).clicked() {
                        self.code_tp20 = None;
                        self.code_tx = format!("{tx:#05x}");
                        self.code_rx = format!("{rx:#05x}");
                        self.code_quelle = None;
                        self.code_read = None;
                        self.code_confirm = false;
                        self.tab = Tab::Codierung;
                    }
                    if ui.button(tr!("🆔 Erweiterte Identifikation")).clicked() {
                        self.ident_tx = format!("{tx:#05x}");
                        self.tab = Tab::Ident;
                    }
                    if ui.button(tr!("⚙ Routine / Stellglied")).on_hover_text(tr!("Expertenseite: UDS-Routinen und Stellglieder mit Kennungen von Hand")).clicked() {
                        self.svc_tx = format!("{tx:#05x}");
                        self.svc_rx = format!("{rx:#05x}");
                        self.tab = Tab::Service;
                    }
                }
                if !self.fz_clear_confirm {
                    if ui.button(tr!("🧹 Fehler löschen…")).clicked() {
                        self.fz_clear_confirm = true;
                    }
                } else {
                    ui.colored_label(stufe_color(Stufe::Hinweis), tr!("Fehlerspeicher dieses Steuergeräts löschen?"));
                    if ui.button(tr!("Ja, löschen")).clicked() {
                        self.fz_clear_confirm = false;
                        self.fz_clear(&m);
                    }
                    if ui.button(tr!("Abbrechen")).clicked() {
                        self.fz_clear_confirm = false;
                    }
                }
            });
            if !self.fz_info.is_empty() {
                ui.label(&self.fz_info);
            }
            let model = self.vehicle_model();
            for line in self.fz_lines.clone() {
                if code_in(&line).is_some() {
                    let notes = code_in(&line).map(|c| self.repairs_for(&c)).unwrap_or_default();
                    self.dtc_row(ui, &line, &model, &notes);
                } else {
                    ui.monospace(line);
                }
            }
        }
        ui.add_space(6.0);
        ui.separator();
    }

    /// Fehlerspeicher eines Steuergeräts löschen, danach neu lesen.
    pub(super) fn fz_clear(&mut self, m: &ecu_werkbank::report::ModuleReport) {
        let be = self.backend();
        let m = m.clone();
        self.im_hintergrund(tr!("Fehlerspeicher {a} löschen", a = m.title()), move || {
            let res: anyhow::Result<()> = (|| {
                if m.protocol == "TP 2.0" {
                    let a = m.address.ok_or_else(|| anyhow::anyhow!(tr!("keine TP-2.0-Adresse")))? as u8;
                    let mut ch = kwp::open_session(&be, a)?;
                    Kwp::new(&mut ch).clear_dtcs()
                } else {
                    let (tx, rx) = ecu_werkbank::profil::parse_can(&m.can).ok_or_else(|| anyhow::anyhow!(tr!("CAN-IDs unbekannt")))?;
                    let tp = be.open(tx, rx)?;
                    let uds = Uds::new(&*tp);
                    uds.diagnostic_session(0x01)?;
                    uds.clear_dtcs()
                }
            })();
            Box::new(move |app: &mut App| match res {
                Ok(()) => {
                    app.status = tr!("Fehlerspeicher {a} gelöscht.", a = m.title());
                    app.fz_read_mit(&m, false, tr!("Gelöscht. Neu gelesen: "));
                }
                Err(e) => {
                    app.fz_info = tr!("Löschen fehlgeschlagen: {e}", e = format!("{:#}", e));
                    app.status = tr!("Löschen fehlgeschlagen.");
                }
            })
        });
    }

    /// Fehler (oder Identifikation) eines Steuergeräts aus dem Scan live lesen.
    pub(super) fn fz_read(&mut self, m: &ecu_werkbank::report::ModuleReport, ident: bool) {
        self.fz_read_mit(m, ident, String::new());
    }

    /// Wie `fz_read`; `vorher` wird vor die Ergebnis-Meldung gesetzt.
    fn fz_read_mit(&mut self, m: &ecu_werkbank::report::ModuleReport, ident: bool, vorher: String) {
        enum Gelesen {
            Zeilen(Vec<String>, String),
            TpDtc(u8, Vec<(kwp::KwpDtc, Option<Vec<u8>>)>),
            UdsDtc(Vec<ecu_werkbank::uds::Dtc>),
        }
        self.fz_lines.clear();
        let be = self.backend();
        let m = m.clone();
        let text = if ident { tr!("Identifikation {a} lesen", a = m.title()) } else { tr!("Fehler {a} lesen", a = m.title()) };
        self.im_hintergrund(text, move || {
            let res: anyhow::Result<Gelesen> = (|| {
                if m.protocol == "TP 2.0" {
                    let a = m.address.ok_or_else(|| anyhow::anyhow!(tr!("keine TP-2.0-Adresse")))? as u8;
                    let mut ch = kwp::open_session(&be, a)?;
                    let mut k = Kwp::new(&mut ch);
                    if ident {
                        let raw = k.read_ecu_id(kwp::ECU_ID_VAG).unwrap_or_default();
                        let mut l = kwp::ident_9b(&raw).unwrap_or_else(|| kwp::ident_strings(&raw));
                        if l.is_empty() {
                            l = kwp::ident_22(&mut k);
                        }
                        if let Some(kwp::TpCodierung::Kurz { wert, .. }) = kwp::kurzcodierung_9b(&raw) {
                            l.push(tr!("Codierung: {a}", a = format!("{wert:07}")));
                        }
                        if let Some((w, i, h)) = raw.get(20..26).filter(|_| kwp::kurzcodierung_9b(&raw).is_some()).and_then(ecu_werkbank::swinfo::betriebsnummer) {
                            l.push(tr!("Betriebsnummer: Werkstatt {w} · Importeur {i} · Händler {h}", w = format!("{w:05}"), i = format!("{i:03}"), h = format!("{h:05}")));
                        }
                        return Ok(Gelesen::Zeilen(l, tr!("Identifikation gelesen (TP 2.0).")));
                    }
                    Ok(Gelesen::TpDtc(a, k.read_dtcs_mit_umgebung()?))
                } else {
                    let (tx, rx) = ecu_werkbank::profil::parse_can(&m.can).ok_or_else(|| anyhow::anyhow!(tr!("CAN-IDs unbekannt")))?;
                    let tp = be.open(tx, rx)?;
                    let uds = Uds::new(&*tp).mit_tx(tx);
                    uds.diagnostic_session(0x01)?;
                    if ident {
                        let mut l = Vec::new();
                        for (did, was) in [(0xF187u16, tr!("Teilenummer")), (0xF189, tr!("Software")), (0xF191, tr!("Hardware")), (0xF197, tr!("System")), (0xF18C, tr!("Seriennummer"))] {
                            if let Ok(d) = uds.read_data_by_id(did) {
                                l.push(format!("{was}: {}", String::from_utf8_lossy(&d).trim()));
                            }
                        }
                        return Ok(Gelesen::Zeilen(l, tr!("Identifikation gelesen (UDS).")));
                    }
                    Ok(Gelesen::UdsDtc(uds.read_dtcs(0xFF)?))
                }
            })();
            let hint = match (&res, &be) {
                (Err(_), Backend::SocketCan { iface }) => ecu_werkbank::connect::can_state(iface).and_then(|st| ecu_werkbank::connect::state_hint(iface, &st)).map(|h| format!(" — {}", h.1)).unwrap_or_default(),
                _ => String::new(),
            };
            Box::new(move |app: &mut App| {
                let (l, info) = match res {
                    Ok(Gelesen::Zeilen(l, info)) => (l, info),
                    Ok(Gelesen::TpDtc(a, d)) => {
                        let n = d.len();
                        (app.tp_dtc_lines(&d, a), if n == 0 { tr!("Keine Fehler gespeichert.") } else { tr!("{n} Fehler (TP 2.0):", n = n) })
                    }
                    Ok(Gelesen::UdsDtc(d)) => {
                        let n = d.len();
                        (app.uds_dtc_zeilen(&d), if n == 0 { tr!("Keine Fehler gespeichert.") } else { tr!("{n} Fehler (UDS):", n = n) })
                    }
                    Err(e) => {
                        app.fz_info = tr!("Keine Antwort: {e}{hint}", e = format!("{:#}", e), hint = hint);
                        app.status = tr!("Steuergerät antwortet nicht.");
                        return;
                    }
                };
                app.fz_lines = l;
                app.fz_info = format!("{vorher}{info}");
                app.status = tr!("Steuergerät gelesen.");
            })
        });
    }

    /// TP-2.0-Logikadresse zu einem gewählten Steuergerät: aus dem Auto-Scan
    /// (gleicher Name), sonst 0x01–0x03 (Motor, Getriebe, ABS sind gleich).
    pub(super) fn tp20_addr_for(&self, addr: Option<u16>) -> Option<u8> {
        let addr = addr?;
        let want = ecu_werkbank::systems::name(addr).to_lowercase();
        let key = want.split([' ', '(']).next().unwrap_or("").to_string();
        if !key.is_empty() {
            if let Some(f) = self.tp_found.iter().find(|f| f.name.to_lowercase().contains(&key)) {
                return Some(f.address);
            }
        }
        (1..=3).contains(&addr).then_some(addr as u8)
    }

    pub(super) fn run_clear(&mut self) {
        let (Some(tx), Some(rx)) = (parse_hex(&self.dtc_tx), parse_hex(&self.dtc_rx)) else {
            self.status = tr!("Ungültige TX/RX-ID.");
            return;
        };
        let be = self.backend();
        self.im_hintergrund(tr!("Fehlercodes löschen"), move || {
            let res = (|| -> anyhow::Result<()> {
                let tp = be.open(tx, rx)?;
                let uds = Uds::new(&*tp);
                uds.diagnostic_session(0x01)?;
                uds.clear_dtcs()
            })();
            Box::new(move |app: &mut App| {
                app.status = match res {
                    Ok(_) => {
                        app.dtc_results.clear();
                        tr!("Fehlercodes gelöscht.")
                    }
                    Err(e) => tr!("Fehler: {e}", e = e),
                };
            })
        });
    }

    /// Eingaben prüfen und einen Sende-Plan bauen — sendet noch nichts.
    pub(super) fn svc_plan_bauen(&self, routine: bool) -> anyhow::Result<SvcPlan> {
        let tx = parse_hex(&self.svc_tx).ok_or_else(|| anyhow::anyhow!(tr!("TX ungültig")))?;
        let rx = parse_hex(&self.svc_rx).ok_or_else(|| anyhow::anyhow!(tr!("RX ungültig")))?;
        if routine {
            let id = parse_hex(&self.routine_id).ok_or_else(|| anyhow::anyhow!(tr!("Routine-ID ungültig")))?;
            // Hex oder dezimal; nie still auf „Start“ ausweichen
            let sub = if self.routine_sub.trim().is_empty() { 1 } else { parse_hex(&self.routine_sub).ok_or_else(|| anyhow::anyhow!(tr!("Sub ungültig")))? };
            let sub = ecu_werkbank::uds::routine_sub_pruefen(sub)?;
            let data = parse_hex_bytes(&self.routine_data)?;
            Ok(SvcPlan::Routine { tx, rx, sub, id, data })
        } else {
            let did = parse_hex(&self.io_did).ok_or_else(|| anyhow::anyhow!(tr!("DID ungültig")))?;
            let ctrl = parse_hex(&self.io_control).ok_or_else(|| anyhow::anyhow!(tr!("Control ungültig")))?;
            let ctrl = u8::try_from(ctrl).map_err(|_| anyhow::anyhow!(tr!("Control ungültig (0x00–0xFF)")))?;
            let state = parse_hex_bytes(&self.io_state)?;
            Ok(SvcPlan::Io { tx, rx, did, ctrl, state })
        }
    }

    /// Nach der Bestätigung: den Plan wirklich senden.
    pub(super) fn svc_senden(&mut self, plan: SvcPlan) {
        let be = self.backend();
        self.svc_result = tr!("Wird gesendet …");
        self.im_hintergrund(tr!("Service senden"), move || {
            let hex = |r: &[u8]| r.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
            let res = (|| -> anyhow::Result<String> {
                let (tx, rx) = match &plan {
                    SvcPlan::Routine { tx, rx, .. } | SvcPlan::Io { tx, rx, .. } => (*tx, *rx),
                };
                let tp = be.open(tx, rx)?;
                let uds = Uds::new(&*tp);
                uds.diagnostic_session(0x03)?;
                Ok(match &plan {
                    SvcPlan::Routine { sub, id, data, .. } => tr!("Routine OK: [{a}]", a = hex(&uds.routine_control(*sub, *id, data)?)),
                    SvcPlan::Io { did, ctrl, state, .. } => tr!("IO-Control OK: [{a}]", a = hex(&uds.io_control(*did, *ctrl, state)?)),
                })
            })();
            Box::new(move |app: &mut App| {
                (app.svc_result, app.status) = match res {
                    Ok(t) => (t, tr!("Service gesendet.")),
                    Err(e) => (tr!("Fehler: {e}", e = format!("{e:#}")), tr!("Service fehlgeschlagen.")),
                };
            })
        });
    }

    /// Verbauliste direkt aus dem Gateway: was das Gateway als verbaut kennt
    /// und wo es einen gespeicherten Fehler meldet (nur lesen, TP 2.0).
    /// Verbauliste aus dem Gateway als Steuergerätebaum (Gateway → Bus → Steuergeräte),
    /// abgeglichen mit dem letzten Scan.
    pub(super) fn ui_gateway_verbauliste(&mut self, ui: &mut egui::Ui, scan: Option<&ecu_werkbank::report::ScanReport>) {
        if let Some(rx) = &self.gw_vb_job {
            if let Ok(r) = rx.try_recv() {
                self.gw_vb = Some(r);
                self.gw_vb_job = None;
            }
        }
        ui.horizontal_wrapped(|ui| {
            let busy = self.gw_vb_job.is_some();
            let ok = self.backend().supports_tp20();
            if ui
                .add_enabled(!busy && ok, egui::Button::new(tr!("🔌 Verbauliste aus dem Gateway lesen")))
                .on_hover_text(tr!("Fragt das Gateway, welche Steuergeräte es als verbaut kennt und welche einen Fehler gespeichert haben. Nur lesen."))
                .on_disabled_hover_text(tr!("Geht nur über TP 2.0 (CAN-Interface oder Demo)."))
                .clicked()
            {
                let (tx, rx) = mpsc::channel();
                let be = self.backend();
                std::thread::spawn(move || {
                    let _ = tx.send(ecu_werkbank::kwp::gateway_verbauliste(&be).map_err(|e| format!("{e:#}")));
                });
                self.gw_vb_job = Some(rx);
            }
            if busy {
                ui.spinner();
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
        });
        match &self.gw_vb {
            Some(Ok(l)) => {
                let aeste = ecu_werkbank::verbauliste::baum(l, scan);
                let n: usize = aeste.iter().map(|a| a.geraete.len()).sum();
                let fehler = aeste.iter().flat_map(|a| &a.geraete).filter(|g| g.fehler_laut_gateway).count();
                let fehlen = aeste.iter().flat_map(|a| &a.geraete).filter(|g| g.im_scan.is_none()).count();
                ui.weak(tr!("Laut Gateway verbaut: {a}, davon {b} mit Fehler.", a = n, b = fehler));
                if scan.is_some() && fehlen > 0 {
                    ui.colored_label(stufe_color(Stufe::Hinweis), tr!("{a} davon im letzten Scan nicht gefunden — antworten sie nicht (Sicherung, Stecker, Steuergerät)?", a = fehlen));
                }
                ui.strong(tr!("🌳 Gateway"));
                for a in &aeste {
                    egui::CollapsingHeader::new(format!("{}  ({})", a.name, a.geraete.len())).id_source(("bus", a.bus)).default_open(true).show(ui, |ui| {
                        for g in &a.geraete {
                            ui.horizontal_wrapped(|ui| {
                                let stufe = if g.fehler_laut_gateway {
                                    Stufe::Warnung
                                } else if scan.is_some() && g.im_scan.is_none() {
                                    Stufe::Hinweis
                                } else {
                                    Stufe::Gut
                                };
                                dot(ui, stufe_color(stufe));
                                let mut st = if g.fehler_laut_gateway { tr!("Fehler gespeichert") } else { tr!("i.O.") };
                                if scan.is_some() && g.im_scan.is_none() {
                                    st.push_str(&tr!(" · im letzten Scan nicht gefunden"));
                                }
                                ui.label(format!("{:02X}  {}  —  {}", g.adresse, g.name, st));
                            });
                        }
                    });
                }
            }
            Some(Err(e)) => {
                ui.colored_label(stufe_color(Stufe::Warnung), e);
            }
            None => {}
        }
    }
}
