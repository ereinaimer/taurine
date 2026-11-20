// Supabase Edge Function: voice
// Secure gateway for Taurine voice dictation.
// Authenticates users via Supabase Auth and proxies to Groq Whisper + Llama 3.1 8B without client-side API keys.

import { createClient } from "@supabase/supabase-js";
import {
  buildLlamaSystemPrompt,
  computeAverageLogprob,
  parseLanguageCandidates,
  resolveCandidateWhisperLanguage,
  selectBestCandidate,
} from "./routing.ts";
import type { WhisperCandidateResult, WhisperResponse } from "./routing.ts";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
  "Access-Control-Allow-Methods": "POST, OPTIONS",
};

const GROQ_WHISPER_URL = "https://api.groq.com/openai/v1/audio/transcriptions";
const GROQ_CHAT_URL = "https://api.groq.com/openai/v1/chat/completions";
const WHISPER_MODEL = "whisper-large-v3";
const CLEANUP_MODEL = "llama-3.1-8b-instant";

export class GroqApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
    this.name = "GroqApiError";
  }
}

function sanitizeCleanedText(rawInput: string, cleaned: string): string {
  let text = cleaned.trim();

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
  }

  return text;
}

async function callGroqWhisper(
  apiKey: string,
  file: Blob,
  prompt?: string | null,
  language?: string | null,
): Promise<WhisperResponse> {
  const groqFormData = new FormData();
  groqFormData.append("file", file, "dictation.wav");
  groqFormData.append("model", WHISPER_MODEL);
  groqFormData.append("response_format", "verbose_json");
  groqFormData.append("temperature", "0");
  if (language && language.trim().length > 0) {
    groqFormData.append("language", language.trim());
  }
  if (prompt && prompt.trim().length > 0) {
    groqFormData.append("prompt", prompt.trim());
  }

  const whisperRes = await fetch(GROQ_WHISPER_URL, {
    method: "POST",
    headers: {
      "Authorization": `Bearer ${apiKey}`,
    },
    body: groqFormData,
  });

  if (!whisperRes.ok) {
    const errorText = await whisperRes.text();
    throw new GroqApiError(whisperRes.status, `Groq Whisper error (${whisperRes.status}): ${errorText}`);
  }

  return (await whisperRes.json()) as WhisperResponse;
}

