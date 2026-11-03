/// Minor units of `1/10_000`. `1.5000` is stored as `15_000`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Money {
    minor: i64,
}

const SCALE: i64 = 10_000;

impl Money {
    pub const ZERO: Self = Self { minor: 0 };

    pub fn is_zero(self) -> bool {
        self.minor == 0
    }

    fn is_negative(self) -> bool {
        self.minor < 0
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.minor
            .checked_add(rhs.minor)
            .map(|minor| Self { minor })
    }

    pub(crate) fn add(self, rhs: Self) -> Self {
        Self {
            minor: self.minor.strict_add(rhs.minor),
        }
    }

    pub(crate) fn sub(self, rhs: Self) -> Self {
        Self {
            minor: self.minor.strict_sub(rhs.minor),
        }
    }

    /// A non-negative amount with at most four fractional digits.
    /// The minor units must fit in `i64` (`922337203685477.5807`).
    pub fn parse(input: &str) -> Option<Self> {
        let (whole, frac) = match input.split_once('.') {
            Some((whole, frac)) => (whole, frac),
            None => (input, ""),
        };
        if whole.is_empty()
            || frac.len() > 4
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || !frac.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }

        let whole: i64 = whole.parse().ok()?;
        let frac: i64 = format!("{frac:0<4}").parse().ok()?;
        whole
            .checked_mul(SCALE)
            .and_then(|scaled| scaled.checked_add(frac))
            .map(|minor| Self { minor })
    }

    pub fn format(self) -> String {
        let negative = self.is_negative();
        let abs = self.minor.unsigned_abs();
        let scale = SCALE as u64;
        let sign = if negative { "-" } else { "" };
        format!("{sign}{}.{:04}", abs / scale, abs % scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_up_to_four_decimal_places_within_i64() {
        assert_eq!(Money::parse("1.5").unwrap().format(), "1.5000");
        assert_eq!(Money::parse("1.2555").unwrap().format(), "1.2555");
        assert_eq!(Money::parse("429496.7296").unwrap().format(), "429496.7296");
        assert_eq!(
            Money::parse("922337203685477.5807").unwrap().format(),
            "922337203685477.5807"
        );
    }

    #[test]
    fn rejects_malformed_amounts_and_values_past_i64() {
        for input in [
            "",
            "1.23456",
            "-1.0",
            ".5",
            "1.2a",
            "922337203685477.5808",
            "922337203685478",
        ] {
            assert!(Money::parse(input).is_none(), "{input}");
        }
    }

    #[test]
    fn formats_four_decimal_places() {
        assert_eq!(Money::ZERO.format(), "0.0000");
        assert_eq!(Money::parse("0.0001").unwrap().format(), "0.0001");
        assert_eq!(Money { minor: -35_000 }.format(), "-3.5000");
    }
}
