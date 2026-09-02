use super::*;

use crate::engine::state::EngineState;

#[test]
fn test_is_conversion_pattern() {
    assert!(is_conversion_pattern("100c=f"));
    assert!(is_conversion_pattern("-40c=f"));
    assert!(is_conversion_pattern("1.5gb=mb"));
    assert!(!is_conversion_pattern("100c"));
}

#[test]
fn test_physical_conversions() {
    let state = EngineState::new();
    assert_eq!(convert("100c=f", &state), Some("212f".to_string()));
    assert_eq!(convert("0c=k", &state), Some("273.15k".to_string()));
    assert_eq!(convert("1.5gb=mb", &state), Some("1500mb".to_string()));
    assert_eq!(convert("10kg=lbs", &state), Some("22.05lbs".to_string()));
    assert_eq!(convert("10miles=km", &state), Some("16.09km".to_string()));
    assert_eq!(convert("60mph=kph", &state), Some("96.56kph".to_string()));
    assert_eq!(convert("32psi=bar", &state), Some("2.21bar".to_string()));
}

#[test]
fn test_currency_fallback() {
    let state = EngineState::new();

    let mut mock = HashMap::new();
    mock.insert("USD".to_string(), 1.0);
    mock.insert("EUR".to_string(), 0.915);
    mock.insert("INR".to_string(), 83.5);

    MOCK_RATES.with(|m| *m.borrow_mut() = Some(mock));

    assert_eq!(convert("100usd=eur", &state), Some("91.5eur".to_string()));
    assert_eq!(
        convert("100eur=inr", &state),
        Some("9125.68inr".to_string())
    ); // 100 * (83.5 / 0.915) = 9125.683...

    MOCK_RATES.with(|m| *m.borrow_mut() = None);
}

#[test]
fn test_convert_natural() {
    let state = EngineState::new();

    let mut mock = HashMap::new();
    mock.insert("USD".to_string(), 1.0);
    mock.insert("EUR".to_string(), 0.915);
    MOCK_RATES.with(|m| *m.borrow_mut() = Some(mock));

    // Test basic NL physical conversion
    assert_eq!(
        convert_natural("32 celsius in fahrenheit", &state),
        Some("89.6 fahrenheit".to_string())
    );
    assert_eq!(
        convert_natural("1.5 gigabytes to megabytes", &state),
        Some("1500 megabytes".to_string())
    );

    // Test currency conversion with symbol
    assert_eq!(
        convert_natural("$100 to Euros", &state),
        Some("91.5 Euros".to_string())
    );

    // Test compact = syntax preserves no space behavior
    assert_eq!(
        convert_natural("1.5gb=mb", &state),
        Some("1500mb".to_string())
    );

    // Test formatting commas preservation
    assert_eq!(
        convert_natural("1,000 miles into kilometers", &state),
        Some("1,609.34 kilometers".to_string())
    );

    MOCK_RATES.with(|m| *m.borrow_mut() = None);
}

#[test]
fn test_nl_prefixes() {
    let state = EngineState::new();

    let mut mock = HashMap::new();
    mock.insert("USD".to_string(), 1.0);
    mock.insert("EUR".to_string(), 0.915);
    MOCK_RATES.with(|m| *m.borrow_mut() = Some(mock));

    // Single-word prefixes
    assert_eq!(
        convert_natural("convert 32 celsius in fahrenheit", &state),
        Some("89.6 fahrenheit".to_string())
    );
    assert_eq!(
        convert_natural("transform 32 celsius in fahrenheit", &state),
        Some("89.6 fahrenheit".to_string())
    );
    assert_eq!(
        convert_natural("change 32 celsius in fahrenheit", &state),
        Some("89.6 fahrenheit".to_string())
    );
    assert_eq!(
        convert_natural("calculate 32 celsius in fahrenheit", &state),
        Some("89.6 fahrenheit".to_string())
    );
    assert_eq!(
        convert_natural("compute 32 celsius in fahrenheit", &state),
        Some("89.6 fahrenheit".to_string())
    );

    // Multi-word prefixes
    assert_eq!(
        convert_natural("what is 32 celsius in fahrenheit", &state),
        Some("89.6 fahrenheit".to_string())
    );
    assert_eq!(
        convert_natural("what's 32 celsius in fahrenheit", &state),
        Some("89.6 fahrenheit".to_string())
    );
    assert_eq!(
        convert_natural("how much is 32 celsius in fahrenheit", &state),
        Some("89.6 fahrenheit".to_string())
    );

    // No prefix — existing behavior unchanged
    assert_eq!(
        convert_natural("32 celsius in fahrenheit", &state),
        Some("89.6 fahrenheit".to_string())
    );

    // Currency with prefix
    assert_eq!(
        convert_natural("convert $100 to Euros", &state),
        Some("91.5 Euros".to_string())
    );
    assert_eq!(
        convert_natural("change $50 to usd", &state),
        Some("50 usd".to_string())
    );

    // Prefix without number should return None
    assert_eq!(convert_natural("convert to euros", &state), None);
    assert_eq!(convert_natural("what is in fahrenheit", &state), None);

    MOCK_RATES.with(|m| *m.borrow_mut() = None);
}

