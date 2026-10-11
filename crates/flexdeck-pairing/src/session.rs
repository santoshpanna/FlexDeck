//! Short-lived pairing windows.
//!
//! A pairing window is the narrow moment in which a device may claim a token.
//! Everything about it is designed to make guessing impractical and to make a
//! failed attempt cheap to recover from:
//!
//! * **Short-lived.** The window closes on a timer, so a QR left on a screen
//!   does not stay usable indefinitely.
//! * **Single-use.** The window is consumed the instant it succeeds. A PIN
//!   observed over someone's shoulder is worthless afterwards.
//! * **Rate-limited.** Wrong guesses are counted, and the window slams shut
//!   once they run out, so a 6-digit PIN cannot be brute-forced.
//! * **Constant-time compared**, so the comparison itself does not leak the
//!   answer digit by digit.
//!
//! `now` is supplied by the caller throughout, which is what makes the expiry
//! boundary testable without sleeping.

use crate::{constant_time_eq, generate_token, to_hex};

/// How many wrong guesses a window tolerates before it closes.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;

/// How long a window stays open, in seconds.
pub const DEFAULT_TTL_SECS: i64 = 120;

/// How long a browser pairing code stays open, in seconds. Longer than a
/// tablet's, because the code is typed on a different device, often in another
/// room.
pub const BROWSER_TTL_SECS: i64 = 300;

/// Characters in a browser pairing code. Crockford base32, so 8 characters is
/// 40 bits: unlike a tablet PIN, this code is typed into a public website, so it
/// must survive guessing at internet scale, not just on a LAN.
pub const BROWSER_CODE_LEN: usize = 8;

/// Crockford's base32 alphabet: no I, L, O or U, so nothing reads as anything else.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// What the agent shows the user: a PIN to type, and the QR contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOffer {
    /// Identifies this window. Travels in the QR and in the redeem request.
    pub id: String,
    /// Six digits, typed by a human. Compared in constant time.
    pub pin: String,
    /// The QR's secret. Travelling in a QR avoids the typing step entirely.
    pub one_time_secret: String,
    /// Unix seconds. Compared with `>=`, so the instant itself is already closed.
    pub expires_at: i64,
}

/// The outcome of an attempt to redeem an offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingOutcome {
    /// Success. Hand this token to the device, and store only its hash.
    Paired { token: String },
    /// Wrong code. `remaining` guesses are left before the window closes.
    WrongCode { remaining: u32 },
    /// The window had already expired. Start a new one.
    Expired,
    /// Attempts ran out. The window is closed and a new one is required.
    Exhausted,
    /// No window is open: none was started, it was already used, or it was
    /// cancelled.
    NoOffer,
}

struct Session {
    offer: SessionOffer,
    remaining_attempts: u32,
}

/// Owns the agent's single outstanding pairing window.
///
/// Only one window exists at a time. Pairing is a deliberate human act, and a
/// second concurrent offer would be one more thing to reason about — with no
/// benefit, since one person is doing the pairing.
pub struct PairingDesk {
    session: Option<Session>,
    ttl_secs: i64,
    max_attempts: u32,
    /// Mints the code a human types: six digits for a tablet, a Crockford code
    /// for a browser.
    code: fn() -> String,
}

impl Default for PairingDesk {
    fn default() -> Self {
        Self::new(DEFAULT_TTL_SECS, DEFAULT_MAX_ATTEMPTS)
    }
}

impl PairingDesk {
    pub fn new(ttl_secs: i64, max_attempts: u32) -> Self {
        Self {
            session: None,
            ttl_secs,
            max_attempts,
            code: random_pin,
        }
    }

    /// A desk for pairing *browsers*, whose code is typed into a public website.
    pub fn for_browsers() -> Self {
        Self {
            code: random_browser_code,
            ..Self::new(BROWSER_TTL_SECS, DEFAULT_MAX_ATTEMPTS)
        }
    }

