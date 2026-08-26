pub mod currency;

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

#[cfg(test)]
thread_local! {
    pub(crate) static MOCK_RATES: std::cell::RefCell<Option<HashMap<String, f64>>> = const { std::cell::RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitCategory {
    Temperature,
    Length,
    Mass,
    Volume,
    Area,
    Time,
    Speed,
    Data,
    DataRate,
    Pressure,
    Power,
    Css,
    Angle,
    Energy,
    Force,
    Frequency,
    Voltage,
    Current,
    Resistance,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ExchangeRatesResponse {
    result: String,
    base_code: String,
    rates: HashMap<String, f64>,
    time_last_update_unix: i64,
}

pub fn is_conversion_pattern(s: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^([+-]?[0-9]+(?:\.[0-9]+)?)([a-zA-Z/0-9_]+)=([a-zA-Z0-9_]+)$")
            .expect("valid conversion regex")
    });
    re.is_match(s)
}

fn get_data_rate_factor(unit: &str) -> Option<f64> {
    match unit {
        "bps" => Some(0.125),
        "Bps" => Some(1.0),
        "kbps" | "Kbps" => Some(125.0),
        "KBps" => Some(1000.0),
        "mbps" | "Mbps" => Some(125000.0),
        "MBps" => Some(1000000.0),
        "gbps" | "Gbps" => Some(125000000.0),
        "GBps" => Some(1000000000.0),
        "tbps" | "Tbps" => Some(125000000000.0),
        "TBps" => Some(1000000000000.0),
        _ => None,
    }
}

fn get_physical_unit_factor(unit: &str) -> Option<(UnitCategory, f64)> {
    if let Some(factor) = get_data_rate_factor(unit) {
        return Some((UnitCategory::DataRate, factor));
    }
    let u = unit.to_lowercase();
    // Disambiguate MW (megawatt) vs mW (milliwatt)
    if u == "mw" {
        return if unit.starts_with('M') {
            Some((UnitCategory::Power, 1000000.0))
        } else {
            Some((UnitCategory::Power, 0.001))
        };
    }
    match u.as_str() {
        // Length (Base: m)
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => {
            Some((UnitCategory::Length, 0.001))
        }
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => {
            Some((UnitCategory::Length, 0.01))
        }
        "m" | "meter" | "meters" | "metre" | "metres" => Some((UnitCategory::Length, 1.0)),
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => {
            Some((UnitCategory::Length, 1000.0))
        }
        "in" | "inch" | "inches" => Some((UnitCategory::Length, 0.0254)),
        "ft" | "foot" | "feet" => Some((UnitCategory::Length, 0.3048)),
        "yd" | "yard" | "yards" => Some((UnitCategory::Length, 0.9144)),
        "mi" | "mile" | "miles" => Some((UnitCategory::Length, 1609.344)),
        "nmi" => Some((UnitCategory::Length, 1852.0)),

        // Mass (Base: g)
        "mg" | "milligram" | "milligrams" => Some((UnitCategory::Mass, 0.001)),
        "ug" | "mcg" | "microgram" | "micrograms" => Some((UnitCategory::Mass, 0.000001)),
        "g" | "gram" | "grams" => Some((UnitCategory::Mass, 1.0)),
        "kg" | "kilogram" | "kilograms" => Some((UnitCategory::Mass, 1000.0)),
        "oz" | "ounce" | "ounces" => Some((UnitCategory::Mass, 28.349523125)),
        "lb" | "lbs" | "pound" | "pounds" => Some((UnitCategory::Mass, 453.59237)),
        "st" | "stone" | "stones" => Some((UnitCategory::Mass, 6350.29318)),
        "ton" | "tons" => Some((UnitCategory::Mass, 907184.74)),
        "tonne" | "tonnes" | "t" => Some((UnitCategory::Mass, 1000000.0)),

        // Volume (Base: l)
        "ml" | "mL" | "milliliter" | "milliliters" | "millilitre" | "millilitres" => {
            Some((UnitCategory::Volume, 0.001))
        }
        "l" | "L" | "liter" | "liters" | "litre" | "litres" => Some((UnitCategory::Volume, 1.0)),
        "tsp" | "teaspoon" | "teaspoons" => Some((UnitCategory::Volume, 0.00492892159375)),
        "tbsp" | "tablespoon" | "tablespoons" => Some((UnitCategory::Volume, 0.01478676478125)),
        "floz" | "fl_oz" => Some((UnitCategory::Volume, 0.0295735295625)),
        "cup" | "cups" => Some((UnitCategory::Volume, 0.2365882365)),
        "pt" | "pint" | "pints" => Some((UnitCategory::Volume, 0.473176473)),
        "qt" | "quart" | "quarts" => Some((UnitCategory::Volume, 0.946352946)),
        "gal" | "gallon" | "gallons" => Some((UnitCategory::Volume, 3.785411784)),

        // Area (Base: m2)
        "m2" | "sqm" => Some((UnitCategory::Area, 1.0)),
        "cm2" => Some((UnitCategory::Area, 0.0001)),
        "km2" => Some((UnitCategory::Area, 1000000.0)),
        "sqft" | "ft2" => Some((UnitCategory::Area, 0.09290304)),
        "acre" | "acres" | "ac" => Some((UnitCategory::Area, 4046.8564224)),
        "sqmi" | "mi2" => Some((UnitCategory::Area, 2589988.110336)),

        // Time (Base: s)
        "s" | "sec" | "secs" | "second" | "seconds" => Some((UnitCategory::Time, 1.0)),
        "ms" | "millisecond" | "milliseconds" => Some((UnitCategory::Time, 0.001)),
        "us" | "microsecond" | "microseconds" => Some((UnitCategory::Time, 0.000001)),
        "ns" | "nanosecond" | "nanoseconds" => Some((UnitCategory::Time, 0.000000001)),
        "min" | "mins" | "minute" | "minutes" => Some((UnitCategory::Time, 60.0)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some((UnitCategory::Time, 3600.0)),
        "d" | "day" | "days" => Some((UnitCategory::Time, 86400.0)),
        "wk" | "week" | "weeks" => Some((UnitCategory::Time, 604800.0)),
        "mo" | "month" | "months" => Some((UnitCategory::Time, 2629746.0)),
        "yr" | "year" | "years" => Some((UnitCategory::Time, 31556952.0)),

        // Speed (Base: m/s)
        "m/s" => Some((UnitCategory::Speed, 1.0)),
        "km/h" | "kph" => Some((UnitCategory::Speed, 1.0 / 3.6)),
        "mph" => Some((UnitCategory::Speed, 0.44704)),
        "knot" | "knots" | "kt" => Some((UnitCategory::Speed, 0.514444)),

        // Data (Base: b)
        "b" | "byte" | "bytes" => Some((UnitCategory::Data, 1.0)),
        "bit" | "bits" => Some((UnitCategory::Data, 0.125)),
        "kb" | "kilobyte" | "kilobytes" => Some((UnitCategory::Data, 1000.0)),
        "kbit" | "kilobit" | "kilobits" => Some((UnitCategory::Data, 125.0)),
        "mb" | "megabyte" | "megabytes" => Some((UnitCategory::Data, 1000000.0)),
        "mbit" | "megabit" | "megabits" => Some((UnitCategory::Data, 125000.0)),
        "gb" | "gigabyte" | "gigabytes" => Some((UnitCategory::Data, 1000000000.0)),
        "gbit" | "gigabit" | "gigabits" => Some((UnitCategory::Data, 125000000.0)),
        "tb" | "terabyte" | "terabytes" => Some((UnitCategory::Data, 1000000000000.0)),
        "pb" | "petabyte" | "petabytes" => Some((UnitCategory::Data, 1000000000000000.0)),
        "kib" | "kibibyte" | "kibibytes" => Some((UnitCategory::Data, 1024.0)),
        "mib" | "mebibyte" | "mebibytes" => Some((UnitCategory::Data, 1048576.0)),
        "gib" | "gibibyte" | "gibibytes" => Some((UnitCategory::Data, 1073741824.0)),
        "tib" | "tebibyte" | "tebibytes" => Some((UnitCategory::Data, 1099511627776.0)),
        "pib" | "pebibyte" | "pebibytes" => Some((UnitCategory::Data, 1125899906842624.0)),

        // Pressure (Base: psi)
        "psi" => Some((UnitCategory::Pressure, 1.0)),
        "bar" => Some((UnitCategory::Pressure, 14.503773773)),
        "pa" | "pascal" | "pascals" => Some((UnitCategory::Pressure, 0.0001450377)),
        "kpa" | "kilopascal" | "kilopascals" => Some((UnitCategory::Pressure, 0.1450377)),
        "atm" | "atmosphere" | "atmospheres" => Some((UnitCategory::Pressure, 14.6959488)),
        "torr" | "mmhg" => Some((UnitCategory::Pressure, 14.6959488 / 760.0)),

        // Power (Base: w)
        "w" | "watt" | "watts" => Some((UnitCategory::Power, 1.0)),
        "kw" | "kilowatt" | "kilowatts" => Some((UnitCategory::Power, 1000.0)),
        "hp" | "horsepower" => Some((UnitCategory::Power, 745.699872)),
        "megawatt" | "megawatts" => Some((UnitCategory::Power, 1000000.0)),
        "milliwatt" | "milliwatts" => Some((UnitCategory::Power, 0.001)),

        // Force (Base: N)
        "n" | "newton" | "newtons" => Some((UnitCategory::Force, 1.0)),
        "dyn" | "dyne" | "dynes" => Some((UnitCategory::Force, 0.00001)),
        "lbf" | "pound_force" => Some((UnitCategory::Force, 4.4482216152605)),
        "kgf" | "kilogram_force" => Some((UnitCategory::Force, 9.80665)),

        // Energy (Base: J)
        "j" | "joule" | "joules" => Some((UnitCategory::Energy, 1.0)),
        "kj" | "kilojoule" | "kilojoules" => Some((UnitCategory::Energy, 1000.0)),
        "mj" | "megajoule" | "megajoules" => Some((UnitCategory::Energy, 1000000.0)),
        "cal" | "calorie" | "calories" | "kcal" | "kilocalorie" | "kilocalories" => {
            Some((UnitCategory::Energy, 4184.0))
        }
        "btu" => Some((UnitCategory::Energy, 1055.06)),
        "wh" | "watt-hour" | "watt-hours" => Some((UnitCategory::Energy, 3600.0)),
        "kwh" | "kilowatt-hour" | "kilowatt-hours" => Some((UnitCategory::Energy, 3600000.0)),
        "ev" | "electronvolt" | "electronvolts" => Some((UnitCategory::Energy, 1.602176634e-19)),

        // Frequency (Base: Hz)
        "hz" | "hertz" => Some((UnitCategory::Frequency, 1.0)),
        "khz" | "kilohertz" => Some((UnitCategory::Frequency, 1000.0)),
        "mhz" | "megahertz" => Some((UnitCategory::Frequency, 1000000.0)),
        "ghz" | "gigahertz" => Some((UnitCategory::Frequency, 1000000000.0)),
        "thz" | "terahertz" => Some((UnitCategory::Frequency, 1000000000000.0)),

        // Voltage (Base: V)
        "v" | "volt" | "volts" => Some((UnitCategory::Voltage, 1.0)),
        "mv" | "millivolt" | "millivolts" => Some((UnitCategory::Voltage, 0.001)),
        "kv" | "kilovolt" | "kilovolts" => Some((UnitCategory::Voltage, 1000.0)),

        // Current (Base: A)
        "a" | "amp" | "amps" | "ampere" | "amperes" => Some((UnitCategory::Current, 1.0)),
        "ma" | "milliamp" | "milliamps" | "milliampere" | "milliamperes" => {
            Some((UnitCategory::Current, 0.001))
        }

        // Resistance (Base: Ω expressed as ohm)
        "ohm" | "ohms" => Some((UnitCategory::Resistance, 1.0)),
        "kohm" | "kiloohm" | "kiloohms" => Some((UnitCategory::Resistance, 1000.0)),
        "megohm" | "megaohm" | "megaohms" => Some((UnitCategory::Resistance, 1000000.0)),

        // Angle Units — deg / rad / grad / turn
        "deg" | "degree" | "degrees" => Some((UnitCategory::Angle, 1.0)),
        "rad" | "radian" | "radians" => Some((UnitCategory::Angle, 180.0 / std::f64::consts::PI)),
        "grad" | "gon" => Some((UnitCategory::Angle, 0.9)),
        "turn" | "turns" => Some((UnitCategory::Angle, 360.0)),

        // CSS Units — px / rem / em
        "px" | "pixel" | "pixels" => Some((UnitCategory::Css, 1.0)),
        "rem" => Some((UnitCategory::Css, 16.0)),
        "em" => Some((UnitCategory::Css, 16.0)),

        // Temperature (handled separately)
        "c" | "celsius" | "f" | "fahrenheit" | "k" | "kelvin" => {
            Some((UnitCategory::Temperature, 0.0))
        }

        _ => None,
    }
}

fn convert_temperature(val: f64, from: &str, to: &str) -> Option<f64> {
    let from_norm = from.to_lowercase();
    let to_norm = to.to_lowercase();
    let celsius = match from_norm.as_str() {
        "c" | "celsius" => val,
        "f" | "fahrenheit" => (val - 32.0) / 1.8,
        "k" | "kelvin" => val - 273.15,
        _ => return None,
    };
    match to_norm.as_str() {
        "c" | "celsius" => Some(celsius),
        "f" | "fahrenheit" => Some(celsius * 1.8 + 32.0),
        "k" | "kelvin" => Some(celsius + 273.15),
        _ => None,
    }
}

/// Convert a color string to a different format.
/// Supports compact syntax (`#ff0000=rgb`) and natural language (`#ff0000 to rgb`).
pub fn convert_color(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Compact syntax: #hex=format or name=format
    static COMPACT_RE: OnceLock<Regex> = OnceLock::new();
    let compact_re = COMPACT_RE.get_or_init(|| {
        Regex::new(r"^(#[0-9a-fA-F]+|[a-zA-Z]\w*)\s*=\s*([a-zA-Z]+)$")
            .expect("valid conversion regex")
    });

    if let Some(caps) = compact_re.captures(s) {
        let color_str = caps.get(1)?.as_str();
        let format = caps.get(2)?.as_str().to_lowercase();
        return format_color(color_str, &format);
    }

    // Natural language syntax: color to format
    static NL_RE: OnceLock<Regex> = OnceLock::new();
    let nl_re = NL_RE.get_or_init(|| {
        Regex::new(r"^(.+)\s+(to|in|into|as)\s+([a-zA-Z]+)$").expect("valid conversion regex")
    });

    if let Some(caps) = nl_re.captures(s) {
        let color_str = caps.get(1)?.as_str().trim();
        let format = caps.get(3)?.as_str().to_lowercase();
        return format_color(color_str, &format);
    }

    None
}

fn format_color(color_str: &str, format: &str) -> Option<String> {
    let transformer_name = match format {
        "hex" => "color.hex",
        "rgb" => "color.rgb",
        "rgba" => "color.rgba",
        "hsl" => "color.hsl",
        "hsla" => "color.hsla",
        _ => return None,
    };
    crate::engine::variables::system::transformers::color::apply(transformer_name, &[], color_str)
}

fn get_cache_path() -> PathBuf {
    crate::paths::get_data_dir().join("exchange_rates.json")
}

fn fetch_rates_sync() -> Option<ExchangeRatesResponse> {
    info!("Fetching exchange rates from API");
    let response = ureq::get("https://open.er-api.com/v6/latest/USD")
        .timeout(std::time::Duration::from_secs(2))
        .call()
        .ok()?;
    let rates: ExchangeRatesResponse = response.into_json().ok()?;
    if let Ok(serialized) = serde_json::to_string(&rates) {
        let cache_path = get_cache_path();
        if let Some(parent) = cache_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let tmp_path = cache_path.with_extension("tmp");
        if fs::write(&tmp_path, serialized).is_ok() {
            let _ = fs::rename(&tmp_path, &cache_path);
        }
    }
    Some(rates)
}

fn trigger_async_fetch() {
    std::thread::spawn(|| {
        if let Some(response) = ureq::get("https://open.er-api.com/v6/latest/USD")
            .timeout(std::time::Duration::from_secs(5))
            .call()
            .ok()
            && let Some(rates) = response.into_json::<ExchangeRatesResponse>().ok()
        {
            if let Ok(serialized) = serde_json::to_string(&rates) {
                let cache_path = get_cache_path();
                let tmp_path = cache_path.with_extension("tmp");
                if fs::write(&tmp_path, serialized).is_ok() {
                    let _ = fs::rename(&tmp_path, &cache_path);
                    info!("Exchange rates cache updated successfully");
                }
            }
        } else {
            error!("Failed to background fetch exchange rates");
        }
    });
}

fn get_fallback_rates() -> HashMap<String, f64> {
    let mut rates = HashMap::new();
    rates.insert("USD".to_string(), 1.0);
    rates.insert("EUR".to_string(), 0.915);
    rates.insert("INR".to_string(), 83.5);
    rates.insert("GBP".to_string(), 0.78);
    rates.insert("JPY".to_string(), 158.0);
    rates.insert("CAD".to_string(), 1.37);
    rates.insert("AUD".to_string(), 1.50);
    rates.insert("CNY".to_string(), 7.25);
    rates
}

fn get_exchange_rates() -> HashMap<String, f64> {
    #[cfg(test)]
    {
        if let Some(mock) = MOCK_RATES.with(|m| m.borrow().clone()) {
            return mock;
        }
    }

    let cache_path = get_cache_path();
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    if cache_path.exists()
        && let Ok(content) = fs::read_to_string(&cache_path)
        && let Ok(rates) = serde_json::from_str::<ExchangeRatesResponse>(&content)
    {
        // Cache is older than 15 minutes (900 seconds)
        let cache_mtime = fs::metadata(&cache_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if current_time - cache_mtime > 900 {
            debug!("Exchange rates cache is stale, triggering background refresh");
            trigger_async_fetch();
        }
        return rates.rates;
    }

    // Try synchronous fetch if cache doesn't exist
    if let Some(rates) = fetch_rates_sync() {
        rates.rates
    } else {
        warn!("Failed to fetch exchange rates, using fallback defaults");
        get_fallback_rates()
    }
}

fn convert_currency(val: f64, from: &str, to: &str) -> Option<f64> {
    let rates = get_exchange_rates();
    let from_rate = rates.get(&from.to_uppercase())?;
    let to_rate = rates.get(&to.to_uppercase())?;

    // Rate(from -> to) = to_rate / from_rate
    Some(val * (to_rate / from_rate))
}

pub fn convert(s: &str, _state: &crate::engine::state::EngineState) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^([+-]?[0-9]+(?:\.[0-9]+)?)([a-zA-Z/0-9_]+)=([a-zA-Z0-9_]+)$")
            .expect("valid conversion regex")
    });
    let caps = re.captures(s)?;
    let val_str = caps.get(1)?.as_str();
    let from_unit = caps.get(2)?.as_str();
    let to_unit = caps.get(3)?.as_str();

    let val = val_str.parse::<f64>().ok()?;

    // Check physical units first to avoid conflicts (e.g. mph/kph collision with 3-letter currency codes)
    let from_cat_opt = get_physical_unit_factor(from_unit);
    let to_cat_opt = get_physical_unit_factor(to_unit);

    if let Some((from_cat, from_factor)) = from_cat_opt
        && let Some((to_cat, to_factor)) = to_cat_opt
    {
        if from_cat != to_cat {
            return None;
        }

        let converted_val = if from_cat == UnitCategory::Temperature {
            convert_temperature(val, from_unit, to_unit)?
        } else {
            let val_in_base = val * from_factor;
            val_in_base / to_factor
        };

        let formatted = if converted_val.abs() < 0.01 && converted_val != 0.0 {
            format!("{:.4}", converted_val)
        } else {
            format!("{:.2}", converted_val)
        };
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        let suffix = if from_cat == UnitCategory::DataRate {
            to_unit.to_string()
        } else {
            to_unit.to_lowercase()
        };
        return Some(format!(
            "{}{}",
            if trimmed.is_empty() { "0" } else { trimmed },
            suffix
        ));
    }

    // Check if it's currency (3 letters and in exchange rates lookup)
    let is_from_currency =
        from_unit.len() == 3 && from_unit.chars().all(|c| c.is_ascii_alphabetic());
    let is_to_currency = to_unit.len() == 3 && to_unit.chars().all(|c| c.is_ascii_alphabetic());

    if is_from_currency && is_to_currency {
        let converted_val = convert_currency(val, from_unit, to_unit)?;
        let formatted = if converted_val.abs() < 0.01 && converted_val != 0.0 {
            format!("{:.4}", converted_val)
        } else {
            format!("{:.2}", converted_val)
        };
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        return Some(format!(
            "{}{}",
            if trimmed.is_empty() { "0" } else { trimmed },
            to_unit.to_lowercase()
        ));
    }

    None
}

