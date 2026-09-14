/**
 * Taurine Variable Engine Simulator
 * This mocks the Rust variable engine logic for the documentation playground.
 * Supports the canonical `[base | category(action, ...)]` transformer syntax only.
 */

// Basic text transformers
const parseHex = (hex: string) => {
  hex = hex.trim().replace(/^#/, '');
  if (hex.length === 3) hex = hex.split('').map(x => x + x).join('');
  if (hex.length === 4) hex = hex.split('').map(x => x + x).join('');
  const r = parseInt(hex.slice(0, 2), 16);
  const g = parseInt(hex.slice(2, 4), 16);
  const b = parseInt(hex.slice(4, 6), 16);
  const a = hex.length === 8 ? parseInt(hex.slice(6, 8), 16) / 255 : 1;
  return { r, g, b, a };
};

const cssNames: Record<string, string> = {
  red: '#FF0000', green: '#00FF00', blue: '#0000FF', white: '#FFFFFF', black: '#000000', rebeccapurple: '#663399'
};

const parseColor = (val: string) => {
  let s = val.trim().toLowerCase();
  if (cssNames[s]) s = cssNames[s];
  if (s.startsWith('#')) return parseHex(s);
  if (s.startsWith('rgb')) {
    const match = s.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
    if (match) return { r: parseInt(match[1]), g: parseInt(match[2]), b: parseInt(match[3]), a: match[4] ? parseFloat(match[4]) : 1 };
  }
  if (s.startsWith('hsl')) {
    // Simple hsl parse fallback for common test cases
    const match = s.match(/hsla?\((\d+),\s*(\d+)%,\s*(\d+)%(?:,\s*([\d.]+))?\)/);
    if (match) {
      // rough approximation of hsl to rgb for standard test cases
      const h = parseInt(match[1]) / 360;
      const s = parseInt(match[2]) / 100;
      const l = parseInt(match[3]) / 100;
      const a = match[4] ? parseFloat(match[4]) : 1;
      let r = l, g = l, b = l;
      if (s !== 0) {
        const hue2rgb = (p: number, q: number, t: number) => {
          if (t < 0) t += 1;
          if (t > 1) t -= 1;
          if (t < 1/6) return p + (q - p) * 6 * t;
          if (t < 1/2) return q;
          if (t < 2/3) return p + (q - p) * (2/3 - t) * 6;
          return p;
        };
        const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
        const p = 2 * l - q;
        r = hue2rgb(p, q, h + 1/3);
        g = hue2rgb(p, q, h);
        b = hue2rgb(p, q, h - 1/3);
      }
      return { r: Math.round(r * 255), g: Math.round(g * 255), b: Math.round(b * 255), a };
    }
  }
  return null;
};

const rgbToHsl = (r: number, g: number, b: number) => {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  let h = 0, s = 0, l = (max + min) / 2;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h /= 6;
  }
  return { h: Math.round(h * 360), s: Math.round(s * 100), l: Math.round(l * 100) };
};

const splitWords = (val: string): string[] =>
  val.match(/[A-Z]{2,}(?=[A-Z][a-z]+[0-9]*|\b)|[A-Z]?[a-z]+[0-9]*|[A-Z]|[0-9]+/g) || [];

const stripArgQuotes = (arg: string): string => {
  const t = arg.trim();
  if (t.length >= 2 && ((t.startsWith('"') && t.endsWith('"')) || (t.startsWith("'") && t.endsWith("'")))) {
    return t.slice(1, -1);
  }
  return t;
};

