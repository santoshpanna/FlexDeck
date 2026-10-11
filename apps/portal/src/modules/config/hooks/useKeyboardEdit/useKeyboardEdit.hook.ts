import { useEffect } from "react";

export interface KeyboardEditValue {
  selectedId: string | null;
  move: (dc: number, dr: number) => void;
  remove: () => void;
  /** Escape drops the selection, which closes whatever it opened. */
  deselect: () => void;
  /** The document's own history. The toolbar that showed these is gone. */
  undo: () => void;
  redo: () => void;
}

/**
 * Move and delete the selected widget from the keyboard.
 *
 * Registered *without* a dependency array on purpose: these handlers close over
 * the current selection, and re-subscribing on every render is cheaper than the
 * stale-closure bugs a dependency list would invite here.
 *
 * Events from a text field are ignored, or typing a label would move the widget
 * with the arrow keys and delete it with Backspace. Escape is the exception — it
 * is the universal "close this", so it works from a field too.
 */
export function useKeyboardEdit({ selectedId, move, remove, deselect, undo, redo }: KeyboardEditValue) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (selectedId) {
          deselect();
          event.preventDefault();
        }
        return;
      }

      const accel = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLInputElement;

      // Undo and redo apply whatever is selected, so they are checked ahead of
      // the selection guard. Inside a text field they are the FIELD's undo, which
      // is why the typing guard still applies to them.
      if (accel && !typing) {
        if (event.key === "z" || event.key === "Z") {
          (event.shiftKey ? redo : undo)();
          event.preventDefault();
          return;
        }
        if (event.key === "y" || event.key === "Y") {
          redo();
          event.preventDefault();
          return;
        }
      }

      if (!selectedId || typing) {
        return;
      }

      switch (event.key) {
        case "ArrowLeft":
          move(-1, 0);
          break;
        case "ArrowRight":
          move(1, 0);
          break;
        case "ArrowUp":
          move(0, -1);
          break;
        case "ArrowDown":
          move(0, 1);
          break;
        case "Delete":
        case "Backspace":
          remove();
          break;
        default:
          return;
      }

      event.preventDefault();
    };

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
}