    /// Open a new window, replacing any window already open.
    pub fn begin(&mut self, now: i64) -> SessionOffer {
        let offer = SessionOffer {
            id: random_hex(8),
            pin: (self.code)(),
            one_time_secret: random_hex(16),
            expires_at: now + self.ttl_secs,
        };

        self.session = Some(Session {
            offer: offer.clone(),
            remaining_attempts: self.max_attempts,
        });

        offer
    }

    /// The currently open offer, or `None` if there is none or it has expired.
    pub fn current(&self, now: i64) -> Option<&SessionOffer> {
        self.session
            .as_ref()
            .filter(|session| now < session.offer.expires_at)
            .map(|session| &session.offer)
    }

    /// Close any open window without pairing.
    pub fn cancel(&mut self) {
        self.session = None;
    }

    /// Redeem using the PIN a human read off the agent's window.
    ///
    /// The input is normalised first (see [`normalise_code`]), so a browser code
    /// typed in lowercase, with or without its dash, still matches.
    pub fn redeem_pin(&mut self, pin: &str, now: i64) -> PairingOutcome {
        let typed = normalise_code(pin);
        self.redeem(now, |offer| {
            constant_time_eq(typed.as_bytes(), offer.pin.as_bytes())
        })
    }

    /// Redeem using the session id and secret carried by the QR code.
    ///
    /// The id is checked as well as the secret, so a secret that leaks without
    /// its id — from a screenshot of the JSON, say — is not enough on its own.
    pub fn redeem_secret(&mut self, session_id: &str, secret: &str, now: i64) -> PairingOutcome {
        self.redeem(now, |offer| {
            constant_time_eq(session_id.as_bytes(), offer.id.as_bytes())
                && constant_time_eq(secret.as_bytes(), offer.one_time_secret.as_bytes())
        })
    }

    fn redeem(&mut self, now: i64, matches: impl FnOnce(&SessionOffer) -> bool) -> PairingOutcome {
        let Some(session) = self.session.as_mut() else {
            return PairingOutcome::NoOffer;
        };

        // `>=` not `>`: the expiry instant is already too late. An inclusive
        // window is easier to reason about than a half-open one, and the
        // difference is a second either way.
        if now >= session.offer.expires_at {
            self.session = None;
            return PairingOutcome::Expired;
        }

        if matches(&session.offer) {
            self.session = None;
            return PairingOutcome::Paired {
                token: generate_token(),
            };
        }

        session.remaining_attempts = session.remaining_attempts.saturating_sub(1);
        if session.remaining_attempts == 0 {
            // Close the window outright rather than allowing one more try:
            // otherwise "attempts left: 0" would still be a usable guess.
            self.session = None;
            return PairingOutcome::Exhausted;
        }

        PairingOutcome::WrongCode {
            remaining: session.remaining_attempts,
        }
    }
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0_u8; bytes];
    rand::fill(&mut buffer);
    to_hex(&buffer)
}

/// Six digits, zero-padded, from the system CSPRNG.
fn random_pin() -> String {
    format!("{:06}", rand::random_range(0..1_000_000_u32))
}

/// [`BROWSER_CODE_LEN`] Crockford characters from the system CSPRNG.
fn random_browser_code() -> String {
    (0..BROWSER_CODE_LEN)
        .map(|_| CROCKFORD[rand::random_range(0..CROCKFORD.len())] as char)
        .collect()
}

/// The canonical form of a typed code: separators and spaces dropped,
/// uppercased, and the letters people mistype for digits read as those digits.
///
/// A tablet PIN is all digits, so this changes nothing for one.
pub fn normalise_code(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect()
}

