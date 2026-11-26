//! K-Leitungs-Steuergeräte hinter dem Gateway („K-Leitung im CAN-Tunnel“).
//!
//! Manche ältere Steuergeräte hängen im Auto an einer K-Leitung hinter dem
//! Gateway, z. B. am Polo 6R (2010) ABS (03) und Lenkhilfe (44). Am
//! TP-2.0-Kanalaufbau antworten sie nur kurz mit `00 D0 xx`. Das Gateway reicht
//! die K-Leitung dann über zwei feste CAN-IDs durch. Aufbau aus einem eigenen
//! Mitschnitt (Werkstattgerät am Y-Kabel, Polo 6R, 26.11.2026):
//!
//! 1. `200#<Ziel> C0 A0` → `2<Ziel>#00 D0 <rx>`: Tester sendet ab jetzt auf
//!    0x7A0, das Gateway antwortet auf 0x7<rx> (ABS 0x7A3, Lenkhilfe 0x7A9).
//! 2. Kanalparameter wie bei TP 2.0: `A0 0F 8A FF 32 FF` → `A1 …`.
//! 3. Nachrichten in Frames mit Opcode/Zähler im ersten Byte wie TP 2.0
//!    (`2n` weitere folgen, `3n` letzter Frame; `0n`/`1n` vom Steuergerät
//!    verlangen eine Quittung `B(n+1)`), aber **ohne Längenfeld**.
//! 4. Erste Nachricht: die 5-Baud-Adresse (mit ungerader Parität, z. B. `83`
//!    für 03, `C4` für 44). Antwort: Sync und Schlüsselwort (`55 01 8A` =
//!    KW1281, `55 6B 8F` = KWP2000). Der Tester schickt das Komplement des
//!    letzten Bytes; bei KWP2000 kommt die komplementierte Adresse zurück.
//! 5. Danach ganze KW1281-Blöcke bzw. KWP2000-Nachrichten mit Kopf
//!    (`8L Ziel F1 Daten… Prüfsumme`).
//!
//! **Nur lesen**: Identifikation, Fehlerspeicher, Messwertgruppen.

use crate::kw1281::Leitung;
use crate::tp20::CanIo;
use crate::tr;
use anyhow::{bail, Context, Result};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const SETUP_ID: u16 = 0x200;
const TESTER_ID: u16 = 0x7A0;
const PARAMETER: [u8; 6] = [0xA0, 0x0F, 0x8A, 0xFF, 0x32, 0xFF];

/// Mit `VAG_TUNNEL_SPUR=1` jeden Frame auf stderr (Fehlersuche am Auto).
fn spur(r: &str, f: &[u8]) {
    if std::env::var("VAG_TUNNEL_SPUR").is_ok() {
        eprintln!("{r} {}", crate::kwp::umgebung_hex(f));
    }
}

/// Bekannte Steuergeräte im Tunnel: (VCDS-Adresse, Ziel am Kanalaufbau,
/// KWP2000-Adresse, Name). Aus Mitschnitten am Polo 6R.
pub const BEKANNT: &[(u8, u8, u8, &str)] = &[(0x03, 0x03, 0x28, "Bremsen (ABS)"), (0x44, 0x09, 0x00, "Lenkhilfe")];

/// Ziel am Kanalaufbau zur Steuergeräte-Adresse (Tabelle, sonst gleich).
pub fn ziel_fuer(adresse: u8) -> u8 {
    BEKANNT.iter().find(|b| b.0 == adresse).map(|b| b.1).unwrap_or(adresse)
}

/// Steuergeräte-Adresse (5-Baud) zum Ziel am Kanalaufbau.
pub fn adresse_fuer(ziel: u8) -> u8 {
    BEKANNT.iter().find(|b| b.1 == ziel).map(|b| b.0).unwrap_or(ziel)
}

/// Ergebnis für den Scan.
pub struct TunnelGeraet {
    pub name: Option<&'static str>,
    pub rx: u16,
    pub protokoll: &'static str,
    pub ident: Vec<String>,
    pub dtcs: Vec<crate::kwp::KwpDtc>,
}