#[test]
fn test_css_px_rem_conversion() {
    let state = EngineState::new();
    assert_eq!(convert("24px=rem", &state), Some("1.5rem".to_string()));
    assert_eq!(convert("1.25rem=px", &state), Some("20px".to_string()));
    assert_eq!(convert("16px=rem", &state), Some("1rem".to_string()));
    assert!(convert("10px=kg", &state).is_none());
}

#[test]
fn test_css_em_conversion() {
    let state = EngineState::new();
    assert_eq!(convert("2em=px", &state), Some("32px".to_string()));
    assert_eq!(convert("24px=em", &state), Some("1.5em".to_string()));
    assert_eq!(convert("1em=rem", &state), Some("1rem".to_string()));
    assert!(convert("10em=kg", &state).is_none());
}

#[test]
fn test_css_em_natural() {
    let state = EngineState::new();
    assert_eq!(
        convert_natural("2em to px", &state),
        Some("32 px".to_string())
    );
    assert_eq!(
        convert_natural("24px to em", &state),
        Some("1.5 em".to_string())
    );
}

#[test]
fn test_inline_color_conversion_compact() {
    assert_eq!(
        convert_color("#ff0000=rgb"),
        Some("rgb(255, 0, 0)".to_string())
    );
    assert_eq!(convert_color("#ff0000=hex"), Some("#FF0000".to_string()));
    assert_eq!(
        convert_color("#00ff00=hsl"),
        Some("hsl(120, 100%, 50%)".to_string())
    );
    assert_eq!(
        convert_color("#ff0000=rgba"),
        Some("rgba(255, 0, 0, 1)".to_string())
    );
    assert_eq!(convert_color("red=hex"), Some("#FF0000".to_string()));
}

#[test]
fn test_inline_color_conversion_natural() {
    assert_eq!(
        convert_color("#3b82f6 to rgb"),
        Some("rgb(59, 130, 246)".to_string())
    );
    assert_eq!(convert_color("red to hex"), Some("#FF0000".to_string()));
    assert_eq!(
        convert_color("rgb(59,130,246) to hex"),
        Some("#3B82F6".to_string())
    );
}

#[test]
fn test_inline_color_conversion_invalid() {
    assert_eq!(convert_color("notacolor=rgb"), None);
    assert_eq!(convert_color("#ff0000=invalid"), None);
    assert_eq!(convert_color(""), None);
}

#[test]
fn test_css_px_rem_natural() {
    let state = EngineState::new();
    assert_eq!(
        convert_natural("24px to rem", &state),
        Some("1.5 rem".to_string())
    );
    assert_eq!(
        convert_natural("1.25rem to px", &state),
        Some("20 px".to_string())
    );
}

// ── Angle unit tests ──────────────────────────────────────────────────

#[test]
fn test_angle_categories_incompatible() {
    let state = EngineState::new();
    assert!(convert("1deg=kg", &state).is_none());
    assert!(convert("1rad=m", &state).is_none());
}

#[test]
fn test_angle_deg_rad_conversion() {
    let state = EngineState::new();
    assert_eq!(convert("180deg=rad", &state), Some("3.14rad".to_string()));
    assert_eq!(convert("1rad=deg", &state), Some("57.3deg".to_string()));
}

