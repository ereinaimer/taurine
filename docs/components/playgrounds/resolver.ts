/**
 * Taurine Variable Engine Simulator
 * This mocks the Rust variable engine logic for the documentation playground.
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
  'url.encode': (val) => encodeURIComponent(val),
  'url.decode': (val) => {
    try {
      return decodeURIComponent(val);
    } catch {
      return val;
    }
  },
  'url.clean': (val) => {
    try {
      const url = new URL(val.trim());
      url.search = '';
      return url.toString();
    } catch {
      const qIdx = val.indexOf('?');
      return qIdx !== -1 ? val.slice(0, qIdx) : val;
    }
  },
  'base64.encode': (val) => btoa(val),
  'base64.decode': (val) => {
    try {
      return atob(val.trim());
    } catch {
      return val;
    }
  },
  'stripemoji': (val) => {
    try {
      return val.replace(/[\p{Emoji_Presentation}\p{Extended_Pictographic}\u200D\uFE0F]/gu, '');
    } catch {
      return val.replace(/[\u2700-\u27BF]|\uD83C[\uDC00-\uDFFF]|\uD83D[\uDC00-\uDFFF]|\uD83E[\uDD10-\uDDFF]/g, '');
    }
  },
  'json.pretty': (val) => {
    try {
      return JSON.stringify(JSON.parse(val.trim()), null, 2);
    } catch {
      return val;
    }
  },
  'json.minify': (val) => {
    try {
      return JSON.stringify(JSON.parse(val.trim()));
    } catch {
      return val;
    }
  },
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
  'color.hex': (val) => {
    const c = parseColor(val);
    if (!c) return val;
    const hex = (x: number) => x.toString(16).toUpperCase().padStart(2, '0');
    const alpha = c.a < 1 ? hex(Math.round(c.a * 255)) : '';
    return `#${hex(c.r)}${hex(c.g)}${hex(c.b)}${alpha}`;
  },
  'color.rgb': (val) => {
    const c = parseColor(val);
    if (!c) return val;
    return c.a < 1 ? `rgba(${c.r}, ${c.g}, ${c.b}, ${c.a})` : `rgb(${c.r}, ${c.g}, ${c.b})`;
  },
  'color.rgba': (val) => {
    const c = parseColor(val);
    if (!c) return val;
    return `rgba(${c.r}, ${c.g}, ${c.b}, ${c.a})`;
  },
  'color.hsl': (val) => {
    const c = parseColor(val);
    if (!c) return val;
    const hsl = rgbToHsl(c.r, c.g, c.b);
    return c.a < 1 ? `hsla(${hsl.h}, ${hsl.s}%, ${hsl.l}%, ${c.a})` : `hsl(${hsl.h}, ${hsl.s}%, ${hsl.l}%)`;
  },
  'color.hsla': (val) => {
    const c = parseColor(val);
    if (!c) return val;
    const hsl = rgbToHsl(c.r, c.g, c.b);
    return `hsla(${hsl.h}, ${hsl.s}%, ${hsl.l}%, ${c.a})`;
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

    // Apply modifiers
    for (const mod of transformersList) {
      if (transformers[mod]) {
        resolvedValue = transformers[mod]!(resolvedValue);
      }
    }

    return resolvedValue;
  });
}
