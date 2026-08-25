use chrono::{DateTime, NaiveTime, TimeZone, Timelike};
use chrono_tz::Tz;
use regex::Regex;
use std::sync::OnceLock;

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

fn has_time_pattern(input: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b\d{1,2}(:\d{2})?\s*(am|pm|a\.m\.|p\.m\.|noon|midnight)\b|\b\d{1,2}:\d{2}\b",
        )
        .expect("valid time regex")
    });
    re.is_match(input)
}

fn parse_time_str(s: &str) -> Option<NaiveTime> {
    let s = s.trim().to_lowercase();
    if s == "noon" || s == "12pm" || s == "12:00pm" || s == "12:00 pm" {
        return NaiveTime::from_hms_opt(12, 0, 0);
    }
    if s == "midnight" || s == "12am" || s == "12:00am" || s == "12:00 am" {
        return NaiveTime::from_hms_opt(0, 0, 0);
    }

    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)^(\d{1,2})(?::(\d{2}))?\s*(am|pm|a\.m\.|p\.m\.)?\s*$")
            .expect("valid time regex")
    });
    if let Some(caps) = re.captures(&s) {
        let hour: u32 = caps[1].parse().ok()?;
        let minute: u32 = caps
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        let is_pm = caps.get(3).is_some_and(|m| {
            let m = m.as_str().to_lowercase();
            m == "pm" || m == "p.m." || m == "p.m" || m == "pm."
        });

        let h24 = if is_pm && hour != 12 {
            hour + 12
        } else if !is_pm && hour == 12 {
            0
        } else {
            hour
        };

        return NaiveTime::from_hms_opt(h24, minute, 0);
    }

    static RE24: OnceLock<Regex> = OnceLock::new();
    let re24 =
        RE24.get_or_init(|| Regex::new(r"^(\d{1,2}):(\d{2})\s*$").expect("valid 24h time regex"));
    if let Some(caps) = re24.captures(&s) {
        let hour: u32 = caps[1].parse().ok()?;
        let minute: u32 = caps[2].parse().ok()?;
        return NaiveTime::from_hms_opt(hour, minute, 0);
    }

    None
}