#[test]
fn test_angle_deg_grad_conversion() {
    let state = EngineState::new();
    assert_eq!(convert("90deg=grad", &state), Some("100grad".to_string()));
    assert_eq!(convert("100grad=deg", &state), Some("90deg".to_string()));
}

#[test]
fn test_angle_deg_turn_conversion() {
    let state = EngineState::new();
    assert_eq!(convert("0.5turn=deg", &state), Some("180deg".to_string()));
    assert_eq!(convert("1turn=deg", &state), Some("360deg".to_string()));
}

#[test]
fn test_angle_rad_grad_conversion() {
    let state = EngineState::new();
    assert_eq!(convert("200grad=rad", &state), Some("3.14rad".to_string()));
    assert_eq!(
        convert("3.1416rad=grad", &state),
        Some("200grad".to_string())
    );
}

#[test]
fn test_angle_natural_deg_to_rad() {
    let state = EngineState::new();
    assert_eq!(
        convert_natural("180 degrees to rad", &state),
        Some("3.14 rad".to_string())
    );
    assert_eq!(
        convert_natural("57.3deg to rad", &state),
        Some("1 rad".to_string())
    );
}

// ── British spelling tests ────────────────────────────────────────────

#[test]
fn test_british_length_metres() {
    let state = EngineState::new();
    assert_eq!(convert("100m=ft", &state), Some("328.08ft".to_string()));
    let result = convert_natural("100 metres to ft", &state);
    assert_eq!(result, Some("328.08 ft".to_string()));
    assert_eq!(convert("5cm=in", &state), Some("1.97in".to_string()));
    assert_eq!(
        convert_natural("5 centimetres to in", &state),
        Some("1.97 in".to_string())
    );
    assert_eq!(convert("1000mm=m", &state), Some("1m".to_string()));
    assert_eq!(
        convert_natural("1000 millimetres to m", &state),
        Some("1 m".to_string())
    );
}

#[test]
fn test_british_length_kilometres() {
    let state = EngineState::new();
    assert_eq!(convert("5km=mi", &state), Some("3.11mi".to_string()));
    assert_eq!(
        convert_natural("5 kilometres to mi", &state),
        Some("3.11 mi".to_string())
    );
}

#[test]
fn test_british_volume_litres() {
    let state = EngineState::new();
    assert_eq!(convert("2l=qt", &state), Some("2.11qt".to_string()));
    assert_eq!(
        convert_natural("2 litres to qt", &state),
        Some("2.11 qt".to_string())
    );
    assert_eq!(convert("500ml=gal", &state), Some("0.13gal".to_string()));
    assert_eq!(
        convert_natural("500 millilitres to gal", &state),
        Some("0.13 gal".to_string())
    );
}

// ── Energy unit tests ─────────────────────────────────────────────────

#[test]
fn test_energy_categories_incompatible() {
    let state = EngineState::new();
    assert!(convert("1J=kg", &state).is_none());
    assert!(convert("1cal=m", &state).is_none());
}

#[test]
fn test_energy_joule_conversions() {
    let state = EngineState::new();
    assert_eq!(convert("1000J=kJ", &state), Some("1kj".to_string()));
    assert_eq!(convert("500J=kJ", &state), Some("0.5kj".to_string()));
}

#[test]
fn test_energy_calorie_conversions() {
    let state = EngineState::new();
    assert_eq!(convert("1cal=J", &state), Some("4184j".to_string()));
    assert_eq!(convert("1000J=cal", &state), Some("0.24cal".to_string()));
}

#[test]
fn test_energy_kcal_conversions() {
    let state = EngineState::new();
    assert_eq!(convert("1kcal=J", &state), Some("4184j".to_string()));
    assert_eq!(convert("250kcal=kJ", &state), Some("1046kj".to_string()));
    assert_eq!(
        convert_natural("500 calories to kJ", &state),
        Some("2092 kJ".to_string())
    );
}

#[test]
fn test_energy_btu_conversion() {
    let state = EngineState::new();
    assert_eq!(convert("1BTU=kJ", &state), Some("1.06kj".to_string()));
    assert_eq!(convert("1BTU=J", &state), Some("1055.06j".to_string()));
}