async function runLlamaCleanup(
  apiKey: string,
  rawTranscript: string,
  customTerms?: string[] | null,
  detectedLanguage?: string | null,
): Promise<string> {
  const trimmedRaw = rawTranscript.trim();
  if (!trimmedRaw) {
    return "";
  }

  const systemPrompt = buildLlamaSystemPrompt(customTerms, detectedLanguage);
  const estimatedTokens = Math.floor((trimmedRaw.split(/\s+/).length * 4) / 3) + 32;
  const dynamicMaxTokens = Math.min(Math.max(estimatedTokens * 2 + 128, 256), 4096);

  try {
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
          { role: "user", content: trimmedRaw },
        ],
        temperature: 0,
        max_tokens: dynamicMaxTokens,
      }),
    });

    if (!res.ok) {
      console.warn(`Llama cleanup returned status ${res.status}; falling back to raw transcript`);
      return trimmedRaw;
    }

    const data = await res.json();
    const content = data.choices?.[0]?.message?.content;
    if (typeof content === "string") {
      return sanitizeCleanedText(trimmedRaw, content);
    }
  } catch (err) {
    console.warn("Llama cleanup fetch error:", err);
  }

  return trimmedRaw;
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

  // 2. Validate server secret
  const groqApiKey = Deno.env.get("GROQ_API_KEY");
  if (!groqApiKey) {
    return new Response(JSON.stringify({ error: "Server configuration error: GROQ_API_KEY secret not set" }), {
      status: 500,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  }

  const contentType = req.headers.get("content-type") || "";

  // 3. Process Request
  try {
    // Mode A: Raw text cleanup request (used for multi-chunk aggregation)
    if (contentType.includes("application/json")) {
      const body = await req.json();
      const rawText = body.raw_text || "";
      const customTerms: string[] | null = Array.isArray(body.custom_terms) ? body.custom_terms : null;
      const detectedLanguage: string | null = body.language || null;

      const cleaned = await runLlamaCleanup(groqApiKey, rawText, customTerms, detectedLanguage);
      return new Response(JSON.stringify({ text: cleaned }), {
        headers: { ...corsHeaders, "Content-Type": "application/json" },
      });
    }

    // Mode B: Audio transcription request (multipart/form-data)
    if (contentType.includes("multipart/form-data")) {
      const formData = await req.formData();
      const file = formData.get("file");
      if (!file || !(file instanceof Blob)) {
        return new Response(JSON.stringify({ error: "Bad Request: Missing audio 'file' in form-data" }), {
          status: 400,
          headers: { ...corsHeaders, "Content-Type": "application/json" },
        });
      }

      const prompt = formData.get("prompt")?.toString() || null;
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
      const candidates = parseLanguageCandidates(preferredLanguagesRaw);

      let winningResponse: WhisperResponse;
      let effectiveLanguage: string | null = null;

      if (candidates.length === 0) {
        // Unconstrained detection (single Whisper call without language parameter)
        winningResponse = await callGroqWhisper(groqApiKey, file, prompt, null);
        effectiveLanguage = winningResponse.language || null;
      } else if (candidates.length === 1) {
        // Exactly 1 candidate: single Whisper call with language: candidate
        const candidate = candidates[0];
        winningResponse = await callGroqWhisper(groqApiKey, file, prompt, candidate);
        effectiveLanguage = winningResponse.language || candidate;
      } else {
        // Multiple candidates: run concurrent Whisper calls via Promise.allSettled across candidate languages (capped at 3).
        // For English in a multilingual list, resolveCandidateWhisperLanguage returns null (unconstrained auto-detection).
        // This prevents Whisper from forcing English translation when non-English audio (e.g. Malayalam) is spoken.
        const candidatePromises = candidates.map(async (candidate) => {
          const whisperLang = resolveCandidateWhisperLanguage(candidate);
          const response = await callGroqWhisper(groqApiKey, file, prompt, whisperLang);
          const avgLogprob = computeAverageLogprob(response.segments);
          return { candidate, response, avgLogprob };
        });

        const settled = await Promise.allSettled(candidatePromises);
        const successfulResults: WhisperCandidateResult[] = [];
        let firstRejection: PromiseRejectedResult | null = null;

        for (const res of settled) {
          if (res.status === "fulfilled") {
            successfulResults.push(res.value);
          } else if (!firstRejection) {
            firstRejection = res;
          }
        }

        if (successfulResults.length === 0) {
          throw firstRejection?.reason || new Error("All language candidate transcription attempts failed");
        }

        const best = selectBestCandidate(successfulResults);
        winningResponse = best.response;
        effectiveLanguage = winningResponse.language || best.candidate;
      }

      const rawText = (winningResponse.text || "").trim();

      if (cleanup && rawText.length > 0) {
        const cleanedText = await runLlamaCleanup(groqApiKey, rawText, customTerms, effectiveLanguage);
        return new Response(JSON.stringify({ text: cleanedText, raw_text: rawText, language: effectiveLanguage }), {
          headers: { ...corsHeaders, "Content-Type": "application/json" },
        });
      }

      return new Response(JSON.stringify({ text: rawText, raw_text: rawText, language: effectiveLanguage }), {
        headers: { ...corsHeaders, "Content-Type": "application/json" },
      });
    }

    return new Response(JSON.stringify({ error: "Unsupported Media Type: Expected multipart/form-data or application/json" }), {
      status: 415,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  } catch (err: unknown) {
    const status = err instanceof GroqApiError ? err.status : 500;
    const message = err instanceof Error ? err.message : String(err);
    console.error("voice gateway unhandled error:", err);
    return new Response(JSON.stringify({ error: `Internal Server Error: ${message}` }), {
      status,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  }
});