fn format_time(t: &chrono::NaiveTime, time_format: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = time_format.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            continue;
        }
        let remaining = &time_format[i..];
        if remaining.starts_with("HH") {
            out.push_str(&format!("{:02}", t.hour()));
            i += 2;
        } else if remaining.starts_with("H") {
            out.push_str(&format!("{}", t.hour()));
            i += 1;
        } else if remaining.starts_with("hh") {
            let h = t.hour() % 12;
            let h12 = if h == 0 { 12 } else { h };
            out.push_str(&format!("{:02}", h12));
            i += 2;
        } else if remaining.starts_with("h") {
            let h = t.hour() % 12;
            let h12 = if h == 0 { 12 } else { h };
            out.push_str(&format!("{}", h12));
            i += 1;
        } else if remaining.starts_with("mm") {
            out.push_str(&format!("{:02}", t.minute()));
            i += 2;
        } else if remaining.starts_with("m") {
            out.push_str(&format!("{}", t.minute()));
            i += 1;
        } else if remaining.starts_with("ss") {
            out.push_str(&format!("{:02}", t.second()));
            i += 2;
        } else if remaining.starts_with("s") {
            out.push_str(&format!("{}", t.second()));
            i += 1;
        } else if remaining.starts_with("A") {
            out.push_str(if t.hour() >= 12 { "PM" } else { "AM" });
            i += 1;
        } else if remaining.starts_with("a") {
            out.push_str(if t.hour() >= 12 { "pm" } else { "am" });
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn chrono_dt_to_formatted(dt: &DateTime<Tz>, time_format: &str) -> String {
    format_time(&dt.naive_local().time(), time_format)
}

static TIMEZONE_MAP: phf::Map<&'static str, chrono_tz::Tz> = phf::phf_map! {
    "ae" => chrono_tz::Asia::Dubai,
    "aedt" => chrono_tz::Australia::Sydney,
    "aest" => chrono_tz::Australia::Sydney,
    "af" => chrono_tz::Asia::Kabul,
    "afghanistan" => chrono_tz::Asia::Kabul,
    "ag" => chrono_tz::America::Antigua,
    "ai" => chrono_tz::America::Anguilla,
    "akdt" => chrono_tz::America::Anchorage,
    "akst" => chrono_tz::America::Anchorage,
    "al" => chrono_tz::Europe::Tirane,
    "albania" => chrono_tz::Europe::Tirane,
    "algeria" => chrono_tz::Africa::Algiers,
    "am" => chrono_tz::Asia::Yerevan,
    "american samoa" => chrono_tz::Pacific::Pago_Pago,
    "amsterdam" => chrono_tz::Europe::Amsterdam,
    "an" => chrono_tz::America::Curacao,
    "anchorage" => chrono_tz::America::Anchorage,
    "angola" => chrono_tz::Africa::Luanda,
    "anguilla" => chrono_tz::America::Anguilla,
    "antigua and barbuda" => chrono_tz::America::Antigua,
    "ao" => chrono_tz::Africa::Luanda,
    "ar" => chrono_tz::America::Argentina::Buenos_Aires,
    "argentina" => chrono_tz::America::Argentina::Buenos_Aires,
    "armenia" => chrono_tz::Asia::Yerevan,
    "aruba" => chrono_tz::America::Aruba,
    "as" => chrono_tz::Pacific::Pago_Pago,
    "at" => chrono_tz::Europe::Vienna,
    "athens" => chrono_tz::Europe::Athens,
    "atlanta" => chrono_tz::America::New_York,
    "au" => chrono_tz::Australia::Sydney,
    "auckland" => chrono_tz::Pacific::Auckland,
    "australia" => chrono_tz::Australia::Sydney,
    "austria" => chrono_tz::Europe::Vienna,
    "aw" => chrono_tz::America::Aruba,
    "awst" => chrono_tz::Australia::Perth,
    "ax" => chrono_tz::Europe::Mariehamn,
    "az" => chrono_tz::Asia::Baku,
    "azerbaijan" => chrono_tz::Asia::Baku,
    "ba" => chrono_tz::Europe::Belgrade,
    "bahamas" => chrono_tz::America::Nassau,
    "bahrain" => chrono_tz::Asia::Bahrain,
    "bangalore" => chrono_tz::Asia::Kolkata,
    "bangkok" => chrono_tz::Asia::Bangkok,
    "bangladesh" => chrono_tz::Asia::Dhaka,
    "barbados" => chrono_tz::America::Barbados,
    "barcelona" => chrono_tz::Europe::Madrid,
    "bb" => chrono_tz::America::Barbados,
    "bd" => chrono_tz::Asia::Dhaka,
    "be" => chrono_tz::Europe::Brussels,
    "beijing" => chrono_tz::Asia::Shanghai,
    "belarus" => chrono_tz::Europe::Minsk,
    "belgium" => chrono_tz::Europe::Brussels,
    "belize" => chrono_tz::America::Belize,
    "benin" => chrono_tz::Africa::PortoNovo,
    "berlin" => chrono_tz::Europe::Berlin,
    "bermuda" => chrono_tz::Atlantic::Bermuda,
    "bf" => chrono_tz::Africa::Ouagadougou,
    "bg" => chrono_tz::Europe::Sofia,
    "bh" => chrono_tz::Asia::Bahrain,
    "bhutan" => chrono_tz::Asia::Thimphu,
    "bi" => chrono_tz::Africa::Bujumbura,
    "bj" => chrono_tz::Africa::PortoNovo,
    "bl" => chrono_tz::America::St_Barthelemy,
    "bm" => chrono_tz::Atlantic::Bermuda,
    "bn" => chrono_tz::Asia::Brunei,
    "bo" => chrono_tz::America::La_Paz,
    "bogota" => chrono_tz::America::Bogota,
    "bolivia" => chrono_tz::America::La_Paz,
    "bosnia and herzegovina" => chrono_tz::Europe::Belgrade,
    "boston" => chrono_tz::America::New_York,
    "botswana" => chrono_tz::Africa::Gaborone,
    "bq" => chrono_tz::America::Kralendijk,
    "br" => chrono_tz::America::Sao_Paulo,
    "brazil" => chrono_tz::America::Sao_Paulo,
    "brisbane" => chrono_tz::Australia::Brisbane,
    "british indian ocean territory" => chrono_tz::Indian::Chagos,
    "british virgin islands" => chrono_tz::America::Tortola,
    "brunei" => chrono_tz::Asia::Brunei,
    "brussels" => chrono_tz::Europe::Brussels,
    "bs" => chrono_tz::America::Nassau,
    "bst" => chrono_tz::Europe::London,
    "bt" => chrono_tz::Asia::Thimphu,
    "budapest" => chrono_tz::Europe::Budapest,
    "buenos aires" => chrono_tz::America::Argentina::Buenos_Aires,
    "bulgaria" => chrono_tz::Europe::Sofia,
    "burkina faso" => chrono_tz::Africa::Ouagadougou,
    "burundi" => chrono_tz::Africa::Bujumbura,
    "bw" => chrono_tz::Africa::Gaborone,
    "by" => chrono_tz::Europe::Minsk,
    "bz" => chrono_tz::America::Belize,
    "ca" => chrono_tz::America::Toronto,
    "cairo" => chrono_tz::Africa::Cairo,
    "cambodia" => chrono_tz::Asia::Phnom_Penh,
    "cameroon" => chrono_tz::Africa::Douala,
    "canada" => chrono_tz::America::Toronto,
    "cape town" => chrono_tz::Africa::Johannesburg,
    "cape verde" => chrono_tz::Atlantic::Cape_Verde,
    "caribbean netherlands" => chrono_tz::America::Kralendijk,
    "casablanca" => chrono_tz::Africa::Casablanca,
    "cayman islands" => chrono_tz::America::Cayman,
    "cc" => chrono_tz::Indian::Cocos,
    "cd" => chrono_tz::Africa::Kinshasa,
    "cdt" => chrono_tz::America::Chicago,
    "central african republic" => chrono_tz::Africa::Bangui,
    "cest" => chrono_tz::Europe::Paris,
    "cet" => chrono_tz::Europe::Paris,
    "cf" => chrono_tz::Africa::Bangui,
    "cg" => chrono_tz::Africa::Brazzaville,
    "ch" => chrono_tz::Europe::Zurich,
    "chad" => chrono_tz::Africa::Ndjamena,
    "chennai" => chrono_tz::Asia::Kolkata,
    "chicago" => chrono_tz::America::Chicago,
    "chile" => chrono_tz::America::Santiago,
    "china" => chrono_tz::Asia::Shanghai,
    "christmas island" => chrono_tz::Indian::Christmas,
    "ci" => chrono_tz::Africa::Abidjan,
    "ck" => chrono_tz::Pacific::Rarotonga,
    "cl" => chrono_tz::America::Santiago,
    "cm" => chrono_tz::Africa::Douala,
    "cn" => chrono_tz::Asia::Shanghai,
    "co" => chrono_tz::America::Bogota,
    "cocos (keeling) islands" => chrono_tz::Indian::Cocos,
    "colombia" => chrono_tz::America::Bogota,
    "comoros" => chrono_tz::Indian::Comoro,
    "congo" => chrono_tz::Africa::Brazzaville,
    "cook islands" => chrono_tz::Pacific::Rarotonga,
    "copenhagen" => chrono_tz::Europe::Copenhagen,
    "costa rica" => chrono_tz::America::Costa_Rica,
    "cr" => chrono_tz::America::Costa_Rica,
    "croatia" => chrono_tz::Europe::Zagreb,
    "cst" => chrono_tz::America::Chicago,
    "cu" => chrono_tz::America::Havana,
    "cuba" => chrono_tz::America::Havana,
    "curaçao" => chrono_tz::America::Curacao,
    "cv" => chrono_tz::Atlantic::Cape_Verde,
    "cw" => chrono_tz::America::Curacao,
    "cx" => chrono_tz::Indian::Christmas,
    "cy" => chrono_tz::Asia::Nicosia,
    "cyprus" => chrono_tz::Asia::Nicosia,
    "cz" => chrono_tz::Europe::Prague,
    "czech republic" => chrono_tz::Europe::Prague,
    "czechia" => chrono_tz::Europe::Prague,
    "côte d'ivoire" => chrono_tz::Africa::Abidjan,
    "dallas" => chrono_tz::America::Chicago,
    "dc" => chrono_tz::America::New_York,
    "de" => chrono_tz::Europe::Berlin,
    "delhi" => chrono_tz::Asia::Kolkata,
    "democratic republic of the congo (kinshasa)" => chrono_tz::Africa::Kinshasa,
    "denmark" => chrono_tz::Europe::Copenhagen,
    "denver" => chrono_tz::America::Denver,
    "dhaka" => chrono_tz::Asia::Dhaka,
    "dj" => chrono_tz::Africa::Djibouti,
    "djibouti" => chrono_tz::Africa::Djibouti,
    "dk" => chrono_tz::Europe::Copenhagen,
    "dm" => chrono_tz::America::Dominica,
    "do" => chrono_tz::America::Santo_Domingo,
    "doha" => chrono_tz::Asia::Qatar,
    "dominica" => chrono_tz::America::Dominica,
    "dominican republic" => chrono_tz::America::Santo_Domingo,
    "dr congo" => chrono_tz::Africa::Kinshasa,
    "dubai" => chrono_tz::Asia::Dubai,
    "dublin" => chrono_tz::Europe::Dublin,
    "dz" => chrono_tz::Africa::Algiers,
    "ec" => chrono_tz::America::Guayaquil,
    "ecuador" => chrono_tz::America::Guayaquil,
    "edt" => chrono_tz::America::New_York,
    "ee" => chrono_tz::Europe::Tallinn,
    "eest" => chrono_tz::Europe::Helsinki,
    "eet" => chrono_tz::Europe::Helsinki,
    "eg" => chrono_tz::Africa::Cairo,
    "egypt" => chrono_tz::Africa::Cairo,
    "eh" => chrono_tz::Africa::El_Aaiun,
    "el salvador" => chrono_tz::America::El_Salvador,
    "equatorial guinea" => chrono_tz::Africa::Malabo,
    "er" => chrono_tz::Africa::Asmara,
    "eritrea" => chrono_tz::Africa::Asmara,
    "es" => chrono_tz::Europe::Madrid,
    "est" => chrono_tz::America::New_York,
    "estonia" => chrono_tz::Europe::Tallinn,
    "eswatini" => chrono_tz::Africa::Mbabane,
    "et" => chrono_tz::Africa::Addis_Ababa,
    "ethiopia" => chrono_tz::Africa::Addis_Ababa,
    "falkland islands" => chrono_tz::Atlantic::Stanley,
    "faroe islands" => chrono_tz::Atlantic::Faeroe,
    "fi" => chrono_tz::Europe::Helsinki,
    "fiji" => chrono_tz::Pacific::Fiji,
    "finland" => chrono_tz::Europe::Helsinki,
    "fj" => chrono_tz::Pacific::Fiji,
    "fk" => chrono_tz::Atlantic::Stanley,
    "fm" => chrono_tz::Pacific::Kosrae,
    "fo" => chrono_tz::Atlantic::Faeroe,
    "fr" => chrono_tz::Europe::Paris,
    "france" => chrono_tz::Europe::Paris,
    "french guiana" => chrono_tz::America::Cayenne,
    "french polynesia" => chrono_tz::Pacific::Tahiti,
    "french southern and antarctic lands" => chrono_tz::Indian::Kerguelen,
    "french southern territories" => chrono_tz::Indian::Kerguelen,
    "ga" => chrono_tz::Africa::Libreville,
    "gabon" => chrono_tz::Africa::Libreville,
    "gambia" => chrono_tz::Africa::Banjul,
    "gb" => chrono_tz::Europe::London,
    "gd" => chrono_tz::America::Grenada,
    "ge" => chrono_tz::Asia::Tbilisi,
    "georgia" => chrono_tz::Asia::Tbilisi,
    "germany" => chrono_tz::Europe::Berlin,
    "gf" => chrono_tz::America::Cayenne,
    "gg" => chrono_tz::Europe::Guernsey,
    "gh" => chrono_tz::Africa::Accra,
    "ghana" => chrono_tz::Africa::Accra,
    "gi" => chrono_tz::Europe::Gibraltar,
    "gibraltar" => chrono_tz::Europe::Gibraltar,
    "gl" => chrono_tz::America::Godthab,
    "gm" => chrono_tz::Africa::Banjul,
    "gmt" => chrono_tz::Europe::London,
    "gn" => chrono_tz::Africa::Conakry,
    "gp" => chrono_tz::America::Guadeloupe,
    "gq" => chrono_tz::Africa::Malabo,
    "gr" => chrono_tz::Europe::Athens,
    "greece" => chrono_tz::Europe::Athens,
    "greenland" => chrono_tz::America::Godthab,
    "grenada" => chrono_tz::America::Grenada,
    "gst" => chrono_tz::Asia::Dubai,
    "gt" => chrono_tz::America::Guatemala,
    "gu" => chrono_tz::Pacific::Guam,
    "guadeloupe" => chrono_tz::America::Guadeloupe,
    "guam" => chrono_tz::Pacific::Guam,
    "guatemala" => chrono_tz::America::Guatemala,
    "guernsey" => chrono_tz::Europe::Guernsey,
    "guinea" => chrono_tz::Africa::Conakry,
    "guinea-bissau" => chrono_tz::Africa::Bissau,
    "guyana" => chrono_tz::America::Guyana,
    "gw" => chrono_tz::Africa::Bissau,
    "gy" => chrono_tz::America::Guyana,
    "haiti" => chrono_tz::America::Tegucigalpa,
    "halifax" => chrono_tz::America::Halifax,
    "hamburg" => chrono_tz::Europe::Berlin,
    "hanoi" => chrono_tz::Asia::Ho_Chi_Minh,
    "helsinki" => chrono_tz::Europe::Helsinki,
    "hk" => chrono_tz::Asia::Hong_Kong,
    "hkt" => chrono_tz::Asia::Hong_Kong,
    "hn" => chrono_tz::America::Tegucigalpa,
    "ho chi minh" => chrono_tz::Asia::Ho_Chi_Minh,
    "honduras" => chrono_tz::America::Tegucigalpa,
    "hong kong" => chrono_tz::Asia::Hong_Kong,
    "honolulu" => chrono_tz::Pacific::Honolulu,
    "houston" => chrono_tz::America::Chicago,
    "hr" => chrono_tz::Europe::Zagreb,
    "hst" => chrono_tz::Pacific::Honolulu,
    "ht" => chrono_tz::America::Tegucigalpa,
    "hu" => chrono_tz::Europe::Budapest,
    "hungary" => chrono_tz::Europe::Budapest,
    "iceland" => chrono_tz::Atlantic::Reykjavik,
    "id" => chrono_tz::Asia::Jakarta,
    "ie" => chrono_tz::Europe::Dublin,
    "il" => chrono_tz::Asia::Jerusalem,
    "im" => chrono_tz::Europe::Isle_of_Man,
    "in" => chrono_tz::Asia::Kolkata,
    "india" => chrono_tz::Asia::Kolkata,
    "indonesia" => chrono_tz::Asia::Jakarta,
    "io" => chrono_tz::Indian::Chagos,
    "iq" => chrono_tz::Asia::Baghdad,
    "ir" => chrono_tz::Asia::Tehran,
    "iran" => chrono_tz::Asia::Tehran,
    "iraq" => chrono_tz::Asia::Baghdad,
    "ireland" => chrono_tz::Europe::Dublin,
    "is" => chrono_tz::Atlantic::Reykjavik,
    "isle of man" => chrono_tz::Europe::Isle_of_Man,
    "israel" => chrono_tz::Asia::Jerusalem,
    "ist" => chrono_tz::Asia::Kolkata,
    "istanbul" => chrono_tz::Europe::Istanbul,
    "it" => chrono_tz::Europe::Rome,
    "italy" => chrono_tz::Europe::Rome,
    "ivory coast" => chrono_tz::Africa::Abidjan,
    "jakarta" => chrono_tz::Asia::Jakarta,
    "jamaica" => chrono_tz::America::Jamaica,
    "japan" => chrono_tz::Asia::Tokyo,
    "je" => chrono_tz::Europe::Jersey,
    "jersey" => chrono_tz::Europe::Jersey,
    "jerusalem" => chrono_tz::Asia::Jerusalem,
    "jm" => chrono_tz::America::Jamaica,
    "jo" => chrono_tz::Asia::Amman,
    "johannesburg" => chrono_tz::Africa::Johannesburg,
    "jordan" => chrono_tz::Asia::Amman,
    "jp" => chrono_tz::Asia::Tokyo,
    "jst" => chrono_tz::Asia::Tokyo,
    "karachi" => chrono_tz::Asia::Karachi,
    "kazakhstan" => chrono_tz::Asia::Almaty,
    "ke" => chrono_tz::Africa::Nairobi,
    "kenya" => chrono_tz::Africa::Nairobi,
    "kg" => chrono_tz::Asia::Bishkek,
    "kh" => chrono_tz::Asia::Phnom_Penh,
    "ki" => chrono_tz::Pacific::Tarawa,
    "kiev" => chrono_tz::Europe::Kyiv,
    "kiribati" => chrono_tz::Pacific::Tarawa,
    "km" => chrono_tz::Indian::Comoro,
    "kn" => chrono_tz::America::St_Kitts,
    "kolkata" => chrono_tz::Asia::Kolkata,
    "kp" => chrono_tz::Asia::Pyongyang,
    "kr" => chrono_tz::Asia::Seoul,
    "kuwait" => chrono_tz::Asia::Kuwait,
    "kw" => chrono_tz::Asia::Kuwait,
    "ky" => chrono_tz::America::Cayman,
    "kyiv" => chrono_tz::Europe::Kyiv,
    "kyrgyzstan" => chrono_tz::Asia::Bishkek,
    "kz" => chrono_tz::Asia::Almaty,
    "la" => chrono_tz::Asia::Vientiane,
    "lagos" => chrono_tz::Africa::Lagos,
    "laos" => chrono_tz::Asia::Vientiane,
    "las vegas" => chrono_tz::America::Los_Angeles,
    "latvia" => chrono_tz::Europe::Riga,
    "lb" => chrono_tz::Asia::Beirut,
    "lc" => chrono_tz::America::St_Lucia,
    "lebanon" => chrono_tz::Asia::Beirut,
    "lesotho" => chrono_tz::Africa::Maseru,
    "li" => chrono_tz::Europe::Vaduz,
    "liberia" => chrono_tz::Africa::Monrovia,
    "libya" => chrono_tz::Africa::Tripoli,
    "liechtenstein" => chrono_tz::Europe::Vaduz,
    "lima" => chrono_tz::America::Lima,
    "lisbon" => chrono_tz::Europe::Lisbon,
    "lithuania" => chrono_tz::Europe::Vilnius,
    "lk" => chrono_tz::Asia::Colombo,
    "london" => chrono_tz::Europe::London,
    "los angeles" => chrono_tz::America::Los_Angeles,
    "lr" => chrono_tz::Africa::Monrovia,
    "ls" => chrono_tz::Africa::Maseru,
    "lt" => chrono_tz::Europe::Vilnius,
    "lu" => chrono_tz::Europe::Luxembourg,
    "luxembourg" => chrono_tz::Europe::Luxembourg,
    "lv" => chrono_tz::Europe::Riga,
    "ly" => chrono_tz::Africa::Tripoli,
    "ma" => chrono_tz::Africa::Casablanca,
    "macao sar china" => chrono_tz::Asia::Macau,
    "macau" => chrono_tz::Asia::Macau,
    "madagascar" => chrono_tz::Indian::Antananarivo,
    "madrid" => chrono_tz::Europe::Madrid,
    "malawi" => chrono_tz::Africa::Blantyre,
    "malaysia" => chrono_tz::Asia::Kuala_Lumpur,
    "maldives" => chrono_tz::Indian::Maldives,
    "mali" => chrono_tz::Africa::Bamako,
    "malta" => chrono_tz::Europe::Malta,
    "manila" => chrono_tz::Asia::Manila,
    "marshall islands" => chrono_tz::Pacific::Majuro,
    "martinique" => chrono_tz::America::Martinique,
    "mauritius" => chrono_tz::Indian::Mauritius,
    "mayotte" => chrono_tz::Indian::Mayotte,
    "mc" => chrono_tz::Europe::Monaco,
    "md" => chrono_tz::Europe::Chisinau,
    "mdt" => chrono_tz::America::Denver,
    "me" => chrono_tz::Europe::Podgorica,
    "melbourne" => chrono_tz::Australia::Melbourne,
    "mexico" => chrono_tz::America::Mexico_City,
    "mexico city" => chrono_tz::America::Mexico_City,
    "mf" => chrono_tz::America::Marigot,
    "mg" => chrono_tz::Indian::Antananarivo,
    "mh" => chrono_tz::Pacific::Majuro,
    "miami" => chrono_tz::America::New_York,
    "micronesia" => chrono_tz::Pacific::Kosrae,
    "milan" => chrono_tz::Europe::Rome,
    "mk" => chrono_tz::Europe::Skopje,
    "ml" => chrono_tz::Africa::Bamako,
    "mm" => chrono_tz::Asia::Rangoon,
    "mn" => chrono_tz::Asia::Choibalsan,
    "mo" => chrono_tz::Asia::Macau,
    "moldova" => chrono_tz::Europe::Chisinau,
    "monaco" => chrono_tz::Europe::Monaco,
    "mongolia" => chrono_tz::Asia::Choibalsan,
    "montenegro" => chrono_tz::Europe::Podgorica,
    "montreal" => chrono_tz::America::Montreal,
    "montserrat" => chrono_tz::America::Montserrat,
    "morocco" => chrono_tz::Africa::Casablanca,
    "moscow" => chrono_tz::Europe::Moscow,
    "mozambique" => chrono_tz::Africa::Maputo,
    "mp" => chrono_tz::Pacific::Saipan,
    "mq" => chrono_tz::America::Martinique,
    "ms" => chrono_tz::America::Montserrat,
    "msk" => chrono_tz::Europe::Moscow,
    "mst" => chrono_tz::America::Denver,
    "mt" => chrono_tz::Europe::Malta,
    "mu" => chrono_tz::Indian::Mauritius,
    "mumbai" => chrono_tz::Asia::Kolkata,
    "munich" => chrono_tz::Europe::Berlin,
    "muscat" => chrono_tz::Asia::Muscat,
    "mv" => chrono_tz::Indian::Maldives,
    "mw" => chrono_tz::Africa::Blantyre,
    "mx" => chrono_tz::America::Mexico_City,
    "my" => chrono_tz::Asia::Kuala_Lumpur,
    "myanmar" => chrono_tz::Asia::Rangoon,
    "myanmar (burma)" => chrono_tz::Asia::Rangoon,
    "mz" => chrono_tz::Africa::Maputo,
    "na" => chrono_tz::Africa::Windhoek,
    "nairobi" => chrono_tz::Africa::Nairobi,
    "namibia" => chrono_tz::Africa::Windhoek,
    "nauru" => chrono_tz::Pacific::Nauru,
    "nc" => chrono_tz::Pacific::Noumea,
    "ne" => chrono_tz::Africa::Niamey,
    "nepal" => chrono_tz::Asia::Kathmandu,
    "netherlands" => chrono_tz::Europe::Amsterdam,
    "netherlands antilles" => chrono_tz::America::Curacao,
    "new caledonia" => chrono_tz::Pacific::Noumea,
    "new york" => chrono_tz::America::New_York,
    "new zealand" => chrono_tz::Pacific::Auckland,
    "newyork" => chrono_tz::America::New_York,
    "nf" => chrono_tz::Pacific::Norfolk,
    "ng" => chrono_tz::Africa::Lagos,
    "ni" => chrono_tz::America::Managua,
    "nicaragua" => chrono_tz::America::Managua,
    "niger" => chrono_tz::Africa::Niamey,
    "nigeria" => chrono_tz::Africa::Lagos,
    "niue" => chrono_tz::Pacific::Niue,
    "nl" => chrono_tz::Europe::Amsterdam,
    "no" => chrono_tz::Europe::Oslo,
    "norfolk island" => chrono_tz::Pacific::Norfolk,
    "north korea" => chrono_tz::Asia::Pyongyang,
    "north macedonia" => chrono_tz::Europe::Skopje,
    "northern mariana islands" => chrono_tz::Pacific::Saipan,
    "norway" => chrono_tz::Europe::Oslo,
    "np" => chrono_tz::Asia::Kathmandu,
    "nr" => chrono_tz::Pacific::Nauru,
    "nu" => chrono_tz::Pacific::Niue,
    "nyc" => chrono_tz::America::New_York,
    "nz" => chrono_tz::Pacific::Auckland,
    "nzdt" => chrono_tz::Pacific::Auckland,
    "nzst" => chrono_tz::Pacific::Auckland,
    "om" => chrono_tz::Asia::Muscat,
    "oman" => chrono_tz::Asia::Muscat,
    "oslo" => chrono_tz::Europe::Oslo,
    "pa" => chrono_tz::America::Panama,
    "pakistan" => chrono_tz::Asia::Karachi,
    "palau" => chrono_tz::Pacific::Palau,
    "palestine" => chrono_tz::Asia::Gaza,
    "palestinian territories" => chrono_tz::Asia::Gaza,
    "panama" => chrono_tz::America::Panama,
    "papua new guinea" => chrono_tz::Pacific::Port_Moresby,
    "paraguay" => chrono_tz::America::Asuncion,
    "paris" => chrono_tz::Europe::Paris,
    "pdt" => chrono_tz::America::Los_Angeles,
    "pe" => chrono_tz::America::Lima,
    "perth" => chrono_tz::Australia::Perth,
    "peru" => chrono_tz::America::Lima,
    "pf" => chrono_tz::Pacific::Tahiti,
    "pg" => chrono_tz::Pacific::Port_Moresby,
    "ph" => chrono_tz::Asia::Manila,
    "philippines" => chrono_tz::Asia::Manila,
    "phoenix" => chrono_tz::America::Phoenix,
    "pitcairn islands" => chrono_tz::Pacific::Pitcairn,
    "pk" => chrono_tz::Asia::Karachi,
    "pkt" => chrono_tz::Asia::Karachi,
    "pl" => chrono_tz::Europe::Warsaw,
    "pm" => chrono_tz::America::Miquelon,
    "pn" => chrono_tz::Pacific::Pitcairn,
    "poland" => chrono_tz::Europe::Warsaw,
    "portland" => chrono_tz::America::Los_Angeles,
    "portugal" => chrono_tz::Europe::Lisbon,
    "pr" => chrono_tz::America::Puerto_Rico,
    "prague" => chrono_tz::Europe::Prague,
    "ps" => chrono_tz::Asia::Gaza,
    "pst" => chrono_tz::America::Los_Angeles,
    "pt" => chrono_tz::Europe::Lisbon,
    "puerto rico" => chrono_tz::America::Puerto_Rico,
    "pw" => chrono_tz::Pacific::Palau,
    "py" => chrono_tz::America::Asuncion,
    "qa" => chrono_tz::Asia::Qatar,
    "qatar" => chrono_tz::Asia::Qatar,
    "re" => chrono_tz::Indian::Reunion,
    "republic of the congo (brazzaville)" => chrono_tz::Africa::Brazzaville,
    "rio" => chrono_tz::America::Sao_Paulo,
    "rio de janeiro" => chrono_tz::America::Sao_Paulo,
    "riyadh" => chrono_tz::Asia::Riyadh,
    "ro" => chrono_tz::Europe::Bucharest,
    "romania" => chrono_tz::Europe::Bucharest,
    "rome" => chrono_tz::Europe::Rome,
    "rs" => chrono_tz::Europe::Belgrade,
    "ru" => chrono_tz::Europe::Moscow,
    "russia" => chrono_tz::Europe::Moscow,
    "rw" => chrono_tz::Africa::Kigali,
    "rwanda" => chrono_tz::Africa::Kigali,
    "réunion" => chrono_tz::Indian::Reunion,
    "sa" => chrono_tz::Asia::Riyadh,
    "saigon" => chrono_tz::Asia::Ho_Chi_Minh,
    "saint barthélemy" => chrono_tz::America::St_Barthelemy,
    "saint helena, ascension and tristan da cunha" => chrono_tz::Atlantic::St_Helena,
    "saint kitts and nevis" => chrono_tz::America::St_Kitts,
    "saint lucia" => chrono_tz::America::St_Lucia,
    "saint martin" => chrono_tz::America::Marigot,
    "saint pierre and miquelon" => chrono_tz::America::Miquelon,
    "saint vincent and the grenadines" => chrono_tz::America::St_Vincent,
    "samoa" => chrono_tz::Pacific::Apia,
    "san francisco" => chrono_tz::America::Los_Angeles,
    "san marino" => chrono_tz::Europe::San_Marino,
    "santiago" => chrono_tz::America::Santiago,
    "sao paulo" => chrono_tz::America::Sao_Paulo,
    "sao tome and principe" => chrono_tz::Africa::Sao_Tome,
    "saudi arabia" => chrono_tz::Asia::Riyadh,
    "sb" => chrono_tz::Pacific::Guadalcanal,
    "sc" => chrono_tz::Indian::Mahe,
    "sd" => chrono_tz::Africa::Khartoum,
    "se" => chrono_tz::Europe::Stockholm,
    "seattle" => chrono_tz::America::Los_Angeles,
    "senegal" => chrono_tz::Africa::Dakar,
    "seoul" => chrono_tz::Asia::Seoul,
    "serbia" => chrono_tz::Europe::Belgrade,
    "seychelles" => chrono_tz::Indian::Mahe,
    "sf" => chrono_tz::America::Los_Angeles,
    "sg" => chrono_tz::Asia::Singapore,
    "sgt" => chrono_tz::Asia::Singapore,
    "sh" => chrono_tz::Atlantic::St_Helena,
    "shanghai" => chrono_tz::Asia::Shanghai,
    "si" => chrono_tz::Europe::Ljubljana,
    "sierra leone" => chrono_tz::Africa::Freetown,
    "singapore" => chrono_tz::Asia::Singapore,
    "sint maarten" => chrono_tz::America::Curacao,
    "sj" => chrono_tz::Arctic::Longyearbyen,
    "sk" => chrono_tz::Europe::Bratislava,
    "sl" => chrono_tz::Africa::Freetown,
    "slovakia" => chrono_tz::Europe::Bratislava,
    "slovenia" => chrono_tz::Europe::Ljubljana,
    "sm" => chrono_tz::Europe::San_Marino,
    "sn" => chrono_tz::Africa::Dakar,
    "so" => chrono_tz::Africa::Mogadishu,
    "solomon islands" => chrono_tz::Pacific::Guadalcanal,
    "somalia" => chrono_tz::Africa::Mogadishu,
    "south africa" => chrono_tz::Africa::Johannesburg,
    "south korea" => chrono_tz::Asia::Seoul,
    "south sudan" => chrono_tz::Africa::Juba,
    "spain" => chrono_tz::Europe::Madrid,
    "sr" => chrono_tz::America::Paramaribo,
    "sri lanka" => chrono_tz::Asia::Colombo,
    "ss" => chrono_tz::Africa::Juba,
    "st" => chrono_tz::Africa::Sao_Tome,
    "st. helena" => chrono_tz::Atlantic::St_Helena,
    "stockholm" => chrono_tz::Europe::Stockholm,
    "sudan" => chrono_tz::Africa::Khartoum,
    "suriname" => chrono_tz::America::Paramaribo,
    "sv" => chrono_tz::America::El_Salvador,
    "svalbard and jan mayen" => chrono_tz::Arctic::Longyearbyen,
    "sweden" => chrono_tz::Europe::Stockholm,
    "switzerland" => chrono_tz::Europe::Zurich,
    "sx" => chrono_tz::America::Curacao,
    "sy" => chrono_tz::Asia::Damascus,
    "sydney" => chrono_tz::Australia::Sydney,
    "syria" => chrono_tz::Asia::Damascus,
    "sz" => chrono_tz::Africa::Mbabane,
    "são tomé and príncipe" => chrono_tz::Africa::Sao_Tome,
    "taipei" => chrono_tz::Asia::Taipei,
    "taiwan" => chrono_tz::Asia::Taipei,
    "tajikistan" => chrono_tz::Asia::Dushanbe,
    "tanzania" => chrono_tz::Africa::Dar_es_Salaam,
    "tc" => chrono_tz::America::Grand_Turk,
    "td" => chrono_tz::Africa::Ndjamena,
    "tel aviv" => chrono_tz::Asia::Jerusalem,
    "tf" => chrono_tz::Indian::Kerguelen,
    "tg" => chrono_tz::Africa::Lome,
    "th" => chrono_tz::Asia::Bangkok,
    "thailand" => chrono_tz::Asia::Bangkok,
    "timor-leste" => chrono_tz::Asia::Dili,
    "tj" => chrono_tz::Asia::Dushanbe,
    "tk" => chrono_tz::Pacific::Fakaofo,
    "tl" => chrono_tz::Asia::Dili,
    "tm" => chrono_tz::Asia::Ashgabat,
    "tn" => chrono_tz::Africa::Tunis,
    "to" => chrono_tz::Pacific::Tongatapu,
    "togo" => chrono_tz::Africa::Lome,
    "tokelau" => chrono_tz::Pacific::Fakaofo,
    "tokyo" => chrono_tz::Asia::Tokyo,
    "tonga" => chrono_tz::Pacific::Tongatapu,
    "toronto" => chrono_tz::America::Toronto,
    "tr" => chrono_tz::Europe::Istanbul,
    "trinidad & tobago" => chrono_tz::America::Port_of_Spain,
    "trinidad and tobago" => chrono_tz::America::Port_of_Spain,
    "tt" => chrono_tz::America::Port_of_Spain,
    "tunisia" => chrono_tz::Africa::Tunis,
    "turkey" => chrono_tz::Europe::Istanbul,
    "turkmenistan" => chrono_tz::Asia::Ashgabat,
    "turks & caicos islands" => chrono_tz::America::Grand_Turk,
    "turks and caicos islands" => chrono_tz::America::Grand_Turk,
    "tuvalu" => chrono_tz::Pacific::Funafuti,
    "tv" => chrono_tz::Pacific::Funafuti,
    "tw" => chrono_tz::Asia::Taipei,
    "tz" => chrono_tz::Africa::Dar_es_Salaam,
    "türkiye" => chrono_tz::Europe::Istanbul,
    "u.s. outlying islands" => chrono_tz::Pacific::Midway,
    "u.s. virgin islands" => chrono_tz::America::St_Thomas,
    "ua" => chrono_tz::Europe::Kyiv,
    "ug" => chrono_tz::Africa::Kampala,
    "uganda" => chrono_tz::Africa::Kampala,
    "ukraine" => chrono_tz::Europe::Kyiv,
    "um" => chrono_tz::Pacific::Midway,
    "united arab emirates" => chrono_tz::Asia::Dubai,
    "united kingdom" => chrono_tz::Europe::London,
    "united states" => chrono_tz::America::New_York,
    "united states minor outlying islands" => chrono_tz::Pacific::Midway,
    "united states virgin islands" => chrono_tz::America::St_Thomas,
    "uruguay" => chrono_tz::America::Montevideo,
    "us" => chrono_tz::America::New_York,
    "utc" => chrono_tz::UTC,
    "uy" => chrono_tz::America::Montevideo,
    "uz" => chrono_tz::Asia::Tashkent,
    "uzbekistan" => chrono_tz::Asia::Tashkent,
    "va" => chrono_tz::Europe::Vatican,
    "vancouver" => chrono_tz::America::Vancouver,
    "vanuatu" => chrono_tz::Pacific::Efate,
    "vatican city" => chrono_tz::Europe::Vatican,
    "vc" => chrono_tz::America::St_Vincent,
    "ve" => chrono_tz::America::Caracas,
    "venezuela" => chrono_tz::America::Caracas,
    "vg" => chrono_tz::America::Tortola,
    "vi" => chrono_tz::America::St_Thomas,
    "vienna" => chrono_tz::Europe::Vienna,
    "vietnam" => chrono_tz::Asia::Bangkok,
    "vn" => chrono_tz::Asia::Bangkok,
    "vu" => chrono_tz::Pacific::Efate,
    "wallis and futuna" => chrono_tz::Pacific::Wallis,
    "warsaw" => chrono_tz::Europe::Warsaw,
    "washington" => chrono_tz::America::New_York,
    "wellington" => chrono_tz::Pacific::Auckland,
    "west" => chrono_tz::Europe::Lisbon,
    "western sahara" => chrono_tz::Africa::El_Aaiun,
    "wet" => chrono_tz::Europe::Lisbon,
    "wf" => chrono_tz::Pacific::Wallis,
    "ws" => chrono_tz::Pacific::Apia,
    "ye" => chrono_tz::Asia::Aden,
    "yemen" => chrono_tz::Asia::Aden,
    "yt" => chrono_tz::Indian::Mayotte,
    "za" => chrono_tz::Africa::Johannesburg,
    "zambia" => chrono_tz::Africa::Lusaka,
    "zimbabwe" => chrono_tz::Africa::Harare,
    "zm" => chrono_tz::Africa::Lusaka,
    "zurich" => chrono_tz::Europe::Zurich,
    "zw" => chrono_tz::Africa::Harare,
    "åland islands" => chrono_tz::Europe::Mariehamn,
};

