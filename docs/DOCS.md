# Taurine Docs Manual

This directory contains the isolated Next.js + Fumadocs documentation site for Taurine. All
Node.js tooling, dependencies, and framework configuration must stay inside `docs/`.

## Prerequisites

- Node.js 22 or newer. The current Fumadocs quick start requires Node.js 22+.
- One JavaScript package manager:
  - `npm`
  - `pnpm`

## Install Dependencies

From the repository root:

```bash
cd docs
npm install
```

If you prefer pnpm:

```bash
cd docs
pnpm install
```

## Run the Development Server

```bash
cd docs
npm run dev
```

Then open `http://localhost:3000/docs`.

With pnpm:

```bash
cd docs
pnpm dev
```

## Build the Static Site

This scaffold uses the official Fumadocs static Next.js template. `next.config.mjs` already sets
`output: 'export'`, so `next build` emits a static site into `docs/out/`.

```bash
cd docs
npm run build
```

To serve the built output locally:

```bash
cd docs
npm run start
```

With pnpm:

```bash
cd docs
pnpm build
pnpm start
```

If you know the deployed docs URL, set `NEXT_PUBLIC_SITE_URL` before building so generated
metadata and Open Graph URLs resolve against the correct origin.

## Project Structure

- `app/`: Next.js App Router entrypoints and layouts.
- `app/docs/`: The documentation layout and catch-all page route that renders docs content.
- `content/docs/`: Markdown and MDX source files for the docs site.
- `components/mdx.tsx`: The shared MDX component mapping used by rendered pages.
- `lib/source.ts`: The Fumadocs `loader()` adapter that turns `content/docs/` into the docs source.
- `source.config.ts`: Fumadocs MDX collection configuration.
- `.source/`: Generated source artifacts produced when `next dev`, `next build`, or
  `fumadocs-mdx` runs.

## File-System Routing

Fumadocs uses the content source to generate slugs and page-tree data, while Next.js handles the
actual route rendering.

- `content/docs/index.mdx` becomes `/docs`.
- `content/docs/getting-started.mdx` becomes `/docs/getting-started`.
- `content/docs/guides/index.mdx` becomes `/docs/guides`.
- `content/docs/guides/install.mdx` becomes `/docs/guides/install`.

Important routing rules from the official Fumadocs page-tree conventions:

- File paths define slugs.
- `index.mdx` maps to its folder path.
- Parenthesized folders such as `content/docs/(internal)/page.mdx` do not affect the URL slug.
- Navigation order is alphabetical unless a folder-level `meta.json` overrides it.

## How `meta.json` Controls Navigation

Create a `meta.json` file inside any folder under `content/docs/` to control display name, open
state, and ordering.

Example:

```json
{
  "title": "Guides",
  "defaultOpen": true,
  "pages": ["index", "getting-started", "..."]
}
```

Notes:

- `title` changes the folder label shown in navigation.
- `defaultOpen` opens the section by default in the sidebar.
- `pages` controls ordering.
- When `pages` is present, items not listed are excluded unless you include `"..."` to pull in the
  rest of the folder alphabetically.

## Required MDX Frontmatter

Every page should declare at least a title and description.

```mdx
---
title: CLI Overview
description: Learn how Taurine commands are organized and invoked.
---
```

Fumadocs uses this frontmatter to populate page titles, descriptions, and the page tree.

## Authoring Guidelines

- Write docs in `content/docs/`.
- Prefer one topic per file.
- Keep titles concise and descriptions specific.
- Use `index.mdx` when a folder needs its own landing page.
- Add `meta.json` when a section needs navigation changes.

## Built-in MDX Components

The generated project already wires in Fumadocs default MDX components through
`components/mdx.tsx`.

### Callouts

Use callouts for notes, warnings, ideas, or success messages.

```mdx
<Callout title="Heads up" type="warn">
  The daemon must be running before this command can attach to a session.
</Callout>
```

Supported common types include `info`, `warn`, `error`, `success`, and `idea`.

### Code Blocks

Use fenced code blocks for commands and examples.

````mdx
```bash title="Build the docs site"
cd docs
npm run build
```
````

Fumadocs provides syntax highlighting by default. You can also add options like titles and line
numbers.

## Contributor Workflow

1. Add or edit MDX files under `content/docs/`.
2. Add or update `meta.json` when a section needs navigation changes.
3. Run `npm run dev` while authoring.
4. Run `npm run build` before submitting changes to confirm the static export still succeeds.

## Isolation Rules

- Do not move `package.json`, lockfiles, or Next.js config files to the repository root.
- Do not add Node.js dependencies to the Rust workspace root for docs work.
- Keep all documentation-site build artifacts and configuration inside `docs/`.