/// Known NL command/query prefixes to strip before parsing a conversion expression.
const NL_PREFIXES: &[&str] = &[
    "convert ",
    "transform ",
    "change ",
    "calculate ",
    "compute ",
    "what is ",
    "what's ",
    "how much is ",
];

fn strip_nl_prefix(s: &str) -> &str {
    let lowered = s.to_lowercase();
    for &prefix in NL_PREFIXES {
        if lowered.starts_with(prefix) {
            return s[prefix.len()..].trim_start();
        }
    }
    s
}

fn preprocess_question_patterns(s: &str) -> Option<String> {
    let trimmed = s.trim().trim_end_matches(['?', '.']);
    let is_all_uppercase = trimmed.chars().all(|c| !c.is_lowercase());

    // Pattern 1: "how many <target> (are)? in <source_spec>"
    // e.g., "how many inches in a foot", "how many cm in 5 inches"
    static HOW_MANY_IN: OnceLock<Regex> = OnceLock::new();
    let how_many_in = HOW_MANY_IN.get_or_init(|| {
        Regex::new(r"(?i)^how many\s+(.+?)\s+(?:are\s+)?in\s+(.+)$").expect("valid regex")
    });

    let mut current = trimmed.to_string();
    if let Some(caps) = how_many_in.captures(trimmed) {
        let mut target = caps.get(1)?.as_str().trim().to_string();
        let source_spec = caps.get(2)?.as_str().trim();
        // Normalize target casing for all-uppercase input
        if is_all_uppercase {
            target = target.to_lowercase();
        }
        current = format!("{} to {}", source_spec, target);
    }

    // Pattern 2: "a/an <source> (to|in|into|as) <target>"
    // e.g., "a mile to feet", "an hour in minutes"
    // Also handles output from Pattern 1 like "a foot to inches"
    static A_AN_SEP: OnceLock<Regex> = OnceLock::new();
    let a_an_sep = A_AN_SEP.get_or_init(|| {
        Regex::new(r"(?i)^(?:a|an)\s+(.+?)\s+(?:to|in|into|as)\s+(.+)$").expect("valid regex")
    });

    if let Some(caps) = a_an_sep.captures(&current) {
        let source = caps.get(1)?.as_str().trim();
        let target = caps.get(2)?.as_str().trim();
        current = format!("1 {} to {}", source, target);
    }

    // Only return Some if we actually transformed the input
    if current != trimmed {
        Some(current)
    } else {
        None
    }
}

