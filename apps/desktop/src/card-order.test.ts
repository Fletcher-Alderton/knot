import { describe, expect, it } from "vitest";
import { compareCardOrder, planCardMove, type OrderedCard } from "./card-order";
const cards = (
  positions: (number | undefined)[] = [1000, 2000, 3000],
): OrderedCard[] =>
  positions.map((position, i) => ({
    id: String.fromCharCode(97 + i),
    column: "todo",
    position,
  }));
const apply = (items: OrderedCard[], moves: ReturnType<typeof planCardMove>) =>
  items.map((card) => ({
    ...card,
    ...moves.find((move) => move.id === card.id),
  }));
const order = (items: OrderedCard[], column = "todo") =>
  items
    .filter((card) => card.column === column)
    .sort(compareCardOrder)
    .map((card) => card.id);

describe("stable card positions", () => {
  it("uses immutable IDs to break equal or missing ranks", () => {
    expect(order(cards([0, undefined, -10]).reverse())).toEqual([
      "c",
      "a",
      "b",
    ]);
  });
  it.each([
    ["a", "c", ["b", "a", "c"], 2500],
    ["a", null, ["b", "c", "a"], 4024],
    ["b", "a", ["b", "a", "c"], -24],
    ["c", "a", ["c", "a", "b"], -24],
    ["c", "b", ["a", "c", "b"], 1500],
  ] as const)(
    "moves %s before %s with one write",
    (id, before, expected, position) => {
      const source = cards();
      const moves = planCardMove(source, id, "todo", before);
      expect(moves).toEqual([{ id, column: "todo", position }]);
      expect(order(apply(source, moves))).toEqual(expected);
      expect(source).toEqual(cards());
    },
  );
  it.each([
    ["a", "b"],
    ["b", "c"],
    ["c", null],
    ["a", "a"],
    ["missing", "a"],
    ["a", "missing"],
  ] as const)("ignores no-op/invalid move %s before %s", (id, before) => {
    expect(planCardMove(cards(), id, "todo", before)).toEqual([]);
  });
  it("rejects a before-card from another column", () => {
    expect(
      planCardMove(
        [...cards(), { id: "other", column: "done", position: 1000 }],
        "a",
        "todo",
        "other",
      ),
    ).toEqual([]);
  });
  it("inserts into another column without rewriting its neighbors", () => {
    const source = [
      ...cards(),
      { id: "other", column: "done", position: 1000 },
    ];
    expect(planCardMove(source, "other", "todo", "b")).toEqual([
      { id: "other", column: "todo", position: 1500 },
    ]);
    expect(planCardMove(source, "a", "empty", null)).toEqual([
      { id: "a", column: "empty", position: 1024 },
    ]);
  });
  it.each([
    [1000, 1000, 1000],
    [1, 2, 3],
    [0, undefined, 0],
  ])(
    "rebalances only when the requested gap is exhausted (%j)",
    (...positions) => {
      const source = cards(positions);
      const moves = planCardMove(source, "c", "todo", "b");
      expect(order(apply(source, moves))).toEqual(["a", "c", "b"]);
      expect(moves.every((move) => Number.isSafeInteger(move.position))).toBe(
        true,
      );
    },
  );
  it("allows zero as a midpoint and negative ranks at the top", () => {
    expect(planCardMove(cards([-10, 10, 20]), "c", "todo", "b")).toEqual([
      { id: "c", column: "todo", position: 0 },
    ]);
    expect(planCardMove(cards([0, 100]), "b", "todo", "a")).toEqual([
      { id: "b", column: "todo", position: -1024 },
    ]);
  });
  it.each([Number.MAX_SAFE_INTEGER, Number.MIN_SAFE_INTEGER])(
    "rebalance keeps extreme ranks safe (%s)",
    (position) => {
      const source = cards([position, position]);
      const before = position > 0 ? null : "a",
        id = position > 0 ? "a" : "b";
      const moves = planCardMove(source, id, "todo", before);
      expect(moves.every((move) => Number.isSafeInteger(move.position))).toBe(
        true,
      );
      expect(order(apply(source, moves))).toEqual(["b", "a"]);
    },
  );
  it("omits unchanged neighbors from a rebalance", () => {
    const moves = planCardMove(cards([1024, 1024, 1024]), "c", "todo", "b");
    expect(moves.map((move) => move.id)).toEqual(["c", "b"]);
  });
  it("matches every requested insertion through repeated gap exhaustion", () => {
    let source = cards([0, 0, 0, 0, 0]);
    for (let round = 0; round < 20; round++)
      for (const id of ["a", "b", "c", "d", "e"])
        for (const before of ["a", "b", "c", "d", "e", null]) {
          if (before === id) continue;
          const expected = order(source).filter((other) => other !== id);
          expected.splice(
            before === null ? expected.length : expected.indexOf(before),
            0,
            id,
          );
          source = apply(source, planCardMove(source, id, "todo", before));
          expect(order(source)).toEqual(expected);
        }
  });
});
