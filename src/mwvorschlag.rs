//! Namensvorschläge für Messwertblöcke (TP 2.0 / KWP) von Benzin-Motorsteuer-
//! geräten (Adresse 01), z. B. MED17 im 1.4 TSI.
//!
//! Grundlage: die allgemein bekannte VAG-Blockbelegung (Block 1–4 Grundwerte,
//! 115 Ladedruck), in eigenen Worten. Die Belegung kann je Software-Stand
//! abweichen. Deshalb wird ein Vorschlag nur übernommen, wenn die
//! Umrechnungsformel des Felds am Auto zur erwarteten Art des Werts passt
//! (Drehzahl, Temperatur, Druck …).

use crate::kwp::BlockValue;

pub struct Feld {
    pub feld: u8,
    /// Formeln, die zu dieser Art Wert passen
    pub formeln: &'static [u8],
    pub name: &'static str,
}

pub struct Block {
    pub block: u8,
    pub name: &'static str,
    pub felder: &'static [Feld],
}

const DREHZAHL: &[u8] = &[0x01];
const TEMPERATUR: &[u8] = &[0x05];
const LAST: &[u8] = &[0x21, 0x02];
const ZEIT_MS: &[u8] = &[0x16, 0x0F];
const SPANNUNG: &[u8] = &[0x15, 0x06];
const DRUCK: &[u8] = &[0x12];
const PROZENT: &[u8] = &[0x21, 0x17, 0x02, 0x03];
const ZUENDWINKEL: &[u8] = &[0x1B];
const LUFTMASSE: &[u8] = &[0x60, 0x13];

/// Vorschläge für Benzin-Motorsteuergeräte (Adresse 01).
pub const MOTOR_BENZIN: &[Block] = &[
    Block {
        block: 1,
        name: "Grundwerte",
        felder: &[
            Feld { feld: 1, formeln: DREHZAHL, name: "Drehzahl" },
            Feld { feld: 2, formeln: TEMPERATUR, name: "Kühlmitteltemperatur" },
            Feld { feld: 3, formeln: &[0x14], name: "Lambda-Regelung" },
            Feld { feld: 4, formeln: &[0x10], name: "Grundeinstell-Bedingungen (Bits)" },
        ],
    },
    Block {
        block: 2,
        name: "Last und Einspritzung",
        felder: &[
            Feld { feld: 1, formeln: DREHZAHL, name: "Drehzahl" },
            Feld { feld: 2, formeln: LAST, name: "Motorlast" },
            Feld { feld: 3, formeln: ZEIT_MS, name: "Einspritzzeit" },
            Feld { feld: 4, formeln: LUFTMASSE, name: "Luftmasse (vermutet)" },
        ],
    },
    Block {
        block: 3,
        name: "Luft, Drosselklappe, Zündung",
        felder: &[
            Feld { feld: 1, formeln: DREHZAHL, name: "Drehzahl" },
            Feld { feld: 2, formeln: LUFTMASSE, name: "Luftmasse (vermutet)" },
            Feld { feld: 3, formeln: PROZENT, name: "Drosselklappe" },
            Feld { feld: 4, formeln: ZUENDWINKEL, name: "Zündwinkel" },
        ],
    },
    Block {
        block: 4,
        name: "Spannung und Temperaturen",
        felder: &[
            Feld { feld: 1, formeln: DREHZAHL, name: "Drehzahl" },
            Feld { feld: 2, formeln: SPANNUNG, name: "Bordspannung" },
            Feld { feld: 3, formeln: TEMPERATUR, name: "Kühlmitteltemperatur" },
            Feld { feld: 4, formeln: TEMPERATUR, name: "Ansauglufttemperatur" },
        ],
    },
    Block {
        block: 115,
        name: "Ladedruck",
        felder: &[
            Feld { feld: 1, formeln: DREHZAHL, name: "Drehzahl" },
            Feld { feld: 2, formeln: LAST, name: "Motorlast" },
            Feld { feld: 3, formeln: DRUCK, name: "Ladedruck soll" },
            Feld { feld: 4, formeln: DRUCK, name: "Ladedruck ist" },
        ],
    },
];

/// Vorschläge für ein Steuergerät (bisher nur Motor 01).
pub fn fuer(adresse: u8) -> &'static [Block] {
    match adresse {
        0x01 => MOTOR_BENZIN,
        _ => &[],
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pruefung {
    /// Formel passt zur erwarteten Art
    Passt,
    /// Feld hat eine andere Formel (Belegung weicht ab)
    Anders(u8),
    /// Feld gibt es im Block nicht oder es ist leer
    Fehlt,
}

/// Jedes vorgeschlagene Feld gegen den gelesenen Block prüfen.
pub fn pruefen<'a>(b: &'a Block, werte: &[BlockValue]) -> Vec<(&'a Feld, Pruefung)> {
    b.felder
        .iter()
        .map(|f| {
            let p = match werte.get(usize::from(f.feld) - 1) {
                None => Pruefung::Fehlt,
                Some(v) if v.is_leer() => Pruefung::Fehlt,
                Some(v) if f.formeln.contains(&v.formula) => Pruefung::Passt,
                Some(v) => Pruefung::Anders(v.formula),
            };
            (f, p)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golf6_bloecke_passen() {
        // Block 1 wie am echten Golf 6 / MED17 gelesen
        let b1 = [
            BlockValue { formula: 0x01, a: 0xC8, b: 0x14 },
            BlockValue { formula: 0x05, a: 0x0A, b: 0xBE },
            BlockValue { formula: 0x14, a: 0x32, b: 0x80 },
            BlockValue { formula: 0x10, a: 0xFF, b: 0x33 },
        ];
        let p = pruefen(&MOTOR_BENZIN[0], &b1);
        assert!(p.iter().all(|(_, x)| *x == Pruefung::Passt));

        // Andere Belegung: Feld 2 ist eine Spannung statt Temperatur
        let mut anders = b1.clone();
        anders[1].formula = 0x15;
        let p = pruefen(&MOTOR_BENZIN[0], &anders[..3]);
        assert_eq!(p[1].1, Pruefung::Anders(0x15));
        assert_eq!(p[3].1, Pruefung::Fehlt);
        assert!(fuer(0x17).is_empty());
    }
}