/// Identifikation und Fehlerspeicher eines Tunnel-Steuergeräts (nur lesen).
pub fn lesen<I: CanIo>(io: I, ziel: u8) -> Result<TunnelGeraet> {
    let adresse = adresse_fuer(ziel);
    let name = BEKANNT.iter().find(|b| b.1 == ziel).map(|b| b.3);
    let mut t = KTunnel::oeffnen(io, ziel)?;
    let rx = t.rx;
    let kw = t.anmelden(adresse)?;
    if kw == [0x01, 0x8A] {
        let mut k = crate::kw1281::Kw1281::nach_fuenf_baud(t)?;
        let f = k.fehler_lesen();
        let mut ident = k.ident.clone();
        k.beenden();
        let f = f?;
        // Fehlerart der K-Leitung (7 Bit) passt nicht ins TP-2.0-Statusbyte:
        // dort nur sporadisch/vorhanden, die Fehlerart als eigene Zeile
        for (c, st) in &f {
            ident.push(format!("{c:05}: {}", crate::kw1281::fehlerart(*st)));
        }
        let dtcs = f.into_iter().map(|(code, st)| crate::kwp::KwpDtc { code, status: if st & 0x80 != 0 { 0x20 } else { 0x60 } }).collect();
        Ok(TunnelGeraet { name, rx, protokoll: "KW1281", ident, dtcs })
    } else {
        let kz = BEKANNT.iter().find(|b| b.1 == ziel && b.2 != 0).map(|b| b.2).context(tr!("KWP2000-Adresse unbekannt — mit --kwp-ziel angeben"))?;
        let mut k = KwpK::verbinden(t, adresse, kw, kz)?;
        let ident: Vec<String> = [0xF197u16, 0xF187, 0xF189].iter().filter_map(|d| k.text(*d)).collect();
        let f = k.fehler_lesen();
        k.beenden();
        Ok(TunnelGeraet { name, rx, protokoll: "KWP2000", ident, dtcs: f? })
    }
}

/// Adresse mit ungerader Parität in Bit 7 (wie bei der 5-Baud-Anmeldung).
pub fn mit_paritaet(adresse: u8) -> u8 {
    let a = adresse & 0x7F;
    if a.count_ones().is_multiple_of(2) {
        a | 0x80
    } else {
        a
    }
}

/// Ist das die kurze Antwort eines Tunnel-Steuergeräts auf den Kanalaufbau?
pub fn ist_tunnel_antwort(r: &[u8]) -> bool {
    r.len() == 3 && r[0] == 0x00 && r[1] == 0xD0
}

/// Geöffneter Tunnel zu einem Steuergerät.
pub struct KTunnel<I: CanIo> {
    io: I,
    rx: u16,
    puffer: VecDeque<u8>,
}

impl<I: CanIo> KTunnel<I> {
    /// Kanal aufbauen (Schritte 1 und 2).
    pub fn oeffnen(io: I, ziel: u8) -> Result<Self> {
        io.set_timeout(Duration::from_millis(500))?;
        io.write(SETUP_ID, &[ziel, 0xC0, (TESTER_ID & 0xFF) as u8])?;
        let r = io.read(SETUP_ID + u16::from(ziel)).with_context(|| tr!("Keine Antwort auf {a} ({b})", a = format!("{ziel:02X}"), b = "Tunnel"))?;
        if !ist_tunnel_antwort(&r) {
            bail!(tr!("Steuergerät {a} ist kein K-Leitungs-Gerät hinter dem Gateway (Antwort {b})", a = format!("{ziel:02X}"), b = crate::kwp::umgebung_hex(&r)));
        }
        let rx = 0x700 | u16::from(r[2]);
        io.set_timeout(Duration::from_millis(1500))?;
        io.write(TESTER_ID, &PARAMETER)?;
        let p = io.read(rx).context(tr!("Tunnel: keine Kanalparameter"))?;
        if p.first() != Some(&0xA1) {
            bail!(tr!("Tunnel: unerwartete Antwort {a}", a = crate::kwp::umgebung_hex(&p)));
        }
        Ok(Self { io, rx, puffer: VecDeque::new() })
    }

