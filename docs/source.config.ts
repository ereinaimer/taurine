import { defineConfig, defineDocs } from 'fumadocs-mdx/config';
import { rehypeCodeDefaultOptions } from 'fumadocs-core/mdx-plugins';
import { metaSchema, pageSchema } from 'fumadocs-core/source/schema';

const CODE_BLOCK_META_REGEX =
  /(?<=^|\s)(?<name>[\w-]+)(?:=(?:"([^"]*)"|'([^']*)'))?/g;
const PLAYABLE_CODE_META_NAMES = new Set([
  'title',
  'tab',
  'trigger',
  'data-trigger',
]);

function parsePlayableCodeMeta(metaString: string): Record<string, unknown> {
  let rest = metaString;
  const attributes: Record<string, unknown> = {};

  rest = rest.replaceAll(
    CODE_BLOCK_META_REGEX,
    (
      match: string,
      name: string,
      doubleQuotedValue?: string,
      singleQuotedValue?: string,
    ) => {
      if (!PLAYABLE_CODE_META_NAMES.has(name)) return match;

      attributes[name] = doubleQuotedValue ?? singleQuotedValue ?? null;
      return '';
    },
  );

  rest = rest.replace(/lineNumbers=(\d+)|lineNumbers/g, (_, start?: string) => {
    attributes['data-line-numbers'] = true;

    if (start !== undefined) {
      attributes['data-line-numbers-start'] = Number(start);
    }

    return '';
  });

  if (
    typeof attributes['data-trigger'] === 'string' &&
    typeof attributes.trigger !== 'string'
  ) {
    attributes.trigger = attributes['data-trigger'];
  }

  attributes.__raw = rest;

  return attributes;
}

// You can customize Zod schemas for frontmatter and `meta.json` here
// see https://fumadocs.dev/docs/mdx/collections
export const docs = defineDocs({
  dir: 'content/docs',
  docs: {
    schema: pageSchema,
    postprocess: {
      includeProcessedMarkdown: true,
    },
  },
  meta: {
    schema: metaSchema,
  },
});

export default defineConfig({
  mdxOptions: {
    rehypeCodeOptions: {
      ...rehypeCodeDefaultOptions,
      parseMetaString: parsePlayableCodeMeta,
    },
  },
});
