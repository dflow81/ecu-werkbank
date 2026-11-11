//! Merkt sich, ob gerade jemand den CAN-Bus benutzt.
//!
//! Jede offene Verbindung (ISO-TP, TP 2.0, Suchlauf, Mitschnitt) hält einen
//! [`Belegt`]-Wächter. So weiß die Fahrzeug-Erkennung im Hintergrund, wann der
//! Bus frei ist, ohne jede einzelne Aufgabe der Oberfläche zu kennen.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static OFFEN: AtomicUsize = AtomicUsize::new(0);
static ZULETZT_MS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// In diesem Thread nicht mitzählen (die Erkennung selbst).
    static LEISE: Cell<bool> = const { Cell::new(false) };
}

fn jetzt_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Wächter: solange er lebt, gilt der Bus als belegt.
pub struct Belegt {
    aktiv: bool,
}

impl Belegt {
    pub fn neu() -> Self {
        let aktiv = !LEISE.with(Cell::get);
        if aktiv {
            OFFEN.fetch_add(1, Ordering::SeqCst);
            ZULETZT_MS.store(jetzt_ms(), Ordering::SeqCst);
        }
        Belegt { aktiv }
    }
}

impl Drop for Belegt {
    fn drop(&mut self) {
        if self.aktiv {
            ZULETZT_MS.store(jetzt_ms(), Ordering::SeqCst);
            OFFEN.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Wie lange der Bus schon frei ist. `None` = gerade belegt.
pub fn frei_seit() -> Option<Duration> {
    if OFFEN.load(Ordering::SeqCst) > 0 {
        return None;
    }
    let z = ZULETZT_MS.load(Ordering::SeqCst);
    Some(Duration::from_millis(jetzt_ms().saturating_sub(z)))
}

/// `f` ausführen, ohne dass die Verbindungen darin als Belegung zählen.
pub fn leise<T>(f: impl FnOnce() -> T) -> T {
    LEISE.with(|l| l.set(true));
    let r = f();
    LEISE.with(|l| l.set(false));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn belegt_und_leise() {
        {
            let _b = Belegt::neu();
            assert!(frei_seit().is_none());
        }
        assert!(frei_seit().is_some());
        leise(|| {
            let _b = Belegt::neu();
            assert!(frei_seit().is_some());
        });
    }
}