    /// Nachricht senden: Frames zu 7 Byte, `2n` … `3n`.
    pub fn senden_nachricht(&mut self, daten: &[u8]) -> Result<()> {
        let teile: Vec<&[u8]> = if daten.is_empty() { vec![&[][..]] } else { daten.chunks(7).collect() };
        for (i, t) in teile.iter().enumerate() {
            let op = if i + 1 == teile.len() { 0x30 } else { 0x20 };
            let mut f = vec![op | (i as u8 & 0x0F)];
            f.extend_from_slice(t);
            spur(">", &f);
            self.io.write(TESTER_ID, &f)?;
            if i + 1 < teile.len() {
                std::thread::sleep(Duration::from_millis(3));
            }
        }
        Ok(())
    }

    /// Nachricht empfangen (Quittungen werden gesendet, Kanal-Frames übergangen).
    pub fn empfangen_nachricht(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let ende = Instant::now() + Duration::from_secs(5);
        while Instant::now() < ende {
            let f = self.io.read(self.rx).context(tr!("K-Leitung: Zeitüberschreitung"))?;
            spur("<", &f);
            let Some(&k) = f.first() else { continue };
            let (op, n) = (k >> 4, k & 0x0F);
            match op {
                0x0..=0x3 => {
                    out.extend_from_slice(&f[1..]);
                    if op <= 1 {
                        self.io.write(TESTER_ID, &[0xB0 | ((n + 1) & 0x0F)])?;
                    }
                    if op == 1 || op == 3 {
                        if op == 3 {
                            // so macht es das Werkstattgerät
                            self.io.write(TESTER_ID, &[0x30])?;
                        }
                        return Ok(out);
                    }
                }
                0xA if k == 0xA8 => bail!(tr!("Tunnel: Gateway hat den Kanal geschlossen")),
                _ => {} // Quittungen, Kanalparameter
            }
        }
        bail!(tr!("K-Leitung: Zeitüberschreitung"))
    }

    /// Kanal schließen.
    pub fn schliessen(&mut self) {
        let _ = self.io.write(TESTER_ID, &[0xA8]);
    }

    /// Anmeldung: 5-Baud-Adresse senden, Sync und Schlüsselwort lesen.
    /// Gibt das Schlüsselwort zurück (`01 8A` = KW1281, sonst KWP2000).
    pub fn anmelden(&mut self, adresse: u8) -> Result<[u8; 2]> {
        self.senden_nachricht(&[mit_paritaet(adresse)])?;
        let a = self.empfangen_nachricht().context(tr!("K-Leitung: keine Antwort auf die Anmeldung (Zündung an? Adresse richtig?)"))?;
        match a.as_slice() {
            [0x55, k1, k2, ..] => {
                // für Kw1281::nach_fuenf_baud aufheben
                self.puffer = a.iter().copied().collect();
                Ok([*k1, *k2])
            }
            _ => bail!(tr!("K-Leitung: Sync {a} statt 55 — falsche Baudrate?", a = crate::kwp::umgebung_hex(&a))),
        }
    }
}

/// KW1281 über den Tunnel: Anmeldung über [`Leitung::fuenf_baud`], danach ganze Blöcke.
impl<I: CanIo> Leitung for KTunnel<I> {
    fn senden(&mut self, b: u8) -> Result<()> {
        self.senden_nachricht(&[b])
    }
    fn empfangen(&mut self, _timeout: Duration) -> Result<u8> {
        if self.puffer.is_empty() {
            let n = self.empfangen_nachricht()?;
            self.puffer.extend(n);
        }
        self.puffer.pop_front().context(tr!("K-Leitung: Zeitüberschreitung"))
    }
    fn fuenf_baud(&mut self, adresse: u8) -> Result<()> {
        self.senden_nachricht(&[mit_paritaet(adresse)])
    }
    fn ganze_bloecke(&self) -> bool {
        true
    }
    fn block_raus(&mut self, block: &[u8]) -> Result<()> {
        self.senden_nachricht(block)
    }
    fn block_rein(&mut self) -> Result<Vec<u8>> {
        let mut b: Vec<u8> = self.puffer.drain(..).collect();
        if b.is_empty() {
            b = self.empfangen_nachricht()?;
        }
        Ok(b)
    }
}

/// KWP2000 mit K-Leitungs-Kopf über den Tunnel (z. B. ABS am Polo 6R).
pub struct KwpK<I: CanIo> {
    pub tunnel: KTunnel<I>,
    ziel: u8,
}

