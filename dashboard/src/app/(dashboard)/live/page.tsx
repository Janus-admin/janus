"use client";

import { useEffect, useRef } from "react";
import { useLiveFeed } from "@/hooks/use-live-feed";
import { useLiveFeedStore } from "@/store";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { format, parseISO } from "date-fns";
import { Wifi, WifiOff, Trash2 } from "lucide-react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

// uPlot latency sparkline
function LatencyChart({ latencies }: { latencies: number[] }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const plotRef = useRef<uPlot | null>(null);

  useEffect(() => {
    if (!containerRef.current) return;

    const data: uPlot.AlignedData = [
      latencies.map((_, i) => i),
      latencies,
    ];

    const opts: uPlot.Options = {
      width: containerRef.current.offsetWidth || 400,
      height: 80,
      cursor: { show: false },
      legend: { show: false },
      scales: { x: { time: false } },
      axes: [
        { show: false },
        {
          size: 48,
          font: "11px sans-serif",
          stroke: "var(--muted-foreground, #888)",
          grid: {
            stroke: "var(--border, #e5e7eb)",
            width: 1,
            dash: [3, 3],
          },
          ticks: { show: false },
        },
      ],
      series: [
        {},
        {
          stroke: "var(--chart-1, #6366f1)",
          width: 1.5,
          fill: "rgba(99,102,241,0.08)",
          spanGaps: true,
        },
      ],
    };

    if (plotRef.current) {
      plotRef.current.destroy();
    }

    plotRef.current = new uPlot(opts, data, containerRef.current);

    return () => {
      plotRef.current?.destroy();
      plotRef.current = null;
    };
  }, [latencies]);

  return <div ref={containerRef} className="w-full" />;
}

function statusBadge(status: "success" | "error") {
  if (status === "success")
    return (
      <Badge
        variant="outline"
        className="text-green-600 border-green-600/30 shrink-0"
      >
        ok
      </Badge>
    );
  return <Badge variant="destructive" className="shrink-0">err</Badge>;
}

function cacheBadge(cacheType: string | null) {
  if (!cacheType) return null;
  return (
    <Badge variant="secondary" className="shrink-0">
      {cacheType}
    </Badge>
  );
}

export default function LivePage() {
  // Start the WebSocket connection when this page is mounted
  useLiveFeed();

  const { events, connected, clearEvents } = useLiveFeedStore();

  // Build a rolling array of the last 60 latencies for the sparkline
  const recentLatencies = events.slice(0, 60).map((e) => e.latency_ms).reverse();

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-semibold mb-1">Live Feed</h1>
          <p className="text-muted-foreground text-sm">
            Real-time stream of incoming gateway requests.
          </p>
        </div>
        <div className="flex items-center gap-3 shrink-0">
          <span className="flex items-center gap-1.5 text-sm">
            {connected ? (
              <>
                <Wifi className="h-4 w-4 text-green-500" />
                <span className="text-green-600">Connected</span>
              </>
            ) : (
              <>
                <WifiOff className="h-4 w-4 text-muted-foreground" />
                <span className="text-muted-foreground">Disconnected</span>
              </>
            )}
          </span>
          <Button
            variant="ghost"
            size="sm"
            onClick={clearEvents}
            disabled={events.length === 0}
          >
            <Trash2 className="h-3.5 w-3.5 mr-1" />
            Clear
          </Button>
        </div>
      </div>

      {/* Latency sparkline */}
      {recentLatencies.length > 1 && (
        <Card>
          <CardHeader className="pb-1">
            <CardTitle className="text-sm font-medium">
              Latency — last {recentLatencies.length} requests
            </CardTitle>
            <CardDescription>ms end-to-end</CardDescription>
          </CardHeader>
          <CardContent>
            <LatencyChart latencies={recentLatencies} />
          </CardContent>
        </Card>
      )}

      {/* Stats row */}
      {events.length > 0 && (
        <div className="grid grid-cols-3 gap-4">
          <Card>
            <CardContent className="pt-4">
              <p className="text-xs text-muted-foreground">Events (buffer)</p>
              <p className="text-xl font-bold tabular-nums">
                {events.length.toLocaleString()}
              </p>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="pt-4">
              <p className="text-xs text-muted-foreground">
                Avg latency (visible)
              </p>
              <p className="text-xl font-bold tabular-nums">
                {Math.round(
                  events.reduce((s, e) => s + e.latency_ms, 0) / events.length
                )}{" "}
                <span className="text-sm font-normal text-muted-foreground">
                  ms
                </span>
              </p>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="pt-4">
              <p className="text-xs text-muted-foreground">
                Cache hits (visible)
              </p>
              <p className="text-xl font-bold tabular-nums">
                {events.filter((e) => e.cache_type).length}
              </p>
            </CardContent>
          </Card>
        </div>
      )}

      {/* Event list */}
      <Card>
        <CardContent className="p-0">
          {events.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-16 text-muted-foreground gap-2">
              <Wifi className="h-8 w-8 opacity-30" />
              <p className="text-sm">
                {connected
                  ? "Waiting for requests…"
                  : "Connecting to event stream…"}
              </p>
            </div>
          ) : (
            <div className="divide-y max-h-[60vh] overflow-y-auto">
              {events.map((e, i) => (
                <div
                  key={i}
                  className="flex items-center gap-3 px-4 py-2.5 text-sm hover:bg-muted/40 transition-colors"
                >
                  <span className="text-xs text-muted-foreground whitespace-nowrap w-16 shrink-0">
                    {format(parseISO(e.ts), "HH:mm:ss")}
                  </span>
                  <span className="font-medium shrink-0">{e.model}</span>
                  <span className="text-muted-foreground text-xs truncate flex-1">
                    {e.api_key_id.slice(0, 8)}…
                  </span>
                  <span className="tabular-nums text-xs text-muted-foreground shrink-0">
                    {e.total_tokens != null
                      ? `${e.total_tokens.toLocaleString()} tok`
                      : "—"}
                  </span>
                  <span className="tabular-nums text-xs shrink-0">
                    {e.latency_ms} ms
                  </span>
                  {cacheBadge(e.cache_type)}
                  {statusBadge(e.status)}
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
