//! CAN-Mitschnitte lesen, dekodieren und gegen die TP-2.0-Annahmen prüfen.
//!
//! Liest `candump`-Logdateien (Format `-L` und das Standard-Bildschirmformat),
//! setzt TP-2.0- und ISO-TP-Nachrichten zusammen und übersetzt KWP2000-,
//! UDS- und OBD-II-Dienste in Klartext. Der [`Report`] zeigt, ob die in
//! `tp20.rs` mit `TODO(verify)` markierten Annahmen im Mitschnitt zutreffen.

use crate::tr;
use crate::kwp;
use crate::obd;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

/// Ein CAN-Frame aus dem Log.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub t: Option<f64>,
    pub id: u32,
    pub extended: bool,
    pub data: Vec<u8>,
}

/// Eine Zeile des Logs parsen. Unterstützt:
/// - `(1698412345.123456) can0 7E0#0210010000000000`  (candump -L)
/// - `  can0  7E0   [8]  02 10 01 00 00 00 00 00`     (candump)
/// - `(1698412345.123456)  can0  7E0   [8]  02 10 …`  (candump -ta)
pub fn parse_line(line: &str) -> Option<Frame> {
    let mut tok: Vec<&str> = line.split_whitespace().collect();
    if tok.is_empty() {
        return None;
    }
    let mut t = None;
    if tok[0].starts_with('(') {
        t = tok[0].trim_matches(|c| c == '(' || c == ')').parse::<f64>().ok();
        tok.remove(0);
    }
    if tok.len() < 2 {
        return None;
    }
    // tok[0] = Interface
    if let Some((id, data)) = tok[1].split_once('#') {
        let extended = id.len() > 3;
        let id = u32::from_str_radix(id, 16).ok()?;
        if data.starts_with('R') {
            return None; // Remote-Frame
        }
        // CAN FD: "##<flags><daten>"
        let data = data.strip_prefix('#').map(|d| d.get(1..).unwrap_or("")).unwrap_or(data);
        return Some(Frame { t, id, extended, data: hex_bytes(data)? });
    }
    // Bildschirmformat: iface ID [n] b0 b1 …
    let extended = tok[1].len() > 3;
    let id = u32::from_str_radix(tok[1], 16).ok()?;
    let len_tok = tok.get(2)?;
    let n: usize = len_tok.trim_matches(|c| c == '[' || c == ']').parse().ok()?;
    let data: Option<Vec<u8>> = tok.iter().skip(3).take(n).map(|b| u8::from_str_radix(b, 16).ok()).collect();
    Some(Frame { t, id, extended, data: data? })
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// Frame im `candump -L`-Format.
pub fn format_line(t: f64, iface: &str, id: u32, data: &[u8]) -> String {
    let hex: String = data.iter().map(|b| format!("{b:02X}")).collect();
    if id > 0x7FF {
        format!("({t:.6}) {iface} {id:08X}#{hex}")
    } else {
        format!("({t:.6}) {iface} {id:03X}#{hex}")
    }
}

/// Eine dekodierte Nachricht oder ein Kanal-Ereignis.
#[derive(Debug, Clone)]
pub struct Event {
    pub t: Option<f64>,
    pub id: u32,
    /// "TP2.0", "KWP", "UDS", "OBD"
    pub proto: &'static str,
    /// "Tester→ECU", "ECU→Tester" oder ""
    pub dir: &'static str,
    pub text: String,
}

impl Event {
    pub fn line(&self) -> String {
        let t = self.t.map(|t| format!("{t:>14.3}")).unwrap_or_else(|| " ".repeat(14));
        format!("{t}  {:>8X}  {:<5}  {:<10}  {}", self.id, self.proto, self.dir, self.text)
    }
}

/// Eine vollständige Diagnose-Nachricht (für die Lernhilfe).
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub t: Option<f64>,
    pub id: u32,
    pub proto: Proto,
    pub from_tester: bool,
    /// TP 2.0: Logikadresse des Steuergeräts
    pub address: Option<u8>,
    pub data: Vec<u8>,
}

/// Beobachteter TP-2.0-Kanalaufbau.
#[derive(Debug, Clone)]
pub struct SetupObs {
    pub address: u8,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
    /// Laut Annahme (Bytes 4–5): ID, auf der der Tester sendet.
    pub tester_tx: u32,
    /// Laut Annahme (Bytes 2–3): ID, auf der das Steuergerät sendet.
    pub ecu_tx: u32,
    /// Some(true) = Tester sendete danach wirklich auf `tester_tx`.
    pub order_confirmed: Option<bool>,
}