/// A code as a person should see it: `7KQ4-M9XR`.
pub fn display_code(code: &str) -> String {
    if code.len() == BROWSER_CODE_LEN {
        format!("{}-{}", &code[..4], &code[4..])
    } else {
        code.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000;

    fn desk() -> PairingDesk {
        // A short window and few attempts keep the tests quick to read.
        PairingDesk::new(60, 3)
    }

    fn paired_token(outcome: PairingOutcome) -> String {
        match outcome {
            PairingOutcome::Paired { token } => token,
            other => panic!("expected Paired, got {other:?}"),
        }
    }

    #[test]
    fn a_browser_code_is_eight_unambiguous_characters() {
        let mut desk = PairingDesk::for_browsers();
        let offer = desk.begin(NOW);

        assert_eq!(offer.pin.len(), BROWSER_CODE_LEN);
        assert!(
            offer.pin.bytes().all(|b| CROCKFORD.contains(&b)),
            "{} is Crockford base32",
            offer.pin
        );
        assert_eq!(offer.expires_at, NOW + BROWSER_TTL_SECS);
    }

    #[test]
    fn a_browser_code_is_accepted_however_it_is_typed() {
        let mut desk = PairingDesk::for_browsers();
        let offer = desk.begin(NOW);
        let typed = display_code(&offer.pin).to_lowercase();

        paired_token(desk.redeem_pin(&format!(" {typed} "), NOW));
    }

    #[test]
    fn normalising_reads_lookalike_letters_as_digits() {
        assert_eq!(normalise_code("7kq4-m9xr"), "7KQ4M9XR");
        assert_eq!(normalise_code("O1L I"), "0111");
        assert_eq!(normalise_code("123456"), "123456", "a PIN is unchanged");
    }

    #[test]
    fn an_offer_has_the_shape_the_ui_expects() {
        let mut desk = desk();
        let offer = desk.begin(NOW);

        assert_eq!(offer.id.len(), 16, "8 random bytes as hex");
        assert_eq!(offer.one_time_secret.len(), 32, "16 random bytes as hex");
        assert_eq!(offer.pin.len(), 6);
        assert!(
            offer.pin.chars().all(|c| c.is_ascii_digit()),
            "{}",
            offer.pin
        );
        assert_eq!(offer.expires_at, NOW + 60);
    }

    #[test]
    fn a_pin_is_always_six_digits_even_when_the_number_is_small() {
        // A PIN of 42 must render as "000042", not "42" — otherwise the user
        // would type a shorter code than the UI showed.
        for _ in 0..200 {
            let pin = random_pin();
            assert_eq!(pin.len(), 6, "{pin} should be zero-padded");
        }
    }

    #[test]
    fn current_reflects_the_open_window() {
        let mut desk = desk();
        assert!(desk.current(NOW).is_none(), "nothing is open yet");

        let offer = desk.begin(NOW);
        assert_eq!(desk.current(NOW), Some(&offer));

        // Still open one second before expiry.
        assert!(desk.current(offer.expires_at - 1).is_some());
        // Closed at the expiry instant: the boundary is inclusive.
        assert!(desk.current(offer.expires_at).is_none());
        assert!(desk.current(offer.expires_at + 1).is_none());
    }

    #[test]
    fn the_correct_pin_pairs_and_yields_a_usable_token() {
        let mut desk = desk();
        let offer = desk.begin(NOW);

        let token = paired_token(desk.redeem_pin(&offer.pin, NOW));

        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_window_is_single_use() {
        let mut desk = desk();
        let offer = desk.begin(NOW);

        paired_token(desk.redeem_pin(&offer.pin, NOW));

        // The same PIN must not work twice: it has been observed, possibly by
        // someone else, and the device already has its token.
        assert_eq!(desk.redeem_pin(&offer.pin, NOW), PairingOutcome::NoOffer);
        assert!(desk.current(NOW).is_none());
    }

    #[test]
    fn a_wrong_pin_counts_down_then_locks_the_window() {
        let mut desk = desk(); // max_attempts = 3
        let offer = desk.begin(NOW);
        // Every digit differs, so this is the same length but definitely
        // wrong: it exercises the byte comparison, not the length check.
        let wrong: String = offer
            .pin
            .chars()
            .map(|digit| if digit == '0' { '1' } else { '0' })
            .collect();

        assert_eq!(
            desk.redeem_pin(&wrong, NOW),
            PairingOutcome::WrongCode { remaining: 2 }
        );
        assert_eq!(
            desk.redeem_pin(&wrong, NOW),
            PairingOutcome::WrongCode { remaining: 1 }
        );
        // The third wrong guess exhausts the window and closes it.
        assert_eq!(desk.redeem_pin(&wrong, NOW), PairingOutcome::Exhausted);

        // Even the *correct* PIN is useless now. Allowing it would mean
        // "attempts remaining: 0" was still one more usable guess.
        assert_eq!(desk.redeem_pin(&offer.pin, NOW), PairingOutcome::NoOffer);
    }

    #[test]
    fn an_expired_window_reports_expiry_rather_than_a_wrong_code() {
        let mut desk = desk();
        let offer = desk.begin(NOW);

        assert_eq!(
            desk.redeem_pin(&offer.pin, offer.expires_at),
            PairingOutcome::Expired
        );
        // And the window is gone afterwards, so a later retry is NoOffer.
        assert_eq!(desk.redeem_pin(&offer.pin, NOW), PairingOutcome::NoOffer);
    }

    #[test]
    fn the_qr_secret_pairs_when_both_id_and_secret_match() {
        let mut desk = desk();
        let offer = desk.begin(NOW);

        let token = paired_token(desk.redeem_secret(&offer.id, &offer.one_time_secret, NOW));
        assert_eq!(token.len(), 64);
    }

    #[test]
    fn a_secret_with_the_wrong_session_id_does_not_pair() {
        let mut desk = desk();
        let offer = desk.begin(NOW);

        // The secret alone must not be sufficient.
        let outcome = desk.redeem_secret("not-the-session-id", &offer.one_time_secret, NOW);
        assert_eq!(outcome, PairingOutcome::WrongCode { remaining: 2 });
        assert!(desk.current(NOW).is_some(), "the real window is still open");
    }

    #[test]
    fn a_correct_session_id_with_the_wrong_secret_does_not_pair() {
        let mut desk = desk();
        let offer = desk.begin(NOW);

        assert_eq!(
            desk.redeem_secret(&offer.id, "wrong-secret", NOW),
            PairingOutcome::WrongCode { remaining: 2 }
        );
    }

    #[test]
    fn redeeming_without_an_open_window_is_no_offer() {
        let mut desk = desk();
        assert_eq!(desk.redeem_pin("123456", NOW), PairingOutcome::NoOffer);
        assert_eq!(
            desk.redeem_secret("id", "secret", NOW),
            PairingOutcome::NoOffer
        );
    }

    #[test]
    fn beginning_a_second_window_retires_the_first() {
        let mut desk = desk();
        let first = desk.begin(NOW);
        let second = desk.begin(NOW);

        assert_ne!(first.id, second.id, "ids identify a window, so they differ");

        // The first window's QR must no longer work.
        assert_eq!(
            desk.redeem_secret(&first.id, &first.one_time_secret, NOW),
            PairingOutcome::WrongCode { remaining: 2 }
        );
        // The second one still does.
        paired_token(desk.redeem_secret(&second.id, &second.one_time_secret, NOW));
    }

    #[test]
    fn cancelling_closes_the_window() {
        let mut desk = desk();
        let offer = desk.begin(NOW);

        desk.cancel();

        assert!(desk.current(NOW).is_none());
        assert_eq!(desk.redeem_pin(&offer.pin, NOW), PairingOutcome::NoOffer);
    }

    #[test]
    fn an_offer_is_still_open_one_second_before_expiry() {
        // Guards the boundary in the direction that would be a real bug: an
        // off-by-one that closed the window a second early would look like a
        // flaky pairing failure to a user, not like a logic error.
        let mut desk = desk();
        let offer = desk.begin(NOW);
        paired_token(desk.redeem_pin(&offer.pin, offer.expires_at - 1));
    }
}