#[test]
fn test_energy_kwh_conversion() {
    let state = EngineState::new();
    assert_eq!(convert("1kWh=J", &state), Some("3600000j".to_string()));
    assert_eq!(convert("1Wh=J", &state), Some("3600j".to_string()));
}

#[test]
fn test_energy_ev_conversion() {
    let state = EngineState::new();
    assert_eq!(convert("1eV=J", &state), Some("0j".to_string()));
}

#[test]
fn test_energy_natural_joules() {
    let state = EngineState::new();
    assert_eq!(
        convert_natural("1000 joules to kilojoules", &state),
        Some("1 kilojoules".to_string())
    );
}

// ── Force unit tests ──────────────────────────────────────────────────

#[test]
fn test_force_categories_incompatible() {
    let state = EngineState::new();
    assert!(convert("1N=kg", &state).is_none());
}

#[test]
fn test_force_newton_dyne() {
    let state = EngineState::new();
    assert_eq!(convert("1N=dyn", &state), Some("100000dyn".to_string()));
    assert_eq!(convert("100000dyn=N", &state), Some("1n".to_string()));
}

#[test]
fn test_force_newton_pound_force() {
    let state = EngineState::new();
    assert_eq!(convert("1N=lbf", &state), Some("0.22lbf".to_string()));
    assert_eq!(convert("10lbf=N", &state), Some("44.48n".to_string()));
}

#[test]
fn test_force_kgf() {
    let state = EngineState::new();
    assert_eq!(convert("1kgf=N", &state), Some("9.81n".to_string()));
    assert_eq!(convert("1N=kgf", &state), Some("0.1kgf".to_string()));
}

#[test]
fn test_force_natural_newtons() {
    let state = EngineState::new();
    assert_eq!(
        convert_natural("10 newtons to dynes", &state),
        Some("1000000 dynes".to_string())
    );
}

// ── Frequency unit tests ──────────────────────────────────────────────

#[test]
fn test_frequency_categories_incompatible() {
    let state = EngineState::new();
    assert!(convert("1Hz=kg", &state).is_none());
}

#[test]
fn test_frequency_si_prefixes() {
    let state = EngineState::new();
    assert_eq!(convert("1000Hz=kHz", &state), Some("1khz".to_string()));
    assert_eq!(convert("1MHz=Hz", &state), Some("1000000hz".to_string()));
    assert_eq!(convert("1GHz=MHz", &state), Some("1000mhz".to_string()));
    assert_eq!(convert("1THz=GHz", &state), Some("1000ghz".to_string()));
}

#[test]
fn test_frequency_natural() {
    let state = EngineState::new();
    assert_eq!(
        convert_natural("1 megahertz to hertz", &state),
        Some("1000000 hertz".to_string())
    );
}

// ── Extension tests: new units in existing categories ──────────────────

#[test]
fn test_extend_length_nautical_mile() {
    let state = EngineState::new();
    assert_eq!(convert("1nmi=m", &state), Some("1852m".to_string()));
    assert_eq!(
        convert_natural("1 nautical mile to km", &state),
        Some("1.85 km".to_string())
    );
}

#[test]
fn test_extend_time_sub_second() {
    let state = EngineState::new();
    assert_eq!(convert("1s=ms", &state), Some("1000ms".to_string()));
    assert_eq!(convert("1000ms=s", &state), Some("1s".to_string()));
    assert_eq!(convert("1s=us", &state), Some("1000000us".to_string()));
    assert_eq!(convert("1s=ns", &state), Some("1000000000ns".to_string()));
}

#[test]
fn test_electricity_voltage() {
    let state = EngineState::new();
    assert_eq!(convert("1V=mV", &state), Some("1000mv".to_string()));
    assert_eq!(convert("1000mV=V", &state), Some("1v".to_string()));
    assert_eq!(convert("1kV=V", &state), Some("1000v".to_string()));
    assert_eq!(
        convert_natural("12 volts to mV", &state),
        Some("12000 mV".to_string())
    );
}

#[test]
fn test_electricity_current() {
    let state = EngineState::new();
    assert_eq!(convert("1A=mA", &state), Some("1000ma".to_string()));
    assert_eq!(convert("1000mA=A", &state), Some("1a".to_string()));
    assert_eq!(
        convert_natural("500 milliamps to A", &state),
        Some("0.5 A".to_string())
    );
}

