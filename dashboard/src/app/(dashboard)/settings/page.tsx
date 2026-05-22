"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { config, cache } from "@/lib/api";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Trash2 } from "lucide-react";

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between py-2 border-b last:border-0">
      <span className="text-sm text-muted-foreground">{label}</span>
      <span className="text-sm font-medium tabular-nums">{value}</span>
    </div>
  );
}

function BoolBadge({ value }: { value: boolean }) {
  return value ? (
    <Badge variant="outline" className="text-green-600 border-green-600/30">
      enabled
    </Badge>
  ) : (
    <Badge variant="secondary">disabled</Badge>
  );
}

export default function SettingsPage() {
  const qc = useQueryClient();
  const [flushOpen, setFlushOpen] = useState(false);

  const { data, isLoading } = useQuery({
    queryKey: ["config"],
    queryFn: () => config.get(),
  });

  const flushMut = useMutation({
    mutationFn: () => cache.flush(),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["cache"] });
      setFlushOpen(false);
    },
  });

  const cfg = data?.data;

  if (isLoading) {
    return (
      <div className="space-y-4">
        <div>
          <h1 className="text-2xl font-semibold mb-1">Settings</h1>
          <p className="text-muted-foreground text-sm">
            Current Velox runtime configuration.
          </p>
        </div>
        <div className="grid gap-4 md:grid-cols-2">
          {Array.from({ length: 4 }).map((_, i) => (
            <Card key={i}>
              <CardHeader>
                <Skeleton className="h-5 w-32" />
              </CardHeader>
              <CardContent className="space-y-2">
                {Array.from({ length: 4 }).map((_, j) => (
                  <Skeleton key={j} className="h-8 w-full" />
                ))}
              </CardContent>
            </Card>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-semibold mb-1">Settings</h1>
        <p className="text-muted-foreground text-sm">
          Current Velox runtime configuration (read-only).
        </p>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        {/* Server */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-base">Server</CardTitle>
            <CardDescription>Network and connection settings</CardDescription>
          </CardHeader>
          <CardContent>
            <Row label="Host" value={cfg?.host ?? "—"} />
            <Row label="Port" value={cfg?.port ?? "—"} />
            <Row
              label="Request timeout"
              value={cfg ? `${cfg.request_timeout_ms} ms` : "—"}
            />
            <Row
              label="DB pool max connections"
              value={cfg?.db_pool_max_connections ?? "—"}
            />
            <Row
              label="JWT expiration"
              value={cfg ? `${cfg.jwt_expiration_hours} h` : "—"}
            />
          </CardContent>
        </Card>

        {/* Logging */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-base">Logging</CardTitle>
            <CardDescription>Observability configuration</CardDescription>
          </CardHeader>
          <CardContent>
            <Row label="Log level" value={cfg?.log_level ?? "—"} />
            <Row
              label="Log request bodies"
              value={<BoolBadge value={cfg?.log_request_bodies ?? false} />}
            />
            <Row
              label="Log response bodies"
              value={<BoolBadge value={cfg?.log_response_bodies ?? false} />}
            />
            <Row
              label="Prometheus"
              value={<BoolBadge value={cfg?.prometheus_enabled ?? false} />}
            />
          </CardContent>
        </Card>

        {/* Cache */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-base">Cache</CardTitle>
            <CardDescription>Exact and semantic cache settings</CardDescription>
          </CardHeader>
          <CardContent>
            <Row
              label="Cache enabled"
              value={<BoolBadge value={cfg?.cache_enabled ?? false} />}
            />
            <Row
              label="TTL"
              value={cfg ? `${cfg.cache_ttl_seconds} s` : "—"}
            />
            <Row
              label="Max entries"
              value={cfg?.cache_max_entries?.toLocaleString() ?? "—"}
            />
            <Row
              label="Semantic cache"
              value={
                <BoolBadge value={cfg?.semantic_cache_available ?? false} />
              }
            />
            <Row
              label="Similarity threshold"
              value={cfg ? cfg.semantic_cache_threshold.toFixed(2) : "—"}
            />
          </CardContent>
        </Card>

        {/* Gateway */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-base">Gateway</CardTitle>
            <CardDescription>Proxy and rate limiting</CardDescription>
          </CardHeader>
          <CardContent>
            <Row
              label="Rate limit window"
              value={cfg ? `${cfg.rate_limit_window_secs} s` : "—"}
            />
            <Row label="Max retries" value={cfg?.max_retries ?? "—"} />
            <Row
              label="Embedding model"
              value={
                <span
                  className="font-mono text-xs max-w-48 truncate block text-right"
                  title={cfg?.embedding_model_path}
                >
                  {cfg?.embedding_model_path ?? "—"}
                </span>
              }
            />
          </CardContent>
        </Card>
      </div>

      {/* Danger Zone */}
      <div className="space-y-3">
        <h2 className="text-base font-medium text-destructive">Danger zone</h2>
        <Card className="border-destructive/30">
          <CardContent className="pt-4">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-sm font-medium">Flush cache</p>
                <p className="text-xs text-muted-foreground mt-0.5">
                  Clears the in-memory hot layer and all persisted cache entries.
                  Requests will hit providers until the cache warms back up.
                </p>
              </div>
              <Button
                variant="outline"
                size="sm"
                className="ml-6 shrink-0 text-destructive border-destructive/30 hover:bg-destructive/10"
                onClick={() => setFlushOpen(true)}
              >
                <Trash2 className="h-3.5 w-3.5 mr-1" />
                Flush cache
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>

      <AlertDialog open={flushOpen} onOpenChange={setFlushOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Flush all cache entries?</AlertDialogTitle>
            <AlertDialogDescription>
              This clears both the in-memory hot layer and all persisted cache
              entries in PostgreSQL. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() => flushMut.mutate()}
              disabled={flushMut.isPending}
            >
              {flushMut.isPending ? "Flushing…" : "Flush cache"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