/// Nachricht mit Kopf: `8L Ziel F1 Daten… Prüfsumme` (L = Länge bis 63).
pub fn kwp_rahmen(ziel: u8, daten: &[u8]) -> Vec<u8> {
    let mut m = if daten.len() < 64 { vec![0x80 | daten.len() as u8, ziel, 0xF1] } else { vec![0x80, ziel, 0xF1, daten.len() as u8] };
    m.extend_from_slice(daten);
    let cs = m.iter().fold(0u8, |a, b| a.wrapping_add(*b));
    m.push(cs);
    m
}

/// Daten aus einer Nachricht mit Kopf (prüft die Prüfsumme).
pub fn kwp_daten(m: &[u8]) -> Result<Vec<u8>> {
    let fehler = || anyhow::anyhow!(tr!("KWP2000: ungültige Nachricht {a}", a = crate::kwp::umgebung_hex(m)));
    let (&fmt, _) = m.split_first().ok_or_else(fehler)?;
    let (len, ab) = if fmt & 0x3F == 0 { (*m.get(3).ok_or_else(fehler)? as usize, 4) } else { ((fmt & 0x3F) as usize, 3) };
    let d = m.get(ab..ab + len).ok_or_else(fehler)?;
    let cs = *m.get(ab + len).ok_or_else(fehler)?;
    if m[..ab + len].iter().fold(0u8, |a, b| a.wrapping_add(*b)) != cs {
        bail!(tr!("KWP2000: Prüfsumme falsch"));
    }
    Ok(d.to_vec())
}

impl<I: CanIo> KwpK<I> {
    /// Nach der Anmeldung (Schlüsselwort ≠ 01 8A): Komplement senden, Adresse zurück.
    pub fn verbinden(mut tunnel: KTunnel<I>, adresse: u8, kw: [u8; 2], ziel: u8) -> Result<Self> {
        tunnel.puffer.clear();
        tunnel.senden_nachricht(&[!kw[1]])?;
        let a = tunnel.empfangen_nachricht()?;
        if a.first() != Some(&!mit_paritaet(adresse)) {
            bail!(tr!("KWP2000: Anmeldung nicht bestätigt ({a})", a = crate::kwp::umgebung_hex(&a)));
        }
        let mut k = Self { tunnel, ziel };
        k.anfrage(&[0x10, 0x89])?;
        Ok(k)
    }

    /// Anfrage senden, Antwort-Daten (ohne Kopf) zurück; wartet bei „Antwort folgt“.
    pub fn anfrage(&mut self, daten: &[u8]) -> Result<Vec<u8>> {
        // Abstand wie beim Werkstattgerät (K-Leitung hinter dem Gateway ist langsam)
        std::thread::sleep(Duration::from_millis(60));
        self.tunnel.senden_nachricht(&kwp_rahmen(self.ziel, daten))?;
        for _ in 0..40 {
            let d = kwp_daten(&self.tunnel.empfangen_nachricht()?)?;
            match d.as_slice() {
                [0x7F, _, 0x78, ..] => continue,
                [0x7F, s, nrc, ..] => bail!(tr!("KWP2000: Dienst {a} abgelehnt (NRC {b})", a = format!("{s:02X}"), b = format!("0x{nrc:02X}"))),
                [s, ..] if *s == daten[0] | 0x40 => return Ok(d),
                _ => continue,
            }
        }
        bail!(tr!("K-Leitung: Antwort endet nicht"))
    }