fn resolve_to_tz(name: &str) -> Option<Tz> {
    let trimmed = name.trim().to_lowercase();
    if let Some(&tz) = TIMEZONE_MAP.get(trimmed.as_str()) {
        Some(tz)
    } else {
        trimmed.parse::<Tz>().ok()
    }
}

pub fn resolve_timezone(name: &str) -> Option<Tz> {
    resolve_to_tz(name)
}

pub fn parse_timezone_expression(input: &str, time_format: &str, dialect: &str) -> Option<String> {
    let trimmed = strip_nl_prefix(input).trim();
    if trimmed.is_empty() {
        return None;
    }

    if has_time_pattern(trimmed) {
        parse_conversion(trimmed, time_format)
    } else if let Some(result) = parse_timezone_relative(trimmed, time_format, dialect) {
        Some(result)
    } else {
        parse_current_time(trimmed, time_format)
    }
}

fn parse_current_time(input: &str, time_format: &str) -> Option<String> {
    let lower = input.to_lowercase();
    let cleaned = lower.trim_end_matches('?').trim();

    let city = if let Some(city) = cleaned.strip_prefix("time in ") {
        city.trim()
    } else if let Some(city) = cleaned.strip_prefix("now in ") {
        city.trim()
    } else if let Some(city) = cleaned.strip_prefix("what time is it in ") {
        city.trim_end_matches(" right now").trim()
    } else if let Some(city) = cleaned.strip_prefix("what's the time in ") {
        city.trim()
    } else if let Some(city) = cleaned.strip_prefix("the time in ") {
        city.trim()
    } else if let Some(city) = cleaned.strip_suffix(" time") {
        city.trim()
    } else {
        cleaned.strip_suffix(" now")?.trim()
    };

    if city.is_empty() {
        return None;
    }

    let tz = resolve_to_tz(city)?;
    let now: DateTime<Tz> = chrono::Utc::now().with_timezone(&tz);
    Some(chrono_dt_to_formatted(&now, time_format))
}

