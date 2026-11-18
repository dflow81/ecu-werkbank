//! CLI: OBD-II.
//! Aus main.rs ausgelagert (Roadmap 27); Aufruf über `enum Command` in main.rs.

use super::*;

// ---------------------------------------------------------------------------
// OBD-II
// ---------------------------------------------------------------------------

pub(super) fn obd_info(be: &Backend, tx: u16, rx: u16) -> Result<()> {
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

pub(super) fn obd_live(
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

pub(super) fn obd_dtc(be: &Backend, tx: u16, rx: u16) -> Result<()> {
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