#[test]
fn test_electricity_resistance() {
    let state = EngineState::new();
    assert_eq!(convert("1kohm=ohm", &state), Some("1000ohm".to_string()));
    assert_eq!(
        convert("1megohm=ohm", &state),
        Some("1000000ohm".to_string())
    );
}

#[test]
fn test_extend_pressure_kpa() {
    let state = EngineState::new();
    assert_eq!(convert("100kpa=psi", &state), Some("14.5psi".to_string()));
    assert_eq!(convert("35psi=kpa", &state), Some("241.32kpa".to_string()));
}

#[test]
fn test_extend_pressure_atm() {
    let state = EngineState::new();
    assert_eq!(convert("1atm=psi", &state), Some("14.7psi".to_string()));
    assert_eq!(convert("1atm=bar", &state), Some("1.01bar".to_string()));
    assert_eq!(convert("1atm=torr", &state), Some("760torr".to_string()));
}

#[test]
fn test_extend_data_bits() {
    let state = EngineState::new();
    assert_eq!(convert("1byte=bit", &state), Some("8bit".to_string()));
    assert_eq!(convert("1kb=kbit", &state), Some("8kbit".to_string()));
    assert_eq!(convert("100mb=mbit", &state), Some("800mbit".to_string()));
    assert_eq!(convert("1gb=gbit", &state), Some("8gbit".to_string()));
    assert_eq!(
        convert_natural("100 megabits to megabytes", &state),
        Some("12.5 megabytes".to_string())
    );
}

#[test]
fn test_extend_data_rate() {
    let state = EngineState::new();

    // Compact syntax
    assert_eq!(convert("50Mbps=MBps", &state), Some("6.25MBps".to_string()));
    assert_eq!(convert("10MBps=Mbps", &state), Some("80Mbps".to_string()));
    assert_eq!(convert("1MBps=Bps", &state), Some("1000000Bps".to_string()));
    assert_eq!(convert("1Gbps=MBps", &state), Some("125MBps".to_string()));
    assert_eq!(convert("1KBps=Kbps", &state), Some("8Kbps".to_string()));
    assert_eq!(convert("1TBps=Gbps", &state), Some("8000Gbps".to_string()));
    assert_eq!(convert("50mbps=MBps", &state), Some("6.25MBps".to_string()));
    assert_eq!(convert("10MBps=mbps", &state), Some("80mbps".to_string()));

    // Cross-category rejections
    assert!(convert("50Mbps=MB", &state).is_none());
    assert!(convert("50mb=mbps", &state).is_none());

    // Natural language
    assert_eq!(
        convert_natural("50 Mbps to MBps", &state),
        Some("6.25 MBps".to_string())
    );
    assert_eq!(
        convert_natural("10 MBps to Mbps", &state),
        Some("80 Mbps".to_string())
    );
    assert_eq!(
        convert_natural("1 gbps to MBps", &state),
        Some("125 MBps".to_string())
    );
    assert_eq!(
        convert_natural("50 mbps to MBps", &state),
        Some("6.25 MBps".to_string())
    );
}

#[test]
fn test_extend_power_mw() {
    let state = EngineState::new();
    assert_eq!(convert("1MW=kW", &state), Some("1000kw".to_string()));
    assert_eq!(convert("1W=mW", &state), Some("1000mw".to_string()));
}

#[test]
fn test_extend_mass_microgram() {
    let state = EngineState::new();
    assert_eq!(convert("1g=ug", &state), Some("1000000ug".to_string()));
}

#[test]
fn test_nl_target_first_pattern() {
    let state = EngineState::new();

    // Basic "how many X in a Y" (singular article in source)
    assert_eq!(
        convert_natural("how many inches in a foot", &state),
        Some("12 inches".to_string())
    );
    assert_eq!(
        convert_natural("how many GB in a TB", &state),
        Some("1000 GB".to_string())
    );

    // "how many X are in Y" (with "are")
    assert_eq!(
        convert_natural("how many inches are in a foot", &state),
        Some("12 inches".to_string())
    );

    // "how many X in N Y" (numeric source value)
    assert_eq!(
        convert_natural("how many centimeters in 5 inches", &state),
        Some("12.7 centimeters".to_string())
    );
    assert_eq!(
        convert_natural("how many meters in 10 feet", &state),
        Some("3.05 meters".to_string())
    );

    // Case insensitivity
    assert_eq!(
        convert_natural("HOW MANY INCHES IN A FOOT", &state),
        Some("12 inches".to_string())
    );

    // Trailing punctuation
    assert_eq!(
        convert_natural("how many inches in a foot?", &state),
        Some("12 inches".to_string())
    );
}

