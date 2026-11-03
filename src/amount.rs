use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};

/// Minor units of `1/10_000`. `1.5000` is stored as `15_000`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount<T> {
    minor: T,
}

impl<T> Amount<T> {
    pub const fn from_minor(minor: T) -> Self {
        Self { minor }
    }
}

impl Amount<u32> {
    pub(crate) fn is_zero(self) -> bool {
        self.minor == 0
    }
}

impl Amount<i64> {
    pub(crate) fn is_negative(self) -> bool {
        self.minor < 0
    }

    pub(crate) fn checked_add_positive(self, rhs: Amount<u32>) -> Option<Self> {
        self.minor
            .checked_add(i64::from(rhs.minor))
            .map(Self::from_minor)
    }

    pub(crate) fn checked_sub_positive(self, rhs: Amount<u32>) -> Option<Self> {
        self.minor
            .checked_sub(i64::from(rhs.minor))
            .map(Self::from_minor)
    }
}

/// A transaction amount. Non-negative, up to `429_496.7295`.
pub type PositiveAmount = Amount<u32>;

/// Available or held funds. Signed because a dispute can push available below zero,
/// and held is a sum of deposits.
pub type SignedAmount = Amount<i64>;

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

pub fn parse_amount(input: &str) -> Result<PositiveAmount, ParseAmountError> {
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

impl Formattable for PositiveAmount {
    fn write_amount(self) -> String {
        write_parts(false, u64::from(self.minor / SCALE), self.minor % SCALE)
    }
}

impl Formattable for SignedAmount {
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

impl From<PositiveAmount> for SignedAmount {
    fn from(amount: PositiveAmount) -> Self {
        Self {
            minor: i64::from(amount.minor),
        }
    }
}

impl Add for PositiveAmount {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            minor: self.minor.strict_add(rhs.minor),
        }
    }
}

impl AddAssign for PositiveAmount {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for PositiveAmount {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self {
            minor: self.minor.strict_sub(rhs.minor),
        }
    }
}

impl SubAssign for PositiveAmount {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Add for SignedAmount {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            minor: self.minor.strict_add(rhs.minor),
        }
    }
}

impl AddAssign for SignedAmount {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Add<PositiveAmount> for SignedAmount {
    type Output = Self;

    fn add(self, rhs: PositiveAmount) -> Self {
        Self {
            minor: self.minor.strict_add(i64::from(rhs.minor)),
        }
    }
}

impl AddAssign<PositiveAmount> for SignedAmount {
    fn add_assign(&mut self, rhs: PositiveAmount) {
        *self = *self + rhs;
    }
}

impl Sub<PositiveAmount> for SignedAmount {
    type Output = Self;

    fn sub(self, rhs: PositiveAmount) -> Self {
        Self {
            minor: self.minor.strict_sub(i64::from(rhs.minor)),
        }
    }
}

impl SubAssign<PositiveAmount> for SignedAmount {
    fn sub_assign(&mut self, rhs: PositiveAmount) {
        *self = *self - rhs;
    }
}

impl PartialEq<PositiveAmount> for SignedAmount {
    fn eq(&self, other: &PositiveAmount) -> bool {
        self.minor == i64::from(other.minor)
    }
}

impl PartialOrd<PositiveAmount> for SignedAmount {
    fn partial_cmp(&self, other: &PositiveAmount) -> Option<Ordering> {
        Some(self.minor.cmp(&i64::from(other.minor)))
    }
}

pub fn deserialize_optional_amount<'de, D>(
    deserializer: D,
) -> Result<Option<PositiveAmount>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptionalAmount;

    impl<'de> Visitor<'de> for OptionalAmount {
        type Value = Option<PositiveAmount>;

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

    fn unsigned(minor: u32) -> PositiveAmount {
        Amount::from_minor(minor)
    }

    fn signed(minor: i64) -> SignedAmount {
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
    fn checked_add_rejects_signed_overflow() {
        let max = signed(i64::MAX);
        assert!(max.checked_add_positive(unsigned(1)).is_none());
        let near = signed(i64::MAX - 10);
        assert_eq!(
            near.checked_add_positive(unsigned(10)).unwrap(),
            signed(i64::MAX)
        );
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
        amount: Option<PositiveAmount>,
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