/** Split on top-level `|`, ignoring pipes inside quotes or parentheses. Mirrors Rust split_pipeline. */
function splitPipeline(input: string): string[] {
  const segments: string[] = [];
  let depth = 0;
  let quote: string | null = null;
  let escaped = false;
  let start = 0;
  for (let i = 0; i < input.length; i++) {
    const ch = input[i];
    if (escaped) { escaped = false; continue; }
    if (ch === '\\') { escaped = true; continue; }
    if (quote) {
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") quote = ch;
    else if (ch === '(' || ch === '[') depth++;
    else if (ch === ')' || ch === ']') depth = Math.max(0, depth - 1);
    else if (ch === '|' && depth === 0) {
      segments.push(input.slice(start, i).trim());
      start = i + 1;
    }
  }
  segments.push(input.slice(start).trim());
  return segments;
}

/** Split a comma-separated argument list, respecting quotes and nested parens. */
function splitArgs(args: string): string[] {
  if (args.trim() === '') return [];
  const parts: string[] = [];
  let depth = 0;
  let quote: string | null = null;
  let escaped = false;
  let start = 0;
  for (let i = 0; i < args.length; i++) {
    const ch = args[i];
    if (quote) {
      if (escaped) escaped = false;
      else if (ch === '\\') escaped = true;
      else if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") quote = ch;
    else if (ch === '(') depth++;
    else if (ch === ')') {
      if (depth === 0) return [];
      depth--;
    } else if (ch === ',' && depth === 0) {
      parts.push(args.slice(start, i).trim());
      start = i + 1;
    }
  }
  if (depth !== 0 || quote) return [];
  parts.push(args.slice(start).trim());
  return parts;
}

/** Parse `name` or `name(arg, ...)` into a canonical transformer call. */
function parseTransformerCall(seg: string): { name: string; args: string[] } | undefined {
  const t = seg.trim();
  const open = t.indexOf('(');
  if (open === -1) return { name: t, args: [] };
  if (!t.endsWith(')')) return undefined;
  return { name: t.slice(0, open).trim(), args: splitArgs(t.slice(open + 1, -1)) };
}

type Handler = (val: string, args: string[]) => string | undefined;

const transformers: Record<string, Handler> = {
  case: (val, args) => {
    if (args.length !== 1) return undefined;
    const words = () => splitWords(val);
    switch (stripArgQuotes(args[0]).toLowerCase()) {
      case 'upper': return val.toUpperCase();
      case 'lower': return val.toLowerCase();
      case 'sentence': return val.charAt(0).toUpperCase() + val.slice(1).toLowerCase();
      case 'title': return val.replace(/\w\S*/g, (txt) => txt.charAt(0).toUpperCase() + txt.substr(1).toLowerCase());
      case 'snake': return words().map((x) => x.toLowerCase()).join('_') || val;
      case 'kebab': return words().map((x) => x.toLowerCase()).join('-') || val;
      case 'pascal': return words().map((x) => x.charAt(0).toUpperCase() + x.slice(1).toLowerCase()).join('') || val;
      case 'camel': {
        const p = words().map((x) => x.charAt(0).toUpperCase() + x.slice(1).toLowerCase()).join('') || val;
        return p ? p.charAt(0).toLowerCase() + p.slice(1) : p;
      }
      case 'slug': return val.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || val;
      default: return undefined;
    }
  },
  count: (val, args) => {
    if (args.length !== 1) return undefined;
    switch (stripArgQuotes(args[0]).toLowerCase()) {
      case 'chars': return String([...val].length);
      case 'words': return String(val.trim() === '' ? 0 : val.trim().split(/\s+/).length);
      default: return undefined;
    }
  },
  truncate: (val, args) => {
    if (args.length !== 1) return undefined;
    const n = parseInt(stripArgQuotes(args[0]), 10);
    if (isNaN(n) || n < 0) return undefined;
    return [...val].slice(0, n).join('');
  },
  repeat: (val, args) => {
    if (args.length !== 1) return undefined;
    const n = parseInt(stripArgQuotes(args[0]), 10);
    if (isNaN(n) || n < 0 || n > 100) return undefined;
    return val.repeat(n);
  },
  replace: (val, args) => {
    if (args.length === 2) {
      return val.split(stripArgQuotes(args[0])).join(stripArgQuotes(args[1]));
    }
    if (args.length === 3 && stripArgQuotes(args[0]).toLowerCase() === 'regex') {
      try {
        return val.replace(new RegExp(stripArgQuotes(args[1]), 'g'), stripArgQuotes(args[2]));
      } catch { return undefined; }
    }
    return undefined;
  },
  slice: (val, args) => {
    if (args.length !== 2) return undefined;
    const chars = [...val];
    const s = parseInt(stripArgQuotes(args[0]), 10);
    const e = parseInt(stripArgQuotes(args[1]), 10);
    if (isNaN(s) || isNaN(e)) return undefined;
    return chars.slice(s, e).join('');
  },
  filter: (val, args) => {
    if (args.length !== 1) return undefined;
    switch (stripArgQuotes(args[0]).toLowerCase()) {
      case 'digits': return val.replace(/[^0-9]/g, '');
      case 'alphanumeric': return val.replace(/[^a-zA-Z0-9]/g, '');
      default: return undefined;
    }
  },
  strip: (val, args) => {
    if (args.length !== 1) return undefined;
    switch (stripArgQuotes(args[0]).toLowerCase()) {
      case 'whitespace': return val.trim();
      case 'emoji':
        try {
          return val.replace(/[\p{Emoji_Presentation}\p{Extended_Pictographic}\u200D\uFE0F]/gu, '');
        } catch {
          return val.replace(/[\u2700-\u27BF]|\uD83C[\uDC00-\uDFFF]|\uD83D[\uDC00-\uDFFF]|\uD83E[\uDD10-\uDDFF]/g, '');
        }
      default: return undefined;
    }
  },
  encode: (val, args) => {
    if (args.length !== 1) return undefined;
    switch (stripArgQuotes(args[0]).toLowerCase()) {
      case 'url': return encodeURIComponent(val);
      case 'base64': return btoa(val);
      default: return undefined;
    }
  },
  decode: (val, args) => {
    if (args.length !== 1) return undefined;
    try {
      switch (stripArgQuotes(args[0]).toLowerCase()) {
        case 'url': return decodeURIComponent(val);
        case 'base64': return atob(val.trim());
        default: return undefined;
      }
    } catch { return undefined; }
  },
  clean: (val, args) => {
    if (args.length !== 1 || stripArgQuotes(args[0]).toLowerCase() !== 'url') return undefined;
    try {
      const url = new URL(val.trim());
      url.search = '';
      url.hash = '';
      return url.toString();
    } catch {
      return val.split('?')[0].split('#')[0];
    }
  },
  wrap: (val, args) => {
    if (args.length !== 1) return undefined;
    switch (stripArgQuotes(args[0]).toLowerCase()) {
      case 'doublequote': return `"${val}"`;
      case 'singlequote': return `'${val}'`;
      case 'backtick': return `\`${val}\``;
      default: return undefined;
    }
  },
  unwrap: (val, args) => {
    if (args.length !== 1 || stripArgQuotes(args[0]).toLowerCase() !== 'quotes') return undefined;
    if (val.length >= 2 && ((val.startsWith('"') && val.endsWith('"')) || (val.startsWith("'") && val.endsWith("'")))) {
      return val.slice(1, -1);
    }
    return val;
  },
  color: (val, args) => {
    if (args.length !== 1) return undefined;
    const c = parseColor(val);
    if (!c) return undefined;
    const hex = (x: number) => x.toString(16).toUpperCase().padStart(2, '0');
    switch (stripArgQuotes(args[0]).toLowerCase()) {
      case 'hex': {
        const alpha = c.a < 1 ? hex(Math.round(c.a * 255)) : '';
        return `#${hex(c.r)}${hex(c.g)}${hex(c.b)}${alpha}`;
      }
      case 'rgb': return c.a < 1 ? `rgba(${c.r}, ${c.g}, ${c.b}, ${c.a})` : `rgb(${c.r}, ${c.g}, ${c.b})`;
      case 'rgba': return `rgba(${c.r}, ${c.g}, ${c.b}, ${c.a})`;
      case 'hsl': {
        const hsl = rgbToHsl(c.r, c.g, c.b);
        return c.a < 1 ? `hsla(${hsl.h}, ${hsl.s}%, ${hsl.l}%, ${c.a})` : `hsl(${hsl.h}, ${hsl.s}%, ${hsl.l}%)`;
      }
      case 'hsla': {
        const hsl = rgbToHsl(c.r, c.g, c.b);
        return `hsla(${hsl.h}, ${hsl.s}%, ${hsl.l}%, ${c.a})`;
      }
      default: return undefined;
    }
  },
  json: (val, args) => {
    if (args.length !== 1) return undefined;
    try {
      let current = JSON.parse(val.trim());
      for (const segment of stripArgQuotes(args[0]).split('.')) {
        if (current === null || current === undefined) return undefined;
        current = /^\d+$/.test(segment) ? current[parseInt(segment, 10)] : current[segment];
      }
      if (current === null || current === undefined) return undefined;
      return typeof current === 'string' ? current : JSON.stringify(current);
    } catch { return undefined; }
  },
  'json.pretty': (val, args) => {
    if (args.length !== 0) return undefined;
    try {
      return JSON.stringify(JSON.parse(val.trim()), null, 2);
    } catch { return undefined; }
  },
  'json.minify': (val, args) => {
    if (args.length !== 0) return undefined;
    try {
      return JSON.stringify(JSON.parse(val.trim()));
    } catch { return undefined; }
  },
  lines: (val, args) => {
    if (args.length < 1) return undefined;
    const lines = val.split('\n');
    const action = stripArgQuotes(args[0]).toLowerCase();
    const rest = args.slice(1);
    switch (action) {
      case 'first': return rest.length === 0 ? (lines[0] ?? '') : undefined;
      case 'last': return rest.length === 0 ? (lines[lines.length - 1] ?? '') : undefined;
      case 'count': return rest.length === 0 ? String(lines.length) : undefined;
      case 'compact': return rest.length === 0 ? lines.filter((l) => l.trim() !== '').join('\n') : undefined;
      case 'unique': return rest.length === 0 ? [...new Set(lines)].join('\n') : undefined;
      case 'prefix': return rest.length === 1 ? lines.map((l) => stripArgQuotes(rest[0]) + l).join('\n') : undefined;
      case 'suffix': return rest.length === 1 ? lines.map((l) => l + stripArgQuotes(rest[0])).join('\n') : undefined;
      case 'join': return rest.length === 1 ? lines.join(stripArgQuotes(rest[0])) : undefined;
      case 'split': {
        if (rest.length !== 1) return undefined;
        const delim = stripArgQuotes(rest[0]);
        return delim === '' ? val : val.split(delim).join('\n');
      }
      case 'sort': {
        const flags = rest.map((a) => stripArgQuotes(a).toLowerCase());
        if (flags.some((f) => f !== 'desc' && f !== 'insensitive' && f !== 'numeric')) return undefined;
        const sorted = [...lines];
        if (flags.includes('numeric')) {
          sorted.sort((a, b) => {
            const x = parseFloat(a.trim()), y = parseFloat(b.trim());
            if (isNaN(x) && isNaN(y)) return 0;
            if (isNaN(x)) return 1;
            if (isNaN(y)) return -1;
            return x - y;
          });
        } else if (flags.includes('insensitive')) {
          sorted.sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()));
        } else {
          sorted.sort();
        }
        if (flags.includes('desc')) sorted.reverse();
        return sorted.join('\n');
      }
      default: return undefined;
    }
  },
  regex: (val, args) => {
    if (args.length < 1 || args.length > 2) return undefined;
    try {
      const group = args.length === 2 ? parseInt(stripArgQuotes(args[1]), 10) : 0;
      if (isNaN(group)) return undefined;
      const m = val.match(new RegExp(stripArgQuotes(args[0])));
      return m?.[group];
    } catch { return undefined; }
  },
  extract: (val, args) => {
    if (args.length !== 1) return undefined;
    const patterns: Record<string, RegExp> = {
      url: /https?:\/\/[^\s<>"']+/g,
      email: /\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/g,
      phone: /(?:\+?\d{1,3}[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b/g,
      mention: /\B@[\w.-]+\b/g,
      hashtag: /\B#[a-zA-Z_][\w-]*\b/g,
      ip: /\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b/g,
    };
    const re = patterns[stripArgQuotes(args[0]).toLowerCase()];
    if (!re) return undefined;
    return (val.match(re) ?? []).join('\n');
  },
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
    const pipeline = splitPipeline(inner);
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

    // 0. Try system variables mock
    if (key.startsWith('use(') && key.endsWith(')')) {
      const inner = key.slice(4, -1).trim();
      let unquoted = inner;
      if ((inner.startsWith('"') && inner.endsWith('"')) || (inner.startsWith("'") && inner.endsWith("'"))) {
        unquoted = inner.slice(1, -1);
      }
      resolvedValue = `(Content of snippet ${unquoted})`;
    }
    // 1. Try named argument
    else if (named[key] !== undefined) {
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

    // Apply canonical transformers; any unknown or failed transformer
    // leaves the tag unresolved, mirroring the Rust engine.
    for (const seg of transformersList) {
      const call = parseTransformerCall(seg);
      const handler = call ? transformers[call.name] : undefined;
      if (!handler) return match;
      const next = handler(resolvedValue, call.args);
      if (next === undefined) return match;
      resolvedValue = next;
    }

    return resolvedValue;
  });
}
