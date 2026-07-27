pub fn convert_to_words(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut is_negative = false;
    let mut remaining = trimmed;

    if remaining.starts_with('-') {
        is_negative = true;
        remaining = &remaining[1..];
    }

    if remaining.is_empty() {
        return None;
    }

    // Find the currency symbol
    let first_char = remaining.chars().next()?;
    let info = CURRENCIES.iter().find(|c| c.symbol == first_char)?;

    // Strip the currency symbol
    remaining = &remaining[first_char.len_utf8()..];

    // Strip commas and validate digits/decimal point
    let cleaned = remaining.replace(',', "");
    if cleaned.is_empty() {
        return None;
    }

    // Ensure it contains only digits and at most one decimal point
    let mut has_decimal = false;
    for c in cleaned.chars() {
        if c == '.' {
            if has_decimal {
                return None; // Multiple decimal points
            }
            has_decimal = true;
        } else if !c.is_ascii_digit() {
            return None; // Invalid character
        }
    }

    // Split into integer and fractional parts
    let (integer_str, fraction_str) = match cleaned.split_once('.') {
        Some((i, f)) => (i, f),
        None => (cleaned.as_str(), ""),
    };

    let integer_val = if integer_str.is_empty() {
        0
    } else {
        integer_str.parse::<u64>().ok()?
    };

    // Normalize fraction to 2 digits
    let fraction_val = if fraction_str.is_empty() {
        0
    } else {
        let mut padded = fraction_str.to_string();
        if padded.len() == 1 {
            padded.push('0');
        } else if padded.len() > 2 {
            padded.truncate(2);
        }
        padded.parse::<u8>().ok()?
    };

    // Convert integer part to words
    let integer_words = if integer_val > 0 {
        if info.is_indian {
            convert_indian_number(integer_val)?
        } else {
            num2words::Num2Words::new(integer_val).to_words().ok()?
        }
    } else {
        String::new()
    };

    // Convert fractional part to words
    let fraction_words = if fraction_val > 0 {
        num2words::Num2Words::new(fraction_val).to_words().ok()?
    } else {
        String::new()
    };

    let mut result = String::new();

    if is_negative && (integer_val > 0 || fraction_val > 0) {
        result.push_str("negative ");
    }

    let major_label = if integer_val == 1 {
        info.major_singular
    } else {
        info.major_plural
    };

    let minor_label = if fraction_val == 1 {
        info.minor_singular
    } else {
        info.minor_plural
    };

    match (integer_val > 0, fraction_val > 0) {
        (true, true) => {
            result.push_str(&format!(
                "{} {} and {} {}",
                integer_words, major_label, fraction_words, minor_label
            ));
        }
        (true, false) => {
            result.push_str(&format!("{} {}", integer_words, major_label));
        }
        (false, true) => {
            result.push_str(&format!("{} {}", fraction_words, minor_label));
        }
        (false, false) => {
            result.push_str(&format!("zero {}", major_label));
        }
    }

    if result.is_empty() {
        return Some(result);
    }
    let mut chars = result.chars();
    let capitalized = match chars.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
    };
    Some(capitalized)
}

struct CurrencyInfo {
    symbol: char,
    major_singular: &'static str,
    major_plural: &'static str,
    minor_singular: &'static str,
    minor_plural: &'static str,
    is_indian: bool,
}