fn parse_conversion(input: &str, time_format: &str) -> Option<String> {
    let lower = input.to_lowercase();

    // Helper to strip " time" suffix from timezone identifiers
    fn strip_tz_suffix(s: &str) -> &str {
        s.strip_suffix(" time").unwrap_or(s).trim()
    }

    // Pattern 1: Conversational long form - "when it is 9am in london what time is it in new york"
    static RE_CONVERSATIONAL: OnceLock<Regex> = OnceLock::new();
    let re_conversational = RE_CONVERSATIONAL.get_or_init(|| {
        Regex::new(
            r"^when\s+it\s+is\s+(\d{1,2}(?::\d{2})?\s*(?:am|pm|a\.m\.|p\.m\.|noon|midnight)?)\s+in\s+(.+?)\s+what\s+time\s+is\s+it\s+in\s+(.+?)\??$",
        )
        .expect("valid conversational time conversion regex")
    });
    if let Some(caps) = re_conversational.captures(&lower) {
        let time_str = caps.get(1)?.as_str().trim();
        let from_tz_str = strip_tz_suffix(caps.get(2)?.as_str().trim());
        let to_tz_str = strip_tz_suffix(caps.get(3)?.as_str().trim());

        if !time_str.is_empty() && !from_tz_str.is_empty() && !to_tz_str.is_empty() {
            let time = parse_time_str(time_str)?;
            let from_tz = resolve_to_tz(from_tz_str)?;
            let to_tz = resolve_to_tz(to_tz_str)?;

            let today_utc = chrono::Utc::now().date_naive();
            let from_dt = from_tz
                .from_local_datetime(&today_utc.and_time(time))
                .earliest()?;

            let to_dt = from_dt.with_timezone(&to_tz);
            let formatted = chrono_dt_to_formatted(&to_dt, time_format);

            let from_date = from_dt.naive_local().date();
            let to_date = to_dt.naive_local().date();
            let day_diff = (to_date - from_date).num_days();

            let result = if day_diff == 0 {
                formatted
            } else if day_diff == 1 {
                format!("{formatted} (+1)")
            } else if day_diff == -1 {
                format!("{formatted} (-1)")
            } else if day_diff > 0 {
                format!("{formatted} (+{day_diff})")
            } else {
                format!("{formatted} ({day_diff})")
            };

            return Some(result);
        }
    }

    // Pattern 2: Standard conversion - "10am pst to ist" or "3pm est in tokyo"
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"^(\d{1,2}(?::\d{2})?\s*(?:am|pm|a\.m\.|p\.m\.|noon|midnight)?)\s+(.+?)\s+(?:to|in)\s+(.+?)$",
        )
        .expect("valid time range regex")
    });
    let caps = re.captures(&lower)?;
    let time_str = caps.get(1)?.as_str().trim();
    let from_tz_str = strip_tz_suffix(caps.get(2)?.as_str().trim());
    let to_tz_str = strip_tz_suffix(caps.get(3)?.as_str().trim());

    if time_str.is_empty() || from_tz_str.is_empty() || to_tz_str.is_empty() {
        return None;
    }

    let time = parse_time_str(time_str)?;
    let from_tz = resolve_to_tz(from_tz_str)?;
    let to_tz = resolve_to_tz(to_tz_str)?;

    let today_utc = chrono::Utc::now().date_naive();
    let from_dt = from_tz
        .from_local_datetime(&today_utc.and_time(time))
        .earliest()?;

    let to_dt = from_dt.with_timezone(&to_tz);
    let formatted = chrono_dt_to_formatted(&to_dt, time_format);

    let from_date = from_dt.naive_local().date();
    let to_date = to_dt.naive_local().date();
    let day_diff = (to_date - from_date).num_days();

    let result = if day_diff == 0 {
        formatted
    } else if day_diff == 1 {
        format!("{formatted} (+1)")
    } else if day_diff == -1 {
        format!("{formatted} (-1)")
    } else if day_diff > 0 {
        format!("{formatted} (+{day_diff})")
    } else {
        format!("{formatted} ({day_diff})")
    };

    Some(result)
}

