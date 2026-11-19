// Supabase Edge Function: voice
// Secure gateway for Taurine voice dictation.
// Authenticates users via Supabase Auth and proxies to Groq Whisper + Llama 3.1 8B without client-side API keys.

import { createClient } from "@supabase/supabase-js";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
  "Access-Control-Allow-Methods": "POST, OPTIONS",
};

const GROQ_WHISPER_URL = "https://api.groq.com/openai/v1/audio/transcriptions";
const GROQ_CHAT_URL = "https://api.groq.com/openai/v1/chat/completions";
const WHISPER_MODEL = "whisper-large-v3";
const CLEANUP_MODEL = "llama-3.1-8b-instant";

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

async function runLlamaCleanup(
  apiKey: string,
  rawTranscript: string,
  customTerms?: string[] | null,
): Promise<string> {
  const trimmedRaw = rawTranscript.trim();
  if (!trimmedRaw) {
    return "";
  }

  let systemPrompt =
    "You are an expert voice dictation cleanup engine. Transform the raw transcribed speech into clean, well-formatted, professional text.\n" +
    "Rules:\n" +
    "- Correct punctuation, capitalization, and minor grammatical slips.\n" +
    "- Remove conversational filler words (e.g. um, uh, ah, like, you know) and accidental stutter repetitions.\n" +
    "- Format numbers, currencies, dates, email addresses, and technical terms appropriately.\n" +
    "- Structure continuous speech into natural, readable paragraphs separated by blank lines (\\n\\n) at major topic transitions or thought shifts (typically every 3 to 6 sentences), or whenever the speaker dictates a new paragraph or new line.\n" +
    "- Format sequences of numbered items, chronologies, rankings, or sequential instructions as numbered lists (e.g. 1., 2., 3.) when dictated or naturally itemized.\n" +
    "- Format itemized features, options, components, or bullet points as clean bulleted lists (using hyphen \"- \") when dictated or naturally itemized.\n" +
    "- For standard continuous thoughts, explanations, and narratives, preserve regular flowing prose in paragraphs rather than forcing lists.\n" +
    "- Preserve the speaker's original intent, vocabulary, tone, and meaning exactly.\n" +
    "- If the speech contains only conversational filler words, stuttering, or ambient noise with no substantive content, output an empty string.\n" +
    "- Output ONLY the final cleaned text with no surrounding quotes, conversational preamble, explanations, markdown code blocks, or notes.";

  if (customTerms && customTerms.length > 0) {
    const validTerms = customTerms
      .map((t) => t.trim())
      .filter((t) => t.length > 0)
      .slice(0, 50);
    if (validTerms.length > 0) {
      systemPrompt += `\n- Domain vocabulary to preserve and format verbatim if phonetically spoken: ${validTerms.join(", ")}.`;
    }
  }

  const estimatedTokens = Math.floor((trimmedRaw.split(/\s+/).length * 4) / 3) + 64;
  const dynamicMaxTokens = Math.min(Math.max(estimatedTokens * 2 + 512, 1024), 8192);

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
        temperature: 0.1,
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

      const cleaned = await runLlamaCleanup(groqApiKey, rawText, customTerms);
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

      // Forward audio to Groq Whisper
      const groqFormData = new FormData();
      groqFormData.append("file", file, "dictation.wav");
      groqFormData.append("model", WHISPER_MODEL);
      groqFormData.append("response_format", "json");
      groqFormData.append("temperature", "0");
      if (prompt && prompt.trim().length > 0) {
        groqFormData.append("prompt", prompt.trim());
      }

      const whisperRes = await fetch(GROQ_WHISPER_URL, {
        method: "POST",
        headers: {
          "Authorization": `Bearer ${groqApiKey}`,
        },
        body: groqFormData,
      });

      if (!whisperRes.ok) {
        const errorText = await whisperRes.text();
        return new Response(JSON.stringify({ error: `Groq Whisper error (${whisperRes.status}): ${errorText}` }), {
          status: whisperRes.status,
          headers: { ...corsHeaders, "Content-Type": "application/json" },
        });
      }

      const whisperJson = await whisperRes.json();
      const rawText = (whisperJson.text || "").trim();

      if (cleanup && rawText.length > 0) {
        const cleanedText = await runLlamaCleanup(groqApiKey, rawText, customTerms);
        return new Response(JSON.stringify({ text: cleanedText, raw_text: rawText }), {
          headers: { ...corsHeaders, "Content-Type": "application/json" },
        });
      }

      return new Response(JSON.stringify({ text: rawText, raw_text: rawText }), {
        headers: { ...corsHeaders, "Content-Type": "application/json" },
      });
    }

    return new Response(JSON.stringify({ error: "Unsupported Media Type: Expected multipart/form-data or application/json" }), {
      status: 415,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    console.error("voice gateway unhandled error:", err);
    return new Response(JSON.stringify({ error: `Internal Server Error: ${message}` }), {
      status: 500,
      headers: { ...corsHeaders, "Content-Type": "application/json" },
    });
  }
});
