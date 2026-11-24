// Supabase Edge Function: voice
// Secure gateway for Nite voice dictation.
// Authenticates users via Supabase Auth and delegates speech-to-text to ElevenLabs Scribe v3
// and post-processing formatting to Groq Llama 3.3 70B without client-side API keys.

import { createClient } from "@supabase/supabase-js";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
  "Access-Control-Allow-Methods": "POST, OPTIONS",
};

const ELEVENLABS_STT_URL = "https://api.elevenlabs.io/v1/speech-to-text";
const ELEVENLABS_MODEL_ID = "scribe_v3";
const GROQ_CHAT_URL = "https://api.groq.com/openai/v1/chat/completions";
const CLEANUP_MODEL = "openai/gpt-oss-120b";

export class VoiceGatewayError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
    this.name = "VoiceGatewayError";
  }
}

interface ScribeResponse {
  text: string;
  language_code?: string;
  language_probability?: number;
}

function sanitizeCleanedText(rawInput: string, cleaned: string): string {
  let text = cleaned.trim();

  // Strip <raw_transcript> ... </raw_transcript> or <raw_speech> ... </raw_speech> tags if echoed
  if (text.startsWith("<raw_transcript>")) {
    text = text.substring("<raw_transcript>".length).trim();
  }
  if (text.endsWith("</raw_transcript>")) {
    text = text.substring(0, text.length - "</raw_transcript>".length).trim();
  }
  if (text.startsWith("<raw_speech>")) {
    text = text.substring("<raw_speech>".length).trim();
  }
  if (text.endsWith("</raw_speech>")) {
    text = text.substring(0, text.length - "</raw_speech>".length).trim();
  }

  // Strip markdown code fence if present
  if (text.startsWith("```")) {
    const firstNewline = text.indexOf("\n");
    text = firstNewline !== -1 ? text.substring(firstNewline + 1) : text.substring(3);
    const lastFence = text.lastIndexOf("```");
    if (lastFence !== -1) {
      text = text.substring(0, lastFence);
    }
    text = text.trim();
  }

  const rawTrimmed = rawInput.trim();
  // Strip surrounding double or single quotes if raw input did not start with quotes
  if (
    ((text.startsWith('"') && text.endsWith('"')) ||
      (text.startsWith("'") && text.endsWith("'"))) &&
    text.length >= 2 &&
    !rawTrimmed.startsWith('"') &&
    !rawTrimmed.startsWith("'")
  ) {
    text = text.substring(1, text.length - 1).trim();
  } else if (text.startsWith("“") && text.endsWith("”") && !rawTrimmed.startsWith("“")) {
    text = text.substring(1, text.length - 1).trim();
  } else if (text.startsWith("‘") && text.endsWith("’") && !rawTrimmed.startsWith("‘")) {
    text = text.substring(1, text.length - 1).trim();
  }

  return text;
}

function parseLanguagePreferences(raw: string | null | undefined): {
  primaryLanguage: string | null;
  secondaryLanguages: string[];
} {
  if (!raw) {
    return { primaryLanguage: null, secondaryLanguages: [] };
  }
  const parts = raw
    .split(",")
    .map((s) => s.trim().toLowerCase())
    .filter((s) => s.length > 0);

  if (parts.length === 0 || parts.includes("auto")) {
    return { primaryLanguage: null, secondaryLanguages: [] };
  }

  const unique = Array.from(new Set(parts));
  const primary = unique[0];
  const secondaries = unique.slice(1, 4); // max 3 secondary language codes
  return { primaryLanguage: primary, secondaryLanguages: secondaries };
}

function sanitizeKeyterm(term: string): string | null {
  const trimmed = term.trim();
  if (!trimmed || trimmed.length > 50) return null;
  // Scribe forbids characters < > { } [ ] \
  const sanitized = trimmed.replace(/[<>{}\[\]\\]/g, "").trim();
  if (!sanitized) return null;
  const words = sanitized.split(/\s+/);
  if (words.length > 5) return null;
  return sanitized;
}

async function callElevenLabsScribe(
  apiKey: string,
  file: Blob,
  preferredLanguagesRaw?: string | null,
  customTerms?: string[] | null,
): Promise<ScribeResponse> {
  const formData = new FormData();
  formData.append("file", file, "dictation.wav");
  formData.append("model_id", ELEVENLABS_MODEL_ID);
  formData.append("diarize", "false");
  formData.append("tag_audio_events", "false");

  const { primaryLanguage, secondaryLanguages } = parseLanguagePreferences(preferredLanguagesRaw);
  if (primaryLanguage) {
    formData.append("language_code", primaryLanguage);
    for (const sec of secondaryLanguages) {
      formData.append("secondary_language_codes", sec);
    }
  }

  if (customTerms && Array.isArray(customTerms)) {
    const seen = new Set<string>();
    for (const term of customTerms) {
      const valid = sanitizeKeyterm(term);
      if (valid && !seen.has(valid.toLowerCase()) && seen.size < 1000) {
        seen.add(valid.toLowerCase());
        formData.append("keyterms", valid);
      }
    }
  }

  const res = await fetch(ELEVENLABS_STT_URL, {
    method: "POST",
    headers: {
      "xi-api-key": apiKey,
    },
    body: formData,
  });

  if (!res.ok) {
    const errorText = await res.text();
    throw new VoiceGatewayError(res.status, `ElevenLabs Scribe error (${res.status}): ${errorText}`);
  }

  return (await res.json()) as ScribeResponse;
}