fn parse_timezone_relative(input: &str, time_format: &str, dialect: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_lowercase();
    if let Some(idx) = lower.rfind(" in ") {
        let relative_expr = trimmed[..idx].trim();
        let city = trimmed[idx + 4..].trim();
        if !relative_expr.is_empty()
            && !city.is_empty()
            && let Some(tz) = resolve_to_tz(city)
        {
            return apply_relative_with_tz(relative_expr, tz, time_format, dialect);
        }
    }

    if let Some(space_idx) = trimmed.find(' ') {
        let first = &trimmed[..space_idx];
        let rest = trimmed[space_idx + 1..].trim();
        if !rest.is_empty()
            && let Some(tz) = resolve_to_tz(first)
        {
            return apply_relative_with_tz(rest, tz, time_format, dialect);
        }
    }

    None
}

fn apply_relative_with_tz(
    relative_expr: &str,
    tz: Tz,
    time_format: &str,
    dialect: &str,
) -> Option<String> {
    use interim::parse_date_string;

    let cleaned = crate::engine::dates::preprocess_date_phrase(relative_expr);
    let now = chrono::Local::now().fixed_offset();
    let primary_dialect = match dialect {
        "us" => interim::Dialect::Us,
        _ => interim::Dialect::Uk,
    };
    let parsed = parse_date_string(&cleaned, now, primary_dialect).ok()?;

    let target_dt = parsed.with_timezone(&tz);
    let formatted = chrono_dt_to_formatted(&target_dt, time_format);

    let source_date = parsed.naive_local().date();
    let target_date = target_dt.naive_local().date();
    let day_diff = (target_date - source_date).num_days();

    let result = if day_diff == 0 {
        formatted
    } else if day_diff == 1 {
        format!("{formatted} (+1)")
    } else if day_diff == -1 {
        format!("{formatted} (-1)")
    } else if day_diff > 0 {
        format!("{formatted} (+{day_diff})")
    } else {
        format!("{formatted} ({day_diff})")
    };

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::{America, Asia, Europe, UTC};

    fn fmt_opt(r: Option<String>) -> String {
        r.unwrap_or_else(|| "NONE".to_string())
    }

    // --- City/abbrev resolution ---

    #[test]
    fn test_resolve_major_cities() {
        assert_eq!(resolve_timezone("tokyo"), Some(Asia::Tokyo));
        assert_eq!(resolve_timezone("dubai"), Some(Asia::Dubai));
        assert_eq!(resolve_timezone("london"), Some(Europe::London));
        assert_eq!(resolve_timezone("paris"), Some(Europe::Paris));
        assert_eq!(resolve_timezone("new york"), Some(America::New_York));
    }

    #[test]
    fn test_resolve_abbreviations() {
        assert_eq!(resolve_timezone("pst"), Some(America::Los_Angeles));
        assert_eq!(resolve_timezone("est"), Some(America::New_York));
        assert_eq!(resolve_timezone("ist"), Some(Asia::Kolkata));
        assert_eq!(resolve_timezone("utc"), Some(UTC));
        assert_eq!(resolve_timezone("jst"), Some(Asia::Tokyo));
    }

    #[test]
    fn test_resolve_unknown_city() {
        assert_eq!(resolve_timezone("asdfgh"), None);
        assert_eq!(resolve_timezone(""), None);
    }

    // --- Expression classification ---

    #[test]
    fn test_detect_current_time_expr() {
        let out = parse_timezone_expression("time in tokyo", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'time in tokyo' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("now in dubai", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'now in dubai' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("tokyo time", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'tokyo time' should be recognized: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_detect_conversion_expr() {
        let out = parse_timezone_expression("10am pst to ist", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'10am pst to ist' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("3pm est in tokyo", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'3pm est in tokyo' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("14:00 UTC in london", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'14:00 UTC in london' should be recognized: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_invalid_expr_returns_none() {
        assert_eq!(
            parse_timezone_expression("hello world", "h:mm A", "uk"),
            None
        );
        assert_eq!(
            parse_timezone_expression("what is the weather", "h:mm A", "uk"),
            None
        );
        assert_eq!(parse_timezone_expression("", "h:mm A", "uk"), None);
    }

    // --- Conversion output format (deterministic, fixed timestamps) ---

    #[test]
    fn test_current_time_output_formatted() {
        let out = parse_timezone_expression("now in tokyo", "h:mm A", "uk");
        assert!(out.is_some(), "current time parsed: {}", fmt_opt(out));
        let s = out.unwrap();
        assert!(
            s.contains("AM") || s.contains("PM"),
            "result contains AM/PM: {s}"
        );
    }

    #[test]
    fn test_conversion_between_tzs() {
        let out = parse_timezone_expression("10am pst to ist", "h:mm A", "uk");
        assert!(out.is_some(), "conversion parsed: {}", fmt_opt(out));
        let s = out.unwrap();
        assert!(
            s.contains("AM") || s.contains("PM"),
            "result contains time: {s}"
        );
    }

    #[test]
    fn test_conversion_with_next_day_indicator() {
        let out = parse_timezone_expression("3pm est in tokyo", "h:mm A", "uk");
        assert!(out.is_some(), "conversion parsed: {}", fmt_opt(out));
        let s = out.unwrap();
        assert!(
            s.contains("(+1)") || s.contains("AM") || s.contains("PM"),
            "result contains day indicator or time: {s}"
        );
    }

    #[test]
    fn test_24h_input() {
        let out = parse_timezone_expression("14:00 UTC in london", "h:mm A", "uk");
        assert!(out.is_some(), "24h input parsed: {}", fmt_opt(out));
    }

    // --- Deterministic format matching ---

    #[test]
    fn test_format_12h() {
        let t = NaiveTime::from_hms_opt(14, 30, 0).unwrap();
        assert_eq!(format_time(&t, "h:mm A"), "2:30 PM");
        assert_eq!(format_time(&t, "hh:mm A"), "02:30 PM");
    }

    #[test]
    fn test_format_24h() {
        let t = NaiveTime::from_hms_opt(14, 30, 0).unwrap();
        assert_eq!(format_time(&t, "HH:mm"), "14:30");
        assert_eq!(format_time(&t, "H:mm"), "14:30");
    }

    #[test]
    fn test_format_am() {
        let t = NaiveTime::from_hms_opt(9, 5, 0).unwrap();
        assert_eq!(format_time(&t, "h:mm A"), "9:05 AM");
        assert_eq!(format_time(&t, "h:mm a"), "9:05 am");
    }

    #[test]
    fn test_format_literal() {
        let t = NaiveTime::from_hms_opt(14, 30, 0).unwrap();
        assert_eq!(format_time(&t, "'Time:' h:mm A"), "Time: 2:30 PM");
    }

    #[test]
    fn test_format_midnight_noon() {
        let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        assert_eq!(format_time(&midnight, "h:mm A"), "12:00 AM");
        let noon = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        assert_eq!(format_time(&noon, "h:mm A"), "12:00 PM");
    }

    // --- Timezone relative expressions ---

    #[test]
    fn test_relative_expr_in_city() {
        let out = parse_timezone_expression("3 hours from now in tokyo", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'3 hours from now in tokyo' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_city_first() {
        let out = parse_timezone_expression("tokyo 3 hours from now", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'tokyo 3 hours from now' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_abbreviation_first() {
        let out = parse_timezone_expression("pst 3 hours from now", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'pst 3 hours from now' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_in_abbreviation() {
        let out = parse_timezone_expression("3 hours from now in pst", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'3 hours from now in pst' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_unknown_city_returns_none() {
        assert_eq!(
            parse_timezone_expression("asdfgh 3 hours from now", "h:mm A", "uk"),
            None,
            "unknown city should not expand"
        );
        assert_eq!(
            parse_timezone_expression("3 hours from now in asdfgh", "h:mm A", "uk"),
            None,
            "unknown city in suffix should not expand"
        );
    }

    #[test]
    fn test_relative_minutes_precision() {
        let out = parse_timezone_expression("30 minutes from now in berlin", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'30 minutes from now in berlin' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_minutes_precision_city_first() {
        let out = parse_timezone_expression("london 30 minutes from now", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'london 30 minutes from now' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_with_day_offset() {
        let out = parse_timezone_expression("11pm est in tokyo", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'11pm est in tokyo' should expand: got {}",
            fmt_opt(out)
        );
        let s = out.unwrap();
        assert!(
            s.contains("(+1)") || s.contains("AM") || s.contains("PM"),
            "result contains day indicator or time: {s}"
        );
    }

    #[test]
    fn test_relative_current_time_still_works() {
        let out = parse_timezone_expression("time in tokyo", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'time in tokyo' should still expand via current_time: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("now in dubai", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'now in dubai' should still expand via current_time: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_resolve_countries() {
        assert_eq!(resolve_timezone("fr"), Some(Europe::Paris));
        assert_eq!(resolve_timezone("france"), Some(Europe::Paris));
        assert_eq!(resolve_timezone("jp"), Some(Asia::Tokyo));
        assert_eq!(resolve_timezone("japan"), Some(Asia::Tokyo));
        assert_eq!(resolve_timezone("united states"), Some(America::New_York));
        assert_eq!(resolve_timezone("us"), Some(America::New_York));
    }

    // --- NL Prefix Stripping Tests ---

    #[test]
    fn test_nl_prefix_stripping_conversion() {
        let out = parse_timezone_expression("convert 10am pst to tokyo time", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'convert 10am pst to tokyo time' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("what is 10am pst to tokyo time", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'what is 10am pst to tokyo time' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("what's 10am pst to tokyo time", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'what's 10am pst to tokyo time' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("how much is 10am pst to tokyo time", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'how much is 10am pst to tokyo time' should be recognized: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_conversational_current_time_queries() {
        let out = parse_timezone_expression("what time is it in london", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'what time is it in london' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("what time is it in london right now", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'what time is it in london right now' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("what time is it in london right now?", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'what time is it in london right now?' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("what's the time in london", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'what's the time in london' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("what's the time in london?", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'what's the time in london?' should be recognized: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_conversational_time_conversions() {
        let out = parse_timezone_expression(
            "when it is 9am in london what time is it in new york",
            "h:mm A",
            "uk",
        );
        assert!(
            out.is_some(),
            "'when it is 9am in london what time is it in new york' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression(
            "when it is 9am in london what time is it in new york?",
            "h:mm A",
            "uk",
        );
        assert!(
            out.is_some(),
            "'when it is 9am in london what time is it in new york?' should be recognized: got {}",
            fmt_opt(out)
        );
    }
}
