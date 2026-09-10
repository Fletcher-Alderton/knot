import { Marked, type TokenizerAndRendererExtension } from "marked";
import markedFootnote from "marked-footnote";
import DOMPurify from "dompurify";
import katex from "katex";
import hljs from "highlight.js";
import "katex/dist/katex.min.css";
import "./markdown.css";

export interface MarkdownNote {
  id: string;
  title: string;
  body: string;
}
export interface MarkdownContext {
  notes?: MarkdownNote[];
  currentId?: string;
  trail?: string[];
}
const escape = (value: string) =>
  value.replace(
    /[&<>"']/g,
    (char) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        char
      ]!,
  );
const slug = (value: string) =>
  value
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}_-]+/gu, "-");
let renderId = 0;
export function findNote(
  target: string,
  context: MarkdownContext,
): MarkdownNote | undefined {
  const name = target
    .split("#")[0]
    .replace(/\.md$/i, "")
    .replace(/^cards\//, "");
  return context.notes?.find((note) =>
    name
      ? note.id === name || note.title.toLowerCase() === name.toLowerCase()
      : note.id === context.currentId,
  );
}
function section(body: string, fragment: string): string {
  if (!fragment) return body;
  if (fragment.startsWith("^")) {
    const id = fragment.slice(1);
    const blocks = body.split(/\n\s*\n/);
    const index = blocks.findIndex((block) =>
      new RegExp(
        `(?:^|\\s)\\^${id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?:\\s|$)`,
      ).test(block),
    );
    if (index < 0) return "";
    return blocks[index].trim() === `^${id}`
      ? blocks[index - 1] || ""
      : blocks[index];
  }
  const lines = body.split("\n");
  let start = -1,
    depth = 0,
    end = lines.length;
  for (let i = 0; i < lines.length; i++) {
    const match = /^(#{1,6})\s+(.+?)\s*#*$/.exec(lines[i]);
    if (!match) continue;
    if (start < 0 && slug(match[2]) === slug(fragment)) {
      start = i;
      depth = match[1].length;
    } else if (start >= 0 && match[1].length <= depth) {
      end = i;
      break;
    }
  }
  return start < 0 ? "" : lines.slice(start, end).join("\n");
}

export interface MarkdownBlock {
  from: number;
  to: number;
  html: string;
}

export function renderObsidianMarkdown(
  value: string,
  context: MarkdownContext = {},
): string {
  return renderDocument(value, context);
}

/** Render with one lexer/reference table and ID namespace for the entire document. */
export function renderObsidianMarkdownBlocks(
  value: string,
  context: MarkdownContext = {},
): MarkdownBlock[] {
  const blocks: MarkdownBlock[] = [];
  renderDocument(value, context, blocks);
  return blocks;
}

function renderDocument(
  value: string,
  context: MarkdownContext,
  blocks?: MarkdownBlock[],
): string {
  const prefix = `md-${++renderId}-`;
  const inlineFootnotes: string[] = [];
  const parser = new Marked({ gfm: true, breaks: true });
  parser.use(markedFootnote({ prefixId: prefix }));
  const inline = (
    name: string,
    pattern: RegExp,
    start: RegExp,
    render: (match: RegExpExecArray) => string,
  ): TokenizerAndRendererExtension => ({
    name,
    level: "inline",
    start: (src) => src.search(start),
    tokenizer(src) {
      const match = pattern.exec(src);
      if (match) return { type: name, raw: match[0], match };
    },
    renderer(token) {
      return render(token.match);
    },
  });
  const parseInline = (text: string) =>
    parser.parseInline(text, { async: false }) as string;
  const wiki = (match: RegExpExecArray) => {
    const embed = !!match[1],
      target = match[2].trim(),
      alias = match[3]?.trim();
    const [path, fragment = ""] = target.split("#");
    const note = findNote(target, context);
    const label = alias || target;
    if (
      embed &&
      /\.(png|jpe?g|gif|webp|svg|avif|bmp|mp3|wav|ogg|m4a|mp4|webm|mov|pdf)$/i.test(
        path,
      )
    ) {
      const size = /^(\d+)(?:x(\d+))?$/.exec(alias || "");
      const attrs = size
        ? ` width="${size[1]}"${size[2] ? ` height="${size[2]}"` : ""}`
        : "";
      const remote = /^https?:\/\//i.test(path);
      const source = remote
        ? `src="${escape(target)}"`
        : `data-attachment="${escape(path)}"`;
      if (/\.(mp3|wav|ogg|m4a)$/i.test(path))
        return `<audio controls ${source} aria-label="${escape(path)}"></audio>`;
      if (/\.(mp4|webm|mov)$/i.test(path))
        return `<video controls ${source}${attrs}></video>`;
      if (/\.pdf$/i.test(path))
        return `<a class="internal-embed attachment-link" data-pdf="true" ${remote ? `href="${escape(target)}" target="_blank" rel="noopener noreferrer"` : `data-attachment-link="${escape(path)}" href="#"`}>PDF · ${escape(label)}</a>`;
      return `<img ${source} alt="${escape(size ? path : label)}"${attrs} loading="lazy">`;
    }
    const link = `<a class="internal-link${note ? "" : " is-unresolved"}" href="#${prefix}${slug(fragment || target)}" data-note-link="${escape(target)}">${escape(label)}</a>`;
    if (!embed) return link;
    if (!note)
      return `<span class="internal-embed is-unresolved">${link}<small>Note not found in this board</small></span>`;
    const trail = context.trail || [];
    if (trail.includes(note.id) || trail.length >= 5)
      return `<span class="internal-embed">${link}<small>Circular embed</small></span>`;
    const content = section(note.body, fragment);
    return `<span class="internal-embed"><span class="embed-title">${link}</span><span class="embed-content">${content ? renderObsidianMarkdown(content, { ...context, currentId: note.id, trail: [...trail, note.id] }) : "<small>Section not found</small>"}</span></span>`;
  };
  parser.use({
    extensions: [
      inline("inlineFootnote", /^\^\[([^\]\n]+)\]/, /\^\[/, (m) => {
        const n = inlineFootnotes.push(m[1]);
        return `<sup><a id="${prefix}inline-ref-${n}" href="#${prefix}inline-${n}">${n}</a></sup>`;
      }),
      inline(
        "tag",
        /^(?:#)([\p{L}_][\p{L}\p{N}_/-]*)(?![\p{L}\p{N}_])/u,
        /#(?=[\p{L}_])/u,
        (m) => `<span class="markdown-tag">#${escape(m[1])}</span>`,
      ),
      inline("comment", /^%%[\s\S]*?%%/, /%%/, () => ""),
      inline(
        "highlight",
        /^==(?=\S)([\s\S]*?\S)==/,
        /==/,
        (m) => `<mark>${parseInline(m[1])}</mark>`,
      ),
      inline(
        "wiki",
        /^(!?)\[\[([^\]\n|]+)(?:\|([^\]\n]*))?\]\]/,
        /!?\[\[/,
        wiki,
      ),
      inline("math", /^\$(?!\$)([^\s$](?:[^$]*?[^\s$])?)\$(?!\d)/, /\$/, (m) =>
        katex.renderToString(m[1], {
          throwOnError: false,
          trust: false,
          output: "htmlAndMathml",
        }),
      ),
      inline(
        "blockId",
        /^\^([\w-]+)(?=\s*$)/,
        /\^[\w-]+\s*$/,
        (m) =>
          `<span id="${prefix}${slug("^" + m[1])}" class="block-reference"></span>`,
      ),
      {
        name: "displayMath",
        level: "block",
        start: (src) => src.search(/^\$\$/m),
        tokenizer(src) {
          const m = /^\$\$\s*\n?([\s\S]+?)\n?\$\$(?:\n|$)/.exec(src);
          if (m) return { type: "displayMath", raw: m[0], text: m[1] };
        },
        renderer(token) {
          return katex.renderToString(token.text, {
            displayMode: true,
            throwOnError: false,
            trust: false,
            output: "htmlAndMathml",
          });
        },
      },
      {
        name: "blockComment",
        level: "block",
        start: (src) => src.search(/^%%/m),
        tokenizer(src) {
          const m = /^%%[\s\S]*?%%(?:\n|$)/.exec(src);
          if (m) return { type: "blockComment", raw: m[0] };
        },
        renderer() {
          return "";
        },
      },
    ],
    renderer: {
      heading({ tokens, depth, text }) {
        return `<h${depth} id="${prefix}${slug(text)}">${this.parser.parseInline(tokens)}</h${depth}>`;
      },
      listitem(token) {
        const match = /^\[([^\]\s])\]\s+/.exec(token.text);
        if (!token.task && match) {
          const status = match[1];
          const first = token.tokens[0];
          if (
            first &&
            first.type === "text" &&
            first.tokens?.[0]?.type === "text" &&
            first.tokens[0].raw.startsWith(match[0])
          ) {
            first.tokens[0].text = first.tokens[0].text.slice(match[0].length);
            const symbols: Record<string, string> = {
              "/": "◐",
              "-": "−",
              ">": "→",
              "<": "←",
              "?": "?",
              "!": "!",
              "*": "★",
              '"': "❝",
              l: "⌖",
              b: "▣",
              i: "i",
              S: "$",
              I: "☀",
              p: "+",
              c: "−",
              f: "♨",
              k: "⚿",
              w: "✓",
              u: "↑",
              d: "↓",
              "+": "+",
              B: "✦",
              a: "◷",
              n: "≡",
              R: "↻",
            };
            return `<li class="alternate-task"><span class="task-checkbox" data-task="${escape(status)}" role="img" aria-label="Task status ${escape(status)}">${escape(symbols[status] || status)}</span>${this.parser.parse(token.tokens)}</li>`;
          }
        }
        return false;
      },
      checkbox({ checked }) {
        return `<span class="task-checkbox${checked ? " is-checked" : ""}" role="checkbox" aria-checked="${!!checked}" aria-disabled="true" aria-label="${checked ? "Completed" : "Incomplete"} task"></span>`;
      },
      code({ text, lang }) {
        const language = (lang || "").split(/\s/)[0].toLowerCase();
        if (language === "mermaid")
          return `<pre class="mermaid-source" data-mermaid="${escape(encodeURIComponent(text))}"><code>${escape(text)}</code></pre>`;
        return `<pre><code class="hljs language-${escape(language)}">${language && hljs.getLanguage(language) ? hljs.highlight(text, { language, ignoreIllegals: true }).value : escape(text)}</code></pre>`;
      },
      blockquote(token) {
        const match = /^\[!([\w-]+)\]([+-])?(?:[ \t]+([^\n]*))?(?:\n|$)/.exec(
          token.text,
        );
        if (!match) return false;
        const type = match[1].toLowerCase(),
          title = match[3] || type.charAt(0).toUpperCase() + type.slice(1);
        const children = token.tokens.slice();
        const first = children[0];
        if (first?.type === "paragraph" && first.tokens) {
          let remaining = match[0].length;
          const bodyTokens = first.tokens.flatMap((part) => {
            if (!remaining) return [part];
            if (part.raw.length <= remaining) {
              remaining -= part.raw.length;
              return [];
            }
            const cut = remaining;
            remaining = 0;
            return [
              {
                ...part,
                raw: part.raw.slice(cut),
                ...("text" in part ? { text: part.text.slice(cut) } : {}),
              },
            ];
          });
          if (bodyTokens.length) children[0] = { ...first, tokens: bodyTokens };
          else children.shift();
        }
        const content = this.parser.parse(children);
        const heading = `<span class="callout-icon" aria-hidden="true">${({ warning: "⚠", danger: "⚠", error: "×", success: "✓", check: "✓", tip: "✦", question: "?", quote: "❝" } as Record<string, string>)[type] || "ⓘ"}</span>${parseInline(title)}`;
        return match[2]
          ? `<details class="callout" data-callout="${escape(type)}" ${match[2] === "+" ? "open" : ""}><summary class="callout-title">${heading}</summary><div class="callout-content">${content}</div></details>`
          : `<aside class="callout" data-callout="${escape(type)}"><div class="callout-title">${heading}</div><div class="callout-content">${content}</div></aside>`;
      },
      image({ href, title, text }) {
        const size = /\|(\d+)(?:x(\d+))?$/.exec(text);
        return `<img ${/^(https?:|\/)/i.test(href) ? `src="${escape(href)}"` : `data-attachment="${escape(href)}"`} alt="${escape(size ? text.slice(0, size.index) : text)}"${title ? ` title="${escape(title)}"` : ""}${size ? ` width="${size[1]}"${size[2] ? ` height="${size[2]}"` : ""}` : ""} loading="lazy">`;
      },
      link({ href, title, tokens }) {
        if (
          /^[a-z][a-z0-9+.-]*:/i.test(href) &&
          !/^(https?:|mailto:)/i.test(href)
        )
          return this.parser.parseInline(tokens);
        const external = /^(https?:|mailto:)/i.test(href);
        return `<a href="${escape(external ? href : href.startsWith("#") ? "#" + prefix + slug(decodeURIComponentSafe(href.slice(1))) : "#")}"${external ? ' target="_blank" rel="noopener noreferrer"' : href.startsWith("#") ? "" : ` class="internal-link" data-note-link="${escape(decodeURIComponentSafe(href))}"`}${title ? ` title="${escape(title)}"` : ""}>${this.parser.parseInline(tokens)}</a>`;
      },
    },
  });
  let html = "";
  if (blocks) {
    const tokens = parser.lexer(value);
    let offset = 0;
    const positions = new Map<
      (typeof tokens)[number],
      { from: number; to: number }
    >();
    for (const token of tokens) {
      // marked-footnote inserts a synthetic footer; it consumes no source.
      if (token.type === "footnotes") continue;
      const from = offset;
      offset += token.raw.length;
      positions.set(token, { from, to: offset });
    }
    if (parser.defaults.walkTokens)
      parser.walkTokens(tokens, parser.defaults.walkTokens);
    for (const token of tokens) {
      if (token.type === "space") continue;
      const position = positions.get(token) || {
        from: value.length,
        to: value.length,
      };
      const rendered = sanitizeMarkdown(parser.parser([token]));
      if (position.to > position.from || rendered)
        blocks.push({ ...position, html: rendered });
    }
  } else {
    html = parser.parse(value || "", { async: false }) as string;
  }
  if (inlineFootnotes.length) {
    const footer = `<section class="footnotes"><ol>${inlineFootnotes.map((text, i) => `<li id="${prefix}inline-${i + 1}">${parseInline(text)} <a href="#${prefix}inline-ref-${i + 1}" aria-label="Back to reference">↩</a></li>`).join("")}</ol></section>`;
    if (blocks)
      blocks.push({
        from: value.length,
        to: value.length,
        html: sanitizeMarkdown(footer),
      });
    else html += footer;
  }
  return sanitizeMarkdown(html);
}

function sanitizeMarkdown(html: string): string {
  // Sanitize after all extensions, including nested embeds, math, and raw HTML.
  const clean = DOMPurify.sanitize(html, {
    FORBID_TAGS: [
      "style",
      "script",
      "iframe",
      "object",
      "embed",
      "form",
      "input",
      "button",
    ],
    ADD_TAGS: ["annotation"],
    ADD_ATTR: ["target"],
    FORBID_ATTR: ["srcset"],
    ALLOW_DATA_ATTR: true,
  });
  const root = document.createElement("div");
  root.innerHTML = clean;
  root.querySelectorAll("a[href]").forEach((a) => {
    const href = a.getAttribute("href") || "";
    if (!/^(https?:|mailto:|#|\/)/i.test(href)) a.removeAttribute("href");
  });
  root.querySelectorAll("img[src],audio[src],video[src]").forEach((el) => {
    if (!/^(https?:|\/)/i.test(el.getAttribute("src") || ""))
      el.removeAttribute("src");
  });
  return root.innerHTML;
}
function decodeURIComponentSafe(value: string) {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

let diagramId = 0;
let mermaidModule: Promise<typeof import("mermaid")> | undefined;
export async function hydrateMarkdown(root: ParentNode): Promise<void> {
  const diagrams = Array.from(
    root.querySelectorAll<HTMLElement>("[data-mermaid]:not([data-rendering])"),
  );
  if (!diagrams.length) return;
  diagrams.forEach((el) => (el.dataset.rendering = "true"));
  mermaidModule ??= import("mermaid");
  try {
    const { default: mermaid } = await mermaidModule;
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      flowchart: { htmlLabels: false },
      theme: matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "neutral",
      suppressErrorRendering: true,
    });
    for (const el of diagrams) {
      if (!el.isConnected) continue;
      try {
        const { svg } = await mermaid.render(
          `diagram-${++diagramId}`,
          decodeURIComponent(el.dataset.mermaid!),
        );
        if (el.isConnected) {
          el.innerHTML = DOMPurify.sanitize(svg, {
            USE_PROFILES: { svg: true, svgFilters: true },
            ADD_TAGS: ["foreignObject"],
            ADD_ATTR: ["dominant-baseline"],
          });
          el.classList.add("mermaid-rendered");
        }
      } catch {
        el.setAttribute(
          "aria-label",
          "Invalid Mermaid diagram; showing source",
        );
      }
    }
  } catch {
    diagrams.forEach((el) =>
      el.setAttribute("aria-label", "Diagram unavailable; showing source"),
    );
  }
}