    /// Kennung lesen (Dienst 0x22) als Text.
    pub fn text(&mut self, did: u16) -> Option<String> {
        let r = self.anfrage(&[0x22, (did >> 8) as u8, did as u8]).ok()?;
        let s = String::from_utf8_lossy(r.get(3..)?).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// Fehlerspeicher (Dienst 0x18).
    pub fn fehler_lesen(&mut self) -> Result<Vec<crate::kwp::KwpDtc>> {
        let r = self.anfrage(&[0x18, 0x02, 0xFF, 0x00])?;
        Ok(crate::kwp::parse_dtcs(&r[1..]))
    }

    pub fn beenden(mut self) {
        self.tunnel.schliessen();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paritaet_wie_mitschnitt() {
        assert_eq!(mit_paritaet(0x03), 0x83);
        assert_eq!(mit_paritaet(0x44), 0xC4);
        assert_eq!(mit_paritaet(0x01), 0x01);
    }

    #[test]
    fn kwp_kopf_wie_mitschnitt() {
        // Polo 6R ABS: 7A0#308228F1108934 → 7A3#1082F128508974
        assert_eq!(kwp_rahmen(0x28, &[0x10, 0x89]), vec![0x82, 0x28, 0xF1, 0x10, 0x89, 0x34]);
        assert_eq!(kwp_daten(&[0x82, 0xF1, 0x28, 0x50, 0x89, 0x74]).unwrap(), vec![0x50, 0x89]);
        assert_eq!(kwp_rahmen(0x28, &[0x3E]), vec![0x81, 0x28, 0xF1, 0x3E, 0xD8]);
        assert!(kwp_daten(&[0x82, 0xF1, 0x28, 0x50, 0x89, 0x75]).is_err());
        assert!(ist_tunnel_antwort(&[0x00, 0xD0, 0xA3]));
        assert!(!ist_tunnel_antwort(&[0x00, 0xD0, 0x00, 0x03, 0x2E, 0x03]));
    }

    /// Simuliertes Gateway mit Lenkhilfe (KW1281) nach dem Mitschnitt.
    struct SimLenkhilfe {
        raus: std::cell::RefCell<VecDeque<(u16, Vec<u8>)>>,
        zaehler: std::cell::Cell<u8>,
    }

    impl SimLenkhilfe {
        fn block(&self, t: u8, d: &[u8]) {
            let z = self.zaehler.get().wrapping_add(1);
            self.zaehler.set(z.wrapping_add(1));
            let mut b = vec![(d.len() + 3) as u8, z, t];
            b.extend_from_slice(d);
            b.push(3);
            let teile: Vec<&[u8]> = b.chunks(7).collect();
            for (i, c) in teile.iter().enumerate() {
                let op = if i + 1 == teile.len() { 0x10 } else { 0x20 };
                let mut f = vec![op | i as u8];
                f.extend_from_slice(c);
                self.raus.borrow_mut().push_back((0x7A9, f));
            }
        }
    }

    impl CanIo for SimLenkhilfe {
        fn write(&self, id: u16, d: &[u8]) -> Result<()> {
            let mut r = self.raus.borrow_mut();
            match (id, d) {
                (0x200, [0x09, 0xC0, 0xA0]) => r.push_back((0x209, vec![0x00, 0xD0, 0xA9])),
                (0x7A0, [0xA0, ..]) => r.push_back((0x7A9, vec![0xA1, 0x03, 0xC2, 0x54, 0x4A, 0xD0])),
                (0x7A0, [0x30, 0xC4]) => r.push_back((0x7A9, vec![0x10, 0x55, 0x01, 0x8A])),
                (0x7A0, [0x30, 0x75]) => {
                    drop(r);
                    self.zaehler.set(0);
                    self.block(0xF6, b"6R0423156B ");
                }
                (0x7A0, [0x30, _len, z, t, ..]) => {
                    drop(r);
                    self.zaehler.set(*z);
                    match *t {
                        0x09 if *z < 4 => self.block(0x09, &[]),
                        0x07 => {
                            self.block(0xFC, &[0x02, 0x62, 0x23]);
                        }
                        0x09 => self.block(0x09, &[]),
                        _ => {}
                    }
                }
                _ => {}
            }
            Ok(())
        }
        fn read(&self, id: u16) -> Result<Vec<u8>> {
            let mut r = self.raus.borrow_mut();
            let i = r.iter().position(|(i, _)| *i == id).context("Timeout")?;
            Ok(r.remove(i).unwrap().1)
        }
    }

    #[test]
    fn lenkhilfe_kw1281_im_tunnel() {
        let sim = SimLenkhilfe { raus: Default::default(), zaehler: Default::default() };
        let mut t = KTunnel::oeffnen(&sim, 0x09).unwrap();
        // Anmeldung wie Kw1281::verbinden, aber über den Tunnel
        t.fuenf_baud(0x44).unwrap();
        let mut k = crate::kw1281::Kw1281::nach_fuenf_baud(t).unwrap();
        assert_eq!(k.ident, vec!["6R0423156B".to_string()]);
        let f = k.fehler_lesen().unwrap();
        assert_eq!(f, vec![(0x0262, 0x23)]);
    }
}
