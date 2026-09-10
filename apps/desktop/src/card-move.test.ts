import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  state,
  render,
  moveCardToSlot,
  setInvokeForTests,
  type BackendCard,
} from "./main";

beforeEach(() => {
  Object.assign(state, {
    boardPath: "/card-move-tests",
    view: "board",
    selected: null,
    dialog: null,
    quickAddOpen: false,
    showArchived: false,
    drafts: {},
    error: "",
    columns: [
      { id: "todo", name: "Todo" },
      { id: "done", name: "Done" },
    ],
  });
  state.cards = ["a", "b", "c"].map((id, i) => ({
    id,
    title: id.toUpperCase(),
    body: "Body " + id,
    column: "todo",
    position: (i + 1) * 1000,
    labels: [],
    updatedAt: 0,
  }));
  render();
});
afterEach(() => {
  setInvokeForTests(null);
  state.selected = null;
  render();
});
const order = () =>
  Array.from(
    document.querySelectorAll<HTMLElement>('[data-drop-column="todo"] > .card'),
    (card) => card.dataset.id,
  );
function backend() {
  const stored = new Map<string, BackendCard>(
    state.cards.map((card) => [card.id, { ...card }]),
  );
  const invoke = vi.fn(async (command: string, args: any) => {
    if (command === "list_cards") return [...stored.values()];
    if (command === "move_card") {
      const saved = {
        ...stored.get(args.id)!,
        column: args.column,
        position: args.position,
      };
      stored.set(args.id, saved);
      return saved;
    }
    throw Error(command);
  });
  setInvokeForTests(invoke as any);
  return { stored, invoke };
}

describe("card movement persistence", () => {
  it("moves downward using neighbors that exclude the source", async () => {
    const { invoke } = backend();
    await moveCardToSlot("a", "todo", "c");
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("move_card", {
      path: "/card-move-tests",
      id: "a",
      column: "todo",
      position: 2500,
    });
    expect(order()).toEqual(["b", "a", "c"]);
    expect(document.activeElement?.getAttribute("data-id")).toBe("a");
    expect(document.querySelector('.board-move-status')?.textContent).toContain(
      "position 2 of 3",
    );
  });
  it("does not write when dropped back in the original slot", async () => {
    const { invoke } = backend();
    await moveCardToSlot("a", "todo", "b");
    expect(invoke).not.toHaveBeenCalled();
  });
  it("persists a precise middle insertion even with equal imported ranks", async () => {
    state.cards.forEach((card) => (card.position = 1000));
    render();
    const { stored } = backend();
    await moveCardToSlot("c", "todo", "b");
    expect(order()).toEqual(["a", "c", "b"]);
    expect(
      [...stored.values()]
        .sort((a, b) => a.position! - b.position!)
        .map((card) => card.id),
    ).toEqual(["a", "c", "b"]);
    expect([...stored.values()].map((card) => card.body)).toEqual([
      "Body a",
      "Body b",
      "Body c",
    ]);
  });
  it("reloads actual persisted ranks after a partial rebalance failure", async () => {
    state.cards.forEach((card) => (card.position = 1000));
    const { stored } = backend();
    let writes = 0;
    setInvokeForTests((async (command: string, args: any) => {
      if (command === "list_cards") return [...stored.values()];
      if (++writes === 2) throw Error("disk full");
      const saved = { ...stored.get(args.id)!, position: args.position };
      stored.set(args.id, saved);
      return saved;
    }) as any);
    await moveCardToSlot("c", "todo", "b");
    expect(state.error).toContain("disk full");
    expect(state.cards.map((card) => card.position)).toEqual(
      [...stored.values()].map((card) => card.position),
    );
    expect(state.cards[0].position).toBe(1024); // first write really succeeded
  });
  it("does not apply a delayed move response to another board", async () => {
    let complete!: (value: BackendCard) => void;
    setInvokeForTests(
      (() =>
        new Promise((resolve) => {
          complete = resolve;
        })) as any,
    );
    const moving = moveCardToSlot("a", "todo", "c");
    state.boardPath = "/other";
    state.cards = [
      {
        id: "a",
        title: "Other",
        body: "Untouched",
        column: "todo",
        labels: [],
        updatedAt: 0,
      },
    ];
    render();
    complete({
      id: "a",
      title: "A",
      body: "Old board",
      column: "todo",
      position: 2500,
    });
    await moving;
    expect(state.cards[0].body).toBe("Untouched");
  });
  it("does not allow overlapping moves while persistence is pending", async () => {
    let complete!: (value: BackendCard) => void;
    const invoke = vi.fn(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    setInvokeForTests(invoke as any);
    const moving = moveCardToSlot("a", "todo", "c");
    await moveCardToSlot("b", "todo", null);
    expect(invoke).toHaveBeenCalledTimes(1);
    complete({
      id: "a",
      title: "A",
      body: "Body a",
      column: "todo",
      position: 2500,
    });
    await moving;
  });
  it("supports Alt+Down and keeps keyboard focus on the moved card", async () => {
    backend();
    const card = document.querySelector<HTMLElement>('[data-id="a"]')!;
    card.focus();
    card.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ArrowDown",
        altKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() => expect(order()).toEqual(["b", "a", "c"]));
    await vi.waitFor(() =>
      expect(document.activeElement?.getAttribute("data-id")).toBe("a"),
    );
  });
});