/// Parses a natural language conversion query (e.g. "100 dollars to Euros"),
/// normalizes the units and currencies, executes the conversion, and formats the result.
pub fn convert_natural(s: &str, state: &crate::engine::state::EngineState) -> Option<String> {
    // Pre-process question patterns to canonical format
    let preprocessed = preprocess_question_patterns(s);
    let input = preprocessed.as_deref().unwrap_or(s);

    // 1. Pre-process separators: pad '=' with spaces to make it a distinct token
    let cleaned = input.replace('=', " = ");

    // 2. Normalize whitespace and convert to lowercase for parsing
    let normalized = cleaned
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .to_lowercase();

    // 3. Tokenize
    let words: Vec<&str> = normalized.split_whitespace().collect();

    // 4. Scan right-to-left to find the rightmost separator token
    //    If a separator word appears at the last position, it's likely the target unit
    //    (e.g. "5cm to in" where "in" = inches), so we skip it and look left.
    let mut sep_index = None;
    let last = words.len().saturating_sub(1);
    for (i, &word) in words.iter().enumerate().rev() {
        if word == "to" || word == "into" || word == "as" || word == "in" || word == "=" {
            if i == last {
                continue;
            }
            sep_index = Some(i);
            break;
        }
    }

    let sep_idx = sep_index?;
    if sep_idx == 0 || sep_idx == words.len() - 1 {
        return None;
    }

    let original_words: Vec<&str> = cleaned.split_whitespace().collect();
    let original_right_part = original_words[sep_idx + 1..].join(" ");
    let original_left_part = original_words[..sep_idx].join(" ");

    let separator = words[sep_idx];
    let to_unit_raw = original_right_part.trim();

    // 5. Parse left side into number and from_unit
    // Handle leading currency symbol if any (e.g. $100 -> from_unit = "usd")
    let trimmed_left = strip_nl_prefix(original_left_part.trim());
    let first_char = trimmed_left.chars().next()?;

    let (val_str, from_unit_raw) = if let Some(iso) = get_currency_by_symbol(first_char) {
        let val_part = &trimmed_left[first_char.len_utf8()..];
        (val_part.trim(), iso)
    } else {
        static LEFT_RE: OnceLock<Regex> = OnceLock::new();
        let left_re = LEFT_RE.get_or_init(|| {
            Regex::new(r"^([+-]?[0-9,]+(?:\.[0-9]+)?)\s*(.*)$").expect("valid conversion regex")
        });
        let caps = left_re.captures(trimmed_left)?;
        let val_part = caps.get(1)?.as_str();
        let unit_part = caps.get(2)?.as_str().trim();
        (val_part, unit_part)
    };

    if val_str.is_empty() || to_unit_raw.is_empty() {
        return None;
    }

    // 6. Normalize units to standard forms for the backend engine
    let from_unit_normalized = normalize_unit_name(from_unit_raw);
    let to_unit_normalized = normalize_unit_name(to_unit_raw);

    // 7. Clean commas from the value and save the intervals for formatting later
    let (cleaned_val_str, intervals) = crate::engine::comma::preprocess(val_str);

    // 8. Reconstruct standard query pattern
    let std_pattern = format!(
        "{}{}{}{}",
        cleaned_val_str, from_unit_normalized, "=", to_unit_normalized
    );

    // 9. Execute conversion
    let converted_res = convert(&std_pattern, state)?;

    // 10. Strip the normalized unit suffix from the result to isolate the numeric value
    let suffix = to_unit_normalized.to_lowercase();
    let numeric_res = if converted_res.to_lowercase().ends_with(&suffix) {
        &converted_res[..converted_res.len() - suffix.len()]
    } else {
        &converted_res
    };
    // 11. Re-apply comma formatting to the numeric part
    let formatted_num = if let Some(ref ivs) = intervals {
        crate::engine::comma::format_result(numeric_res, ivs)
    } else if let Ok(source_val) = cleaned_val_str.parse::<f64>()
        && source_val.abs() >= 1000.0
        && let Ok(num) = numeric_res.parse::<f64>()
        && num.abs() >= 1000.0
    {
        crate::engine::comma::format_result(numeric_res, &[3])
    } else {
        numeric_res.to_string()
    };

    // 12. Reconstruct output respecting original target casing and spacing
    let is_natural_sep =
        separator == "to" || separator == "into" || separator == "as" || separator == "in";
    let output = if is_natural_sep || to_unit_raw.len() > 3 {
        format!("{} {}", formatted_num, to_unit_raw)
    } else {
        format!("{}{}", formatted_num, to_unit_raw)
    };

    Some(output)
}

