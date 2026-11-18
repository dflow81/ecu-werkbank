//! CAN-Mitschnitte in andere Formate umwandeln, damit man sie mit gängigen
//! Werkzeugen ansehen kann:
//!
//! - **PCAP** (Linktyp 227 = SocketCAN) — öffnet Wireshark direkt
//! - **ASC** (Vector-ASCII) — lesen viele CAN-Werkzeuge (CANalyzer, SavvyCAN, PCAN-View)
//!
//! Eingabe ist ein Mitschnitt im candump-Format (siehe [`crate::canlog::parse_line`]).
//! BLF und MDF4 sind Binärformate mit Kompression — siehe ROADMAP.md.

use crate::canlog::{parse_line, Frame};

/// Ausgabeformat nach Dateiendung (.pcap / .asc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pcap,
    Asc,
}

impl Format {
    pub fn aus_pfad(p: &std::path::Path) -> Option<Format> {
        match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "pcap" => Some(Format::Pcap),
            "asc" => Some(Format::Asc),
            _ => None,
        }
    }
}

/// Alle Frames eines candump-Textes (Zeilen ohne Frame werden übersprungen).
pub fn frames(text: &str) -> Vec<Frame> {
    text.lines().filter_map(parse_line).collect()
}

/// Mitschnitt als PCAP (Linktyp 227, SocketCAN-Frames, Zeit in µs).
pub fn pcap(frames: &[Frame]) -> Vec<u8> {
    let mut o = Vec::with_capacity(24 + frames.len() * 32);
    o.extend_from_slice(&0xA1B2_C3D4u32.to_le_bytes()); // Magic, µs-Auflösung
    o.extend_from_slice(&2u16.to_le_bytes());
    o.extend_from_slice(&4u16.to_le_bytes());
    o.extend_from_slice(&0i32.to_le_bytes()); // Zeitzone
    o.extend_from_slice(&0u32.to_le_bytes()); // Genauigkeit
    o.extend_from_slice(&65535u32.to_le_bytes()); // snaplen
    o.extend_from_slice(&227u32.to_le_bytes()); // LINKTYPE_CAN_SOCKETCAN
    for f in frames {
        let t = f.t.unwrap_or(0.0).max(0.0);
        let s = t.trunc() as u32;
        let us = ((t.fract() * 1e6).round() as u32).min(999_999);
        let n = f.data.len().min(64);
        // struct can_frame: ID (Netzwerk-Byte-Reihenfolge, Bit 31 = 29-Bit-ID), Länge, 3 frei, Daten
        let mut id = f.id;
        if f.extended {
            id |= 0x8000_0000;
        }
        let mut rec = Vec::with_capacity(8 + n.max(8));
        rec.extend_from_slice(&id.to_be_bytes());
        rec.push(n as u8);
        rec.extend_from_slice(&[0, 0, 0]);
        rec.extend_from_slice(&f.data[..n]);
        if n < 8 {
            rec.resize(16, 0);
        }
        o.extend_from_slice(&s.to_le_bytes());
        o.extend_from_slice(&us.to_le_bytes());
        o.extend_from_slice(&(rec.len() as u32).to_le_bytes());
        o.extend_from_slice(&(rec.len() as u32).to_le_bytes());
        o.extend_from_slice(&rec);
    }
    o
}

/// Mitschnitt als Vector-ASC (Zeit relativ zum ersten Frame, Kanal 1).
pub fn asc(frames: &[Frame]) -> String {
    let t0 = frames.iter().find_map(|f| f.t).unwrap_or(0.0);
    let mut o = String::from("date Thu Jan 1 00:00:00.000 am 1970\nbase hex  timestamps absolute\nno internal events logged\n");
    for f in frames {
        let t = f.t.map(|t| t - t0).unwrap_or(0.0);
        let id = if f.extended { format!("{:X}x", f.id) } else { format!("{:X}", f.id) };
        let daten: Vec<String> = f.data.iter().map(|b| format!("{b:02X}")).collect();
        o.push_str(&format!("{t:>11.6} 1  {id:<15} Rx   d {} {}\n", f.data.len(), daten.join(" ")));
    }
    o
}

/// In das gewünschte Format umwandeln.
pub fn umwandeln(text: &str, format: Format) -> Vec<u8> {
    let f = frames(text);
    match format {
        Format::Pcap => pcap(&f),
        Format::Asc => asc(&f).into_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "(1000.500000) can0 7E0#0210010000000000\n(1000.750000) can0 18DAF110#0322F19000\n";

    #[test]
    fn pcap_aufbau() {
        let p = umwandeln(LOG, Format::Pcap);
        assert_eq!(&p[..4], &[0xD4, 0xC3, 0xB2, 0xA1]);
        assert_eq!(u32::from_le_bytes(p[20..24].try_into().unwrap()), 227);
        // erster Datensatz: 1000 s, 500000 µs, 16 Byte
        assert_eq!(u32::from_le_bytes(p[24..28].try_into().unwrap()), 1000);
        assert_eq!(u32::from_le_bytes(p[28..32].try_into().unwrap()), 500_000);
        assert_eq!(&p[40..45], &[0x00, 0x00, 0x07, 0xE0, 8]);
        // zweiter: 29-Bit-ID mit Kennzeichen, Länge 5
        let r2 = 24 + 16 + 16 + 16;
        assert_eq!(&p[r2..r2 + 5], &[0x98, 0xDA, 0xF1, 0x10, 5]);
        assert_eq!(p.len(), r2 + 16);
    }

    #[test]
    fn asc_aufbau() {
        let a = asc(&frames(LOG));
        let z: Vec<&str> = a.lines().collect();
        assert!(z[3].starts_with("   0.000000 1  7E0"), "{}", z[3]);
        assert!(z[3].ends_with("d 8 02 10 01 00 00 00 00 00"));
        assert!(z[4].contains("18DAF110x") && z[4].starts_with("   0.250000"));
    }
}
