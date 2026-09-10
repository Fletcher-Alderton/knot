import { afterEach, describe, expect, it, vi } from "vitest";
import { bindCardDragging } from "./card-drag";

let cleanup = () => {};
afterEach(() => {
  cleanup();
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  vi.useRealTimers();
});
function rect(left: number, top: number, width: number, height: number) {
  return {
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    x: left,
    y: top,
    toJSON() {
      return this;
    },
  };
}
function setup() {
  const root = document.createElement("div");
  document.body.append(root);
  root.innerHTML =
    '<div class="board-scroll"><div class="columns"><section class="column"><div data-drop-column="todo"><article class="card" data-id="a" tabindex="0">A <a href="#">link</a></article><article class="card" data-id="b" tabindex="0">B</article><article class="card" data-id="c" tabindex="0">C</article></div></section><section class="column"><div data-drop-column="done"></div></section></div></div>';
  const scroller = root.querySelector<HTMLElement>(".board-scroll")!;
  vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue(
    rect(0, 0, 400, 300),
  );
  const zones = Array.from(
    root.querySelectorAll<HTMLElement>("[data-drop-column]"),
  );
  zones.forEach((zone, i) =>
    vi
      .spyOn(zone, "getBoundingClientRect")
      .mockReturnValue(rect(i * 200, 0, 190, 300)),
  );
  const cards = Array.from(root.querySelectorAll<HTMLElement>(".card"));
  cards.forEach((card, i) =>
    vi
      .spyOn(card, "getBoundingClientRect")
      .mockReturnValue(rect(10, 20 + i * 80, 170, 60)),
  );
  Object.defineProperty(document, "elementFromPoint", {
    configurable: true,
    value: vi.fn((x: number) => (x < 200 ? zones[0] : zones[1])),
  });
  const options = { open: vi.fn(), move: vi.fn(), shift: vi.fn() };
  cleanup = bindCardDragging(root, options);
  const pointer = (
    type: string,
    x: number,
    y: number,
    target: EventTarget = document,
    buttons = 1,
  ) => {
    const event = new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      clientX: x,
      clientY: y,
      button: 0,
      buttons,
    });
    Object.defineProperty(event, "pointerId", { value: 1 });
    target.dispatchEvent(event);
  };
  const start = () => pointer("pointerdown", 40, 40, cards[0]);
  return { root, scroller, zones, cards, options, pointer, start };
}

