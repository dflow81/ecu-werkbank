//! Mitschnitt abspielen — **nur auf ein virtuelles CAN-Interface (vcan)**,
//! nie auf ein echtes Auto. Gedacht zum Testen: GUI, CAN-Monitor oder
//! eigene Werkzeuge bekommen „echten“ Verkehr, ohne dass ein Auto dran ist.
//!
//! vcan einrichten (einmal, braucht Administratorrechte):
//! `sudo modprobe vcan && sudo ip link add vcan0 type vcan && sudo ip link set vcan0 up`

use crate::canlog::Frame;
use anyhow::{bail, Context, Result};
use std::time::{Duration, Instant};

/// Art eines Interfaces aus der Ausgabe von `ip -details link show` („vcan“, „can“ …).
pub fn art_aus_ip(ausgabe: &str) -> Option<String> {
    // Zeile wie: "    link/can  promiscuity 0 ... \n    vcan addrgenmode ..." bzw. "can state ERROR-ACTIVE ..."
    ausgabe.lines().skip(1).map(str::trim).find_map(|z| {
        let wort = z.split_whitespace().next()?;
        matches!(wort, "vcan" | "can" | "vxcan" | "slcan").then(|| wort.to_string())
    })
}

/// Prüft, dass `iface` ein virtuelles CAN-Interface ist. Sonst Fehler — abgespielt
/// wird dann nichts.
pub fn nur_vcan(iface: &str) -> Result<()> {
    let out = std::process::Command::new("ip").args(["-details", "link", "show", iface]).output().context(crate::tr!("„ip“ nicht gefunden"))?;
    if !out.status.success() {
        bail!(crate::tr!("Interface {a} gibt es nicht. vcan einrichten: sudo modprobe vcan && sudo ip link add {a} type vcan && sudo ip link set {a} up", a = iface));
    }
    match art_aus_ip(&String::from_utf8_lossy(&out.stdout)).as_deref() {
        Some("vcan") => Ok(()),
        Some(a) => bail!(crate::tr!("{i} ist ein echtes CAN-Interface ({a}). Abspielen geht nur auf vcan — nie aufs Auto.", i = iface, a = a)),
        None => bail!(crate::tr!("{i} ist kein CAN-Interface.", i = iface)),
    }
}

/// Wartezeiten zwischen den Frames: Abstand der Zeitstempel, geteilt durch `tempo`.
/// Frames ohne Zeitstempel kommen ohne Pause; Rücksprünge zählen als 0.
pub fn pausen(frames: &[Frame], tempo: f64) -> Vec<Duration> {
    let tempo = if tempo > 0.0 { tempo } else { 1.0 };
    let mut vorher: Option<f64> = None;
    frames
        .iter()
        .map(|f| {
            let p = match (vorher, f.t) {
                (Some(a), Some(b)) if b > a => Duration::from_secs_f64(((b - a) / tempo).min(10.0)),
                _ => Duration::ZERO,
            };
            if f.t.is_some() {
                vorher = f.t;
            }
            p
        })
        .collect()
}

/// Frames im Takt abspielen; `senden` schickt einen Frame, `weiter` kann abbrechen.
/// Gibt die Zahl gesendeter Frames zurück. Pausen über 10 s werden gekürzt.
pub fn abspielen(frames: &[Frame], tempo: f64, mut senden: impl FnMut(&Frame) -> Result<()>, mut weiter: impl FnMut() -> bool) -> Result<usize> {
    let start = Instant::now();
    let mut soll = Duration::ZERO;
    for (n, (f, p)) in frames.iter().zip(pausen(frames, tempo)).enumerate() {
        if !weiter() {
            return Ok(n);
        }
        soll += p;
        if let Some(rest) = soll.checked_sub(start.elapsed()) {
            std::thread::sleep(rest);
        }
        senden(f)?;
    }
    Ok(frames.len())
}

