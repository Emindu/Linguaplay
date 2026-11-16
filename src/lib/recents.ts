// "Recently opened" media library — a VLC-style recent-files list, not an
// imported/managed library. Files stay wherever they are on disk; we just
// remember the path/URL, a title, and how far you got, so the library page
// can offer "resume where you left off" without owning or copying anything.
//
// Persisted to localStorage rather than the SQLite vocabulary DB: it's pure
// client-side bookkeeping (no cross-referencing, no queries beyond "give me
// the list"), so a Rust round trip would be pure overhead here.

export type RecentItem = {
  /** The local file path or YouTube URL — doubles as the dedupe key. */
  id: string;
  kind: "local" | "youtube";
  path: string;
  title: string;
  positionMs: number;
  durationMs: number;
  lastOpenedAt: number;
};

const STORAGE_KEY = "videoLearnRecents";
const MAX_ITEMS = 20;

export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

export function loadRecents(): RecentItem[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function saveRecents(items: RecentItem[]): void {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(items));
  } catch {
    // storage full/unavailable — recents are a convenience, not critical
  }
}

/** Adds or refreshes a recent entry, moving it to the front of the list. */
export function upsertRecent(item: Omit<RecentItem, "lastOpenedAt">): void {
  const items = loadRecents().filter((r) => r.id !== item.id);
  items.unshift({ ...item, lastOpenedAt: Date.now() });
  saveRecents(items.slice(0, MAX_ITEMS));
}

/** Updates just the playback progress of an existing entry, if present. */
export function updateRecentProgress(id: string, positionMs: number, durationMs: number): void {
  const items = loadRecents();
  const idx = items.findIndex((r) => r.id === id);
  if (idx === -1) return;
  items[idx] = { ...items[idx], positionMs, durationMs };
  saveRecents(items);
}

export function removeRecent(id: string): void {
  saveRecents(loadRecents().filter((r) => r.id !== id));
}

/** Puts a removed entry back exactly where (and as it) was — used by "Undo". */
export function restoreRecent(item: RecentItem, index: number): void {
  const items = loadRecents().filter((r) => r.id !== item.id);
  items.splice(Math.min(index, items.length), 0, item);
  saveRecents(items.slice(0, MAX_ITEMS));
}
