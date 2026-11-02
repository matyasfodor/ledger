use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::Deserialize;

/// Minor units of `1/10_000`. `1.5000` is stored as `15_000`.
pub type Amount = u64;

pub const SCALE: Amount = 10_000;

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

pub fn parse_amount(input: &str) -> Result<Amount, ParseAmountError> {
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

    let whole: Amount = whole.parse().map_err(|_| invalid())?;
    let frac = format!("{frac:0<4}");
    let frac: Amount = frac.parse().map_err(|_| invalid())?;
    whole
        .checked_mul(SCALE)
        .and_then(|scaled| scaled.checked_add(frac))
        .ok_or_else(invalid)
}

/// Four decimal places, matching the account output (`1.5000`).
pub fn format_amount(amount: Amount) -> String {
    let whole = amount / SCALE;
    let frac = amount % SCALE;
    format!("{whole}.{frac:04}")
}

pub fn deserialize_optional_amount<'de, D>(deserializer: D) -> Result<Option<Amount>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptionalAmount;

    impl<'de> Visitor<'de> for OptionalAmount {
        type Value = Option<Amount>;

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

    #[test]
    fn parses_up_to_four_decimal_places() {
        assert_eq!(parse_amount("0.0001").unwrap(), 1);
        assert_eq!(parse_amount("1").unwrap(), 10_000);
        assert_eq!(parse_amount("1.5").unwrap(), 15_000);
        assert_eq!(parse_amount("1.25").unwrap(), 12_500);
        assert_eq!(parse_amount("1.5000").unwrap(), 15_000);
        assert_eq!(parse_amount("3.2515").unwrap(), 32_515);
        assert_eq!(parse_amount("10.5").unwrap(), 105_000);
    }

    #[test]
    fn rejects_malformed_amounts() {
        for input in ["", "1.23456", "1.2.3", "-1.0", "+1.0", ".5", "1.2a", " 1.0"] {
            assert!(parse_amount(input).is_err(), "{input}");
        }
    }

    #[test]
    fn formats_four_decimal_places() {
        assert_eq!(format_amount(0), "0.0000");
        assert_eq!(format_amount(1), "0.0001");
        assert_eq!(format_amount(15_000), "1.5000");
        assert_eq!(format_amount(32_515), "3.2515");
    }

    #[test]
    fn formatted_amounts_parse_back() {
        for amount in [0, 1, 15_000, 32_515, 110_000] {
            let formatted = format_amount(amount);
            assert_eq!(parse_amount(&formatted).unwrap(), amount);
        }
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        #[serde(deserialize_with = "deserialize_optional_amount")]
        amount: Option<Amount>,
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
                Row { amount: Some(15_000) },
                Row { amount: None },
                Row { amount: Some(1) },
            ]
        );
    }
}
