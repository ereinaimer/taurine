//! Multi-tier intent matching engine for diagnostic suggestions.

/// Finds the best matching candidate for a given user input across three tiers:
/// 1. Prefix match (e.g., "pyt" -> "python")
/// 2. Substring match (e.g., "theme" -> "audio_theme")
/// 3. Edit distance / Typo / Transposition (Damerau-Levenshtein, e.g., "shft" -> "shift", "atl" -> "alt")
pub fn find_best_match<'a>(input: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let input_clean = input.trim();
    if input_clean.is_empty() || candidates.is_empty() {
        return None;
    }

    // Exact match check (case-insensitive)
    for &cand in candidates {
        if cand.trim().eq_ignore_ascii_case(input_clean) {
            return Some(cand);
        }
    }

    let input_lower = input_clean.to_lowercase();

    // Tier 1: Prefix match
    let prefix_matches: Vec<&'a str> = candidates
        .iter()
        .copied()
        .filter(|cand| cand.trim().to_lowercase().starts_with(&input_lower))
        .collect();

    if !prefix_matches.is_empty() {
        return prefix_matches
            .into_iter()
            .min_by_key(|cand| (cand.trim().len().abs_diff(input_lower.len()), cand.len()));
    }

    // Tier 2: Substring match
    let substring_matches: Vec<&'a str> = candidates
        .iter()
        .copied()
        .filter(|cand| cand.trim().to_lowercase().contains(&input_lower))
        .collect();

    if !substring_matches.is_empty() {
        return substring_matches
            .into_iter()
            .min_by_key(|cand| (cand.trim().len().abs_diff(input_lower.len()), cand.len()));
    }

    // Tier 3: Typo / Transposition (Damerau-Levenshtein)
    let max_dist = match input_clean.len() {
        0..=3 => 1,
        4..=7 => 2,
        _ => 3,
    };

    let mut best: Option<(&'a str, usize)> = None;
    for &cand in candidates {
        let cand_clean = cand.trim().to_lowercase();
        let dist = strsim::damerau_levenshtein(&input_lower, &cand_clean);
        if dist <= max_dist {
            match best {
                None => best = Some((cand, dist)),
                Some((best_cand, best_dist)) => {
                    let cand_len_diff = cand.trim().len().abs_diff(input_lower.len());
                    let best_len_diff = best_cand.trim().len().abs_diff(input_lower.len());
                    if dist < best_dist || (dist == best_dist && cand_len_diff < best_len_diff) {
                        best = Some((cand, dist));
                    }
                }
            }
        }
    }

    best.map(|(cand, _)| cand)
}
