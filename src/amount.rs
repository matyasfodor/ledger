use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};

/// Minor units of `1/10_000`. `1.5000` is stored as `15_000`.
///
/// Transaction amounts use [`Amount<u32>`] (up to `429_496.7295`). Account
/// `available` uses [`Amount<i64>`], because a dispute can push it below zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount<T> {
    minor: T,
}

impl<T> Amount<T> {
    pub const fn from_minor(minor: T) -> Self {
        Self { minor }
    }
}

const SCALE: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseAmountError {
    input: String,
}

impl fmt::Display for ParseAmountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid amount '{}'", self.input)
    }
}

impl std::error::Error for ParseAmountError {}

pub fn parse_amount(input: &str) -> Result<Amount<u32>, ParseAmountError> {
    let invalid = || ParseAmountError {
        input: input.to_owned(),
    };
    let (whole, frac) = match input.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (input, ""),
    };
    if whole.is_empty()
        || frac.len() > 4
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(invalid());
    }

    let whole: u32 = whole.parse().map_err(|_| invalid())?;
    let frac = format!("{frac:0<4}");
    let frac: u32 = frac.parse().map_err(|_| invalid())?;
    whole
        .checked_mul(SCALE)
        .and_then(|scaled| scaled.checked_add(frac))
        .map(Amount::from_minor)
        .ok_or_else(invalid)
}

pub trait Formattable {
    fn write_amount(self) -> String;
}

impl Formattable for Amount<u32> {
    fn write_amount(self) -> String {
        write_parts(false, u64::from(self.minor / SCALE), self.minor % SCALE)
    }
}

impl Formattable for Amount<i64> {
    fn write_amount(self) -> String {
        let negative = self.minor < 0;
        let abs = self.minor.unsigned_abs();
        let scale = u64::from(SCALE);
        write_parts(negative, abs / scale, (abs % scale) as u32)
    }
}

fn write_parts(negative: bool, whole: u64, frac: u32) -> String {
    let sign = if negative { "-" } else { "" };
    format!("{sign}{whole}.{frac:04}")
}

/// Four decimal places, matching the account output (`1.5000`, `-3.5000`).
pub fn format_amount(amount: impl Formattable) -> String {
    amount.write_amount()
}

impl From<Amount<u32>> for Amount<i64> {
    fn from(amount: Amount<u32>) -> Self {
        Self {
            minor: i64::from(amount.minor),
        }
    }
}

impl Add for Amount<u32> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            minor: self.minor + rhs.minor,
        }
    }
}

impl AddAssign for Amount<u32> {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Amount<u32> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self {
            minor: self.minor - rhs.minor,
        }
    }
}

impl SubAssign for Amount<u32> {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Add<Amount<u32>> for Amount<i64> {
    type Output = Self;

    fn add(self, rhs: Amount<u32>) -> Self {
        Self {
            minor: self.minor + i64::from(rhs.minor),
        }
    }
}

impl AddAssign<Amount<u32>> for Amount<i64> {
    fn add_assign(&mut self, rhs: Amount<u32>) {
        *self = *self + rhs;
    }
}

impl Sub<Amount<u32>> for Amount<i64> {
    type Output = Self;

    fn sub(self, rhs: Amount<u32>) -> Self {
        Self {
            minor: self.minor - i64::from(rhs.minor),
        }
    }
}

impl SubAssign<Amount<u32>> for Amount<i64> {
    fn sub_assign(&mut self, rhs: Amount<u32>) {
        *self = *self - rhs;
    }
}

impl PartialEq<Amount<u32>> for Amount<i64> {
    fn eq(&self, other: &Amount<u32>) -> bool {
        self.minor == i64::from(other.minor)
    }
}

impl PartialOrd<Amount<u32>> for Amount<i64> {
    fn partial_cmp(&self, other: &Amount<u32>) -> Option<Ordering> {
        Some(self.minor.cmp(&i64::from(other.minor)))
    }
}

pub fn deserialize_optional_amount<'de, D>(deserializer: D) -> Result<Option<Amount<u32>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptionalAmount;

    impl<'de> Visitor<'de> for OptionalAmount {
        type Value = Option<Amount<u32>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an optional amount with up to 4 decimal places")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            let value = String::deserialize(deserializer)?;
            parse_amount(&value).map(Some).map_err(de::Error::custom)
        }
    }

    deserializer.deserialize_option(OptionalAmount)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn unsigned(minor: u32) -> Amount<u32> {
        Amount::from_minor(minor)
    }

    fn signed(minor: i64) -> Amount<i64> {
        Amount::from_minor(minor)
    }

    #[test]
    fn parses_up_to_four_decimal_places() {
        assert_eq!(parse_amount("0.0001").unwrap(), unsigned(1));
        assert_eq!(parse_amount("1").unwrap(), unsigned(10_000));
        assert_eq!(parse_amount("1.5").unwrap(), unsigned(15_000));
        assert_eq!(parse_amount("1.25").unwrap(), unsigned(12_500));
        assert_eq!(parse_amount("1.5000").unwrap(), unsigned(15_000));
        assert_eq!(parse_amount("3.2515").unwrap(), unsigned(32_515));
        assert_eq!(parse_amount("10.5").unwrap(), unsigned(105_000));
    }

    #[test]
    fn rejects_malformed_amounts() {
        for input in ["", "1.23456", "1.2.3", "-1.0", "+1.0", ".5", "1.2a", " 1.0"] {
            assert!(parse_amount(input).is_err(), "{input}");
        }
    }

    #[test]
    fn formats_four_decimal_places() {
        assert_eq!(format_amount(unsigned(0)), "0.0000");
        assert_eq!(format_amount(unsigned(1)), "0.0001");
        assert_eq!(format_amount(unsigned(15_000)), "1.5000");
        assert_eq!(format_amount(unsigned(32_515)), "3.2515");
        assert_eq!(format_amount(signed(-40_000)), "-4.0000");
        assert_eq!(format_amount(signed(-35_000)), "-3.5000");
    }

    #[test]
    fn signed_available_can_subtract_an_unsigned_deposit() {
        let available = signed(50_000) - unsigned(80_000);
        assert_eq!(available, signed(-30_000));
        assert!(available < unsigned(0));
    }

    #[test]
    fn formatted_amounts_parse_back() {
        for amount in [0, 1, 15_000, 32_515, 110_000] {
            let formatted = format_amount(unsigned(amount));
            assert_eq!(parse_amount(&formatted).unwrap(), unsigned(amount));
        }
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        #[serde(deserialize_with = "deserialize_optional_amount")]
        amount: Option<Amount<u32>>,
    }

    #[test]
    fn deserializes_csv_amounts() {
        let mut reader = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader("type,amount\ndeposit,1.5\ndispute,\ndeposit,0.0001\n".as_bytes());
        let rows: Vec<Row> = reader.deserialize().collect::<Result<_, _>>().unwrap();
        assert_eq!(
            rows,
            vec![
                Row {
                    amount: Some(unsigned(15_000))
                },
                Row { amount: None },
                Row {
                    amount: Some(unsigned(1))
                },
            ]
        );
    }
}
