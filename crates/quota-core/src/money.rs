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
    let text = expand_exponent(raw.trim())?;
    if text.is_empty() {
        return Err("空の金額です".into());
    }
    let (negative, rest) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix('+') {
        (false, rest)
    } else {
        (false, text.as_str())
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

/// Turn `1.25e2` into `125` without going through `f64`.
fn expand_exponent(text: &str) -> Result<String, String> {
    let Some(split) = text.find(['e', 'E']) else {
        return Ok(text.to_string());
    };
    let mantissa = &text[..split];
    let exponent_text = &text[split + 1..];
    if mantissa.is_empty() || exponent_text.is_empty() {
        return Err(format!("金額を解釈できません: {text}"));
    }
    let exponent: i32 = exponent_text
        .parse()
        .map_err(|_| format!("金額を解釈できません: {text}"))?;
    let (negative, rest) = if let Some(rest) = mantissa.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = mantissa.strip_prefix('+') {
        (false, rest)
    } else {
        (false, mantissa)
    };
    if rest.is_empty() {
        return Err(format!("金額を解釈できません: {text}"));
    }
    let mut digits = String::new();
    let mut point = None;
    for ch in rest.chars() {
        if ch == '.' {
            if point.is_some() {
                return Err(format!("金額を解釈できません: {text}"));
            }
            point = Some(digits.len());
        } else if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            return Err(format!("金額を解釈できません: {text}"));
        }
    }
    if digits.is_empty() {
        return Err(format!("金額を解釈できません: {text}"));
    }
    let point = point.unwrap_or(digits.len()) as i32;
    let shifted = point + exponent;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if shifted <= 0 {
        out.push_str("0.");
        for _ in 0..(-shifted) {
            out.push('0');
        }
        out.push_str(&digits);
    } else if shifted as usize >= digits.len() {
        out.push_str(&digits);
        for _ in 0..(shifted as usize - digits.len()) {
            out.push('0');
        }
    } else {
        let index = shifted as usize;
        out.push_str(&digits[..index]);
        out.push('.');
        out.push_str(&digits[index..]);
    }
    Ok(out)
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
        assert_eq!(parse_major_to_cents("1.005e0").unwrap(), 101);
        assert_eq!(parse_major_to_cents("1e-5").unwrap(), 0);
        assert_eq!(parse_major_to_cents("1.25e2").unwrap(), 12_500);
        let number = serde_json::from_str::<serde_json::Number>("1e-5").unwrap();
        assert_eq!(json_number_to_cents(&number).unwrap(), 0);
    }
}