/// Prüfergebnis eines Mitschnitts.
#[derive(Debug, Default, Clone)]
pub struct Report {
    pub frames: usize,
    pub unparsed_lines: usize,
    pub setups: Vec<SetupObs>,
    pub param_frames: Vec<(u32, Vec<u8>)>,
    pub length_ok: usize,
    pub length_bad: usize,
    /// Neue Nachricht setzte die Sequenz fort (Annahme: läuft über den Kanal weiter)
    pub seq_continued: usize,
    /// Neue Nachricht begann wieder bei 0
    pub seq_restarted: usize,
    pub seq_errors: usize,
    pub keepalives: usize,
    pub formulas: BTreeMap<u8, usize>,
    pub tp_messages: usize,
    pub isotp_messages: usize,
}

impl Report {
    pub fn text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "{}", tr!("Prüfbericht"));
        let _ = writeln!(s, "  {}", tr!("Frames gelesen: {a} (nicht lesbare Zeilen: {b})", a = self.frames, b = self.unparsed_lines));
        let _ = writeln!(s, "  {}", tr!("Nachrichten: {a} TP 2.0/KWP, {b} ISO-TP (UDS/OBD)", a = self.tp_messages, b = self.isotp_messages));
        if self.setups.is_empty() {
            let _ = writeln!(s, "  {}", tr!("TP 2.0: kein Kanalaufbau im Mitschnitt (0x200 … 0xC0)."));
            return s;
        }
        let _ = writeln!(s, "\n  {}", tr!("TP-2.0-Kanalaufbau:"));
        for o in &self.setups {
            let verdict = match o.order_confirmed {
                Some(true) => crate::trs!("✓ ID-Reihenfolge bestätigt"),
                Some(false) => crate::trs!("✗ IDs VERTAUSCHT — tp20.rs anpassen!"),
                None => crate::trs!("? danach kein Kanalparameter-Frame gesehen"),
            };
            let _ = writeln!(
                s,
                "    {:#04x}: {}\n          → {}",
                o.address,
                tr!("Anfrage {a}  Antwort {b}", a = hex(&o.request), b = hex(&o.response)),
                tr!("Tester sendet {a}, Steuergerät sendet {b}  {verdict}", a = format!("{:#05x}", o.tester_tx), b = format!("{:#05x}", o.ecu_tx), verdict = verdict)
            );
        }
        if !self.param_frames.is_empty() {
            let _ = writeln!(s, "\n  {}", tr!("Kanalparameter (A0/A1):"));
            for (id, d) in self.param_frames.iter().take(6) {
                let _ = writeln!(s, "    {id:#05x}: {}", hex(d));
            }
        }
        let _ = writeln!(
            s,
            "\n  {}",
            tr!(
                "Längenfeld (2 Byte vor KWP-Daten): {a} passend, {b} unpassend {c}",
                a = self.length_ok,
                b = self.length_bad,
                c = if self.length_bad == 0 && self.length_ok > 0 { "✓" } else if self.length_bad > 0 { crate::trs!("✗ prüfen") } else { "" }
            )
        );
        let _ = writeln!(
            s,
            "  {}",
            tr!(
                "Sequenz bei neuer Nachricht: {a}× fortgesetzt, {b}× bei 0 neu, {c} Fehler {d}",
                a = self.seq_continued,
                b = self.seq_restarted,
                c = self.seq_errors,
                d = match (self.seq_continued, self.seq_restarted) {
                    (c, 0) if c > 0 => crate::trs!("✓ läuft über den Kanal weiter (wie implementiert)"),
                    (0, r) if r > 0 => crate::trs!("✗ beginnt je Nachricht neu — tp20.rs anpassen!"),
                    _ => "",
                }
            )
        );
        let _ = writeln!(s, "  Keep-Alive (A3): {}×", self.keepalives);
        if !self.formulas.is_empty() {
            let _ = writeln!(s, "\n  {}", tr!("Messwert-Formeln im Mitschnitt (Kennbyte: Anzahl, mit Anzeige vergleichen):"));
            for (f, n) in &self.formulas {
                let probe = kwp::BlockValue { formula: *f, a: 1, b: 1 };
                let known = if probe.decode().is_some() { crate::trs!("bekannt") } else { crate::trs!("UNBEKANNT") };
                let _ = writeln!(s, "    F{f:02X}: {n}×  ({known})");
            }
        }
        s
    }
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

