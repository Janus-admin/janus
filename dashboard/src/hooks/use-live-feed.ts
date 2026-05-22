"use client";

import { useEffect, useRef } from "react";
import { useLiveFeedStore, type LiveEvent } from "@/store";

/**
 * Opens a WebSocket connection to /admin/stream and feeds events into
 * the global live feed store.  Call this once in the Live page (or the
 * dashboard layout if you want the counter to persist across page changes).
 */
export function useLiveFeed() {
  const { setConnected, pushEvent } = useLiveFeedStore();
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (typeof window === "undefined") return;

    // Derive WebSocket URL from current location so it works regardless of
    // whether the app is embedded in Rust (:8080) or run via Next.js dev server.
    const apiBase =
      process.env.NEXT_PUBLIC_API_URL ?? window.location.origin;
    const wsUrl = apiBase.replace(/^http/, "ws") + "/admin/stream";

    function connect() {
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onopen = () => setConnected(true);

      ws.onmessage = (evt) => {
        try {
          const event = JSON.parse(evt.data) as LiveEvent;
          pushEvent(event);
        } catch {
          // ignore malformed frames
        }
      };

      ws.onclose = () => {
        setConnected(false);
        // Reconnect after 3 s.
        setTimeout(connect, 3_000);
      };

      ws.onerror = () => ws.close();
    }

    connect();

    return () => {
      wsRef.current?.close();
    };
  }, [setConnected, pushEvent]);
}