describe("card drag interactions", () => {
  it("keeps clicks below the movement threshold as clicks", () => {
    const { start, pointer, cards, options } = setup();
    start();
    pointer("pointermove", 42, 42);
    pointer("pointerup", 42, 42);
    cards[0].click();
    expect(options.open).toHaveBeenCalledWith("a");
    expect(options.move).not.toHaveBeenCalled();
    expect(document.querySelector(".drag-ghost")).toBeNull();
  });
  it("reuses the gap preview and drops at the indicated slot", () => {
    const { start, pointer, options, cards } = setup();
    start();
    pointer("pointermove", 40, 170);
    const marker = document.querySelector<HTMLElement>(".card-drop-indicator")!;
    expect(marker).not.toBeNull();
    expect(marker.style.top).toBe("180px");
    expect(cards[2].classList.contains("card-drop-shift")).toBe(true);
    expect(cards[1].classList.contains("card-drop-shift")).toBe(false);
    expect(cards[2].parentElement?.classList.contains("has-drop-gap")).toBe(
      true,
    );
    pointer("pointermove", 40, 170);
    expect(document.querySelector(".card-drop-indicator")).toBe(marker);
    expect(
      Array.from(
        cards[0].parentElement!.querySelectorAll(".card"),
        (el) => (el as HTMLElement).dataset.id,
      ),
    ).toEqual(["a", "b", "c"]);
    pointer("pointerup", 40, 170);
    expect(options.move).toHaveBeenCalledWith("a", "todo", "c");
    expect(
      document.querySelector(".drag-ghost,.card-drop-indicator,.dragging"),
    ).toBeNull();
  });
  it("keeps the gap stable while neighbors animate and the pointer enters it", () => {
    const { start, pointer, cards, zones, options } = setup();
    let progress = 0;
    cards.forEach((card, i) =>
      vi
        .spyOn(card, "getBoundingClientRect")
        .mockImplementation(() =>
          rect(
            10,
            20 +
              i * 80 +
              (card.classList.contains("card-drop-shift") ? 76 * progress : 0),
            170,
            60,
          ),
        ),
    );
    start();
    pointer("pointermove", 40, 170);
    const marker = document.querySelector<HTMLElement>(".card-drop-indicator")!;
    for (progress of [0.25, 0.5, 1]) {
      pointer("pointermove", 40, 220);
      expect(marker.style.top).toBe("180px");
      expect(cards[2].classList.contains("card-drop-shift")).toBe(true);
    }
    pointer("pointerup", 40, 220);
    expect(options.move).toHaveBeenCalledWith("a", "todo", "c");
    expect(cards[2].classList.contains("card-drop-shift")).toBe(false);
    expect(zones[0].classList.contains("has-drop-gap")).toBe(false);
    expect(zones[0].style.getPropertyValue("--card-drop-gap")).toBe("");
  });
  it("closes the previous column's gap when moving to an empty column", () => {
    const { start, pointer, cards, zones, options } = setup();
    start();
    pointer("pointermove", 40, 170);
    pointer("pointermove", 240, 70);
    expect(
      cards.every((card) => !card.classList.contains("card-drop-shift")),
    ).toBe(true);
    expect(zones[0].classList.contains("has-drop-gap")).toBe(false);
    expect(zones[1].classList.contains("has-drop-gap")).toBe(true);
    pointer("pointerup", 240, 70);
    expect(options.move).toHaveBeenCalledWith("a", "done", null);
    expect(zones[1].classList.contains("has-drop-gap")).toBe(false);
  });
  it("uses the same stable gap for native drag-and-drop", () => {
    const { cards, zones, options } = setup();
    cards[0].dispatchEvent(
      new Event("dragstart", { bubbles: true, cancelable: true }),
    );
    zones[0].dispatchEvent(
      new MouseEvent("dragover", {
        bubbles: true,
        cancelable: true,
        clientY: 170,
      }),
    );
    expect(cards[2].classList.contains("card-drop-shift")).toBe(true);
    zones[0].dispatchEvent(
      new MouseEvent("drop", { bubbles: true, cancelable: true, clientY: 220 }),
    );
    expect(options.move).toHaveBeenCalledWith("a", "todo", "c");
    expect(
      document.querySelector(
        ".has-drop-gap,.card-drop-shift,.card-drop-indicator",
      ),
    ).toBeNull();
  });
  it.each([
    [40, 70, "start"],
    [40, 170, "between"],
    [40, 270, "end"],
    [240, 70, "empty"],
  ] as const)("anchors the pulse overlay at %s,%s as %s", (x, y, placement) => {
    const { start, pointer } = setup();
    start();
    pointer("pointermove", x, y);
    expect(
      document.querySelector<HTMLElement>(".card-drop-indicator")?.dataset
        .placement,
    ).toBe(placement);
  });
  it.each(["escape", "cancel", "blur", "dispose"])(
    "cancels and cleans up on %s",
    (kind) => {
      const { start, pointer, options } = setup();
      start();
      pointer("pointermove", 40, 170);
      if (kind === "escape")
        document.dispatchEvent(
          new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
        );
      if (kind === "cancel") pointer("pointercancel", 40, 170);
      if (kind === "blur") window.dispatchEvent(new Event("blur"));
      if (kind === "dispose") cleanup();
      pointer("pointerup", 40, 170);
      expect(options.move).not.toHaveBeenCalled();
      expect(
        document.querySelector(".drag-ghost,.card-drop-indicator,.dragging"),
      ).toBeNull();
    },
  );
  it("does not persist the last valid slot when released off the board", () => {
    const { start, pointer, options } = setup();
    start();
    pointer("pointermove", 40, 170);
    pointer("pointerup", 500, 170);
    expect(options.move).not.toHaveBeenCalled();
    expect(document.querySelector(".drag-ghost")).toBeNull();
  });
  it("ignores another pointer and cancels a lost primary button", () => {
    const { start, pointer, options } = setup();
    start();
    pointer("pointermove", 40, 170);
    document.dispatchEvent(
      new MouseEvent("pointerup", { bubbles: true, clientX: 40, clientY: 170 }),
    );
    expect(document.querySelector(".drag-ghost")).not.toBeNull();
    pointer("pointermove", 40, 170, document, 0);
    pointer("pointerup", 40, 170);
    expect(options.move).not.toHaveBeenCalled();
    expect(document.querySelector(".drag-ghost")).toBeNull();
  });
  it("scrolls near the viewport edge while held still and stops after cancellation", async () => {
    vi.useFakeTimers();
    const { start, pointer, scroller } = setup();
    start();
    pointer("pointermove", 40, 290);
    await vi.advanceTimersByTimeAsync(80);
    expect(scroller.scrollTop).toBeGreaterThan(0);
    pointer("pointercancel", 40, 290);
    const top = scroller.scrollTop;
    await vi.advanceTimersByTimeAsync(80);
    expect(scroller.scrollTop).toBe(top);
  });
  it("allows the next deliberate click after a drag", async () => {
    vi.useFakeTimers();
    const { start, pointer, cards, options } = setup();
    start();
    pointer("pointermove", 40, 170);
    pointer("pointerup", 40, 170);
    cards[0].click();
    expect(options.open).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    cards[1].click();
    expect(options.open).toHaveBeenCalledWith("b");
  });
  it("supports keyboard movement and leaves card links alone", () => {
    const { cards, options, pointer } = setup();
    cards[0].dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ArrowUp",
        altKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    cards[0].dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ArrowDown",
        altKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(options.shift.mock.calls).toEqual([
      ["a", "up"],
      ["a", "down"],
    ]);
    const link = cards[0].querySelector("a")!;
    pointer("pointerdown", 40, 40, link);
    pointer("pointermove", 40, 170);
    pointer("pointerup", 40, 170);
    link.click();
    expect(options.move).not.toHaveBeenCalled();
    expect(options.open).not.toHaveBeenCalled();
  });
});
