//! Steuergeräte nach Systemen gruppieren (VAG-Adressliste, eigene Bezeichnungen).
//! Für die Übersicht „Sicherheit & Assistenz“ — nur lesend.

/// Systemgruppe einer VAG-Diagnoseadresse.
pub fn gruppe(addr: u16) -> &'static str {
    match addr {
        0x15 | 0x65 => "Airbag & Rückhaltesysteme",
        0x03 | 0x44 | 0x53 | 0x13 | 0x3C | 0xA5 | 0x6C | 0x76 | 0x10 | 0x7D | 0xA9 => "Fahrwerk & Assistenz",
        0x01 | 0x02 | 0x11 | 0x51 | 0x8C | 0x61 => "Motor & Antrieb",
        0x09 | 0x46 | 0x42 | 0x52 | 0x62 | 0x72 | 0x0D | 0x36 | 0x6D | 0x55 => "Karosserie & Komfort",
        0x08 | 0x18 => "Klima & Heizung",
        0x17 | 0x19 | 0x25 | 0x16 => "Instrumente & Gateway",
        0x5F | 0x56 | 0x37 | 0x47 | 0x77 | 0x0E | 0x2E => "Infotainment",
        _ => "Sonstige",
    }
}

/// Kurzname einer Adresse (eigene Bezeichnungen der genormten VAG-Adressliste).
pub fn name(addr: u16) -> &'static str {
    match addr {
        0x01 => "Motor",
        0x02 => "Getriebe",
        0x03 => "Bremsen (ABS/ESP)",
        0x08 => "Klimaanlage",
        0x09 => "Bordnetz",
        0x10 => "Einparkhilfe 2",
        0x13 => "Abstandsregelung (ACC)",
        0x15 => "Airbag",
        0x16 => "Lenkradelektronik",
        0x17 => "Kombiinstrument",
        0x19 => "Gateway",
        0x25 => "Wegfahrsperre",
        0x3C => "Spurwechselassistent",
        0x42 => "Türelektronik Fahrer",
        0x44 => "Lenkhilfe",
        0x46 => "Komfortsystem",
        0x52 => "Türelektronik Beifahrer",
        0x53 => "Parkbremse",
        0x55 => "Leuchtweitenregelung",
        0x04 => "Lenkwinkelsensor",
        0x37 => "Navigation",
        0x62 => "Türelektronik hinten links",
        0x69 => "Anhängersteuergerät",
        0x72 => "Türelektronik hinten rechts",
        0x56 => "Radio",
        0x5F => "Informationselektronik",
        0x65 => "Reifendruck",
        0x6C => "Rückfahrkamera",
        0x76 => "Einparkhilfe",
        0xA5 => "Frontsensorik Fahrerassistenz",
        _ => "",
    }
}

/// Gruppe für Sicherheit/Assistenz?
pub fn sicherheit(addr: u16) -> bool {
    matches!(gruppe(addr), "Airbag & Rückhaltesysteme" | "Fahrwerk & Assistenz")
}

/// Gruppe eines Steuergeräts aus dem Scan. Bei TP 2.0 sind die Logikadressen
/// keine Diagnoseadressen — dort zählt der Name aus der Antwort.
pub fn gruppe_modul(protocol: &str, address: Option<u16>, name: &str) -> &'static str {
    // UDS mit VAG-Adresse: nach Adresse. Ohne Adresse (Golf 6: nur CAN-ID,
    // z. B. Airbag 0x715) oder TP 2.0: nach dem Namen.
    if protocol != "TP 2.0" {
        if let Some(g) = address.map(gruppe).filter(|g| *g != "Sonstige") {
            return g;
        }
    }
    gruppe_name(name)
}

/// Gruppe aus dem Namen des Steuergeräts.
fn gruppe_name(name: &str) -> &'static str {
    let n = name.to_lowercase();
    let has = |w: &str| n.contains(w);
    if has("airbag") {
        "Airbag & Rückhaltesysteme"
    } else if has("abs") || has("servolenkung") || has("parkbremse") || has("einparkhilfe") {
        "Fahrwerk & Assistenz"
    } else if has("motor") || has("getriebe") || has("wählhebel") {
        "Motor & Antrieb"
    } else if has("tür") || has("bordnetz") || has("komfort") || has("anhänger") {
        "Karosserie & Komfort"
    } else if has("klima") {
        "Klima & Heizung"
    } else if has("kombi") || has("gateway") || has("lenksäule") || has("wegfahrsperre") {
        "Instrumente & Gateway"
    } else if has("radio") || has("navigation") || has("infotainment") {
        "Infotainment"
    } else {
        "Sonstige"
    }
}

/// Gehört das Steuergerät zu Sicherheit/Assistenz?
pub fn sicherheit_modul(protocol: &str, address: Option<u16>, name: &str) -> bool {
    matches!(gruppe_modul(protocol, address, name), "Airbag & Rückhaltesysteme" | "Fahrwerk & Assistenz")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golf6_uds_ohne_adresse_nach_name() {
        // Golf 6: UDS-Steuergeräte nur mit CAN-ID, ohne VAG-Adresse
        assert!(sicherheit_modul("UDS", None, "Airbag"));
        assert_eq!(gruppe_modul("UDS", None, "Radio/Navigation"), "Infotainment");
        assert_eq!(gruppe_modul("UDS", Some(0x15), ""), "Airbag & Rückhaltesysteme");
        assert!(sicherheit_modul("TP 2.0", Some(0x03), "ABS/ESP"));
        assert!(!sicherheit_modul("TP 2.0", Some(0x01), "Motorsteuergerät"));
    }
}
