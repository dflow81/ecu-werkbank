//! Bilder (Firmenlogo) für die PDF-Berichte: JPEG wird unverändert
//! eingebettet, PNG entpackt und als FlateDecode-Bild (mit Transparenz als
//! Maske) neu gepackt. Ohne fremde Bild-Bibliothek; zlib über miniz_oxide.

use anyhow::{bail, Context, Result};

/// Ein Bild, fertig für ein PDF-XObject.
#[derive(Debug, Clone, PartialEq)]
pub struct Bild {
    pub breite: u32,
    pub hoehe: u32,
    /// DeviceRGB oder DeviceGray
    pub farbraum: &'static str,
    /// DCTDecode (JPEG) oder FlateDecode
    pub filter: &'static str,
    pub daten: Vec<u8>,
    /// Transparenz (Graustufen, FlateDecode), falls das PNG einen Alphakanal hat
    pub maske: Option<Vec<u8>>,
}

impl Bild {
    /// PNG oder JPEG lesen (an der Dateikennung erkannt).
    pub fn lesen(roh: &[u8]) -> Result<Bild> {
        if roh.starts_with(&[0xFF, 0xD8]) {
            jpeg(roh)
        } else if roh.starts_with(b"\x89PNG\r\n\x1a\n") {
            png(roh)
        } else {
            bail!(crate::tr!("Bild nicht erkannt — bitte PNG oder JPG verwenden."))
        }
    }

    /// Seitenverhältnis Breite/Höhe.
    pub fn verhaeltnis(&self) -> f32 {
        self.breite as f32 / self.hoehe.max(1) as f32
    }
}

/// JPEG: Größe und Farbkanäle aus dem SOF-Abschnitt, Daten unverändert.
fn jpeg(roh: &[u8]) -> Result<Bild> {
    let mut i = 2;
    while i + 4 <= roh.len() {
        if roh[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = roh[i + 1];
        // Füllbytes und Marker ohne Länge
        if marker == 0xFF || (0xD0..=0xD9).contains(&marker) || marker == 0x01 {
            i += if marker == 0xFF { 1 } else { 2 };
            continue;
        }
        let laenge = u16::from_be_bytes([roh[i + 2], roh[i + 3]]) as usize;
        // SOF0..SOF15 außer DHT (C4), JPG (C8), DAC (CC)
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            let s = roh.get(i + 4..i + 4 + 6).context(crate::tr!("JPG unvollständig"))?;
            let hoehe = u32::from(u16::from_be_bytes([s[1], s[2]]));
            let breite = u32::from(u16::from_be_bytes([s[3], s[4]]));
            let farbraum = match s[5] {
                1 => "DeviceGray",
                3 => "DeviceRGB",
                _ => bail!(crate::tr!("JPG mit {a} Farbkanälen (CMYK?) — bitte als RGB-JPG oder PNG speichern.", a = s[5])),
            };
            if breite == 0 || hoehe == 0 {
                bail!(crate::tr!("JPG ohne Größe"));
            }
            return Ok(Bild { breite, hoehe, farbraum, filter: "DCTDecode", daten: roh.to_vec(), maske: None });
        }
        i += 2 + laenge;
    }
    bail!(crate::tr!("JPG ohne Bildgröße (SOF) — Datei beschädigt?"))
}