/// Mitschnitt auf ein vcan-Interface abspielen (prüft vorher [`nur_vcan`]).
pub fn auf_vcan(iface: &str, frames: &[Frame], tempo: f64, weiter: impl FnMut() -> bool) -> Result<usize> {
    use crate::can::{CanFrame, CanSocket, EmbeddedFrame, ExtendedId, Socket, StandardId};
    nur_vcan(iface)?;
    let sock = CanSocket::open(iface).with_context(|| crate::tr!("{a} öffnen", a = iface))?;
    abspielen(
        frames,
        tempo,
        |f| {
            let frame = if f.extended {
                ExtendedId::new(f.id).and_then(|id| CanFrame::new(id, &f.data))
            } else {
                u16::try_from(f.id).ok().and_then(StandardId::new).and_then(|id| CanFrame::new(id, &f.data))
            };
            let Some(frame) = frame else { return Ok(()) }; // CAN FD/zu lang: überspringen
            sock.write_frame(&frame).context(crate::tr!("Senden auf {a}", a = iface))?;
            Ok(())
        },
        weiter,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_art_erkennen() {
        let vcan = "5: vcan0: <NOARP,UP,LOWER_UP> mtu 72 qdisc noqueue state UNKNOWN mode DEFAULT group default qlen 1000\n    link/can  promiscuity 0  allmulti 0 minmtu 0 maxmtu 0 \n    vcan addrgenmode none numtxqueues 1 numrxqueues 1\n";
        let echt = "4: can0: <NOARP,UP,LOWER_UP,ECHO> mtu 16 qdisc pfifo_fast state UP mode DEFAULT group default qlen 10\n    link/can  promiscuity 0  allmulti 0 minmtu 0 maxmtu 0 \n    can state ERROR-ACTIVE (berr-counter tx 0 rx 0) restart-ms 0 \n\t  bitrate 500000 sample-point 0.875\n";
        let eth = "2: eth0: <BROADCAST,MULTICAST,UP> mtu 1500\n    link/ether 00:11:22:33:44:55 brd ff:ff:ff:ff:ff:ff\n";
        assert_eq!(art_aus_ip(vcan).as_deref(), Some("vcan"));
        assert_eq!(art_aus_ip(echt).as_deref(), Some("can"));
        assert_eq!(art_aus_ip(eth), None);
    }

    #[test]
    fn echtes_interface_wird_abgelehnt() {
        // „lo“ gibt es überall und ist kein CAN — darf nie angenommen werden
        assert!(nur_vcan("lo").is_err());
        assert!(nur_vcan("gibt-es-nicht0").is_err());
    }

    fn fr(t: Option<f64>) -> Frame {
        Frame { t, id: 0x100, extended: false, data: vec![1] }
    }

    #[test]
    fn takt_und_tempo() {
        let f = [fr(Some(10.0)), fr(Some(10.5)), fr(None), fr(Some(11.0)), fr(Some(9.0)), fr(Some(100.0))];
        let p = pausen(&f, 2.0);
        assert_eq!(p[0], Duration::ZERO);
        assert_eq!(p[1], Duration::from_millis(250));
        assert_eq!(p[2], Duration::ZERO);
        assert_eq!(p[3], Duration::from_millis(250), "ohne Zeitstempel zählt der letzte bekannte");
        assert_eq!(p[4], Duration::ZERO, "Rücksprung");
        assert_eq!(p[5], Duration::from_secs(10), "lange Pause gekürzt");
        let mut gesendet = 0;
        let start = Instant::now();
        let n = abspielen(&f[..2], 10.0, |_| {
            gesendet += 1;
            Ok(())
        }, || true)
        .unwrap();
        assert_eq!((n, gesendet), (2, 2));
        assert!(start.elapsed() >= Duration::from_millis(45));
        let n = abspielen(&f, 1.0, |_| Ok(()), || false).unwrap();
        assert_eq!(n, 0, "Abbruch");
    }
}