function buildLlamaSystemPrompt(customTerms?: string[] | null): string {
  let prompt =
    "You are an expert voice dictation editor. Your task is to polish raw speech transcripts into clean, professional, readable text while preserving the speaker's meaning, vocabulary, and language.\n\n" +
    "# Core Editing Rules:\n\n" +
    "1. Eliminate Disfluencies, Stutters & False Starts:\n" +
    "- Remove verbal fillers (e.g. um, uh, ah, like, you know) and trailing hesitations.\n" +
    "- Clean up cut-off or restarted words (e.g., convert 'w- how-- if' to 'how if', 'con-- I am' to 'I am', 'web s- WebSocket' to 'WebSocket').\n" +
    "- Collapse accidental stuttered repetitions (e.g., convert 'We, we have' to 'We have', 'and, and then' to 'and then').\n" +
    "- Remove speech-recognition punctuation artifacts such as double commas (,,), stray dashes (--), and awkward mid-sentence ellipses (...).\n\n" +
    "2. Paragraphs & Structural Formatting:\n" +
    "- Organize continuous speech into logical paragraphs separated by blank lines (\\n\\n) at natural thought transitions, major topic shifts, or dictated paragraph breaks.\n" +
    "- Format sequential steps or itemized points as numbered lists (1., 2.) or clean bullet points (- Item) only when the speaker is clearly itemizing items or lists.\n\n" +
    "3. Punctuation & Capitalization:\n" +
    "- Apply natural, fluent punctuation (commas, periods, question marks) and sentence capitalization.\n" +
    "- Format numbers, currencies, dates, and technical abbreviations according to standard conventions.\n\n" +
    "4. Zero Translation & Script Preservation:\n" +
    "- NEVER translate. Keep the exact language, dialect, and script spoken by the user (including Indic, East Asian, Cyrillic, Latin, or mixed code-switching).\n" +
    "- Preserve technical terms, product names, and code identifiers in Latin script exactly as spoken.\n\n" +
    "5. Output Contract (Strict Negative Constraints):\n" +
    "- Output ONLY the final polished text.\n" +
    "- Do NOT include conversational preamble, commentary, explanations, or notes.\n" +
    "- Do NOT wrap output in quotation marks or markdown code fences (```).";

  if (customTerms && customTerms.length > 0) {
    const valid = customTerms
      .map((t) => t.trim())
      .filter((t) => t.length > 0)
      .slice(0, 50);
    if (valid.length > 0) {
      prompt += `\n\n# Vocabulary to preserve verbatim if present:\n${valid.join(", ")}`;
    }
  }

  return prompt;
}

