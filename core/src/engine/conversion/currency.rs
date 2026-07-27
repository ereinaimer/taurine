struct IsoCurrencyInfo {
    code: &'static str,
    major_singular: &'static str,
    major_plural: &'static str,
    minor_singular: &'static str,
    minor_plural: &'static str,
    is_indian: bool,
}

const ISO_CURRENCIES: &[IsoCurrencyInfo] = &[
    IsoCurrencyInfo {
        code: "USD",
        major_singular: "dollar",
        major_plural: "dollars",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "EUR",
        major_singular: "euro",
        major_plural: "euros",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "GBP",
        major_singular: "pound",
        major_plural: "pounds",
        minor_singular: "penny",
        minor_plural: "pence",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "JPY",
        major_singular: "yen",
        major_plural: "yen",
        minor_singular: "sen",
        minor_plural: "sen",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "INR",
        major_singular: "rupee",
        major_plural: "rupees",
        minor_singular: "paisa",
        minor_plural: "paise",
        is_indian: true,
    },
    IsoCurrencyInfo {
        code: "KRW",
        major_singular: "won",
        major_plural: "won",
        minor_singular: "jeon",
        minor_plural: "jeon",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "ILS",
        major_singular: "shekel",
        major_plural: "shekels",
        minor_singular: "agora",
        minor_plural: "agorot",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "VND",
        major_singular: "dong",
        major_plural: "dong",
        minor_singular: "hao",
        minor_plural: "hao",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "RUB",
        major_singular: "ruble",
        major_plural: "rubles",
        minor_singular: "kopek",
        minor_plural: "kopeks",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "BTC",
        major_singular: "bitcoin",
        major_plural: "bitcoins",
        minor_singular: "satoshi",
        minor_plural: "satoshis",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "BDT",
        major_singular: "taka",
        major_plural: "taka",
        minor_singular: "poisha",
        minor_plural: "poisha",
        is_indian: true,
    },
    IsoCurrencyInfo {
        code: "AUD",
        major_singular: "dollar",
        major_plural: "dollars",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "CAD",
        major_singular: "dollar",
        major_plural: "dollars",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "CHF",
        major_singular: "franc",
        major_plural: "francs",
        minor_singular: "centime",
        minor_plural: "centimes",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "CNY",
        major_singular: "yuan",
        major_plural: "yuan",
        minor_singular: "fen",
        minor_plural: "fen",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "SEK",
        major_singular: "krona",
        major_plural: "kronor",
        minor_singular: "ore",
        minor_plural: "ore",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "NZD",
        major_singular: "dollar",
        major_plural: "dollars",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "MXN",
        major_singular: "peso",
        major_plural: "pesos",
        minor_singular: "centavo",
        minor_plural: "centavos",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "SGD",
        major_singular: "dollar",
        major_plural: "dollars",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "HKD",
        major_singular: "dollar",
        major_plural: "dollars",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "NOK",
        major_singular: "krone",
        major_plural: "kroner",
        minor_singular: "ore",
        minor_plural: "ore",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "TRY",
        major_singular: "lira",
        major_plural: "liras",
        minor_singular: "kurus",
        minor_plural: "kurus",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "BRL",
        major_singular: "real",
        major_plural: "reais",
        minor_singular: "centavo",
        minor_plural: "centavos",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "ZAR",
        major_singular: "rand",
        major_plural: "rand",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "DKK",
        major_singular: "krone",
        major_plural: "kroner",
        minor_singular: "ore",
        minor_plural: "ore",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "PLN",
        major_singular: "zloty",
        major_plural: "zlotys",
        minor_singular: "grosz",
        minor_plural: "groszy",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "TWD",
        major_singular: "dollar",
        major_plural: "dollars",
        minor_singular: "cent",
        minor_plural: "cents",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "THB",
        major_singular: "baht",
        major_plural: "baht",
        minor_singular: "satang",
        minor_plural: "satang",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "IDR",
        major_singular: "rupiah",
        major_plural: "rupiah",
        minor_singular: "sen",
        minor_plural: "sen",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "HUF",
        major_singular: "forint",
        major_plural: "forints",
        minor_singular: "filler",
        minor_plural: "fillers",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "CLP",
        major_singular: "peso",
        major_plural: "pesos",
        minor_singular: "centavo",
        minor_plural: "centavos",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "PHP",
        major_singular: "peso",
        major_plural: "pesos",
        minor_singular: "centavo",
        minor_plural: "centavos",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "AED",
        major_singular: "dirham",
        major_plural: "dirhams",
        minor_singular: "fils",
        minor_plural: "fils",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "COP",
        major_singular: "peso",
        major_plural: "pesos",
        minor_singular: "centavo",
        minor_plural: "centavos",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "SAR",
        major_singular: "riyal",
        major_plural: "riyals",
        minor_singular: "halala",
        minor_plural: "halalas",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "MYR",
        major_singular: "ringgit",
        major_plural: "ringgit",
        minor_singular: "sen",
        minor_plural: "sen",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "RON",
        major_singular: "leu",
        major_plural: "lei",
        minor_singular: "ban",
        minor_plural: "bani",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "PEN",
        major_singular: "sol",
        major_plural: "soles",
        minor_singular: "centimo",
        minor_plural: "centimos",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "ARS",
        major_singular: "peso",
        major_plural: "pesos",
        minor_singular: "centavo",
        minor_plural: "centavos",
        is_indian: false,
    },
    IsoCurrencyInfo {
        code: "EGP",
        major_singular: "pound",
        major_plural: "pounds",
        minor_singular: "piastre",
        minor_plural: "piastres",
        is_indian: false,
    },
];

