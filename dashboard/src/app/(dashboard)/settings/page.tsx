"use client";

import { useEffect, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { config, cache, type PatchConfigRequest } from "@/lib/api";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
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
import { Trash2, Save } from "lucide-react";

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between py-2 border-b last:border-0">
      <span className="text-sm text-muted-foreground">{label}</span>
      <span className="text-sm font-medium tabular-nums">{value}</span>
    </div>
  );
}

// ── Editable settings section ─────────────────────────────────────────────────

interface EditableSettings {
  log_request_bodies: boolean;
  log_response_bodies: boolean;
  cache_enabled: boolean;
  max_retries: number;
  semantic_cache_threshold: number;
}

function EditableSection({ cfg }: { cfg: EditableSettings & { semantic_cache_available: boolean } }) {
  const qc = useQueryClient();
  const [local, setLocal] = useState<EditableSettings>({
    log_request_bodies: cfg.log_request_bodies,
    log_response_bodies: cfg.log_response_bodies,
    cache_enabled: cfg.cache_enabled,
    max_retries: cfg.max_retries,
    semantic_cache_threshold: cfg.semantic_cache_threshold,
  });
  const [dirty, setDirty] = useState(false);

  // Sync when remote data changes.
  useEffect(() => {
    setLocal({
      log_request_bodies: cfg.log_request_bodies,
      log_response_bodies: cfg.log_response_bodies,
      cache_enabled: cfg.cache_enabled,
      max_retries: cfg.max_retries,
      semantic_cache_threshold: cfg.semantic_cache_threshold,
    });
    setDirty(false);
  }, [cfg.log_request_bodies, cfg.log_response_bodies, cfg.cache_enabled, cfg.max_retries, cfg.semantic_cache_threshold]);

  const patchMut = useMutation({
    mutationFn: (body: PatchConfigRequest) => config.patch(body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["config"] });
      setDirty(false);
    },
  });

  function update<K extends keyof EditableSettings>(
    key: K,
    value: EditableSettings[K]
  ) {
    setLocal((prev) => ({ ...prev, [key]: value }));
    setDirty(true);
  }

  function save() {
    patchMut.mutate({
      log_request_bodies: local.log_request_bodies,
      log_response_bodies: local.log_response_bodies,
      cache_enabled: local.cache_enabled,
      max_retries: local.max_retries,
      semantic_cache_threshold: local.semantic_cache_threshold,
    });
  }

  return (
    <>
      {/* Logging */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Logging</CardTitle>
          <CardDescription>Observability configuration</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex items-center justify-between py-1">
            <Label htmlFor="log-req" className="text-sm text-muted-foreground font-normal cursor-pointer">
              Log request bodies
            </Label>
            <Switch
              id="log-req"
              checked={local.log_request_bodies}
              onCheckedChange={(v) => update("log_request_bodies", v)}
            />
          </div>
          <div className="flex items-center justify-between py-1">
            <Label htmlFor="log-res" className="text-sm text-muted-foreground font-normal cursor-pointer">
              Log response bodies
            </Label>
            <Switch
              id="log-res"
              checked={local.log_response_bodies}
              onCheckedChange={(v) => update("log_response_bodies", v)}
            />
          </div>
        </CardContent>
      </Card>

      {/* Cache */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Cache</CardTitle>
          <CardDescription>Exact and semantic cache settings</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex items-center justify-between py-1">
            <Label htmlFor="cache-enabled" className="text-sm text-muted-foreground font-normal cursor-pointer">
              Cache enabled
            </Label>
            <Switch
              id="cache-enabled"
              checked={local.cache_enabled}
              onCheckedChange={(v) => update("cache_enabled", v)}
            />
          </div>
          <div className="flex items-center justify-between py-1 border-t pt-3">
            <div>
              <Label htmlFor="sem-threshold" className="text-sm text-muted-foreground font-normal">
                Similarity threshold
              </Label>
              <p className="text-xs text-muted-foreground">0.0 – 1.0</p>
            </div>
            <Input
              id="sem-threshold"
              type="number"
              step="0.01"
              min={0}
              max={1}
              className="w-24 h-8 text-sm"
              value={local.semantic_cache_threshold}
              onChange={(e) => {
                const v = parseFloat(e.target.value);
                if (!isNaN(v)) update("semantic_cache_threshold", v);
              }}
            />
          </div>
        </CardContent>
      </Card>

      {/* Gateway */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Gateway</CardTitle>
          <CardDescription>Proxy and retry settings</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex items-center justify-between py-1">
            <div>
              <Label htmlFor="max-retries" className="text-sm text-muted-foreground font-normal">
                Max retries
              </Label>
              <p className="text-xs text-muted-foreground">Per provider, before failover</p>
            </div>
            <Input
              id="max-retries"
              type="number"
              min={0}
              max={10}
              className="w-24 h-8 text-sm"
              value={local.max_retries}
              onChange={(e) => {
                const v = parseInt(e.target.value);
                if (!isNaN(v)) update("max_retries", v);
              }}
            />
          </div>
        </CardContent>
      </Card>

      {dirty && (
        <div className="flex justify-end">
          <Button onClick={save} disabled={patchMut.isPending} size="sm">
            <Save className="h-3.5 w-3.5 mr-1.5" />
            {patchMut.isPending ? "Saving…" : "Save changes"}
          </Button>
        </div>
      )}
    </>
  );
}

// ── Main page ─────────────────────────────────────────────────────────────────

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
          <p className="text-muted-foreground text-sm">Runtime configuration.</p>
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
          Runtime configuration — editable fields take effect immediately without a restart.
        </p>
      </div>

      {/* Read-only: Server */}
      <div className="space-y-2">
        <h2 className="text-sm font-medium text-muted-foreground uppercase tracking-wide">
          Read-only (require restart)
        </h2>
        <div className="grid gap-4 md:grid-cols-2">
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
              <Row
                label="Prometheus"
                value={
                  cfg?.prometheus_enabled ? (
                    <Badge variant="outline" className="text-green-600 border-green-600/30">
                      enabled
                    </Badge>
                  ) : (
                    <Badge variant="secondary">disabled</Badge>
                  )
                }
              />
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-base">Cache (static)</CardTitle>
              <CardDescription>Require restart to change</CardDescription>
            </CardHeader>
            <CardContent>
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
                  cfg?.semantic_cache_available ? (
                    <Badge variant="outline" className="text-green-600 border-green-600/30">
                      available
                    </Badge>
                  ) : (
                    <Badge variant="secondary">unavailable</Badge>
                  )
                }
              />
              <Row
                label="Log level"
                value={cfg?.log_level ?? "—"}
              />
              <Row
                label="Rate limit window"
                value={cfg ? `${cfg.rate_limit_window_secs} s` : "—"}
              />
            </CardContent>
          </Card>
        </div>
      </div>

      {/* Editable */}
      <div className="space-y-2">
        <h2 className="text-sm font-medium text-muted-foreground uppercase tracking-wide">
          Live settings
        </h2>
        <div className="grid gap-4 md:grid-cols-2">
          {cfg && (
            <EditableSection
              cfg={{
                log_request_bodies: cfg.log_request_bodies,
                log_response_bodies: cfg.log_response_bodies,
                cache_enabled: cfg.cache_enabled,
                max_retries: cfg.max_retries,
                semantic_cache_threshold: cfg.semantic_cache_threshold,
                semantic_cache_available: cfg.semantic_cache_available,
              }}
            />
          )}
        </div>
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