const CURRENCIES: &[CurrencyInfo] = &[
    CurrencyInfo {
        symbol: '$',
        major_singular: "dollar",
        major_plural: "dollars",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '€',
        major_singular: "euro",
        major_plural: "euros",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '£',
        major_singular: "pound",
        major_plural: "pounds",
        minor_singular: "penny",
        minor_plural: "pence",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '¥',
        major_singular: "yen",
        major_plural: "yen",
        minor_singular: "sen",
        minor_plural: "sen",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₹',
        major_singular: "rupee",
        major_plural: "rupees",
        minor_singular: "paisa",
        minor_plural: "paise",
        is_indian: true,
    },
    CurrencyInfo {
        symbol: '₩',
        major_singular: "won",
        major_plural: "won",
        minor_singular: "jeon",
        minor_plural: "jeon",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₪',
        major_singular: "shekel",
        major_plural: "shekels",
        minor_singular: "agora",
        minor_plural: "agorot",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₫',
        major_singular: "dong",
        major_plural: "dong",
        minor_singular: "hao",
        minor_plural: "hao",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₭',
        major_singular: "kip",
        major_plural: "kip",
        minor_singular: "att",
        minor_plural: "att",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₮',
        major_singular: "tugrik",
        major_plural: "tugriks",
        minor_singular: "mongo",
        minor_plural: "mongo",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₱',
        major_singular: "peso",
        major_plural: "pesos",
        minor_singular: "centavo",
        minor_plural: "centavos",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₲',
        major_singular: "guarani",
        major_plural: "guaranis",
        minor_singular: "centimo",
        minor_plural: "centimos",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₴',
        major_singular: "hryvnia",
        major_plural: "hryvnias",
        minor_singular: "kopiyka",
        minor_plural: "kopiykas",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₵',
        major_singular: "cedi",
        major_plural: "cedis",
        minor_singular: "pesewa",
        minor_plural: "pesewas",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₸',
        major_singular: "tenge",
        major_plural: "tenge",
        minor_singular: "tiyn",
        minor_plural: "tiyn",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₺',
        major_singular: "lira",
        major_plural: "liras",
        minor_singular: "kurus",
        minor_plural: "kurus",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₼',
        major_singular: "manat",
        major_plural: "manats",
        minor_singular: "qapik",
        minor_plural: "qapik",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₽',
        major_singular: "ruble",
        major_plural: "rubles",
        minor_singular: "kopek",
        minor_plural: "kopeks",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₾',
        major_singular: "lari",
        major_plural: "lari",
        minor_singular: "tetri",
        minor_plural: "tetri",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₿',
        major_singular: "bitcoin",
        major_plural: "bitcoins",
        minor_singular: "satoshi",
        minor_plural: "satoshis",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '؋',
        major_singular: "afghani",
        major_plural: "afghanis",
        minor_singular: "pul",
        minor_plural: "pul",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '৳',
        major_singular: "taka",
        major_plural: "taka",
        minor_singular: "poisha",
        minor_plural: "poisha",
        is_indian: true,
    },
    CurrencyInfo {
        symbol: '៛',
        major_singular: "riel",
        major_plural: "riels",
        minor_singular: "sen",
        minor_plural: "sen",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₦',
        major_singular: "naira",
        major_plural: "nairas",
        minor_singular: "kobo",
        minor_plural: "kobo",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₡',
        major_singular: "colon",
        major_plural: "colones",
        minor_singular: "centimo",
        minor_plural: "centimos",
        is_indian: false,
    },
    CurrencyInfo {
        symbol: '₳',
        major_singular: "austral",
        major_plural: "australes",
        minor_singular: "centavo",
        minor_plural: "centavos",
        is_indian: false,
    },
];

fn convert_indian_number(mut num: u64) -> Option<String> {
    if num == 0 {
        return Some("zero".to_string());
    }

    let mut parts = Vec::new();

    if num >= 10_000_000 {
        let crores = num / 10_000_000;
        let crore_words = num2words::Num2Words::new(crores).to_words().ok()?;
        parts.push(format!("{} crore", crore_words));
        num %= 10_000_000;
    }

    if num >= 100_000 {
        let lakhs = num / 100_000;
        let lakh_words = num2words::Num2Words::new(lakhs).to_words().ok()?;
        parts.push(format!("{} lakh", lakh_words));
        num %= 100_000;
    }

    if num >= 1_000 {
        let thousands = num / 1_000;
        let thousand_words = num2words::Num2Words::new(thousands).to_words().ok()?;
        parts.push(format!("{} thousand", thousand_words));
        num %= 1_000;
    }

    if num >= 100 {
        let hundreds = num / 100;
        let hundred_words = num2words::Num2Words::new(hundreds).to_words().ok()?;
        parts.push(format!("{} hundred", hundred_words));
        num %= 100;
    }

    if num > 0 {
        let remaining_words = num2words::Num2Words::new(num).to_words().ok()?;
        parts.push(remaining_words);
    }

    Some(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_currency_conversion_usd() {
        assert_eq!(
            convert_to_words("$1,200").as_deref(),
            Some("One thousand two hundred dollars")
        );
        assert_eq!(
            convert_to_words("$1.50").as_deref(),
            Some("One dollar and fifty cents")
        );
        assert_eq!(
            convert_to_words("$0.99").as_deref(),
            Some("Ninety-nine cents")
        );
        assert_eq!(
            convert_to_words("-$5").as_deref(),
            Some("Negative five dollars")
        );
        assert_eq!(convert_to_words("$0").as_deref(), Some("Zero dollars"));
    }

    #[test]
    fn test_currency_conversion_inr() {
        assert_eq!(
            convert_to_words("₹1,00,000").as_deref(),
            Some("One lakh rupees")
        );
        assert_eq!(
            convert_to_words("₹12,34,567.50").as_deref(),
            Some(
                "Twelve lakh thirty-four thousand five hundred sixty-seven rupees and fifty paise"
            )
        );
        assert_eq!(
            convert_to_words("₹10,00,00,000").as_deref(),
            Some("Ten crore rupees")
        );
    }

    #[test]
    fn test_currency_conversion_other() {
        assert_eq!(
            convert_to_words("€12.50").as_deref(),
            Some("Twelve euros and fifty cents")
        );
        assert_eq!(convert_to_words("£1").as_deref(), Some("One pound"));
    }

    #[test]
    fn test_currency_conversion_invalid() {
        // Negative sign after currency symbol
        assert_eq!(convert_to_words("$-50.25"), None);
        // No symbol
        assert_eq!(convert_to_words("1200"), None);
        // Invalid number
        assert_eq!(convert_to_words("abc"), None);
        // Normal decimals without currency symbol
        assert_eq!(convert_to_words("3.14"), None);
    }
}
