//! CLI: Sprache, Service-Anzeige, Schnell-Check, Batterie, Handy-Anzeige.
//! Aus main.rs ausgelagert (Roadmap 27); Aufruf über `enum Command` in main.rs.

use super::*;

// ---------------------------------------------------------------------------
// Schnell-Check, Batterie
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Service-Anzeige
// ---------------------------------------------------------------------------

pub(super) fn sprache_cmd(setzen: Option<String>, vorlage: Option<String>, datei: Option<PathBuf>, verwaist: bool, aufraeumen: bool) -> Result<()> {
    use ecu_werkbank::i18n;
    if verwaist || aufraeumen {
        let p = datei.context(tr!("Bitte mit --datei die Sprachdatei angeben, z. B. --datei lang/en.toml"))?;
        let text = ecu_werkbank::ablage::lesen(&p).with_context(|| tr!("Lesen: {a}", a = p.display()))?;
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
            ecu_werkbank::ablage::schreiben(&p, neu)?;
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
        ecu_werkbank::ablage::schreiben(&p, i18n::vorlage(&c))?;
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
pub(super) fn service_cfg(be: &Backend, fz: Option<&str>) -> Result<(Option<ecu_werkbank::vehicle::Vehicle>, service::Config)> {
    let v = match fz {
        Some(q) => Some(ecu_werkbank::vehicle::find(q).with_context(|| tr!("Fahrzeug '{q}' unbekannt", q = q))?),
        // am echten Auto sonst das in der Oberfläche gewählte Fahrzeug
        None if !matches!(be, Backend::Demo) => load_active().and_then(|k| ecu_werkbank::vehicle::load(&k)),
        None => None,
    };
    let cfg = service::config_fuer(be, v.as_ref());
    Ok((v, cfg))
}

pub(super) fn service_ordner(be: &Backend, v: Option<&ecu_werkbank::vehicle::Vehicle>) -> Result<std::path::PathBuf> {
    service::ordner(be, v).context(tr!("Bitte das Fahrzeug angeben: -f \"B XY 123\" (die Daten werden im Fahrzeugordner gespeichert)"))
}

use service::merk_pfad;

pub(super) fn service_cmd(be: &Backend, a: ServiceAktion, fz: Option<&str>) -> Result<()> {
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

pub(super) fn need_vehicle(q: Option<&str>) -> Result<ecu_werkbank::vehicle::Vehicle> {
    let q = q.context(tr!("Bitte das Fahrzeug angeben: -f \"B XY 123\" (siehe `ecu-werkbank fahrzeuge`)"))?;
    ecu_werkbank::vehicle::find(q).with_context(|| tr!("Fahrzeug '{q}' unbekannt", q = q))
}

pub(super) fn quick_check(be: &Backend, db: &LabelDb, fahrzeug: Option<&str>) -> Result<()> {
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

pub(super) fn battery_test(be: &Backend, seconds: u64, fahrzeug: Option<&str>) -> Result<()> {
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
    ecu_werkbank::ablage::schreiben(&out, ecu_werkbank::chart::to_pdf(ecu_werkbank::trs!("Batterie- und Starttest"), &meta, &[col]))?;
    println!("{}", tr!("PDF: {a}", a = out.display()));
    Ok(())
}

pub(super) fn handy(be: &Backend, db: &LabelDb, port: u16, pids: &[u16], seconds: u64, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::{limits::Stufe, obd, web};
    let pids: Vec<u8> = if pids.is_empty() { vec![0x0C, 0x0D, 0x05, 0x04, 0x42] } else { pids.iter().map(|p| *p as u8).collect() };
    let data: web::Shared = Default::default();
    let srv = web::serve(port, data.clone())?;
    let title = fahrzeug.and_then(ecu_werkbank::vehicle::find).map(|v| v.label()).unwrap_or_else(|| "ECUWerkbank".into());
    println!("{}", tr!("Handy-Anzeige läuft. QR-Code mit der Handy-Kamera scannen oder im Handy-Browser öffnen:\n\n    {a}\n", a = srv.url));
    if let Ok(code) = qrcode::QrCode::new(srv.url.as_bytes()) {
        // hell auf dunkel getauscht, damit der Code im (meist dunklen) Terminal stimmt
        let bild = code.render::<qrcode::render::unicode::Dense1x2>().dark_color(qrcode::render::unicode::Dense1x2::Light).light_color(qrcode::render::unicode::Dense1x2::Dark).quiet_zone(true).build();
        println!("{bild}\n");
    }
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
