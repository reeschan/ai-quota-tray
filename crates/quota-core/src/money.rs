use serde::{Deserialize, Serialize};

/// A currency amount that was reported by a source, stored as a decimal string
/// in major units (`"110.00"`). Sums are done in integer cents and formatted back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    pub amount: String,
    pub currency: String,
}

impl Money {
    pub fn from_cents(cents: i64, currency: &str) -> Self {
        Self {
            amount: format_cents(cents),
            currency: currency.to_string(),
        }
    }

    pub fn cents(&self) -> Result<i64, String> {
        parse_major_to_cents(&self.amount)
    }
}

pub fn format_cents(cents: i64) -> String {
    let negative = cents < 0;
    let abs = cents.unsigned_abs();
    let major = abs / 100;
    let minor = abs % 100;
    if negative {
        format!("-{major}.{minor:02}")
    } else {
        format!("{major}.{minor:02}")
    }
}

/// Parse a major-unit decimal into cents. A third fractional digit rounds half up.
pub fn parse_major_to_cents(raw: &str) -> Result<i64, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Err("空の金額です".into());
    }
    let (negative, rest) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix('+') {
        (false, rest)
    } else {
        (false, text)
    };
    if rest.is_empty() {
        return Err(format!("金額を解釈できません: {raw}"));
    }
    let mut pieces = rest.split('.');
    let whole = pieces.next().unwrap_or("");
    let frac = pieces.next().unwrap_or("");
    if pieces.next().is_some()
        || whole.is_empty()
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !frac.chars().all(|c| c.is_ascii_digit())
    {
        return Err(format!("金額を解釈できません: {raw}"));
    }
    if whole.len() > 15 {
        return Err("金額が大きすぎます".into());
    }
    let whole_value: i64 = whole
        .parse()
        .map_err(|_| "金額が大きすぎます".to_string())?;
    let mut cents = whole_value
        .checked_mul(100)
        .ok_or_else(|| "金額が大きすぎます".to_string())?;
    let bytes = frac.as_bytes();
    let first = digit_at(bytes, 0);
    let second = digit_at(bytes, 1);
    let third = digit_at(bytes, 2);
    cents = cents
        .checked_add(first * 10 + second)
        .ok_or_else(|| "金額が大きすぎます".to_string())?;
    if third >= 5 {
        cents = cents
            .checked_add(1)
            .ok_or_else(|| "金額が大きすぎます".to_string())?;
    }
    if negative {
        cents = cents
            .checked_neg()
            .ok_or_else(|| "金額が大きすぎます".to_string())?;
    }
    Ok(cents)
}

fn digit_at(bytes: &[u8], index: usize) -> i64 {
    bytes
        .get(index)
        .map(|byte| i64::from(byte - b'0'))
        .unwrap_or(0)
}

pub fn json_number_to_cents(number: &serde_json::Number) -> Result<i64, String> {
    parse_major_to_cents(&number.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_major_units() {
        assert_eq!(parse_major_to_cents("110.00").unwrap(), 11_000);
        assert_eq!(parse_major_to_cents("0.06").unwrap(), 6);
        assert_eq!(parse_major_to_cents("1.005").unwrap(), 101);
        assert_eq!(parse_major_to_cents("1.004").unwrap(), 100);
        assert_eq!(parse_major_to_cents("-10.50").unwrap(), -1_050);
        assert_eq!(parse_major_to_cents("10.").unwrap(), 1_000);
        assert_eq!(format_cents(-50), "-0.50");
        assert_eq!(format_cents(0), "0.00");
        assert!(parse_major_to_cents("").is_err());
        assert!(parse_major_to_cents("1.2.3").is_err());
        assert!(parse_major_to_cents("abc").is_err());
    }
}
