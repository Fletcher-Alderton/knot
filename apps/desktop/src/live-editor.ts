import { EditorState, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  WidgetType,
  keymap,
  type DecorationSet,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownKeymap } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { type MarkdownBlock } from "./markdown";

interface Options {
  value: string;
  render: (document: string) => MarkdownBlock[];
  hydrate: (root: ParentNode) => void;
  onChange: (source: string) => void;
}
const focusChanged = StateEffect.define<boolean>();

/** Markdown stays the document model. Only inactive blocks are replaced visually. */
export function createLiveEditor(
  parent: HTMLElement,
  options: Options,
): EditorView {
  class RenderedBlock extends WidgetType {
    constructor(
      readonly source: string,
      readonly from: number,
      readonly to: number,
      readonly html: string,
    ) {
      super();
    }
    eq(other: RenderedBlock) {
      return (
        this.source === other.source &&
        this.from === other.from &&
        this.to === other.to &&
        this.html === other.html
      );
    }
    toDOM(view: EditorView) {
      const dom = document.createElement("div");
      dom.className = "live-markdown-block markdown-rendered";
      dom.innerHTML = this.html;
      dom.addEventListener("mousedown", (event) => {
        const target = event.target as HTMLElement;
        if (
          target.closest("summary,audio,video") ||
          ((event.metaKey || event.ctrlKey) && target.closest("a"))
        )
          return;
        event.preventDefault();
        // Locate the clicked text in the source before revealing its Markdown.
        // This keeps clicking a rendered sentence close to its original caret position.
        const range = document.caretRangeFromPoint?.(
          event.clientX,
          event.clientY,
        );
        let position = this.from;
        if (range && dom.contains(range.startContainer)) {
          // Walk preceding text nodes too: repeated words in separate emphasis
          // spans must map to successive source occurrences, not always the first.
          const walker = document.createTreeWalker(dom, NodeFilter.SHOW_TEXT);
          let cursor = 0;
          for (let node = walker.nextNode(); node; node = walker.nextNode()) {
            const text = node.textContent || "";
            const found = text.trim() ? this.source.indexOf(text, cursor) : -1;
            if (found >= 0) {
              if (node === range.startContainer) {
                position += found + range.startOffset;
                break;
              }
              cursor = found + text.length;
            }
            if (node === range.startContainer) break;
          }
        }
        view.dispatch({
          selection: { anchor: Math.min(this.to, position) },
          effects: focusChanged.of(true),
          scrollIntoView: true,
        });
        view.focus();
      });
      // Widgets are attached after toDOM returns; hydration needs connected nodes.
      requestAnimationFrame(() => {
        if (dom.isConnected) options.hydrate(dom);
      });
      return dom;
    }
    ignoreEvent() {
      return true;
    }
  }
  function decorations(
    state: EditorState,
    focused: boolean,
    blocks: MarkdownBlock[],
  ): DecorationSet {
    const ranges = [];
    for (const { from, to, html } of blocks) {
      const widget = new RenderedBlock(
        state.doc.sliceString(from, to),
        from,
        to,
        html,
      );
      if (from === to) {
        ranges.push(
          Decoration.widget({ widget, block: true, side: 1 }).range(from),
        );
        continue;
      }
      // Half-open block ranges give a boundary cursor exactly one owner.
      const active =
        focused &&
        state.selection.ranges.some((range) =>
          range.empty
            ? range.from >= from &&
              (range.from < to ||
                (to === state.doc.length && range.from === to))
            : range.from < to && range.to > from,
        );
      if (!active)
        ranges.push(
          Decoration.replace({ widget, block: true }).range(from, to),
        );
    }
    return Decoration.set(ranges, true);
  }
  const rendered = StateField.define<{
    focused: boolean;
    blocks: MarkdownBlock[];
    decorations: DecorationSet;
  }>({
    create(state) {
      const blocks = options.render(state.doc.toString());
      return {
        focused: false,
        blocks,
        decorations: decorations(state, false, blocks),
      };
    },
    update(value, tr) {
      let focused = value.focused;
      for (const effect of tr.effects)
        if (effect.is(focusChanged)) focused = effect.value;
      if (tr.docChanged || tr.selection || focused !== value.focused) {
        const blocks = tr.docChanged
          ? options.render(tr.state.doc.toString())
          : value.blocks;
        return {
          focused,
          blocks,
          decorations: decorations(tr.state, focused, blocks),
        };
      }
      return value;
    },
    provide: (field) =>
      EditorView.decorations.from(field, (value) => value.decorations),
  });
  return new EditorView({
    parent,
    state: EditorState.create({
      doc: options.value,
      extensions: [
        markdown(),
        history(),
        keymap.of([...markdownKeymap, ...defaultKeymap, ...historyKeymap]),
        EditorView.lineWrapping,
        syntaxHighlighting(
          HighlightStyle.define([
            { tag: tags.heading1, class: "live-heading live-h1" },
            { tag: tags.heading2, class: "live-heading live-h2" },
            { tag: tags.heading3, class: "live-heading live-h3" },
            { tag: tags.heading, class: "live-heading" },
            { tag: tags.strong, fontWeight: "700" },
            { tag: tags.emphasis, fontStyle: "italic" },
            { tag: tags.strikethrough, textDecoration: "line-through" },
            { tag: tags.monospace, class: "live-code" },
            { tag: tags.link, class: "live-link" },
            { tag: tags.processingInstruction, class: "live-syntax" },
          ]),
        ),
        rendered,
        EditorView.contentAttributes.of({
          "aria-label": "Markdown body",
          spellcheck: "true",
          "data-live-editor": "true",
        }),
        EditorView.domEventHandlers({
          focus(_event, view) {
            view.dispatch({ effects: focusChanged.of(true) });
          },
          blur(_event, view) {
            view.dispatch({ effects: focusChanged.of(false) });
          },
        }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) options.onChange(update.state.doc.toString());
        }),
        EditorView.theme({
          "&": {
            backgroundColor: "transparent",
            color: "var(--text)",
            minHeight: "320px",
          },
          "&.cm-focused": { outline: "none" },
          ".cm-scroller": {
            fontFamily: "inherit",
            lineHeight: "1.65",
            overflow: "visible",
          },
          ".cm-content": {
            padding: "0",
            minHeight: "320px",
            caretColor: "var(--text)",
          },
          ".cm-line": { padding: "0" },
          ".cm-cursor": { borderLeftColor: "var(--text)" },
          ".cm-selectionBackground": {
            backgroundColor:
              "color-mix(in srgb, var(--text) 15%, transparent) !important",
          },
        }),
      ],
    }),
  });
}