enum CurrencySource {
    Symbol(&'static CurrencyInfo),
    IsoCode(&'static IsoCurrencyInfo),
}

pub fn has_currency_prefix(input: &str) -> bool {
    let trimmed = input.trim();
    let mut remaining = trimmed;
    if remaining.starts_with('-') {
        remaining = &remaining[1..];
    }
    remaining = remaining.trim_start();

    if remaining.chars().count() >= 4 {
        let code: String = remaining.chars().take(3).collect();
        if code.chars().all(|c| c.is_ascii_uppercase())
            && let Some(next_char) = remaining.chars().nth(3)
            && next_char.is_whitespace()
            && ISO_CURRENCIES.iter().any(|c| c.code == code.as_str())
        {
            return true;
        }
    }

    if let Some(first_char) = remaining.chars().next()
        && CURRENCIES.iter().any(|c| c.symbol == first_char)
    {
        return true;
    }

    false
}

pub fn convert_to_words(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut is_negative = false;
    let mut remaining = trimmed;

    // 1. Check for leading negative sign
    if remaining.starts_with('-') {
        is_negative = true;
        remaining = &remaining[1..];
    }
    remaining = remaining.trim_start();

    if remaining.is_empty() {
        return None;
    }

    // 2. Check for currency code or symbol
    let mut source = None;
    let mut parsed_code_prefix = false;

    if remaining.chars().count() >= 4 {
        let code: String = remaining.chars().take(3).collect();
        if code.chars().all(|c| c.is_ascii_uppercase())
            && let Some(next_char) = remaining.chars().nth(3)
            && next_char.is_whitespace()
            && let Some(info) = ISO_CURRENCIES.iter().find(|c| c.code == code.as_str())
        {
            source = Some(CurrencySource::IsoCode(info));
            remaining = &remaining[3..];
            parsed_code_prefix = true;
        }
    }

    if source.is_none()
        && let Some(first_char) = remaining.chars().next()
        && let Some(info) = CURRENCIES.iter().find(|c| c.symbol == first_char)
    {
        source = Some(CurrencySource::Symbol(info));
        remaining = &remaining[first_char.len_utf8()..];
    }

    remaining = remaining.trim_start();

    // 3. Check for negative sign after the prefix (only allowed for ISO codes)
    if remaining.starts_with('-') {
        if parsed_code_prefix {
            if is_negative {
                return None; // Double negative is invalid
            }
            is_negative = true;
            remaining = &remaining[1..];
            remaining = remaining.trim_start();
        } else {
            return None; // Negative after symbol is invalid
        }
    }

    // 4. Strip commas and validate digits/decimal point
    let cleaned = remaining.replace(',', "");
    if cleaned.is_empty() || !cleaned.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }

    let mut has_decimal = false;
    for c in cleaned.chars() {
        if c == '.' {
            if has_decimal {
                return None;
            }
            has_decimal = true;
        } else if !c.is_ascii_digit() {
            return None;
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

    // Get currency details if present
    let (is_indian, major_singular, major_plural, minor_singular, minor_plural) = match source {
        Some(CurrencySource::Symbol(info)) => (
            info.is_indian,
            info.major_singular,
            info.major_plural,
            info.minor_singular,
            info.minor_plural,
        ),
        Some(CurrencySource::IsoCode(info)) => (
            info.is_indian,
            info.major_singular,
            info.major_plural,
            info.minor_singular,
            info.minor_plural,
        ),
        None => (false, "", "", "", ""),
    };

    // Convert integer part to words
    let integer_words = if integer_val > 0 {
        if is_indian {
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

    if source.is_some() {
        // Currency formatting
        let major_label = if integer_val == 1 {
            major_singular
        } else {
            major_plural
        };

        let minor_label = if fraction_val == 1 {
            minor_singular
        } else {
            minor_plural
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
    } else {
        // Plain number formatting
        match (integer_val > 0, fraction_val > 0) {
            (true, true) => {
                result.push_str(&format!("{} point {}", integer_words, fraction_words));
            }
            (true, false) => {
                result.push_str(&integer_words);
            }
            (false, true) => {
                result.push_str(&format!("zero point {}", fraction_words));
            }
            (false, false) => {
                result.push_str("zero");
            }
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
        // Double negative sign
        assert_eq!(convert_to_words("-USD -42"), None);
        // Invalid number
        assert_eq!(convert_to_words("abc"), None);
    }

    #[test]
    fn test_currency_conversion_iso_codes() {
        assert_eq!(
            convert_to_words("INR 14,500").as_deref(),
            Some("Fourteen thousand five hundred rupees")
        );
        assert_eq!(
            convert_to_words("USD 3,200").as_deref(),
            Some("Three thousand two hundred dollars")
        );
        assert_eq!(
            convert_to_words("EUR 50.99").as_deref(),
            Some("Fifty euros and ninety-nine cents")
        );
        assert_eq!(convert_to_words("INR 0").as_deref(), Some("Zero rupees"));
        assert_eq!(
            convert_to_words("-USD 50.25").as_deref(),
            Some("Negative fifty dollars and twenty-five cents")
        );
        assert_eq!(
            convert_to_words("USD -50.25").as_deref(),
            Some("Negative fifty dollars and twenty-five cents")
        );
    }

    #[test]
    fn test_currency_conversion_plain_numbers() {
        assert_eq!(convert_to_words("0").as_deref(), Some("Zero"));
        assert_eq!(
            convert_to_words("-42").as_deref(),
            Some("Negative forty-two")
        );
        assert_eq!(
            convert_to_words("3.14").as_deref(),
            Some("Three point fourteen")
        );
    }
}
