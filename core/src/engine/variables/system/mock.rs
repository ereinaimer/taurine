use fake::Fake;
use fake::faker::address::en::{
    BuildingNumber, CityName, CountryName, Latitude, Longitude, StateName, StreetName, ZipCode,
};
use fake::faker::company::en::{Bs, CatchPhrase, CompanyName};
use fake::faker::creditcard::en::CreditCardNumber;
use fake::faker::currency::en::{CurrencyCode, CurrencyName};
use fake::faker::http::en::ValidStatusCode;
use fake::faker::internet::en::{DomainSuffix, FreeEmail, Password, UserAgent, Username};
use fake::faker::job::en::Title as JobTitle;
use fake::faker::name::en::{FirstName, LastName, Name, Suffix, Title as NameTitle};
use fake::faker::phone_number::en::{CellNumber, PhoneNumber};
use rand::seq::IndexedRandom;

const HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

pub fn resolve(key: &str) -> Option<String> {
    let modifier = key.strip_prefix("mock.")?.trim();

    match parse_modifier(modifier)? {
        ("name", None) => Some(Name().fake()),
        ("first_name", None) => Some(FirstName().fake()),
        ("last_name", None) => Some(LastName().fake()),
        ("title", None) => Some(NameTitle().fake()),
        ("suffix", None) => Some(Suffix().fake()),
        ("address", None) => Some(format_address()),
        ("city", None) => Some(CityName().fake()),
        ("state", None) => Some(StateName().fake()),
        ("zip_code", None) => Some(ZipCode().fake()),
        ("country", None) => Some(CountryName().fake()),
        ("latitude", None) => Some(Latitude().fake()),
        ("longitude", None) => Some(Longitude().fake()),
        ("email", None) => Some(FreeEmail().fake()),
        ("domain", None) => Some(format_domain()),
        ("user_agent", None) => Some(UserAgent().fake()),
        ("password", Some(args)) => {
            let len = parse_password_len(args)?;
            let end = len.checked_add(1)?;
            Some(Password(len..end).fake())
        }
        ("username", None) => Some(Username().fake()),
        ("company", None) => Some(CompanyName().fake()),
        ("job_title", None) => Some(JobTitle().fake()),
        ("catch_phrase", None) => Some(CatchPhrase().fake()),
        ("bs", None) => Some(Bs().fake()),
        ("credit_card", None) => Some(CreditCardNumber().fake()),
        ("currency_name", None) => Some(CurrencyName().fake()),
        ("currency_code", None) => Some(CurrencyCode().fake()),
        ("phone_number", None) => Some(PhoneNumber().fake()),
        ("cell_number", None) => Some(CellNumber().fake()),
        ("status_code", None) => Some(format_status_code()),
        // fake-rs exposes status-code fakers but not HTTP-method fakers.
        ("method", None) => Some(random_method()?),
        _ => None,
    }
}

fn parse_modifier(input: &str) -> Option<(&str, Option<&str>)> {
    if let Some(paren_idx) = input.find('(') {
        let variant = input[..paren_idx].trim();
        let (args, trailing) = scan_parenthesized(&input[paren_idx..])?;
        if !variant.is_empty() && trailing.trim().is_empty() {
            Some((variant, Some(args)))
        } else {
            None
        }
    } else if input.contains(')') {
        None
    } else {
        Some((input.trim(), None)).filter(|(variant, _)| !variant.is_empty())
    }
}

fn scan_parenthesized(input: &str) -> Option<(&str, &str)> {
    if !input.starts_with('(') {
        return None;
    }

    let mut depth = 0usize;
    let mut start = None;

    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => {
                if depth == 0 {
                    start = Some(idx + ch.len_utf8());
                }
                depth += 1;
            }
            ')' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    let start = start?;
                    return Some((input[start..idx].trim(), &input[idx + 1..]));
                }
            }
            _ => {}
        }
    }

    None
}

fn parse_password_len(args: &str) -> Option<usize> {
    let trimmed = args.trim();
    if trimmed.is_empty() || trimmed.contains(',') {
        return None;
    }

    trimmed.parse::<usize>().ok()
}

fn format_address() -> String {
    format!(
        "{} {}, {}, {} {}",
        BuildingNumber().fake::<String>(),
        StreetName().fake::<String>(),
        CityName().fake::<String>(),
        StateName().fake::<String>(),
        ZipCode().fake::<String>(),
    )
}

fn format_domain() -> String {
    format!(
        "{}.{}",
        sanitize_domain_label(&Username().fake::<String>()),
        DomainSuffix().fake::<String>()
    )
}

fn sanitize_domain_label(input: &str) -> String {
    let label = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if label.is_empty() {
        "example".to_string()
    } else {
        label
    }
}

fn random_method() -> Option<String> {
    let mut rng = rand::rng();
    HTTP_METHODS
        .choose(&mut rng)
        .map(|method| (*method).to_string())
}

fn format_status_code() -> String {
    ValidStatusCode()
        .fake::<String>()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_identity_variants() {
        assert!(!resolve("mock.name").unwrap().trim().is_empty());
        assert!(!resolve("mock.first_name").unwrap().trim().is_empty());
    }

    #[test]
    fn resolves_geography_variants() {
        let address = resolve("mock.address").unwrap();
        let latitude = resolve("mock.latitude").unwrap();

        assert!(address.contains(','));
        assert!(latitude.parse::<f64>().is_ok());
    }

    #[test]
    fn resolves_web_variants() {
        let email = resolve("mock.email").unwrap();
        let password = resolve("mock.password(12)").unwrap();

        assert!(email.contains('@'));
        assert_eq!(password.chars().count(), 12);
    }

    #[test]
    fn resolves_professional_variants() {
        assert!(!resolve("mock.company").unwrap().trim().is_empty());
        assert!(!resolve("mock.job_title").unwrap().trim().is_empty());
    }

    #[test]
    fn resolves_financial_variants() {
        let code = resolve("mock.currency_code").unwrap();

        assert_eq!(code.chars().count(), 3);
        assert!(!resolve("mock.credit_card").unwrap().trim().is_empty());
    }

    #[test]
    fn resolves_communication_variants() {
        assert!(!resolve("mock.phone_number").unwrap().trim().is_empty());
        assert!(!resolve("mock.cell_number").unwrap().trim().is_empty());
    }

    #[test]
    fn resolves_development_variants() {
        let status = resolve("mock.status_code").unwrap();
        let method = resolve("mock.method").unwrap();

        assert!(status.parse::<u16>().is_ok());
        assert!(HTTP_METHODS.contains(&method.as_str()));
    }

    #[test]
    fn rejects_invalid_password_args() {
        assert_eq!(resolve("mock.password"), None);
        assert_eq!(resolve("mock.password()"), None);
        assert_eq!(resolve("mock.password(nope)"), None);
        assert_eq!(resolve("mock.password(12, 16)"), None);
    }
}
