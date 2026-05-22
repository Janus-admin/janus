import { create } from "zustand";

interface UIState {
  sidebarOpen: boolean;
  setSidebarOpen: (open: boolean) => void;
  toggleSidebar: () => void;
}

export const useUIStore = create<UIState>((set) => ({
  sidebarOpen: true,
  setSidebarOpen: (open) => set({ sidebarOpen: open }),
  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
}));

// Live feed state — populated by the WebSocket hook.
export interface LiveEvent {
  model: string;
  api_key_id: string;
  prompt_tokens: number | null;
  total_tokens: number | null;
  latency_ms: number;
  status: "success" | "error";
  cache_type: string | null;
  similarity: number | null;
  stream: boolean;
  ts: string;
}

interface LiveFeedState {
  events: LiveEvent[];
  connected: boolean;
  setConnected: (v: boolean) => void;
  pushEvent: (e: LiveEvent) => void;
  clearEvents: () => void;
}

export const useLiveFeedStore = create<LiveFeedState>((set) => ({
  events: [],
  connected: false,
  setConnected: (connected) => set({ connected }),
  pushEvent: (e) =>
    set((s) => ({
      // Keep at most 500 events in memory; newest first.
      events: [e, ...s.events].slice(0, 500),
    })),
  clearEvents: () => set({ events: [] }),
}));
