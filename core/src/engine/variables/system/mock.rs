use fake::Fake;
use fake::faker::address::en::{
    BuildingNumber, CityName, CountryName, StateName, StreetName, ZipCode,
};
use fake::faker::company::en::CompanyName;
use fake::faker::creditcard::en::CreditCardNumber;
use fake::faker::internet::en::{DomainSuffix, FreeEmail, Username};
use fake::faker::job::en::Title as JobTitle;
use fake::faker::name::en::{FirstName, LastName, Name};
use fake::faker::phone_number::en::{CellNumber, PhoneNumber};

pub fn resolve(key: &str) -> Option<String> {
    let modifier = key.strip_prefix("mock.")?.trim();

    match parse_modifier(modifier)? {
        ("name", None) => Some(Name().fake()),
        ("first_name", None) => Some(FirstName().fake()),
        ("last_name", None) => Some(LastName().fake()),
        ("address", None) => Some(format_address()),
        ("city", None) => Some(CityName().fake()),
        ("state", None) => Some(StateName().fake()),
        ("zip_code", None) => Some(ZipCode().fake()),
        ("country", None) => Some(CountryName().fake()),
        ("email", None) => Some(FreeEmail().fake()),
        ("domain", None) => Some(format_domain()),
        ("username", None) => Some(Username().fake()),
        ("company", None) => Some(CompanyName().fake()),
        ("job_title", None) => Some(JobTitle().fake()),
        ("credit_card", None) => Some(CreditCardNumber().fake()),
        ("phone_number", None) => Some(PhoneNumber().fake()),
        ("cell_number", None) => Some(CellNumber().fake()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_identity_variants() {
        assert!(!resolve("mock.name").unwrap().trim().is_empty());
        assert!(!resolve("mock.first_name").unwrap().trim().is_empty());
        assert!(!resolve("mock.last_name").unwrap().trim().is_empty());
    }

    #[test]
    fn resolves_geography_variants() {
        let address = resolve("mock.address").unwrap();
        assert!(address.contains(','));
        assert!(!resolve("mock.city").unwrap().trim().is_empty());
        assert!(!resolve("mock.state").unwrap().trim().is_empty());
        assert!(!resolve("mock.zip_code").unwrap().trim().is_empty());
        assert!(!resolve("mock.country").unwrap().trim().is_empty());
    }

    #[test]
    fn resolves_web_variants() {
        let email = resolve("mock.email").unwrap();
        assert!(email.contains('@'));
        assert!(!resolve("mock.username").unwrap().trim().is_empty());
        assert!(resolve("mock.domain").unwrap().contains('.'));
    }

    #[test]
    fn resolves_professional_variants() {
        assert!(!resolve("mock.company").unwrap().trim().is_empty());
        assert!(!resolve("mock.job_title").unwrap().trim().is_empty());
    }

    #[test]
    fn resolves_financial_and_communication_variants() {
        assert!(!resolve("mock.credit_card").unwrap().trim().is_empty());
        assert!(!resolve("mock.phone_number").unwrap().trim().is_empty());
        assert!(!resolve("mock.cell_number").unwrap().trim().is_empty());
    }

    #[test]
    fn rejects_pruned_variants() {
        assert_eq!(resolve("mock.password(12)"), None);
        assert_eq!(resolve("mock.bs"), None);
        assert_eq!(resolve("mock.status_code"), None);
    }
}