#[test]
fn test_nl_singular_articles_pattern() {
    let state = EngineState::new();

    // "a <unit> to <unit>"
    assert_eq!(
        convert_natural("a mile to feet", &state),
        Some("5280 feet".to_string())
    );
    assert_eq!(
        convert_natural("a km to meters", &state),
        Some("1000 meters".to_string())
    );

    // "an <unit> to <unit>" (vowel-initial units)
    assert_eq!(
        convert_natural("an hour to minutes", &state),
        Some("60 minutes".to_string())
    );
    assert_eq!(
        convert_natural("an inch to cm", &state),
        Some("2.54 cm".to_string())
    );

    // Other separators: in, into, as
    assert_eq!(
        convert_natural("a mile in feet", &state),
        Some("5280 feet".to_string())
    );
    assert_eq!(
        convert_natural("a mile into feet", &state),
        Some("5280 feet".to_string())
    );
    assert_eq!(
        convert_natural("a kg as lbs", &state),
        Some("2.2 lbs".to_string())
    );
}

#[test]
fn test_nl_complex_prepositional_pattern() {
    let state = EngineState::new();

    // "how many X are in N Y"
    assert_eq!(
        convert_natural("how many centimeters are in 5 inches", &state),
        Some("12.7 centimeters".to_string())
    );
    assert_eq!(
        convert_natural("how many km are in 3 miles", &state),
        Some("4.83 km".to_string())
    );
    assert_eq!(
        convert_natural("how many GB are in 2 TB", &state),
        Some("2000 GB".to_string())
    );

    // With decimal values
    assert_eq!(
        convert_natural("how many ml are in 1.5 cups", &state),
        Some("354.88 ml".to_string())
    );
}

#[test]
fn test_nl_question_patterns_output_formatting() {
    let state = EngineState::new();

    // Preserves target unit casing/spacing (natural language style)
    assert_eq!(
        convert_natural("how many INCHES in a foot", &state),
        Some("12 INCHES".to_string())
    );
    assert_eq!(
        convert_natural("how many Feet in a mile", &state),
        Some("5280 Feet".to_string())
    );

    // Comma formatting for large numbers
    assert_eq!(
        convert_natural("how many meters in 10000 feet", &state),
        Some("3,048 meters".to_string())
    );

    // Currency works through transform
    let mut mock = HashMap::new();
    mock.insert("USD".to_string(), 1.0);
    mock.insert("EUR".to_string(), 0.915);
    MOCK_RATES.with(|m| *m.borrow_mut() = Some(mock));

    assert_eq!(
        convert_natural("how many euros in 100 dollars", &state),
        Some("91.5 euros".to_string())
    );

    MOCK_RATES.with(|m| *m.borrow_mut() = None);

    // Verify "are there in" phrasing
    assert_eq!(
        convert_natural("how many inches are there in a foot", &state),
        Some("12 inches".to_string())
    );

    // Verify "one" prefix
    assert_eq!(
        convert_natural("one hour to minutes", &state),
        Some("60 minutes".to_string())
    );

    // Verify optimized output comma-formatting for small source / large target values
    assert_eq!(
        convert_natural("a mile to feet", &state),
        Some("5280 feet".to_string())
    );
}

#[test]
fn test_has_natural_conversion_intent() {
    assert!(has_natural_conversion_intent("100 dollars to euros"));
    assert!(has_natural_conversion_intent("5cm in inches"));
    assert!(has_natural_conversion_intent("how many cm in a foot"));
    assert!(has_natural_conversion_intent("a mile to km"));
    assert!(has_natural_conversion_intent("100c=f"));
    assert!(!has_natural_conversion_intent("just ordinary conversation"));
    assert!(!has_natural_conversion_intent("the quick brown fox"));
}
