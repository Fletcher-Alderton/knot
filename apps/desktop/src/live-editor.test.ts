import { afterEach, describe, expect, it } from "vitest";
import { EditorView } from "@codemirror/view";
import { undo, redo } from "@codemirror/commands";
import { createLiveEditor } from "./live-editor";
import { renderObsidianMarkdownBlocks } from "./markdown";

const views: EditorView[] = [];
function editor(value: string) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const changes: string[] = [];
  const view = createLiveEditor(parent, {
    value,
    render: (source) => renderObsidianMarkdownBlocks(source),
    hydrate: () => {},
    onChange: (source) => changes.push(source),
  });
  views.push(view);
  return { parent, view, changes };
}
afterEach(() => {
  views.splice(0).forEach((view) => {
    view.destroy();
    view.dom.parentElement?.remove();
  });
});

describe("in-place Markdown editor", () => {
  it("resolves fragment links against headings in other blocks", () => {
    const { parent } = editor("[Jump](#heading)\n\n# Heading");
    const link = parent.querySelector<HTMLAnchorElement>("a")!;
    expect(link.hash).toBe("#" + parent.querySelector("h1")!.id);
  });

  it("shares footnote numbering and one footer across blocks", () => {
    const { parent } = editor(
      "First[^a].\n\nSecond[^b] and again[^a].\n\n[^a]: Alpha note\n[^b]: Beta note",
    );
    expect(parent.querySelectorAll(".footnotes")).toHaveLength(1);
    const refs = Array.from(
      parent.querySelectorAll<HTMLAnchorElement>("sup a"),
    );
    expect(refs.map((link) => link.textContent)).toEqual(["1", "2", "1"]);
    for (const link of refs)
      expect(
        parent.querySelector(`[id="${link.hash.slice(1)}"]`),
      ).not.toBeNull();
  });

  it("keeps multiline math and comments together", () => {
    const { parent } = editor(
      "$$\nx +\n\ny\n$$\n\n%%\nhidden\n\nstill hidden\n%%\n\nVisible",
    );
    expect(parent.querySelector(".katex-display")).not.toBeNull();
    expect(parent.textContent).not.toContain("hidden");
    expect(parent.textContent).toContain("Visible");
  });

  it("does not resolve reference definitions inside fenced code", () => {
    const { parent } = editor(
      "[missing]\n\n```text\n[missing]: https://example.com\n```",
    );
    expect(parent.querySelector("a")).toBeNull();
  });

  it("preserves multiline reference titles across blocks", () => {
    const { parent } = editor(
      '[Link][ref]\n\n[ref]: https://example.com\n  "Reference title"',
    );
    expect(parent.querySelector("a")?.title).toBe("Reference title");
  });

  it("reveals only the block owning a boundary cursor", () => {
    const { parent, view } = editor("# One\n# Two\n");
    view.dispatch({ selection: { anchor: 6 } });
    view.focus();
    expect(parent.querySelectorAll("h1")).toHaveLength(1);
    expect(parent.querySelector("h1")?.textContent).toBe("One");
  });

  it("places clicks in the matching repeated formatted text", () => {
    const source = "**hello** and **hello**";
    const { parent, view } = editor(source);
    const second = parent.querySelectorAll("strong")[1];
    const range = document.createRange();
    range.setStart(second.firstChild!, 2);
    const previous = Object.getOwnPropertyDescriptor(
      document,
      "caretRangeFromPoint",
    );
    Object.defineProperty(document, "caretRangeFromPoint", {
      configurable: true,
      value: () => range,
    });
    try {
      second.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
      expect(view.state.selection.main.head).toBe(
        source.lastIndexOf("hello") + 2,
      );
    } finally {
      if (previous)
        Object.defineProperty(document, "caretRangeFromPoint", previous);
      else delete (document as Partial<Document>).caretRangeFromPoint;
    }
  });

  it("keeps selected Markdown editable and supports exact undo and redo", () => {
    const original = "# One\n\n**Two**";
    const { parent, view, changes } = editor(original);
    view.focus();
    view.dispatch({ selection: { anchor: 0, head: original.length } });
    expect(parent.querySelector(".live-markdown-block")).toBeNull();
    view.dispatch({
      changes: { from: 0, to: original.length, insert: "==Changed==" },
    });
    expect(changes.at(-1)).toBe("==Changed==");
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(original);
    expect(redo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("==Changed==");
  });
});
