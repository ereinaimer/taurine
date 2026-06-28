/**
 * Taurine Variable Engine Simulator
 * This mocks the Rust variable engine logic for the documentation playground.
 */

// Basic text transformers
const transformers: Record<string, (val: string) => string> = {
  upper: (val) => val.toUpperCase(),
  uppercase: (val) => val.toUpperCase(),
  lower: (val) => val.toLowerCase(),
  lowercase: (val) => val.toLowerCase(),
  title: (val) =>
    val.replace(
      /\w\S*/g,
      (txt) => txt.charAt(0).toUpperCase() + txt.substr(1).toLowerCase()
    ),
  titlecase: (val) =>
    val.replace(
      /\w\S*/g,
      (txt) => txt.charAt(0).toUpperCase() + txt.substr(1).toLowerCase()
    ),
  urlencode: (val) => encodeURIComponent(val),
  snake: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.toLowerCase())
      .join('_') || val,
  snakecase: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.toLowerCase())
      .join('_') || val,
  kebab: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.toLowerCase())
      .join('-') || val,
  kebabcase: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.toLowerCase())
      .join('-') || val,
  pascal: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.charAt(0).toUpperCase() + x.slice(1).toLowerCase())
      .join('') || val,
  pascalcase: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.charAt(0).toUpperCase() + x.slice(1).toLowerCase())
      .join('') || val,
  camel: (val) => {
    const p =
      val
        .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
        ?.map((x) => x.charAt(0).toUpperCase() + x.slice(1).toLowerCase())
        .join('') || val;
    return p ? p.charAt(0).toLowerCase() + p.slice(1) : p;
  },
  camelcase: (val) => {
    const p =
      val
        .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
        ?.map((x) => x.charAt(0).toUpperCase() + x.slice(1).toLowerCase())
        .join('') || val;
    return p ? p.charAt(0).toLowerCase() + p.slice(1) : p;
  },
  shoutysnake: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.toUpperCase())
      .join('_') || val,
  shoutysnakecase: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.toUpperCase())
      .join('_') || val,
  shoutykebab: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.toUpperCase())
      .join('-') || val,
  shoutykebabcase: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.toUpperCase())
      .join('-') || val,
  train: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.charAt(0).toUpperCase() + x.slice(1).toLowerCase())
      .join('-') || val,
  traincase: (val) =>
    val
      .match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g)
      ?.map((x) => x.charAt(0).toUpperCase() + x.slice(1).toLowerCase())
      .join('-') || val,
};

function parseArguments(input: string, prefix: string): { positional: string[]; named: Record<string, string> } {
  const positional: string[] = [];
  const named: Record<string, string> = {};

  // Strip prefix and trailing space
  if (!input.startsWith(prefix)) return { positional, named };
  let argString = input.slice(prefix.length).trimEnd();
  
  if (!argString.startsWith(':')) return { positional, named };
  argString = argString.slice(1); // remove the leading colon

  // Regex to split by colon, respecting quotes
  // Matches either quoted string "..." or '...' or anything up to next colon
  const regex = /([^:"']+)|"([^"]*)"|'([^']*)'/g;
  const parts: string[] = [];
  let currentPart = '';
  
  // A simple split by ':' won't work for quoted colons, so we parse carefully:
  let i = 0;
  while (i < argString.length) {
    if (argString[i] === '"' || argString[i] === "'") {
      const quote = argString[i];
      let end = argString.indexOf(quote, i + 1);
      if (end === -1) end = argString.length;
      currentPart += argString.slice(i + 1, end);
      i = end + 1;
    } else if (argString[i] === ':') {
      parts.push(currentPart);
      currentPart = '';
      i++;
    } else {
      currentPart += argString[i];
      i++;
    }
  }
  parts.push(currentPart);

  for (const part of parts) {
    const eqIndex = part.indexOf('=');
    if (eqIndex !== -1) {
      const key = part.slice(0, eqIndex);
      const val = part.slice(eqIndex + 1);
      named[key] = val;
    } else {
      positional.push(part);
    }
  }

  return { positional, named };
}

export function resolveTemplate(template: string, input: string, prefix: string): string {
  if (!input.startsWith(prefix) || !input.endsWith(' ')) {
    return template; // Only process if it matches trigger and space
  }

  const { positional, named } = parseArguments(input, prefix);
  let positionalIndex = 0;

  const tagRegex = /\[([^\[\]]+)\]/g;

  return template.replace(tagRegex, (match, inner) => {
    const pipeline = inner.split('|').map((s: string) => s.trim());
    const baseExpr = pipeline[0];
    const transformersList = pipeline.slice(1);

    let key = baseExpr;
    let defaultValue: string | undefined = undefined;
    if (baseExpr.includes('=')) {
      const eqIdx = baseExpr.indexOf('=');
      key = baseExpr.substring(0, eqIdx).trim();
      defaultValue = baseExpr.substring(eqIdx + 1).trim();
    }

    let resolvedValue: string | undefined;

    // 1. Try named argument
    if (named[key] !== undefined) {
      resolvedValue = named[key];
    }
    // 2. Try positional index if key is a number
    else if (!isNaN(Number(key))) {
      const val = positional[Number(key)];
      resolvedValue = (val === '' || val === undefined) && defaultValue !== undefined ? defaultValue : val;
    }
    // 3. Fallback to sequence of positionals if not a number
    else {
      const val = positional[positionalIndex];
      resolvedValue = (val === '' || val === undefined) && defaultValue !== undefined ? defaultValue : val;
      if (val !== undefined) positionalIndex++;
    }
    
    if (resolvedValue === undefined) {
      return match; // unresolved
    }

    // Apply modifiers
    for (const mod of transformersList) {
      if (transformers[mod]) {
        resolvedValue = transformers[mod]!(resolvedValue);
      }
    }

    return resolvedValue;
  });
}
