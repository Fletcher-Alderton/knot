interface DragOptions {
  open: (id: string) => void;
  move: (id: string, column: string, beforeId: string | null) => void;
  shift: (id: string, direction: "up" | "down" | "left" | "right") => void;
  canDrag?: () => boolean;
}
interface Slot {
  zone: HTMLElement;
  beforeId: string | null;
}
interface Gesture {
  card: HTMLElement;
  id: string;
  pointerId: number;
  startX: number;
  startY: number;
  x: number;
  y: number;
  offsetX: number;
  offsetY: number;
  started: boolean;
}
const interactive =
  'button,a,summary,audio,video,input,select,textarea,[contenteditable="true"]';

/** One drag controller per board render. Preview animation never drives hit-testing. */
export function bindCardDragging(
  root: HTMLElement,
  options: DragOptions,
): () => void {
  const removeListeners: (() => void)[] = [];
  const listen = (
    target: EventTarget,
    type: string,
    handler: EventListener,
  ) => {
    target.addEventListener(type, handler);
    removeListeners.push(() => target.removeEventListener(type, handler));
  };
  let gesture: Gesture | null = null;
  let nativeId: string | null = null;
  let ghost: HTMLElement | null = null;
  let marker: HTMLElement | null = null;
  let slot: Slot | null = null;
  const gap = 76; // 68px pulsing box and the usual 8px card spacing.
  const geometry = new Map<HTMLElement, { top: number; height: number }>();
  const shifted = new Set<HTMLElement>();
  let previewZone: HTMLElement | null = null;
  let frame = 0;
  let suppressClick = false;
  let clickTimer: ReturnType<typeof setTimeout> | undefined;
  const scroller = root.querySelector<HTMLElement>(".board-scroll");
  const cardsIn = (zone: HTMLElement, id: string) =>
    Array.from(zone.children).filter(
      (el): el is HTMLElement =>
        el instanceof HTMLElement &&
        el.matches(".card") &&
        el.dataset.id !== id,
    );
  const unshiftedRect = (card: HTMLElement, zone: HTMLElement) => {
    let measured = geometry.get(card);
    const zoneTop = zone.getBoundingClientRect().top - zone.scrollTop;
    if (!measured) {
      const rect = card.getBoundingClientRect();
      measured = {
        // offsetTop ignores even an unfinished return animation from a prior drag.
        top:
          card.offsetParent === zone
            ? card.offsetTop + zone.clientTop
            : rect.top - zoneTop,
        height: rect.height,
      };
      geometry.set(card, measured);
    }
    return { top: zoneTop + measured.top, height: measured.height };
  };
  const findSlot = (zone: HTMLElement, id: string, y: number): Slot => {
    // The visible gap belongs to its current slot. Below it, undo only the
    // preview's displacement before testing the original card midpoints.
    if (slot?.zone === zone && marker) {
      const top =
        zone.getBoundingClientRect().top +
        zone.clientTop -
        zone.scrollTop +
        parseFloat(marker.style.top);
      if (y >= top && y <= top + gap) return slot;
      if (y > top + gap) y -= gap;
    }
    return {
      zone,
      beforeId:
        cardsIn(zone, id).find((card) => {
          const rect = unshiftedRect(card, zone);
          return y < rect.top + rect.height / 2;
        })?.dataset.id || null,
    };
  };
  const resetPreview = () => {
    shifted.forEach((card) => card.classList.remove("card-drop-shift"));
    shifted.clear();
    previewZone?.classList.remove("has-drop-gap");
    previewZone?.style.removeProperty("--card-drop-gap");
    previewZone = null;
  };
  const clearMarker = () => {
    slot?.zone.classList.remove("drag-over");
    resetPreview();
    marker?.remove();
    marker = null;
    slot = null;
  };
  const showSlot = (next: Slot, id: string) => {
    slot?.zone.classList.remove("drag-over");
    if (previewZone !== next.zone) resetPreview();
    slot = next;
    next.zone.classList.add("drag-over");
    if (!marker) {
      marker = document.createElement("div");
      marker.className = "card-drop-indicator";
      marker.dataset.cardDropPreview = "true";
      marker.setAttribute("aria-hidden", "true");
    }
    if (marker.parentElement !== next.zone) next.zone.append(marker);
    const cards = cardsIn(next.zone, id);
    // Snapshot the entire list (including the dimmed source) before animating it.
    const physicalCards = Array.from(next.zone.children).filter(
      (card): card is HTMLElement =>
        card instanceof HTMLElement && card.matches(".card"),
    );
    physicalCards.forEach((card) => unshiftedRect(card, next.zone));
    const index = cards.findIndex((card) => card.dataset.id === next.beforeId);
    const before = cards[index];
    marker.dataset.placement = !cards.length
      ? "empty"
      : index === 0
        ? "start"
        : before
          ? "between"
          : "end";
    const rect = next.zone.getBoundingClientRect();
    const last = cards.length
      ? unshiftedRect(cards[cards.length - 1], next.zone)
      : null;
    const edge = before
      ? unshiftedRect(before, next.zone).top
      : last
        ? last.top + last.height + 8
        : physicalCards.length
          ? unshiftedRect(physicalCards[0], next.zone).top
          : rect.top + 12;
    marker.style.top = `${Math.max(0, edge - rect.top + next.zone.scrollTop - next.zone.clientTop)}px`;
    previewZone = next.zone;
    previewZone.style.setProperty("--card-drop-gap", `${gap}px`);
    previewZone.classList.add("has-drop-gap");
    physicalCards.forEach((card) => {
      const moveAside = unshiftedRect(card, next.zone).top + 0.5 >= edge;
      card.classList.toggle("card-drop-shift", moveAside);
      if (moveAside) shifted.add(card);
      else shifted.delete(card);
    });
  };
  const zoneAt = (x: number, y: number): HTMLElement | null => {
    if (!scroller) return null;
    const bounds = scroller.getBoundingClientRect();
    // Ignore off-board releases; never fall back to the last valid column.
    if (
      bounds.width &&
      (x < bounds.left ||
        x > bounds.right ||
        y < bounds.top ||
        y > bounds.bottom)
    )
      return null;
    const hit = document.elementFromPoint(x, y);
    const zone =
      hit?.closest<HTMLElement>("[data-drop-column]") ||
      hit?.closest(".column")?.querySelector<HTMLElement>("[data-drop-column]");
    if (zone && root.contains(zone)) return zone;
    return (
      Array.from(root.querySelectorAll<HTMLElement>(".columns > .column"))
        .find((column) => {
          const rect = column.getBoundingClientRect();
          return x >= rect.left && x <= rect.right && y >= rect.top;
        })
        ?.querySelector<HTMLElement>("[data-drop-column]") || null
    );
  };
  const update = () => {
    if (!gesture?.started) return;
    const { x, y, id, offsetX, offsetY } = gesture;
    if (ghost)
      ghost.style.transform = `translate3d(${x - offsetX}px,${y - offsetY}px,0)`;
    const zone = zoneAt(x, y);
    if (zone) showSlot(findSlot(zone, id, y), id);
    else clearMarker();
  };
  const scrollTick = () => {
    if (!gesture?.started || !scroller) return;
    const rect = scroller.getBoundingClientRect();
    const speed = (point: number, start: number, end: number) =>
      point < start + 48
        ? -12 * Math.min(1, (start + 48 - point) / 48)
        : point > end - 48
          ? 12 * Math.min(1, (point - end + 48) / 48)
          : 0;
    if (
      gesture.x >= rect.left &&
      gesture.x <= rect.right &&
      gesture.y >= rect.top &&
      gesture.y <= rect.bottom
    ) {
      const left = scroller.scrollLeft,
        top = scroller.scrollTop;
      scroller.scrollLeft += speed(gesture.x, rect.left, rect.right);
      scroller.scrollTop += speed(gesture.y, rect.top, rect.bottom);
      if (scroller.scrollLeft !== left || scroller.scrollTop !== top) update();
    }
    frame = requestAnimationFrame(scrollTick);
  };
  const suppressReleaseClick = () => {
    suppressClick = true;
    clearTimeout(clickTimer);
    clickTimer = setTimeout(() => {
      suppressClick = false;
    }, 0);
  };
  const finish = (cancelled: boolean) => {
    const current = gesture;
    const destination = slot;
    gesture = null;
    cancelAnimationFrame(frame);
    ghost?.remove();
    ghost = null;
    current?.card.classList.remove("dragging");
    if (current?.card.hasPointerCapture?.(current.pointerId))
      current.card.releasePointerCapture(current.pointerId);
    clearMarker();
    geometry.clear();
    if (!current?.started) return;
    suppressReleaseClick();
    if (!cancelled && destination)
      options.move(
        current.id,
        destination.zone.dataset.dropColumn!,
        destination.beforeId,
      );
  };
  const clearNative = () => {
    root
      .querySelectorAll(".card.dragging")
      .forEach((card) => card.classList.remove("dragging"));
    nativeId = null;
    clearMarker();
    geometry.clear();
  };

  root.querySelectorAll<HTMLElement>(".columns .card").forEach((card) => {
    listen(card, "click", (event) => {
      if (suppressClick || gesture?.started) {
        event.preventDefault();
        return;
      }
      if (!(event.target as HTMLElement).closest(interactive))
        options.open(card.dataset.id!);
    });
    listen(card, "keydown", (event) => {
      const key = event as KeyboardEvent;
      if ((event.target as HTMLElement).closest(interactive)) return;
      if (
        key.altKey &&
        ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(key.key)
      ) {
        key.preventDefault();
        if (options.canDrag?.() !== false)
          options.shift(
            card.dataset.id!,
            key.key.slice(5).toLowerCase() as Parameters<
              DragOptions["shift"]
            >[1],
          );
      } else if (key.key === "Enter" || key.key === " ") {
        key.preventDefault();
        options.open(card.dataset.id!);
      }
    });
    listen(card, "pointerdown", (event) => {
      const pointer = event as PointerEvent;
      if (
        pointer.button !== 0 ||
        gesture ||
        options.canDrag?.() === false ||
        (event.target as HTMLElement).closest(interactive)
      )
        return;
      const rect = card.getBoundingClientRect();
      gesture = {
        card,
        id: card.dataset.id!,
        pointerId: pointer.pointerId,
        startX: pointer.clientX,
        startY: pointer.clientY,
        x: pointer.clientX,
        y: pointer.clientY,
        offsetX: pointer.clientX - rect.left,
        offsetY: pointer.clientY - rect.top,
        started: false,
      };
    });
    listen(card, "dragstart", (event) => {
      if (options.canDrag?.() === false || gesture?.started) {
        event.preventDefault();
        return;
      }
      gesture = null;
      nativeId = card.dataset.id!;
      card.classList.add("dragging");
      const transfer = (event as DragEvent).dataTransfer;
      if (transfer) {
        transfer.effectAllowed = "move";
        transfer.setData("text/plain", nativeId);
        transfer.setData("application/x-irohmd-card", nativeId);
      }
    });
    listen(card, "dragend", () => clearNative());
  });
  listen(document, "pointermove", (event) => {
    const pointer = event as PointerEvent;
    if (!gesture || pointer.pointerId !== gesture.pointerId) return;
    if (pointer.buttons === 0) {
      finish(true);
      return;
    }
    gesture.x = pointer.clientX;
    gesture.y = pointer.clientY;
    if (!gesture.started) {
      if (
        Math.hypot(
          pointer.clientX - gesture.startX,
          pointer.clientY - gesture.startY,
        ) < 6
      )
        return;
      gesture.started = true;
      gesture.card.classList.add("dragging");
      ghost = gesture.card.cloneNode(true) as HTMLElement;
      ghost.classList.remove("dragging", "selected");
      ghost.classList.add("drag-ghost");
      ghost.removeAttribute("data-id");
      ghost.setAttribute("aria-hidden", "true");
      ghost.inert = true;
      ghost.querySelectorAll("[id]").forEach((el) => el.removeAttribute("id"));
      ghost.style.width = `${gesture.card.getBoundingClientRect().width}px`;
      ghost.style.transformOrigin = `${gesture.offsetX}px ${gesture.offsetY}px`;
      document.body.append(ghost);
      if (pointer.pointerId !== undefined) {
        try {
          gesture.card.setPointerCapture?.(pointer.pointerId);
        } catch {
          /* Pointer may already have been released. */
        }
      }
      frame = requestAnimationFrame(scrollTick);
    }
    event.preventDefault();
    update();
  });
  listen(document, "pointerup", (event) => {
    const pointer = event as PointerEvent;
    if (!gesture || pointer.pointerId !== gesture.pointerId) return;
    gesture.x = pointer.clientX;
    gesture.y = pointer.clientY;
    update();
    finish(false);
  });
  listen(document, "pointercancel", (event) => {
    if (gesture && (event as PointerEvent).pointerId === gesture.pointerId)
      finish(true);
  });
  listen(document, "keydown", (event) => {
    if ((event as KeyboardEvent).key === "Escape" && (gesture || nativeId)) {
      event.preventDefault();
      finish(true);
      clearNative();
    }
  });
  listen(window, "blur", () => {
    finish(true);
    clearNative();
  });
  root.querySelectorAll<HTMLElement>("[data-drop-column]").forEach((zone) => {
    const over: EventListener = (event) => {
      if (!nativeId) return;
      event.preventDefault();
      event.stopPropagation();
      showSlot(
        findSlot(zone, nativeId, (event as DragEvent).clientY),
        nativeId,
      );
      const transfer = (event as DragEvent).dataTransfer;
      if (transfer) transfer.dropEffect = "move";
    };
    listen(zone, "dragenter", over);
    listen(zone, "dragover", over);
    listen(zone, "dragleave", (event) => {
      if (!zone.contains((event as DragEvent).relatedTarget as Node))
        clearMarker();
    });
    listen(zone, "drop", (event) => {
      const drag = event as DragEvent;
      const id = nativeId;
      if (!id) return;
      event.preventDefault();
      event.stopPropagation();
      const destination = findSlot(zone, id, drag.clientY);
      clearNative();
      suppressReleaseClick();
      options.move(id, zone.dataset.dropColumn!, destination.beforeId);
    });
  });
  return () => {
    finish(true);
    clearNative();
    clearTimeout(clickTimer);
    removeListeners.forEach((remove) => remove());
  };
}
