import test from "node:test";
import assert from "node:assert/strict";
import {
  buildLlamaSystemPrompt,
  computeAverageLogprob,
  parseLanguageCandidates,
  resolveCandidateWhisperLanguage,
  selectBestCandidate,
} from "./routing.ts";
import type { WhisperCandidateResult } from "./routing.ts";

test("parseLanguageCandidates handles null, empty, and auto", () => {
  assert.deepEqual(parseLanguageCandidates(null), []);
  assert.deepEqual(parseLanguageCandidates(undefined), []);
  assert.deepEqual(parseLanguageCandidates(""), []);
  assert.deepEqual(parseLanguageCandidates("   "), []);
  assert.deepEqual(parseLanguageCandidates("auto"), []);
  assert.deepEqual(parseLanguageCandidates("AUTO"), []);
  assert.deepEqual(parseLanguageCandidates("en, auto, ml"), []);
  assert.deepEqual(parseLanguageCandidates("auto,es"), []);
});

test("parseLanguageCandidates normalizes, deduplicates, and caps at 3", () => {
  assert.deepEqual(parseLanguageCandidates("en"), ["en"]);
  assert.deepEqual(parseLanguageCandidates("  EN  "), ["en"]);
  assert.deepEqual(parseLanguageCandidates("en, ml"), ["en", "ml"]);
  assert.deepEqual(parseLanguageCandidates("en, ML, en, ja"), ["en", "ml", "ja"]);
  assert.deepEqual(parseLanguageCandidates("en, ml, ja, es, de"), ["en", "ml", "ja"]);
});

test("computeAverageLogprob calculates mean across segments", () => {
  assert.equal(computeAverageLogprob(undefined), -Infinity);
  assert.equal(computeAverageLogprob([]), -Infinity);
  assert.equal(computeAverageLogprob([{ avg_logprob: -0.5 }]), -0.5);

  const segments = [
    { avg_logprob: -0.2 },
    { avg_logprob: -0.4 },
    { avg_logprob: -0.6 },
  ];
  assert.ok(Math.abs(computeAverageLogprob(segments) - (-0.4)) < 1e-6);
});

test("selectBestCandidate favors non-empty transcripts over higher logprob empty ones", () => {
  const emptyWithHighLogprob: WhisperCandidateResult = {
    candidate: "ja",
    response: { text: "   ", segments: [{ avg_logprob: -0.05 }] },
    avgLogprob: -0.05,
  };
  const nonEmptyWithLowerLogprob: WhisperCandidateResult = {
    candidate: "en",
    response: { text: "Hello world", segments: [{ avg_logprob: -0.3 }] },
    avgLogprob: -0.3,
  };

  const best = selectBestCandidate([emptyWithHighLogprob, nonEmptyWithLowerLogprob]);
  assert.equal(best.candidate, "en");
  assert.equal(best.response.text, "Hello world");
});

test("selectBestCandidate selects highest logprob among non-empty candidates", () => {
  const candidateEn: WhisperCandidateResult = {
    candidate: "en",
    response: { text: "Namskaram", segments: [{ avg_logprob: -0.85 }] },
    avgLogprob: -0.85,
  };
  const candidateMl: WhisperCandidateResult = {
    candidate: "ml",
    response: { text: "നമസ്കാരം", segments: [{ avg_logprob: -0.22 }] },
    avgLogprob: -0.22,
  };

  const best = selectBestCandidate([candidateEn, candidateMl]);
  assert.equal(best.candidate, "ml");
  assert.equal(best.response.text, "നമസ്കാരം");
});

test("selectBestCandidate falls back gracefully when all candidates are empty", () => {
  const empty1: WhisperCandidateResult = {
    candidate: "en",
    response: { text: "", segments: [{ avg_logprob: -0.9 }] },
    avgLogprob: -0.9,
  };
  const empty2: WhisperCandidateResult = {
    candidate: "ml",
    response: { text: "   ", segments: [{ avg_logprob: -0.4 }] },
    avgLogprob: -0.4,
  };

  const best = selectBestCandidate([empty1, empty2]);
  assert.equal(best.candidate, "ml");
});

test("computeAverageLogprob ignores NaN or non-number segments", () => {
  const segments = [
    { avg_logprob: -0.2 },
    { avg_logprob: NaN },
    { avg_logprob: undefined },
    { avg_logprob: -0.8 },
  ];
  assert.ok(Math.abs(computeAverageLogprob(segments) - (-0.5)) < 1e-6);
});

test("selectBestCandidate handles single candidate or throws on empty", () => {
  const single: WhisperCandidateResult = {
    candidate: "en",
    response: { text: "Hello", segments: [{ avg_logprob: -0.2 }] },
    avgLogprob: -0.2,
  };
  assert.equal(selectBestCandidate([single]), single);
  assert.throws(() => selectBestCandidate([]), /empty result set/);
});

test("resolveCandidateWhisperLanguage maps English to null and preserves non-English", () => {
  assert.equal(resolveCandidateWhisperLanguage(null), null);
  assert.equal(resolveCandidateWhisperLanguage(undefined), null);
  assert.equal(resolveCandidateWhisperLanguage(""), null);
  assert.equal(resolveCandidateWhisperLanguage("en"), null);
  assert.equal(resolveCandidateWhisperLanguage("EN"), null);
  assert.equal(resolveCandidateWhisperLanguage("english"), null);
  assert.equal(resolveCandidateWhisperLanguage("auto"), null);
  assert.equal(resolveCandidateWhisperLanguage("ml"), "ml");
  assert.equal(resolveCandidateWhisperLanguage("ML"), "ml");
  assert.equal(resolveCandidateWhisperLanguage("ja"), "ja");
  assert.equal(resolveCandidateWhisperLanguage("hi"), "hi");
});

test("buildLlamaSystemPrompt strictly forbids translation and enforces native script", () => {
  const promptEn = buildLlamaSystemPrompt(null, "en");
  assert.ok(promptEn.includes("CRITICAL MANDATE - ZERO TRANSLATION"));
  assert.ok(promptEn.includes("NEVER translate the speaker's words"));
  assert.ok(promptEn.includes("മലയാളം"));
  assert.ok(promptEn.includes("日本語"));
  assert.ok(promptEn.includes("हिन्दी"));
  assert.ok(!promptEn.includes("Explicit Target Language"));

  const promptMl = buildLlamaSystemPrompt(["Supabase", "Taurine"], "malayalam");
  assert.ok(promptMl.includes("CRITICAL MANDATE - ZERO TRANSLATION"));
  assert.ok(promptMl.includes("Explicit Target Language & Native Script"));
  assert.ok(promptMl.includes("malayalam"));
  assert.ok(promptMl.includes("DO NOT translate to English"));
  assert.ok(promptMl.includes("Supabase, Taurine"));
});
