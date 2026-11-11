//! GUI: App-Kern (Aufbau, Backend, Scan/DTC-Läufe, Fahrzeug-Steuergeräteliste, Service senden)

use super::*;

impl App {
    pub(super) fn new() -> Self {
        let labels_dir = ecu_werkbank::labels::default_dir();
        let db = LabelDb::load_dir(&labels_dir).unwrap_or_default();
        // Optionaler Start-Reiter (nur für Demo/Screenshots): VAG_TAB=messwerte|labels
        let tabsel = std::env::var("VAG_TAB").unwrap_or_default();
        let (tab, seed) = match tabsel.as_str() {
            "messwerte" => (Tab::Messwerte, true),
            "scope" | "armaturen" => (Tab::Scope, false),
            "codierung" => (Tab::Codierung, false),
            "service" => (Tab::Service, false),
            "tp20" | "blockscan" => (Tab::Tp20, false),
            "obd" => (Tab::Obd, false),
            "mitschnitt" => (Tab::Mitschnitt, false),
            "editor" => (Tab::Editor, false),
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
            "labels" | "namen" => (Tab::Namen, false),
            "ident" => (Tab::Ident, false),
            "codierhelfer" | "codier-helfer" => (Tab::CodierHelfer, false),
            "lexikon" | "grundlexikon" => (Tab::GrundLexikon, false),
            "tp20-block" | "favoriten" | "grundeinstellung" => (Tab::Tp20, false),
            "service-frage" => (Tab::Service, false),
            "editor-scan" => (Tab::Editor, false),
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
        let mut meas_inputs = HashMap::new();
        if seed {
            meas_inputs.insert((0x01u16, 0xF40Cu16), "1180".to_string());
            meas_inputs.insert((0x01u16, 0xF405u16), "5A".to_string());
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
            qn_addr: "0x09".into(),
            qn_block: String::new(),
            qn_feld: String::new(),
            qn_name: String::new(),
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
            erk_an: !ecu_werkbank::history::data_dir().join("erkennung-aus").exists(),
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
            meas_inputs,
            code_ecu,
            code_bytes,
            code_tx: "0x7E0".into(),
            code_rx: "0x7E8".into(),
            code_read: None,
            code_confirm: false,
            code_quelle: None,
            code_bk: None,
            code_texte: ecu_werkbank::codiertexte::Texte::laden(),
            code_edit: None,
            code_nur_benannt: false,
            code_tp20: None,
            sw_last: None,
            sw_diff: Vec::new(),
            ident_tx: "0x7E0".into(),
            ident_result: None,
            chl_teil: String::new(),
            chl_result: None,
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
            ft_code: String::new(),
            ft_text: String::new(),
            ds: ecu_werkbank::datensicherung::laden(),
            ds_job: None,
            ds_geprueft: false,
            fg_job: None,
            fg_progress: String::new(),
            fg_geraete: Vec::new(),
            fg_texte: Default::default(),
            fg_nur_leer: false,
            si_job: None,
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
            ed_path: None,
            ed_file: LabelFile::default(),
            ed_addr: String::new(),
            ed_tx: String::new(),
            ed_rx: String::new(),
            ed_new_name: String::new(),
            tp_last_read: std::time::Instant::now(),
            tp_lines: Vec::new(),
            tp_block_values: Vec::new(),
            tp_block_raw: Vec::new(),
            motor: motor::State::default(),
            bs_job: None,
            bs_at: 0,
            bs_result: None,
            bs_namen: Default::default(),
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
            lbl_ordner_eingabe: None,
            mw_edit: None,
            tp_sweep: None,
            tp_sweep_at: 0,
            tp_found: Vec::new(),
            voll_scan: false,
            tp_autostart: tabsel == "tp20",
            demo_autostart: tabsel.clone(),
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
    }

    pub(super) fn run_scan(&mut self) {
        self.scan_results.clear();
        for i in 0..8u16 {
            let (tx, rx) = (0x7E0 + i, 0x7E8 + i);
            let ok = self.backend().open(tx, rx)
                .and_then(|tp| Uds::new(&*tp).diagnostic_session(0x01).map(|_| ()))
                .is_ok();
            if ok {
                self.scan_results.push((tx, rx));
            }
        }
        self.status = if self.scan_results.is_empty() {
            tr!("Keine ECU auf {a}. Zündung an? Bitrate 500k?", a = self.iface)
        } else {
            tr!("{a} Steuergerät(e) gefunden.", a = self.scan_results.len())
        };
    }

    /// (2) Auto-Scan: Label-ECUs mit CAN-IDs + OBD-Bereich.
    pub(super) fn run_autoscan(&mut self) {
        self.scan_results.clear();
        let mut targets: Vec<(u16, u16)> =
            self.db.ecus_with_can().into_iter().map(|(_, tx, rx)| (tx, rx)).collect();
        for i in 0..8u16 {
            targets.push((0x7E0 + i, 0x7E8 + i));
        }
        for (tx, rx) in targets {
            let ok = self.backend().open(tx, rx)
                .and_then(|tp| Uds::new(&*tp).diagnostic_session(0x01).map(|_| ()))
                .is_ok();
            if ok && !self.scan_results.contains(&(tx, rx)) {
                self.scan_results.push((tx, rx));
            }
        }
        self.status = tr!("Auto-Scan: {a} Steuergerät(e).", a = self.scan_results.len());
    }

    pub(super) fn run_dtc(&mut self) {
        self.dtc_results.clear();
        self.dtc_info.clear();
        let (tx, rx) = match (parse_hex(&self.dtc_tx), parse_hex(&self.dtc_rx)) {
            (Some(t), Some(r)) => (t, r),
            _ => {
                self.status = tr!("Ungültige TX/RX-ID.");
                return;
            }
        };
        // 1. UDS (neuere Steuergeräte)
        let uds_res = (|| -> anyhow::Result<Vec<String>> {
            let tp = self.backend().open(tx, rx)?;
            let uds = Uds::new(&*tp);
            uds.diagnostic_session(0x01)?;
            let dtcs = uds.read_dtcs(0xFF)?;
            Ok(dtcs
                .iter()
                .map(|d| {
                    let code = d.code();
                    let text = self.db.dtc_description(&code).unwrap_or("");
                    let art = ecu_werkbank::dtc_db::fehlerart_kurz(d.fehlerart()).map(|t| format!(" ({t})")).unwrap_or_default();
                    format!("{code}{art}  {text}  ({})", ecu_werkbank::report::uds_status_text(d.status))
                })
                .collect())
        })();
        let uds_err = match uds_res {
            Ok(lines) => {
                self.dtc_info = tr!("UDS ({tx}/{rx}): {a}", tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx), a = if lines.is_empty() { tr!("keine Fehler gespeichert.") } else { tr!("gelesen.") });
                self.dtc_results = lines;
                self.status = tr!("Fehlerspeicher gelesen.");
                return;
            }
            Err(e) => format!("{e:#}"),
        };
        // 2. TP 2.0 (ältere VAG, z. B. Golf 6 Motor) — passende Logikadresse suchen
        if self.backend().supports_tp20() {
            if let Some(a) = self.tp20_addr_for(self.selected_ecu) {
                let res = (|| -> anyhow::Result<Vec<(kwp::KwpDtc, Option<Vec<u8>>)>> {
                    let mut ch = kwp::open_session(&self.backend(), a)?;
                    Kwp::new(&mut ch).read_dtcs_mit_umgebung()
                })();
                match res {
                    Ok(dtcs) => {
                        self.dtc_results = self.tp_dtc_lines(&dtcs, a);
                        self.dtc_info = tr!("Über TP 2.0 (Adresse {a}) gelesen — das Steuergerät spricht kein UDS.{b}", a = format!("{:#04x}", a), b = if dtcs.is_empty() { ecu_werkbank::trs!(" Keine Fehler gespeichert.") } else { "" });
                        self.status = tr!("Fehlerspeicher gelesen.");
                        return;
                    }
                    Err(e) => {
                        self.dtc_info = tr!("Keine Antwort — weder UDS ({uds_err}) noch TP 2.0 ({e}). Zündung an? Verbindung prüfen.", uds_err = uds_err, e = format!("{:#}", e));
                        self.status = tr!("Fehlerspeicher nicht lesbar.");
                        return;
                    }
                }
            }
        }
        self.dtc_info = tr!("Keine Antwort über UDS: {uds_err}. Ältere Steuergeräte: Auto-Scan starten (findet TP 2.0) oder Seite „TP 2.0“.", uds_err = uds_err);
        self.status = tr!("Fehlerspeicher nicht lesbar.");
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
            let up = std::fs::read_to_string(format!("/sys/class/net/{iface}/flags"))
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
            ui.weak(tr!("Noch kein Scan gespeichert — „📄 Als PDF speichern“ oder Schnell-Check einmal ausführen, dann stehen hier alle Steuergeräte des Autos."));
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
            });
        if vb_export {
            let txt = ecu_werkbank::verbauliste::text(&gruppen);
            match ecu_werkbank::vehicle::load(&key).and_then(|v| v.sub("berichte").ok()) {
                Some(dir) => {
                    let p = dir.join(ecu_werkbank::vehicle::stamped("verbauliste", "txt"));
                    match std::fs::write(&p, &txt) {
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
        match res {
            Ok(()) => {
                self.fz_read(m, false);
                self.fz_info = tr!("Gelöscht. Neu gelesen: {a}", a = self.fz_info);
                self.status = tr!("Fehlerspeicher {a} gelöscht.", a = m.title());
            }
            Err(e) => {
                self.fz_info = tr!("Löschen fehlgeschlagen: {e}", e = format!("{:#}", e));
                self.status = tr!("Löschen fehlgeschlagen.");
            }
        }
    }

    /// Fehler (oder Identifikation) eines Steuergeräts aus dem Scan live lesen.
    pub(super) fn fz_read(&mut self, m: &ecu_werkbank::report::ModuleReport, ident: bool) {
        self.fz_lines.clear();
        let be = self.backend();
        let res: anyhow::Result<(Vec<String>, String)> = (|| {
            if m.protocol == "TP 2.0" {
                let a = m.address.ok_or_else(|| anyhow::anyhow!(tr!("keine TP-2.0-Adresse")))? as u8;
                let mut ch = kwp::open_session(&be, a)?;
                let mut k = Kwp::new(&mut ch);
                if ident {
                    let mut l = k.read_ecu_id(kwp::ECU_ID_VAG).map(|d| kwp::ident_strings(&d)).unwrap_or_default();
                    if l.is_empty() {
                        l = kwp::ident_22(&mut k);
                    }
                    return Ok((l, tr!("Identifikation gelesen (TP 2.0).")));
                }
                let d = k.read_dtcs_mit_umgebung()?;
                let n = d.len();
                Ok((self.tp_dtc_lines(&d, a), if n == 0 { tr!("Keine Fehler gespeichert.") } else { tr!("{n} Fehler (TP 2.0):", n = n) }))
            } else {
                let (tx, rx) = ecu_werkbank::profil::parse_can(&m.can).ok_or_else(|| anyhow::anyhow!(tr!("CAN-IDs unbekannt")))?;
                let tp = be.open(tx, rx)?;
                let uds = Uds::new(&*tp);
                uds.diagnostic_session(0x01)?;
                if ident {
                    let mut l = Vec::new();
                    for (did, was) in [(0xF187u16, tr!("Teilenummer")), (0xF189, tr!("Software")), (0xF191, tr!("Hardware")), (0xF197, tr!("System")), (0xF18C, tr!("Seriennummer"))] {
                        if let Ok(d) = uds.read_data_by_id(did) {
                            l.push(format!("{was}: {}", String::from_utf8_lossy(&d).trim()));
                        }
                    }
                    return Ok((l, tr!("Identifikation gelesen (UDS).")));
                }
                let d = uds.read_dtcs(0xFF)?;
                let n = d.len();
                let lines = d
                    .iter()
                    .map(|d| {
                        let code = d.code();
                        let text = self.db.dtc_description(&code).unwrap_or("");
                        let art = ecu_werkbank::dtc_db::fehlerart_kurz(d.fehlerart()).map(|t| format!(" ({t})")).unwrap_or_default();
                        format!("{code}{art}  {text}  ({})", ecu_werkbank::report::uds_status_text(d.status))
                    })
                    .collect();
                Ok((lines, if n == 0 { tr!("Keine Fehler gespeichert.") } else { tr!("{n} Fehler (UDS):", n = n) }))
            }
        })();
        match res {
            Ok((l, info)) => {
                self.fz_lines = l;
                self.fz_info = info;
                self.status = tr!("Steuergerät gelesen.");
            }
            Err(e) => {
                let hint = match &be {
                    Backend::SocketCan { iface } => ecu_werkbank::connect::can_state(iface).and_then(|st| ecu_werkbank::connect::state_hint(iface, &st)).map(|h| format!(" — {}", h.1)).unwrap_or_default(),
                    _ => String::new(),
                };
                self.fz_info = tr!("Keine Antwort: {e}{hint}", e = format!("{:#}", e), hint = hint);
                self.status = tr!("Steuergerät antwortet nicht.");
            }
        }
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
        let res = (|| -> anyhow::Result<()> {
            let tx = parse_hex(&self.dtc_tx).ok_or_else(|| anyhow::anyhow!(tr!("TX ungültig")))?;
            let rx = parse_hex(&self.dtc_rx).ok_or_else(|| anyhow::anyhow!(tr!("RX ungültig")))?;
            let tp = self.backend().open(tx, rx)?;
            let uds = Uds::new(&*tp);
            uds.diagnostic_session(0x01)?;
            uds.clear_dtcs()
        })();
        self.status = match res {
            Ok(_) => {
                self.dtc_results.clear();
                tr!("Fehlercodes gelöscht.")
            }
            Err(e) => tr!("Fehler: {e}", e = e),
        };
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
        let hex = |r: &[u8]| r.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
        let res = (|| -> anyhow::Result<String> {
            let (tx, rx) = match &plan {
                SvcPlan::Routine { tx, rx, .. } | SvcPlan::Io { tx, rx, .. } => (*tx, *rx),
            };
            let tp = self.backend().open(tx, rx)?;
            let uds = Uds::new(&*tp);
            uds.diagnostic_session(0x03)?;
            Ok(match &plan {
                SvcPlan::Routine { sub, id, data, .. } => tr!("Routine OK: [{a}]", a = hex(&uds.routine_control(*sub, *id, data)?)),
                SvcPlan::Io { did, ctrl, state, .. } => tr!("IO-Control OK: [{a}]", a = hex(&uds.io_control(*did, *ctrl, state)?)),
            })
        })();
        self.svc_result = match res {
            Ok(t) => t,
            Err(e) => tr!("Fehler: {e}", e = format!("{e:#}")),
        };
    }

}