struct TpChan {
    address: u8,
    from_tester: bool,
    buf: Vec<u8>,
    next_seq: Option<u8>,
    in_message: bool,
    setup_idx: usize,
}

struct IsoAsm {
    len: usize,
    buf: Vec<u8>,
    next_sn: u8,
}

/// Zustandsbehafteter Decoder; Frames der Reihe nach mit [`Decoder::feed`] füttern.
#[derive(Default)]
pub struct Decoder {
    pending_setup: HashMap<u8, Vec<u8>>,
    tp: HashMap<u32, TpChan>,
    iso: HashMap<u32, IsoAsm>,
    pub events: Vec<Event>,
    pub messages: Vec<Message>,
    pub report: Report,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ganzen Logtext dekodieren.
    pub fn decode_text(text: &str) -> Self {
        let mut d = Self::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            match parse_line(l) {
                Some(f) => d.feed(&f),
                None => d.report.unparsed_lines += 1,
            }
        }
        d
    }

    pub fn feed(&mut self, f: &Frame) {
        self.report.frames += 1;
        if !f.extended && f.id == 0x200 && f.data.len() >= 7 && f.data[1] == 0xC0 {
            self.pending_setup.insert(f.data[0], f.data.clone());
            self.push(f, "TP2.0", "Tester→ECU", tr!("Kanalaufbau zu Steuergerät {a}", a = format!("{:#04x}", f.data[0])));
            return;
        }
        if !f.extended && (0x201..=0x27F).contains(&f.id) && f.data.len() >= 6 && (0xD0..=0xD8).contains(&f.data[1]) {
            self.on_setup_reply(f);
            return;
        }
        if self.tp.contains_key(&f.id) {
            self.on_tp_frame(f);
            return;
        }
        let iso_id = if f.extended {
            matches!(f.id >> 16, 0x17FC | 0x17FE | 0x18DA | 0x18DB)
        } else {
            (0x600..=0x7FF).contains(&f.id)
        };
        if iso_id {
            self.on_isotp_frame(f);
        }
    }

    fn push(&mut self, f: &Frame, proto: &'static str, dir: &'static str, text: String) {
        self.events.push(Event { t: f.t, id: f.id, proto, dir, text });
    }

    fn on_setup_reply(&mut self, f: &Frame) {
        let address = (f.id - 0x200) as u8;
        let d = &f.data;
        if d[1] != 0xD0 {
            let why = match d[1] {
                0xD6 => crate::trs!("Anwendungstyp nicht unterstützt"),
                0xD7 => crate::trs!("vorübergehend nicht verfügbar"),
                0xD8 => crate::trs!("keine Ressourcen frei"),
                _ => crate::trs!("abgelehnt"),
            };
            self.push(f, "TP2.0", "ECU→Tester", tr!("Kanal zu {address} abgelehnt: {why}", address = format!("{:#04x}", address), why = why));
            return;
        }
        let ecu_tx = u32::from(d[2]) | (u32::from(d[3] & 0x07) << 8);
        let tester_tx = u32::from(d[4]) | (u32::from(d[5] & 0x07) << 8);
        let request = self.pending_setup.remove(&address).unwrap_or_default();
        self.report.setups.push(SetupObs {
            address,
            request,
            response: d.clone(),
            tester_tx,
            ecu_tx,
            order_confirmed: None,
        });
        let idx = self.report.setups.len() - 1;
        for (id, from_tester) in [(tester_tx, true), (ecu_tx, false)] {
            self.tp.insert(id, TpChan { address, from_tester, buf: vec![], next_seq: None, in_message: false, setup_idx: idx });
        }
        self.push(
            f,
            "TP2.0",
            "ECU→Tester",
            tr!("Kanal zu {address} offen: Tester sendet {tester_tx}, Steuergerät {ecu_tx}", address = format!("{:#04x}", address), tester_tx = format!("{:#05x}", tester_tx), ecu_tx = format!("{:#05x}", ecu_tx)),
        );
    }

    fn on_tp_frame(&mut self, f: &Frame) {
        let Some(&b0) = f.data.first() else { return };
        let ch = self.tp.get_mut(&f.id).expect("geprüft");
        let dir = if ch.from_tester { "Tester→ECU" } else { "ECU→Tester" };
        let address = ch.address;
        match b0 {
            0xA0 | 0xA1 => {
                let idx = ch.setup_idx;
                let from_tester = ch.from_tester;
                if b0 == 0xA0 && self.report.setups[idx].order_confirmed.is_none() {
                    // Der Tester schickt A0 zuerst — auf welcher ID?
                    self.report.setups[idx].order_confirmed = Some(from_tester);
                }
                self.report.param_frames.push((f.id, f.data.clone()));
                let what = if b0 == 0xA0 { crate::trs!("Kanalparameter (Anfrage)") } else { crate::trs!("Kanalparameter (Antwort)") };
                let text = tr!("{what}: Blockgröße {a}", what = what, a = f.data.get(1).copied().unwrap_or(0));
                self.push(f, "TP2.0", dir, text);
                return;
            }
            0xA3 => {
                self.report.keepalives += 1;
                return; // Keep-Alive nicht einzeln anzeigen
            }
            0xA8 => {
                self.push(f, "TP2.0", dir, tr!("Kanal zu {address} geschlossen", address = format!("{:#04x}", address)));
                return;
            }
            _ => {}
        }
        let kind = b0 & 0xF0;
        if !matches!(kind, 0x00 | 0x10 | 0x20 | 0x30) {
            return; // ACKs (0xB?, 0x9?)
        }
        let seq = b0 & 0x0F;
        if !ch.in_message {
            if let Some(n) = ch.next_seq {
                if seq == n && n != 0 {
                    self.report.seq_continued += 1;
                } else if seq == 0 && n != 0 {
                    self.report.seq_restarted += 1;
                } else if seq != n {
                    self.report.seq_errors += 1;
                }
            }
            ch.in_message = true;
            ch.buf.clear();
        } else if ch.next_seq.is_some_and(|n| n != seq) {
            self.report.seq_errors += 1;
        }
        ch.next_seq = Some((seq + 1) & 0x0F);
        ch.buf.extend_from_slice(&f.data[1..]);
        // 0x1X / 0x3X = letztes Paket der Nachricht
        if kind == 0x10 || kind == 0x30 {
            ch.in_message = false;
            let buf = std::mem::take(&mut ch.buf);
            if buf.len() < 2 {
                self.report.length_bad += 1;
                return;
            }
            let len = (usize::from(buf[0]) << 8 | usize::from(buf[1])) & 0x7FFF;
            let body = &buf[2..];
            // Passend: Länge deckt sich mit den Daten (bis auf Füllbytes im letzten Frame).
            if body.len() >= len && body.len() - len < 7 {
                self.report.length_ok += 1;
            } else {
                self.report.length_bad += 1;
            }
            let msg = body[..len.min(body.len())].to_vec();
            self.report.tp_messages += 1;
            if let Some(&sid) = msg.first() {
                if sid == 0x61 {
                    for v in kwp::parse_block(msg.get(2..).unwrap_or(&[])) {
                        *self.report.formulas.entry(v.formula).or_default() += 1;
                    }
                }
            }
            let text = format!("[{address:#04x}] {}", describe(Proto::Kwp, &msg));
            let from_tester = dir == "Tester→ECU";
            self.messages.push(Message { t: f.t, id: f.id, proto: Proto::Kwp, from_tester, address: Some(address), data: msg.clone() });
            self.push(f, "KWP", dir, text);
        }
    }

    fn on_isotp_frame(&mut self, f: &Frame) {
        let d = &f.data;
        let Some(&pci) = d.first() else { return };
        let msg = match pci >> 4 {
            0x0 => {
                let len = usize::from(pci & 0x0F);
                if len == 0 || len >= d.len() {
                    return;
                }
                Some(d[1..=len].to_vec())
            }
            0x1 => {
                if d.len() < 3 {
                    return;
                }
                let len = (usize::from(pci & 0x0F) << 8) | usize::from(d[1]);
                self.iso.insert(f.id, IsoAsm { len, buf: d[2..].to_vec(), next_sn: 1 });
                None
            }
            0x2 => {
                let Some(a) = self.iso.get_mut(&f.id) else { return };
                if pci & 0x0F != a.next_sn {
                    self.iso.remove(&f.id);
                    return;
                }
                a.next_sn = (a.next_sn + 1) & 0x0F;
                a.buf.extend_from_slice(&d[1..]);
                if a.buf.len() >= a.len {
                    let mut a = self.iso.remove(&f.id).expect("vorhanden");
                    a.buf.truncate(a.len);
                    Some(a.buf)
                } else {
                    None
                }
            }
            _ => None, // Flow Control
        };
        let Some(msg) = msg else { return };
        let Some(&sid) = msg.first() else { return };
        self.report.isotp_messages += 1;
        let base = if sid == 0x7F { msg.get(1).copied().unwrap_or(0) } else { sid & !0x40 };
        let proto = if (0x01..=0x0A).contains(&base) { Proto::Obd } else { Proto::Uds };
        let dir = if sid == 0x7F || sid >= 0x40 { "ECU→Tester" } else { "Tester→ECU" };
        let name = if proto == Proto::Obd { "OBD" } else { "UDS" };
        let text = describe(proto, &msg);
        self.messages.push(Message { t: f.t, id: f.id, proto, from_tester: !(sid == 0x7F || sid >= 0x40), address: None, data: msg.clone() });
        self.push(f, name, dir, text);
    }

    /// Alle Ereignisse als Textzeilen.
    pub fn lines(&self) -> Vec<String> {
        self.events.iter().map(Event::line).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Proto {
    Kwp,
    Uds,
    Obd,
}

/// Eine Diagnose-Nachricht in Klartext übersetzen.
pub fn describe(proto: Proto, msg: &[u8]) -> String {
    let Some(&sid) = msg.first() else { return "leer".into() };
    let a = |i: usize| msg.get(i).copied().unwrap_or(0);
    if sid == 0x7F {
        return tr!("Negative Antwort auf {a}: {b}", a = service_name(proto, a(1)), b = nrc_name(a(2)));
    }
    let resp = sid >= 0x40;
    let base = if resp { sid - 0x40 } else { sid };
    let name = service_name(proto, base);
    let detail = match (proto, base, resp) {
        (_, 0x10, false) => tr!("Sitzung {a}", a = format!("{:#04x}", a(1))),
        (Proto::Obd, 0x01, false) => obd::pid_info(a(1))
            .map(|(n, _)| format!("PID {:#04x} {n}", a(1)))
            .unwrap_or_else(|| format!("PID {:#04x}", a(1))),
        (Proto::Obd, 0x01, true) => {
            let pid = a(1);
            match (obd::pid_info(pid), obd::decode_pid(pid, msg.get(2..).unwrap_or(&[]))) {
                (Some((n, u)), Some(v)) => format!("{n} = {v:.1} {u}"),
                _ if pid == 0x01 && msg.len() >= 6 => {
                    let r = obd::parse_readiness([a(2), a(3), a(4), a(5)]);
                    tr!("MIL {a}, {b} Fehler, Readiness {c}", a = if r.mil { crate::trs!("an") } else { crate::trs!("aus") }, b = r.dtc_count, c = if r.all_complete() { crate::trs!("fertig") } else { crate::trs!("unvollständig") })
                }
                _ => tr!("PID {pid}: {a}", pid = format!("{:#04x}", pid), a = hex(msg.get(2..).unwrap_or(&[]))),
            }
        }
        (Proto::Obd, 0x03 | 0x07 | 0x0A, true) => {
            let c = obd::parse_dtcs(&msg[1..]);
            if c.is_empty() { tr!("keine Fehler") } else { c.join(", ") }
        }
        (Proto::Obd, 0x09, true) if a(1) == 0x02 => {
            format!("VIN {}", String::from_utf8_lossy(msg.get(3..).unwrap_or(&[])))
        }
        (Proto::Kwp, 0x1A, false) => tr!("Option {a}", a = format!("{:#04x}", a(1))),
        (Proto::Kwp, 0x1A, true) => kwp::ident_strings(msg.get(2..).unwrap_or(&[])).join(" | "),
        (Proto::Kwp, 0x18, true) => {
            let d = kwp::parse_dtcs(&msg[1..]);
            if d.is_empty() {
                tr!("keine Fehler")
            } else {
                d.iter()
                    .map(|x| match x.obd_code(None) {
                        Some(p) => format!("{} ({p})", x.vag_code()),
                        None => x.vag_code(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        }
        (Proto::Kwp, 0x21, false) => tr!("Block {a}", a = format!("{:03}", a(1))),
        (Proto::Kwp, 0x21, true) => {
            let vals: Vec<String> = kwp::parse_block(msg.get(2..).unwrap_or(&[])).iter().map(|v| v.display()).collect();
            tr!("Block {a}: {b}", a = format!("{:03}", a(1)), b = vals.join(" | "))
        }
        (Proto::Uds, 0x22, false) => format!("DID {:#06x}", u16::from_be_bytes([a(1), a(2)])),
        (Proto::Uds, 0x22, true) => {
            let data = msg.get(3..).unwrap_or(&[]);
            let val = if !data.is_empty() && data.iter().all(|b| (0x20..=0x7E).contains(b)) {
                format!("\"{}\"", String::from_utf8_lossy(data))
            } else {
                hex(data)
            };
            tr!("DID {a} = {val}", a = format!("{:#06x}", u16::from_be_bytes([a(1), a(2)])), val = val)
        }
        (Proto::Uds, 0x19, true) if a(1) == 0x02 => {
            let codes: Vec<String> = msg
                .get(3..)
                .unwrap_or(&[])
                .as_chunks::<4>().0.iter()
                .map(|c| obd::dtc_code(c[0], c[1]))
                .collect();
            if codes.is_empty() { tr!("keine Fehler") } else { codes.join(", ") }
        }
        (Proto::Uds, 0x2E, false) => tr!("DID {a} ← {b}", a = format!("{:#06x}", u16::from_be_bytes([a(1), a(2)])), b = hex(msg.get(3..).unwrap_or(&[]))),
        (Proto::Uds, 0x31, _) => tr!("Routine {a} (Sub {b})", a = format!("{:#06x}", u16::from_be_bytes([a(2), a(3)])), b = a(1)),
        (_, 0x27, _) => tr!("Stufe {a}", a = format!("{:#04x}", a(1))),
        _ => hex(&msg[1..]),
    };
    let arrow = if resp { "✓ " } else { "" };
    if detail.is_empty() {
        format!("{arrow}{name}")
    } else {
        format!("{arrow}{name}: {detail}")
    }
}

fn service_name(proto: Proto, sid: u8) -> &'static str {
    match (proto, sid) {
        (Proto::Obd, 0x01) => crate::trs!("Live-Daten"),
        (Proto::Obd, 0x02) => crate::trs!("Freeze-Frame"),
        (Proto::Obd, 0x03) => crate::trs!("Fehlercodes"),
        (Proto::Obd, 0x04) => crate::trs!("Fehlercodes löschen"),
        (Proto::Obd, 0x07) => crate::trs!("Sporadische Fehler"),
        (Proto::Obd, 0x09) => crate::trs!("Fahrzeuginfo"),
        (Proto::Obd, 0x0A) => crate::trs!("Permanente Fehler"),
        (_, 0x10) => crate::trs!("Diagnosesitzung"),
        (_, 0x11) => crate::trs!("Steuergerät-Reset"),
        (_, 0x14) => crate::trs!("Fehlerspeicher löschen"),
        (Proto::Kwp, 0x18) => crate::trs!("Fehlerspeicher lesen"),
        (Proto::Uds, 0x19) => crate::trs!("Fehlerspeicher lesen"),
        (Proto::Kwp, 0x1A) => crate::trs!("Identifikation"),
        (Proto::Kwp, 0x21) => crate::trs!("Messwertblock"),
        (Proto::Uds, 0x22) => crate::trs!("Daten lesen"),
        (_, 0x27) => crate::trs!("Zugriffsberechtigung"),
        (_, 0x28) => crate::trs!("Kommunikation steuern"),
        (_, 0x2E) => crate::trs!("Daten schreiben"),
        (_, 0x2F) => crate::trs!("Stellglied"),
        (_, 0x31) => crate::trs!("Routine"),
        (Proto::Kwp, 0x3B) => crate::trs!("Anpassung schreiben"),
        (_, 0x3E) => crate::trs!("Tester aktiv"),
        (_, 0x85) => crate::trs!("DTC-Speicherung steuern"),
        _ => crate::trs!("Dienst"),
    }
}

fn nrc_name(nrc: u8) -> String {
    let n = match nrc {
        0x10 => crate::trs!("allgemein abgelehnt"),
        0x11 => crate::trs!("Dienst nicht unterstützt"),
        0x12 => crate::trs!("Unterfunktion nicht unterstützt"),
        0x13 => crate::trs!("falsche Länge/Format"),
        0x21 => crate::trs!("beschäftigt, wiederholen"),
        0x22 => crate::trs!("Bedingungen nicht erfüllt"),
        0x31 => crate::trs!("außerhalb des Bereichs"),
        0x33 => crate::trs!("Zugriff verweigert"),
        0x35 => crate::trs!("falscher Schlüssel"),
        0x78 => crate::trs!("Antwort folgt"),
        0x7E..=0x80 => crate::trs!("in dieser Sitzung nicht möglich"),
        _ => "",
    };
    if n.is_empty() { format!("NRC {nrc:#04x}") } else { format!("{n} ({nrc:#04x})") }
}

/// ISO-TP-Frames einer Nachricht (für Beispiel-Mitschnitte).
/// Liefert (CAN-ID, Daten); Flow Control kommt von `fc_id`.
pub fn isotp_frames(id: u32, fc_id: u32, msg: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let pad = |mut v: Vec<u8>| {
        v.resize(8, 0xAA);
        v
    };
    if msg.len() <= 7 {
        return vec![(id, pad([vec![msg.len() as u8], msg.to_vec()].concat()))];
    }
    let mut out = vec![(id, pad([vec![0x10 | (msg.len() >> 8) as u8, msg.len() as u8], msg[..6].to_vec()].concat()))];
    out.push((fc_id, pad(vec![0x30, 0x00, 0x00])));
    for (i, c) in msg[6..].chunks(7).enumerate() {
        out.push((id, pad([vec![0x20 | ((i + 1) & 0x0F) as u8], c.to_vec()].concat())));
    }
    out
}

/// Beispiel-Mitschnitt „Werkstattgerät hebt Leerlauf an + ändert Codierung“
/// (für die Lernhilfe; gleicher Grundverkehr wie [`demo_log`]).
pub fn demo_log_aktion() -> String {
    use crate::sim;
    let mut frames: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut ex = |req: Vec<u8>| {
        frames.extend(isotp_frames(0x7E0, 0x7E8, &req));
        if let Some(resp) = sim::uds_response(0x7E0, &req) {
            frames.extend(isotp_frames(0x7E8, 0x7E0, &resp));
        }
    };
    ex(vec![0x10, 0x03]);
    ex(vec![0x22, 0xF4, 0x0C]);
    // Leerlaufdrehzahl 1000 → 1200 → 1400 → zurück
    for rpm in [1000u16, 1200, 1400] {
        let b = rpm.to_be_bytes();
        ex(vec![0x2F, (sim::DEMO_IDLE_DID >> 8) as u8, sim::DEMO_IDLE_DID as u8, 0x03, b[0], b[1]]);
        ex(vec![0x01, 0x0C]);
    }
    ex(vec![0x2F, (sim::DEMO_IDLE_DID >> 8) as u8, sim::DEMO_IDLE_DID as u8, 0x00]);
    // Codierung lesen und ein Bit ändern
    ex(vec![0x22, 0x06, 0x00]);
    ex(vec![0x2E, 0x06, 0x00, 0x00, 0x1E, 0x07]);
    ex(vec![0x22, 0x06, 0x00]);
    ex(vec![0x2E, 0x06, 0x00, 0x00, 0x1A, 0x07]); // zurück
    sim::reset();
    let mut out = String::from("# Beispiel-Mitschnitt: Leerlauf anheben + Codierung ändern (Demo, Werte erfunden)\n");
    for (i, (id, d)) in frames.iter().enumerate() {
        out.push_str(&format_line(1_700_000_100.0 + i as f64 * 0.005, "can0", *id, d));
        out.push('\n');
    }
    out
}

/// Erzeugt einen Beispiel-Mitschnitt aus dem Demo-Fahrzeug (UDS, OBD, TP 2.0),
/// damit der Decoder ohne Auto ausprobiert werden kann.
pub fn demo_log() -> String {
    use crate::sim;
    use crate::tp20::Tp20;
    let mut frames: Vec<(u32, Vec<u8>)> = Vec::new();
    // UDS + OBD am Motor (0x7E0/0x7E8)
    for req in [
        vec![0x10, 0x01],
        vec![0x22, 0xF1, 0x90],
        vec![0x19, 0x02, 0xFF],
        vec![0x01, 0x0C],
        vec![0x01, 0x01],
        vec![0x09, 0x02],
    ] {
        frames.extend(isotp_frames(0x7E0, 0x7E8, &req));
        if let Some(resp) = sim::uds_response(0x7E0, &req) {
            frames.extend(isotp_frames(0x7E8, 0x7E0, &resp));
        }
    }
    // TP 2.0 + KWP2000 zum Motor (0x01)
    let io = sim::DemoCanIo::with_log();
    if let Ok(mut ch) = Tp20::connect_io(&io, 0x01) {
        {
            let mut k = kwp::Kwp::new(&mut ch);
            let _ = k.start_session(kwp::SESSION_VAG_DIAG);
            let _ = k.read_ecu_id(kwp::ECU_ID_VAG);
            let _ = k.read_dtcs();
            for _ in 0..3 {
                let _ = k.read_block(1);
            }
        }
        ch.disconnect();
    }
    frames.extend(io.take_log().into_iter().map(|(id, d)| (u32::from(id), d)));
    let mut out = String::from("# Beispiel-Mitschnitt aus dem Demo-Fahrzeug (Werte erfunden)\n");
    for (i, (id, d)) in frames.iter().enumerate() {
        out.push_str(&format_line(1_700_000_000.0 + i as f64 * 0.005, "can0", *id, d));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_candump_formats() {
        let a = parse_line("(1698412345.123456) can0 7E0#0210010000000000").unwrap();
        assert_eq!((a.id, a.data[1], a.t.is_some()), (0x7E0, 0x10, true));
        let b = parse_line("  can0  7E8   [8]  06 50 01 00 32 01 F4 AA").unwrap();
        assert_eq!((b.id, b.data.len()), (0x7E8, 8));
        let c = parse_line("(1.0)  can0  200   [7]  01 C0 00 10 00 03 01").unwrap();
        assert_eq!(c.data, vec![0x01, 0xC0, 0x00, 0x10, 0x00, 0x03, 0x01]);
        let e = parse_line("(1.0) can0 17FC0076#0322F19000000000").unwrap();
        assert!(e.extended);
        assert!(parse_line("Müll").is_none());
    }

    #[test]
    fn documented_handshake_confirms_id_order() {
        let log = "\
(1.000) can0 200#01C00010000301
(1.010) can0 201#00D0000340070
(1.010) can0 201#00D00003400701
(1.020) can0 740#A00F8AFF32FF
(1.030) can0 300#A10F8AFF4AFF
(1.040) can0 740#1000021089
(1.050) can0 300#B1
(1.060) can0 300#1000025089
(1.070) can0 740#B1
(1.080) can0 740#1100021A9B
";
        let d = Decoder::decode_text(log);
        assert_eq!(d.report.unparsed_lines, 1); // ungerade Hex-Länge
        assert_eq!(d.report.setups.len(), 1);
        let s = &d.report.setups[0];
        assert_eq!((s.tester_tx, s.ecu_tx), (0x740, 0x300));
        assert_eq!(s.order_confirmed, Some(true));
        assert_eq!(d.report.length_ok, 3);
        assert_eq!(d.report.seq_continued, 1); // zweite Tester-Nachricht mit Seq 1
        assert!(d.lines().iter().any(|l| l.contains("Diagnosesitzung: Sitzung 0x89")));
    }

    #[test]
    fn swapped_ids_are_reported() {
        let log = "\
(1.0) can0 200#01C00010000301
(1.0) can0 201#00D00003400701
(1.0) can0 300#A00F8AFF32FF
";
        let d = Decoder::decode_text(log);
        assert_eq!(d.report.setups[0].order_confirmed, Some(false));
        assert!(d.report.text().contains("VERTAUSCHT"));
    }

    #[test]
    fn demo_log_roundtrip() {
        let log = demo_log();
        let d = Decoder::decode_text(&log);
        let text = d.lines().join("\n");
        assert!(text.contains("VIN WVWZZZ1KZAW000001"), "{text}");
        assert!(text.contains("Drehzahl ="), "{text}");
        assert!(text.contains("Block 001:"), "{text}");
        assert!(text.contains("03G906016"), "{text}");
        assert_eq!(d.report.setups[0].order_confirmed, Some(true));
        assert_eq!(d.report.length_bad, 0);
    }
}