async function runLlamaCleanup(
  apiKey: string,
  rawTranscript: string,
  customTerms?: string[] | null,
): Promise<string> {
  const trimmedRaw = rawTranscript.trim();
  if (!trimmedRaw) {
    return "";
  }

  const systemPrompt = buildLlamaSystemPrompt(customTerms);
  const estimatedTokens = Math.floor((trimmedRaw.split(/\s+/).length * 4) / 3) + 32;
  const dynamicMaxTokens = Math.min(Math.max(estimatedTokens * 2 + 128, 256), 4096);

  try {
    console.log(`[Voice Gateway] Calling Groq Chat cleanup with model="${CLEANUP_MODEL}"`);
    const res = await fetch(GROQ_CHAT_URL, {
      method: "POST",
      headers: {
        "Authorization": `Bearer ${apiKey}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        model: CLEANUP_MODEL,
        messages: [
          { role: "system", content: systemPrompt },
          { role: "user", content: `<raw_transcript>\n${trimmedRaw}\n</raw_transcript>` },
        ],
        temperature: 0,
        max_tokens: dynamicMaxTokens,
      }),
    });

    if (!res.ok) {
      const errText = await res.text();
      console.error(`[Voice Gateway] Groq Chat error for model="${CLEANUP_MODEL}" (HTTP ${res.status}): ${errText}`);
      return trimmedRaw;
    }

    const data = await res.json();
    const content = data.choices?.[0]?.message?.content;
    if (typeof content === "string") {
      const cleaned = sanitizeCleanedText(trimmedRaw, content);
      console.log(`[Voice Gateway] Groq Chat cleanup succeeded: "${cleaned}"`);
      return cleaned;
    }
    return trimmedRaw;
  } catch (err) {
    console.error(`[Voice Gateway] Groq Chat exception for model="${CLEANUP_MODEL}":`, err);
    return trimmedRaw;
  }
}

Deno.serve(async (req: Request) => {
  if (req.method === "OPTIONS") {
    return new Response("ok", { headers: corsHeaders });
  }

  if (req.method !== "POST") {
    return new Response(JSON.stringify({ error: "Method not allowed" }), {
      status: 405,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  }

  // 1. Authenticate caller via Supabase Auth
  const authHeader = req.headers.get("Authorization");
  if (!authHeader || !authHeader.startsWith("Bearer ")) {
    return new Response(JSON.stringify({ error: "Unauthorized: Missing Authorization header" }), {
      status: 401,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL");
  const supabaseAnonKey = Deno.env.get("SUPABASE_ANON_KEY");
  if (!supabaseUrl || !supabaseAnonKey) {
    return new Response(JSON.stringify({ error: "Server configuration error: Supabase environment missing" }), {
      status: 500,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  }

  const supabase = createClient(supabaseUrl, supabaseAnonKey, {
    global: { headers: { Authorization: authHeader } },
  });

  const token = authHeader.replace("Bearer ", "").trim();
  const { data: { user }, error: authError } = await supabase.auth.getUser(token);
  if (authError || !user) {
    return new Response(JSON.stringify({ error: "Unauthorized: Invalid or expired session token" }), {
      status: 401,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  }

  // 2. Validate server secrets
  const elevenlabsApiKey = Deno.env.get("ELEVENLABS_API_KEY");
  const groqApiKey = Deno.env.get("GROQ_API_KEY");

  const contentType = req.headers.get("content-type") || "";

  // 3. Process Request
  try {
    // Mode A: Raw text cleanup request (used for multi-chunk aggregation)
    if (contentType.includes("application/json")) {
      if (!groqApiKey) {
        return new Response(JSON.stringify({ error: "Server configuration error: GROQ_API_KEY secret not set" }), {
          status: 500,
          headers: { ...corsHeaders, "Content-Type": "application/json" },
        });
      }

      const body = await req.json();
      const rawText = body.raw_text || "";
      const customTerms: string[] | null = Array.isArray(body.custom_terms) ? body.custom_terms : null;

      console.log(`[Voice Gateway] JSON cleanup request received: raw_text length=${rawText.length}`);
      const cleaned = await runLlamaCleanup(groqApiKey, rawText, customTerms);
      return new Response(JSON.stringify({ text: cleaned }), {
        headers: { ...corsHeaders, "Content-Type": "application/json" },
      });
    }

    // Mode B: Audio transcription request (multipart/form-data)
    if (contentType.includes("multipart/form-data")) {
      if (!elevenlabsApiKey) {
        return new Response(JSON.stringify({ error: "Server configuration error: ELEVENLABS_API_KEY secret not set" }), {
          status: 500,
          headers: { ...corsHeaders, "Content-Type": "application/json" },
        });
      }

      const formData = await req.formData();
      const file = formData.get("file");
      if (!file || !(file instanceof Blob)) {
        return new Response(JSON.stringify({ error: "Bad Request: Missing audio 'file' in form-data" }), {
          status: 400,
          headers: { ...corsHeaders, "Content-Type": "application/json" },
        });
      }

      const cleanup = formData.get("cleanup") !== "false";
      let customTerms: string[] | null = null;
      const termsRaw = formData.get("custom_terms");
      if (typeof termsRaw === "string") {
        try {
          const parsed = JSON.parse(termsRaw);
          if (Array.isArray(parsed)) {
            customTerms = parsed;
          }
        } catch {
          customTerms = termsRaw.split(",").map((s) => s.trim());
        }
      }

      const preferredLanguagesRaw = formData.get("preferred_languages")?.toString() || null;

      console.log(`[Voice Gateway] ElevenLabs Scribe request: preferred_languages="${preferredLanguagesRaw}", customTerms count=${customTerms?.length || 0}`);

      const scribeResponse = await callElevenLabsScribe(
        elevenlabsApiKey,
        file,
        preferredLanguagesRaw,
        customTerms,
      );

      const rawText = (scribeResponse.text || "").trim();
      const detectedLanguage = scribeResponse.language_code || null;

      console.log(`[Voice Gateway] Scribe completed: language="${detectedLanguage}", raw_text length=${rawText.length}`);

      if (cleanup && groqApiKey && rawText.length > 0) {
        console.log(`[Voice Gateway] Initiating Llama 3.3 70B cleanup on raw_text`);
        const cleanedText = await runLlamaCleanup(groqApiKey, rawText, customTerms);
        const finalText = (cleanedText && cleanedText.trim().length > 0) ? cleanedText : rawText;
        return new Response(
          JSON.stringify({ text: finalText, raw_text: rawText, language: detectedLanguage }),
          { headers: { ...corsHeaders, "Content-Type": "application/json" } },
        );
      }

      return new Response(
        JSON.stringify({ text: rawText, raw_text: rawText, language: detectedLanguage }),
        { headers: { ...corsHeaders, "Content-Type": "application/json" } },
      );
    }

    return new Response(JSON.stringify({ error: "Unsupported Media Type: Expected multipart/form-data or application/json" }), {
      status: 415,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  } catch (err: unknown) {
    const status = err instanceof VoiceGatewayError ? err.status : 500;
    const message = err instanceof Error ? err.message : String(err);
    console.error("[Voice Gateway] Unhandled error:", err);
    return new Response(JSON.stringify({ error: `Voice Gateway Error: ${message}` }), {
      status,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  }
});