/// PNG: alle üblichen Farbarten mit 8 oder 16 Bit, Palette auch mit 1/2/4 Bit;
/// ohne Zeilensprung (Interlace).
fn png(roh: &[u8]) -> Result<Bild> {
    let mut i = 8;
    let (mut breite, mut hoehe, mut tiefe, mut art, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat = Vec::new();
    while i + 8 <= roh.len() {
        let len = u32::from_be_bytes([roh[i], roh[i + 1], roh[i + 2], roh[i + 3]]) as usize;
        let typ = &roh[i + 4..i + 8];
        let inhalt = roh.get(i + 8..i + 8 + len).context(crate::tr!("PNG unvollständig"))?;
        match typ {
            b"IHDR" if len >= 13 => {
                breite = u32::from_be_bytes([inhalt[0], inhalt[1], inhalt[2], inhalt[3]]);
                hoehe = u32::from_be_bytes([inhalt[4], inhalt[5], inhalt[6], inhalt[7]]);
                tiefe = inhalt[8];
                art = inhalt[9];
                interlace = inhalt[12];
            }
            b"PLTE" => palette = inhalt.as_chunks::<3>().0.to_vec(),
            b"tRNS" => trns = inhalt.to_vec(),
            b"IDAT" => idat.extend_from_slice(inhalt),
            b"IEND" => break,
            _ => {}
        }
        i += 12 + len;
    }
    if breite == 0 || hoehe == 0 || breite > 10_000 || hoehe > 10_000 {
        bail!(crate::tr!("PNG mit ungültiger Größe"));
    }
    if interlace != 0 {
        bail!(crate::tr!("PNG mit Zeilensprung (Interlace) — bitte ohne Interlace speichern."));
    }
    let kanaele: usize = match art {
        0 => 1, // Grau
        2 => 3, // RGB
        3 => 1, // Palette
        4 => 2, // Grau + Alpha
        6 => 4, // RGBA
        _ => bail!(crate::tr!("PNG-Farbart {a} unbekannt", a = art)),
    };
    let gueltig = match art {
        3 => matches!(tiefe, 1 | 2 | 4 | 8),
        _ => matches!(tiefe, 8 | 16),
    };
    if !gueltig {
        bail!(crate::tr!("PNG mit {a} Bit je Farbe — bitte als 8-Bit-PNG speichern.", a = tiefe));
    }
    let roh_daten = miniz_oxide::inflate::decompress_to_vec_zlib(&idat).map_err(|e| anyhow::anyhow!(crate::tr!("PNG entpacken: {a:?}", a = e)))?;
    // Zeilen entfiltern
    let bits_pro_pixel = kanaele * usize::from(tiefe);
    let zeile = (breite as usize * bits_pro_pixel).div_ceil(8);
    let bpp = bits_pro_pixel.div_ceil(8).max(1);
    let (b, h) = (breite as usize, hoehe as usize);
    if roh_daten.len() < h * (zeile + 1) {
        bail!(crate::tr!("PNG-Daten zu kurz"));
    }
    let mut pixel = vec![0u8; h * zeile];
    for y in 0..h {
        let filter = roh_daten[y * (zeile + 1)];
        let quelle = &roh_daten[y * (zeile + 1) + 1..(y + 1) * (zeile + 1)];
        let (davor, rest) = pixel.split_at_mut(y * zeile);
        let oben: Option<&[u8]> = (y > 0).then(|| &davor[(y - 1) * zeile..]);
        let ziel = &mut rest[..zeile];
        for x in 0..zeile {
            let a = if x >= bpp { ziel[x - bpp] } else { 0 };
            let o = oben.map_or(0, |r| r[x]);
            let c = if x >= bpp { oben.map_or(0, |r| r[x - bpp]) } else { 0 };
            ziel[x] = quelle[x].wrapping_add(match filter {
                0 => 0,
                1 => a,
                2 => o,
                3 => ((u16::from(a) + u16::from(o)) / 2) as u8,
                4 => paeth(a, o, c),
                f => bail!(crate::tr!("PNG-Filter {a} unbekannt", a = f)),
            });
        }
    }
    // in 8-Bit-Farbe (+ Alpha) umsetzen
    let probe = |zeile_daten: &[u8], x: usize, k: usize| -> u8 {
        if tiefe == 16 {
            zeile_daten[(x * kanaele + k) * 2]
        } else if tiefe == 8 {
            zeile_daten[x * kanaele + k]
        } else {
            // Palette mit 1/2/4 Bit: Index aus dem Byte holen
            let pro_byte = 8 / usize::from(tiefe);
            let byte = zeile_daten[x / pro_byte];
            let shift = 8 - usize::from(tiefe) * (x % pro_byte + 1);
            (byte >> shift) & ((1u8 << tiefe) - 1)
        }
    };
    let grau = matches!(art, 0 | 4);
    let mut farbe = Vec::with_capacity(b * h * if grau { 1 } else { 3 });
    let mut alpha = Vec::with_capacity(b * h);
    let mut hat_alpha = false;
    for y in 0..h {
        let z = &pixel[y * zeile..(y + 1) * zeile];
        for x in 0..b {
            let (rgb, a): (&[u8], u8) = match art {
                0 => (&[probe(z, x, 0)][..], 255),
                2 => (&[probe(z, x, 0), probe(z, x, 1), probe(z, x, 2)][..], 255),
                3 => {
                    let idx = usize::from(probe(z, x, 0));
                    let p = palette.get(idx).copied().unwrap_or([0, 0, 0]);
                    farbe.extend_from_slice(&p);
                    let a = trns.get(idx).copied().unwrap_or(255);
                    hat_alpha |= a != 255;
                    alpha.push(a);
                    continue;
                }
                4 => (&[probe(z, x, 0)][..], probe(z, x, 1)),
                _ => (&[probe(z, x, 0), probe(z, x, 1), probe(z, x, 2)][..], probe(z, x, 3)),
            };
            farbe.extend_from_slice(rgb);
            hat_alpha |= a != 255;
            alpha.push(a);
        }
    }
    let packen = |d: &[u8]| miniz_oxide::deflate::compress_to_vec_zlib(d, 6);
    Ok(Bild {
        breite,
        hoehe,
        farbraum: if grau { "DeviceGray" } else { "DeviceRGB" },
        filter: "FlateDecode",
        daten: packen(&farbe),
        maske: hat_alpha.then(|| packen(&alpha)),
    })
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = ((p - i16::from(a)).abs(), (p - i16::from(b)).abs(), (p - i16::from(c)).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Base64 (Standard-Alphabet, mit `=`), um das Logo in der TOML-Ablage zu speichern.
pub fn base64(daten: &[u8]) -> String {
    const ZEICHEN: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(daten.len().div_ceil(3) * 4);
    for c in daten.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= c.len() {
                out.push(ZEICHEN[(n >> (18 - 6 * k)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 zurück in Bytes (Leerzeichen/Zeilenumbrüche werden übersprungen).
pub fn base64_lesen(text: &str) -> Result<Vec<u8>> {
    let wert = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let zeichen: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(zeichen.len() * 3 / 4);
    for c in zeichen.chunks(4) {
        let mut n = 0u32;
        for (k, &z) in c.iter().enumerate() {
            n |= wert(z).context(crate::tr!("Base64 ungültig"))? << (18 - 6 * k);
        }
        for k in 0..c.len().saturating_sub(1) {
            out.push((n >> (16 - 8 * k)) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kleines PNG selbst bauen (RGBA, Filter 0/1/2/4 gemischt).
    fn png_bauen(b: u32, h: u32, art: u8, pixel: &[u8], filter_zeile: impl Fn(usize) -> u8) -> Vec<u8> {
        let kan = match art {
            2 => 3,
            6 => 4,
            0 => 1,
            _ => 1,
        };
        let zeile = b as usize * kan;
        let mut roh = Vec::new();
        for y in 0..h as usize {
            let f = filter_zeile(y);
            roh.push(f);
            let z = &pixel[y * zeile..(y + 1) * zeile];
            for x in 0..zeile {
                let a = if x >= kan { z[x - kan] } else { 0 };
                let o = if y > 0 { pixel[(y - 1) * zeile + x] } else { 0 };
                let c = if y > 0 && x >= kan { pixel[(y - 1) * zeile + x - kan] } else { 0 };
                let vorhersage = match f {
                    1 => a,
                    2 => o,
                    3 => ((u16::from(a) + u16::from(o)) / 2) as u8,
                    4 => paeth(a, o, c),
                    _ => 0,
                };
                roh.push(z[x].wrapping_sub(vorhersage));
            }
        }
        let chunk = |typ: &[u8], d: &[u8]| {
            let mut v = (d.len() as u32).to_be_bytes().to_vec();
            v.extend_from_slice(typ);
            v.extend_from_slice(d);
            v.extend_from_slice(&[0, 0, 0, 0]); // CRC wird nicht geprüft
            v
        };
        let mut ihdr = b.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, art, 0, 0, 0]);
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&roh, 6)));
        out.extend(chunk(b"IEND", &[]));
        out
    }

    #[test]
    fn png_mit_alpha_und_filtern() {
        let (b, h) = (3u32, 4u32);
        let pixel: Vec<u8> = (0..(b * h * 4) as u8).map(|i| i.wrapping_mul(37)).collect();
        let p = png_bauen(b, h, 6, &pixel, |y| [0, 1, 2, 4][y % 4]);
        let bild = Bild::lesen(&p).unwrap();
        assert_eq!((bild.breite, bild.hoehe, bild.farbraum), (3, 4, "DeviceRGB"));
        let farbe = miniz_oxide::inflate::decompress_to_vec_zlib(&bild.daten).unwrap();
        let alpha = miniz_oxide::inflate::decompress_to_vec_zlib(bild.maske.as_ref().unwrap()).unwrap();
        let erwartet_farbe: Vec<u8> = pixel.chunks(4).flat_map(|c| [c[0], c[1], c[2]]).collect();
        let erwartet_alpha: Vec<u8> = pixel.chunks(4).map(|c| c[3]).collect();
        assert_eq!(farbe, erwartet_farbe);
        assert_eq!(alpha, erwartet_alpha);
        // RGB ohne Alpha: keine Maske
        let rgb: Vec<u8> = (0..27u8).collect();
        let bild = Bild::lesen(&png_bauen(3, 3, 2, &rgb, |_| 3)).unwrap();
        assert!(bild.maske.is_none());
        assert_eq!(miniz_oxide::inflate::decompress_to_vec_zlib(&bild.daten).unwrap(), rgb);
    }

    #[test]
    fn jpeg_groesse_und_fremdes_format() {
        // minimaler JPEG-Kopf: SOI, APP0 (Länge 16), SOF0 mit 3 Kanälen 120×40
        let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        j.extend_from_slice(&[0u8; 14]);
        j.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 8, 0, 40, 0, 120, 3]);
        j.extend_from_slice(&[0u8; 9]);
        let bild = Bild::lesen(&j).unwrap();
        assert_eq!((bild.breite, bild.hoehe, bild.farbraum, bild.filter), (120, 40, "DeviceRGB", "DCTDecode"));
        assert!(Bild::lesen(b"GIF89a").is_err());
    }

    #[test]
    fn base64_hin_und_zurueck() {
        for d in [&b""[..], b"a", b"ab", b"abc", b"\x00\xff\x10\x80\x7f"] {
            assert_eq!(base64_lesen(&base64(d)).unwrap(), d);
        }
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
    }
}
