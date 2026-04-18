import fs from "node:fs";
import path from "node:path";
import matter from "gray-matter";
import { defaultDocSlug, flatSidebarItems } from "@/lib/sidebar.config";

/** Monorepo root (parent of `website/`) */
const REPO_ROOT = path.join(process.cwd(), "..");
/** mdBook sources — single source of truth with the published book */
const DOCS_SRC = path.join(REPO_ROOT, "docs", "src");

export type DocHeading = {
  id: string;
  text: string;
  level: 2 | 3;
};

export type LoadedDoc = {
  slug: string;
  title: string;
  description: string;
  content: string;
  headings: DocHeading[];
  sectionTitle: string;
};

const slugify = (value: string) =>
  value
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, "")
    .trim()
    .replace(/\s+/g, "-");

const fromSidebar = new Map(flatSidebarItems.map((item) => [item.slug, item]));

/** URL slug → file path under `docs/src` */
export function slugToDocPath(slug: string): string {
  if (slug === "introduction") {
    return path.join(DOCS_SRC, "index.md");
  }
  const segments = slug.split("/").filter(Boolean);
  return path.join(DOCS_SRC, ...segments) + ".md";
}

const INCLUDE_RE = /\{\{#include\s+([^}]+)\}\}/g;

function expandIncludes(markdown: string, currentFile: string, depth = 0): string {
  if (depth > 12) {
    return markdown;
  }
  return markdown.replace(INCLUDE_RE, (_, rawRel: string) => {
    const rel = rawRel.trim();
    const abs = path.resolve(path.dirname(currentFile), rel);
    const normalizedRoot = path.normalize(REPO_ROOT);
    const normalizedAbs = path.normalize(abs);
    if (!normalizedAbs.startsWith(normalizedRoot)) {
      return `\n\n_Include path escapes repository: ${rel}_\n\n`;
    }
    if (!fs.existsSync(abs)) {
      return `\n\n_Include not found: ${rel}_\n\n`;
    }
    const inner = fs.readFileSync(abs, "utf8");
    return `\n\n${expandIncludes(inner, abs, depth + 1)}\n\n`;
  });
}

/**
 * Turn mdBook links `[x](page.md)` and `[x](dir/other.md#h)` into internal `/docs/...` routes.
 * Skips `../` paths (left as-is for rare non-doc links).
 */
/** MDX treats `<https://...>` like JSX; convert autolinks to markdown `[url](url)`. */
function sanitizeAngleBracketUrls(markdown: string): string {
  return markdown.replace(/<(https?:\/\/[^>\s]+)>/g, (_, url: string) => `[${url}](${url})`);
}

function rewriteMdBookLinks(markdown: string): string {
  return markdown.replace(/\]\(([^)]+)\)/g, (full, target: string) => {
    if (
      target.startsWith("http://") ||
      target.startsWith("https://") ||
      target.startsWith("/") ||
      target.startsWith("#")
    ) {
      return full;
    }
    const hashIdx = target.indexOf("#");
    const pathPart = hashIdx >= 0 ? target.slice(0, hashIdx) : target;
    const hash = hashIdx >= 0 ? target.slice(hashIdx) : "";
    if (!pathPart.endsWith(".md") || pathPart.includes("..")) {
      return full;
    }
    let slug = pathPart.replace(/^\.\//, "").replace(/\.md$/, "").replace(/\\/g, "/");
    if (slug === "index") {
      slug = "introduction";
    }
    return `](/docs/${slug}${hash})`;
  });
}

const extractHeadings = (source: string): DocHeading[] => {
  const lines = source.split("\n");
  const headings: DocHeading[] = [];

  for (const line of lines) {
    if (line.startsWith("## ")) {
      const text = line.replace(/^## /, "").trim();
      headings.push({ level: 2, text, id: slugify(text) });
    } else if (line.startsWith("### ")) {
      const text = line.replace(/^### /, "").trim();
      headings.push({ level: 3, text, id: slugify(text) });
    }
  }

  return headings;
};

const missingPage = (slug: string, title: string) => `---
title: "${title}"
description: "Not published."
---

## Not available

This page is not part of the nestrs documentation in \`docs/src/\`. If you followed a link here, please [open an issue](https://github.com/Joshyahweh/nestrs/issues).

[← Back to documentation](/docs/${defaultDocSlug})
`;

function collectAllDocSlugs(): string[] {
  const slugs = new Set<string>();

  const walk = (dir: string, prefix: string) => {
    if (!fs.existsSync(dir)) return;
    for (const name of fs.readdirSync(dir, { withFileTypes: true })) {
      const rel = prefix ? `${prefix}/${name.name}` : name.name;
      if (name.isDirectory()) {
        walk(path.join(dir, name.name), rel);
      } else if (name.name.endsWith(".md") && name.name !== "SUMMARY.md") {
        let slug = rel.replace(/\.md$/, "").replace(/\\/g, "/");
        if (slug === "index") {
          slug = "introduction";
        }
        slugs.add(slug);
      }
    }
  };

  walk(DOCS_SRC, "");

  for (const entry of flatSidebarItems) {
    slugs.add(entry.slug);
  }

  return [...slugs];
}

export const resolveSlug = (segments?: string[]) => {
  if (!segments || segments.length === 0) return defaultDocSlug;
  return segments.join("/");
};

export const getAllSlugs = () => {
  const nested = collectAllDocSlugs().map((s) => s.split("/").filter(Boolean));
  const keys = new Set(nested.map((s) => s.join("/")));
  // Pre-render `/docs` (same content as `/docs/introduction`)
  if (!keys.has("")) {
    nested.unshift([]);
  }
  return nested;
};

export const getDoc = (segments?: string[]): LoadedDoc => {
  const slug = resolveSlug(segments);
  const entry = fromSidebar.get(slug);
  const filePath = slugToDocPath(slug);

  let raw: string;
  if (!fs.existsSync(filePath)) {
    raw = missingPage(slug, entry?.title ?? slug);
  } else {
    raw = fs.readFileSync(filePath, "utf8");
    raw = expandIncludes(raw, filePath);
    raw = sanitizeAngleBracketUrls(raw);
    raw = rewriteMdBookLinks(raw);
  }

  const { data, content } = matter(raw);
  const headings = extractHeadings(content);

  const fileTitleMatch = content.match(/^#\s+(.+)$/m);
  const title = String(data.title ?? entry?.title ?? fileTitleMatch?.[1] ?? slug);

  return {
    slug,
    title,
    description: String(data.description ?? entry?.title ?? `nestrs — ${title}`),
    content,
    headings,
    sectionTitle: entry?.sectionTitle ?? "Documentation"
  };
};

export type SearchEntry = {
  slug: string;
  title: string;
  sectionTitle: string;
  headings: { id: string; text: string }[];
};

export const getSearchIndex = (): SearchEntry[] =>
  flatSidebarItems.map((entry) => {
    const doc = getDoc(entry.slug.split("/"));
    return {
      slug: entry.slug,
      title: entry.title,
      sectionTitle: entry.sectionTitle,
      headings: doc.headings.filter((h) => h.level === 2).map((h) => ({ id: h.id, text: h.text }))
    };
  });

export const getPrevNext = (slug: string) => {
  const index = flatSidebarItems.findIndex((entry) => entry.slug === slug);
  return {
    prev: index > 0 ? flatSidebarItems[index - 1] : null,
    next: index >= 0 && index < flatSidebarItems.length - 1 ? flatSidebarItems[index + 1] : null
  };
};
