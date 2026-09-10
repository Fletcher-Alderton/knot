import { EditorView } from "@codemirror/view";
import { describe, it, expect } from "vitest";
import { renderObsidianMarkdown as render } from "./markdown";
const dom = (source: string) => {
  const el = document.createElement("div");
  el.innerHTML = render(source);
  return el;
};
describe("Obsidian Markdown", () => {
  it("renders extensions without changing inline or fenced code", () => {
    const el = dom(
      "==**bright**== %%hidden%%\n\n`==literal== %%visible%% [[note]]`\n\n```text\n==literal==\n%%visible%%\n```",
    );
    expect(el.querySelector("mark strong")?.textContent).toBe("bright");
    expect(el.textContent).not.toContain("hidden");
    expect(el.querySelector("code")?.textContent).toContain("%%visible%%");
    expect(el.querySelector("pre code")?.textContent).toContain("==literal==");
  });
  it("supports nested and collapsible callouts", () => {
    const el = dom(
      "> [!warning]- **Careful**\n> First\n>\n> > [!tip] Hint\n> > ==Yes==",
    );
    expect(el.querySelector("details")?.hasAttribute("open")).toBe(false);
    expect(el.querySelector("summary strong")?.textContent).toBe("Careful");
    expect(el.querySelector("[data-callout=tip] mark")?.textContent).toBe(
      "Yes",
    );
  });
  it("supports tables, tasks, alternate tasks and strikethrough", () => {
    const el = dom(
      "| A | B |\n| - | - |\n| ==x== | ~~y~~ |\n\n- [x] Done\n- [ ] Todo\n- [/] Doing\n- [-] Cancelled",
    );
    expect(el.querySelector("table mark")).not.toBeNull();
    expect(el.querySelector("del")).not.toBeNull();
    expect(el.querySelectorAll(".task-checkbox")).toHaveLength(4);
    expect(el.querySelector('[data-task="/"]')).not.toBeNull();
  });
  it("renders math and language highlighting, and defers Mermaid", () => {
    const el = dom(
      "$x^2$\n\n$$\n\\frac{1}{2}\n$$\n\n```js\nconst n = 1;\n```\n\n```mermaid\ngraph TD\n A-->B\n```",
    );
    expect(el.querySelectorAll(".katex")).toHaveLength(2);
    expect(el.querySelector(".hljs-keyword")).not.toBeNull();
    expect(
      decodeURIComponent(
        el.querySelector("[data-mermaid]")?.getAttribute("data-mermaid") || "",
      ),
    ).toContain("A-->B");
  });
  it("renders reference and inline footnotes with unique targets across cards", () => {
    const a = dom(
      "> [!note] Callout\n> Before the footnote.\n\nA[^n] and ^[inline **note**]\n\n[^n]: A footnote",
    );
    const b = dom("B[^n]\n\n[^n]: Another");
    expect(a.querySelector(".footnotes")?.textContent).toContain("A footnote");
    expect(a.textContent).toContain("inline note");
    const ids = new Set(
      Array.from(a.querySelectorAll("[id]")).map((el) => el.id),
    );
    expect(
      Array.from(b.querySelectorAll("[id]")).some((el) => ids.has(el.id)),
    ).toBe(false);
  });
  it("resolves board notes and embeds sections and blocks without infinite recursion", () => {
    const notes = [
      {
        id: "a",
        title: "Alpha",
        body: "# Heading\n\nWanted ^block\n\n# Other\n\nUnwanted\n\n![[Alpha]]",
      },
    ];
    const el = document.createElement("div");
    el.innerHTML = render(
      "[[Alpha|Alias]]\n\n![[Alpha#Heading]]\n\n![[Alpha#^block]]",
      { notes },
    );
    expect(el.querySelector("a")?.textContent).toBe("Alias");
    expect(el.querySelectorAll(".embed-content")).toHaveLength(2);
    expect(el.textContent).toContain("Wanted");
    expect(el.textContent).not.toContain("Unwanted");
    expect(render("![[Alpha]]", { notes })).toContain("Circular embed");
  });
  it("preserves image sizing and exposes local media for scoped loading", () => {
    const el = dom(
      "![[assets/a.png|200x100]]\n\n![Alt|80](assets/b.png)\n\n![[clip.mp3]]",
    );
    expect(el.querySelector("img")?.getAttribute("width")).toBe("200");
    expect(el.querySelector("img")?.getAttribute("height")).toBe("100");
    expect(el.querySelectorAll("[data-attachment]")).toHaveLength(3);
  });
  it("sanitizes raw HTML, unsafe protocols, embeds and math", () => {
    const el = dom(
      '<script>alert(1)</script>\n\n<img src="x" onerror="alert(1)"> <iframe src="https://example.com"></iframe>\n\n[x](javascript:alert(1))\n\n$\\href{javascript:alert(1)}{click}$',
    );
    expect(el.querySelector("script,iframe,[onerror]")).toBeNull();
    expect(el.querySelector('[href^="javascript:"]')).toBeNull();
  });
  it("handles escaped syntax and incomplete input while typing", () => {
    expect(
      dom("\\==literal== \\[[note]] %%unfinished").querySelector("mark,a"),
    ).toBeNull();
    expect(() => render("$unfinished\n\n> [!note]\n> Text")).not.toThrow();
  });
});

describe("editor integration", () => {
  it("edits one live surface and saves exact Markdown", async () => {
    const {
      state,
      render: renderApp,
      setInvokeForTests,
    } = await import("./main");
    state.boardPath = "/board";
    state.view = "board";
    state.columns = [{ id: "todo", name: "Todo" }];
    state.cards = [
      {
        id: "edit",
        title: "Edit",
        body: "==Original==",
        column: "todo",
        labels: [],
        updatedAt: 0,
      },
    ];
    state.selected = "edit";
    state.error = "";
    renderApp();
    const source = document.querySelector<HTMLTextAreaElement>("#body")!;
    const markdown = "> [!tip] Draft\n> ==Changed==\n\n`%%literal%%`";
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor')!)!;
    view.dispatch({changes:{from:0,to:view.state.doc.length,insert:markdown}});
    expect(document.querySelector('#card-live-editor mark')?.textContent).toBe('Changed');
    expect(document.querySelector('[data-md-mode],.ce-preview')).toBeNull();
    expect(source.hidden).toBe(true);
    expect(source.value).toBe(markdown);
    let saved = "";
    setInvokeForTests(async (_command, args) => {
      saved = (args!.input as { body: string }).body;
      return { id: "edit", ...(args!.input as object) } as never;
    });
    document.querySelector<HTMLButtonElement>("#save")!.click();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(saved).toBe(markdown);
    expect(document.querySelector(".card-body mark")?.textContent).toBe(
      "Changed",
    );
    setInvokeForTests(null);
  });
});
