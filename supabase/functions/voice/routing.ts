// Language routing and disambiguation utilities for Taurine Voice Gateway

export interface WhisperSegment {
  id?: number;
  seek?: number;
  start?: number;
  end?: number;
  text?: string;
  tokens?: number[];
  temperature?: number;
  avg_logprob?: number;
  compression_ratio?: number;
  no_speech_prob?: number;
}

export interface WhisperResponse {
  text: string;
  language?: string;
  segments?: WhisperSegment[];
}

export interface WhisperCandidateResult {
  candidate: string | null;
  response: WhisperResponse;
  avgLogprob: number;
}

/**
 * Parses the raw `preferred_languages` form value.
 * Splits by comma, trims, lowercases, filters empty.
 * If empty or contains "auto", returns [] (signaling unconstrained detection).
 * Capped at 3 candidate languages, deduplicated while preserving order.
 */
export function parseLanguageCandidates(raw: string | null | undefined): string[] {
  if (!raw) {
    return [];
  }

  const parts = raw
    .split(",")
    .map((s) => s.trim().toLowerCase())
    .filter((s) => s.length > 0);

  if (parts.length === 0 || parts.includes("auto")) {
    return [];
  }

  const unique = Array.from(new Set(parts));
  return unique.slice(0, 3);
}

/**
 * Computes average `avg_logprob` across all segments in a Whisper verbose JSON response.
 * Returns -Infinity if no valid segments or logprobs exist.
 */
export function computeAverageLogprob(segments?: WhisperSegment[] | null): number {
  if (!segments || segments.length === 0) {
    return -Infinity;
  }

  let sum = 0;
  let count = 0;
  for (const seg of segments) {
    if (typeof seg.avg_logprob === "number" && !isNaN(seg.avg_logprob)) {
      sum += seg.avg_logprob;
      count++;
    }
  }

  return count > 0 ? sum / count : -Infinity;
}

/**
 * Selects the winning candidate from a list of candidate results.
 * Favors non-empty transcripts, and among eligible candidates selects the one
 * with the highest avgLogprob (closer to 0).
 */
export function selectBestCandidate<T extends { response: { text?: string }; avgLogprob: number }>(
  results: T[],
): T {
  if (results.length === 0) {
    throw new Error("Cannot select best candidate from empty result set");
  }

  if (results.length === 1) {
    return results[0];
  }

  const nonEmpty = results.filter((r) => (r.response.text ?? "").trim().length > 0);
  const pool = nonEmpty.length > 0 ? nonEmpty : results;

  let best = pool[0];
  for (let i = 1; i < pool.length; i++) {
    if (pool[i].avgLogprob > best.avgLogprob) {
      best = pool[i];
    }
  }

  return best;
}

/**
 * Builds the Llama 3.1 cleanup system prompt.
 * Instructs rendering in authentic native script while preserving English technical terms in Latin script.
 */
export function buildLlamaSystemPrompt(
  customTerms?: string[] | null,
  detectedLanguage?: string | null,
): string {
  let systemPrompt =
    "You are an expert voice dictation cleanup engine. Transform raw transcribed speech into clean, well-formatted, professional text.\n" +
    "Rules:\n" +
    "- Correct punctuation, capitalization, and minor grammatical slips.\n" +
    "- Remove conversational filler words (e.g. um, uh, ah, like, you know) and accidental stutter repetitions.\n" +
    "- Format numbers, currencies, dates, email addresses, and technical terms appropriately.\n" +
    "- Structure continuous speech into natural paragraphs separated by blank lines (\\n\\n) at major topic transitions or thought shifts, or when the speaker dictates a new paragraph.\n" +
    "- Format sequences of numbered items or sequential instructions as numbered lists (e.g. 1., 2., 3.) when itemized.\n" +
    "- Format itemized features, options, or components as clean bulleted lists (- ) when itemized.\n" +
    "- Preserve regular flowing prose in paragraphs rather than forcing lists for standard explanations.\n" +
    "- Preserve the speaker's original intent, vocabulary, tone, and meaning exactly.\n" +
    "- Universal Native Script Rule: If speech belonging to any language typically written in a non-Latin script (including Indic languages like മലയാളം for Malayalam or हिन्दी for Hindi, East Asian languages like 日本語 for Japanese or Chinese, Cyrillic, Arabic, Hebrew, Greek, etc.) is transcribed in Latin or Romanized phonetic transliteration (e.g. Manglish, Hinglish, Romaji, Pinyin, or Romanized speech), render it in its authentic, standard native script and orthography. Never translate the meaning into English or another language; transcribe the speaker's exact spoken words in their authentic language and native script.\n" +
    "- Code-Switching & Technical Words: When a speaker naturally mixes languages (e.g. using English technical words, programming identifiers, software products, or brand names within non-English speech), preserve those genuine English terms in standard Latin text while keeping all surrounding language in its authentic native script.\n" +
    "- If the speech contains only conversational filler words, stuttering, or ambient noise with no substantive content, output an empty string.\n" +
    "- Output ONLY the final cleaned text with no surrounding quotes, conversational preamble, explanations, markdown code blocks, or notes.";

  if (detectedLanguage && detectedLanguage.toLowerCase() !== "english" && detectedLanguage.toLowerCase() !== "en") {
    systemPrompt += `\n- Explicit Target Language & Native Script: The audio was spoken in or identified as ${detectedLanguage}. You MUST render the speech in its authentic native script (e.g. മലയാളം for Malayalam, 日本語 for Japanese, हिन्दी for Hindi, etc.). If the raw transcript is in Latin/English characters or phonetic transliteration, convert and restore it completely into the authentic native script of ${detectedLanguage}. NEVER leave non-English speech phonetically transliterated in Latin characters.\n- Technical Term Preservation: Keep genuine English technical terms, coding keywords, software names, and brand identifiers in standard Latin script while rendering all surrounding speech in authentic native script.`;
  }

  if (customTerms && customTerms.length > 0) {
    const validTerms = customTerms
      .map((t) => t.trim())
      .filter((t) => t.length > 0)
      .slice(0, 50);
    if (validTerms.length > 0) {
      systemPrompt += `\n- Domain vocabulary to preserve and format verbatim if phonetically spoken: ${validTerms.join(", ")}.`;
    }
  }

  return systemPrompt;
}