fn get_currency_by_symbol(c: char) -> Option<&'static str> {
    match c {
        '$' => Some("usd"),
        '€' => Some("eur"),
        '£' => Some("gbp"),
        '₹' => Some("inr"),
        '¥' => Some("jpy"),
        '₩' => Some("krw"),
        '₪' => Some("ils"),
        '₫' => Some("vnd"),
        _ => None,
    }
}

fn normalize_unit_name(name: &str) -> String {
    let trimmed = name.trim();
    if get_data_rate_factor(trimmed).is_some() {
        return trimmed.to_string();
    }
    let lower = name.to_lowercase();

    // Normalization rules for multi-word units
    let multi_word_rules = [
        (r"\bsquare\s+feet\b", "sqft"),
        (r"\bsquare\s+foot\b", "sqft"),
        (r"\bsquare\s+meters?\b", "m2"),
        (r"\bsquare\s+metres?\b", "m2"),
        (r"\bsquare\s+miles?\b", "sqmi"),
        (r"\bsquare\s+kilometers?\b", "km2"),
        (r"\bsquare\s+kilometres?\b", "km2"),
        (r"\bsquare\s+centimeters?\b", "cm2"),
        (r"\bsquare\s+centimetres?\b", "cm2"),
        (r"\bfluid\s+ounces?\b", "floz"),
        (r"\bfluid\s+oz\b", "floz"),
        (r"\bfl\s+oz\b", "floz"),
        (r"\bmiles\s+per\s+hour\b", "mph"),
        (r"\bkilometers?\s+per\s+hour\b", "kph"),
        (r"\bkilometres?\s+per\s+hour\b", "kph"),
        (r"\bmeters?\s+per\s+second\b", "m/s"),
        (r"\bmetres?\s+per\s+second\b", "m/s"),
        (r"\bkilowatt[-\s]hours?\b", "kwh"),
        (r"\bwatt[-\s]hours?\b", "wh"),
        (r"\bpounds?[-\s]force\b", "pound_force"),
        (r"\bkilograms?[-\s]force\b", "kilogram_force"),
        (r"\bnautical\s+miles?\b", "nmi"),
    ];

    let mut normalized = lower.clone();
    for (pat, rep) in multi_word_rules {
        let re = Regex::new(pat).expect("valid static regex");
        normalized = re.replace_all(&normalized, rep).to_string();
    }

    // Currency names to 3-letter codes mapping
    match normalized.as_str() {
        "dollars" | "dollar" | "us dollars" | "us dollar" => "usd".to_string(),
        "euros" | "euro" => "eur".to_string(),
        "pounds" | "pound" | "british pounds" | "pound sterling" | "sterling" => "gbp".to_string(),
        "yen" => "jpy".to_string(),
        "rupees" | "rupee" => "inr".to_string(),
        "won" => "krw".to_string(),
        "shekels" | "shekel" | "new shekels" | "israeli shekels" => "ils".to_string(),
        "dong" => "vnd".to_string(),
        "rubles" | "ruble" => "rub".to_string(),
        "yuan" | "renminbi" => "cny".to_string(),
        "francs" | "franc" => "chf".to_string(),
        "pesos" | "peso" => "mxn".to_string(),
        _ => normalized,
    }
}

#[cfg(test)]
mod tests;
