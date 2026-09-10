export interface OrderedCard {
  id: string;
  column: string;
  position?: number;
}
export function compareCardOrder(a: OrderedCard, b: OrderedCard): number {
  return (a.position ?? 0) - (b.position ?? 0) || a.id.localeCompare(b.id);
}
export interface CardMove {
  id: string;
  column: string;
  position: number;
}

/** Insert before a destination card (null = end), without including the source as a neighbor. */
export function planCardMove(
  cards: readonly OrderedCard[],
  id: string,
  column: string,
  beforeId: string | null,
): CardMove[] {
  const source = cards.find((card) => card.id === id);
  if (!source || beforeId === id) return [];
  const current = cards
    .filter((card) => card.column === column)
    .sort(compareCardOrder);
  const destination = current.filter((card) => card.id !== id);
  const index =
    beforeId === null
      ? destination.length
      : destination.findIndex((card) => card.id === beforeId);
  if (index < 0) return [];
  destination.splice(index, 0, source);
  if (
    source.column === column &&
    current.every((card, i) => card.id === destination[i].id)
  )
    return [];
  const before = index > 0 ? (destination[index - 1].position ?? 0) : undefined;
  const after =
    index < destination.length - 1
      ? (destination[index + 1].position ?? 0)
      : undefined;
  const position =
    before === undefined
      ? after === undefined
        ? 1024
        : after - 1024
      : after === undefined
        ? before + 1024
        : Math.floor(before / 2 + after / 2);
  if (
    Number.isSafeInteger(position) &&
    (before === undefined ||
      (Number.isSafeInteger(before) && position > before)) &&
    (after === undefined || (Number.isSafeInteger(after) && position < after))
  ) {
    return [{ id, column, position }];
  }
  // Imported/default ranks may tie, or repeated midpoints can exhaust integer space.
  // Only those cases need multiple writes; ordinary moves leave every other card alone.
  return destination.flatMap((card, i) => {
    const position = (i + 1) * 1024;
    return card.column === column && card.position === position
      ? []
      : [{ id: card.id, column, position }];
  });
}
