"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  providers,
  type VeloxProvider,
  type UpdateProviderRequest,
} from "@/lib/api";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod/v4";
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { format, parseISO } from "date-fns";
import {
  CheckCircle2,
  XCircle,
  HelpCircle,
  Pencil,
  RefreshCw,
} from "lucide-react";

const editSchema = z.object({
  api_key: z.string().optional(),
  priority: z.string().optional(),
  timeout_ms: z.string().optional(),
  max_retries: z.string().optional(),
});
type EditForm = z.infer<typeof editSchema>;

function HealthIcon({ status }: { status: string }) {
  if (status === "healthy")
    return <CheckCircle2 className="h-4 w-4 text-green-500" />;
  if (status === "unhealthy")
    return <XCircle className="h-4 w-4 text-red-500" />;
  return <HelpCircle className="h-4 w-4 text-muted-foreground" />;
}

function healthBadge(status: string) {
  if (status === "healthy")
    return (
      <Badge variant="outline" className="text-green-600 border-green-600/30">
        healthy
      </Badge>
    );
  if (status === "unhealthy") return <Badge variant="destructive">unhealthy</Badge>;
  return <Badge variant="secondary">unknown</Badge>;
}

function providerLabel(name: string): string {
  const labels: Record<string, string> = {
    openai: "OpenAI",
    anthropic: "Anthropic",
    bedrock: "AWS Bedrock",
    gemini: "Google Gemini",
    groq: "Groq",
  };
  return labels[name.toLowerCase()] ?? name;
}

export default function ProvidersPage() {
  const qc = useQueryClient();
  const [editTarget, setEditTarget] = useState<VeloxProvider | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ["providers"],
    queryFn: () => providers.list(),
    refetchInterval: 30_000,
  });

  const toggleMut = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      providers.update(id, { is_enabled: enabled }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["providers"] }),
  });

  const testMut = useMutation({
    mutationFn: (id: string) => providers.test(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["providers"] }),
  });

  const editMut = useMutation({
    mutationFn: ({
      id,
      body,
    }: {
      id: string;
      body: UpdateProviderRequest;
    }) => providers.update(id, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["providers"] });
      setEditTarget(null);
    },
  });

  const form = useForm<EditForm>({
    resolver: zodResolver(editSchema),
  });

  function openEdit(provider: VeloxProvider) {
    setEditTarget(provider);
    form.reset({
      api_key: "",
      priority: String(provider.priority),
      timeout_ms: String(provider.timeout_ms),
      max_retries: String(provider.max_retries),
    });
  }

  function onSubmit(values: EditForm) {
    if (!editTarget) return;
    const body: UpdateProviderRequest = {};
    if (values.api_key) body.api_key = values.api_key;
    if (values.priority) body.priority = parseInt(values.priority);
    if (values.timeout_ms) body.timeout_ms = parseInt(values.timeout_ms);
    if (values.max_retries) body.max_retries = parseInt(values.max_retries);
    editMut.mutate({ id: editTarget.id, body });
  }

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-2xl font-semibold mb-1">Providers</h1>
        <p className="text-muted-foreground text-sm">
          Configure OpenAI, Anthropic, Google Gemini, Groq, and AWS Bedrock provider settings.
        </p>
      </div>

      {isLoading ? (
        <div className="grid gap-4 md:grid-cols-3">
          {Array.from({ length: 3 }).map((_, i) => (
            <Card key={i}>
              <CardHeader>
                <Skeleton className="h-5 w-28" />
                <Skeleton className="h-4 w-20" />
              </CardHeader>
              <CardContent className="space-y-3">
                {Array.from({ length: 3 }).map((_, j) => (
                  <Skeleton key={j} className="h-4 w-full" />
                ))}
              </CardContent>
            </Card>
          ))}
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-3">
          {(data?.data ?? []).map((p) => (
            <Card
              key={p.id}
              className={p.is_enabled ? "" : "opacity-60"}
            >
              <CardHeader className="pb-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <HealthIcon status={p.health_status} />
                    <CardTitle className="text-base">
                      {providerLabel(p.id)}
                    </CardTitle>
                  </div>
                  <Switch
                    checked={p.is_enabled}
                    onCheckedChange={(enabled) =>
                      toggleMut.mutate({ id: p.id, enabled })
                    }
                    disabled={toggleMut.isPending}
                  />
                </div>
                <CardDescription className="flex items-center gap-2">
                  {healthBadge(p.health_status)}
                  {p.last_health_check && (
                    <span className="text-xs text-muted-foreground">
                      {format(parseISO(p.last_health_check), "HH:mm")}
                    </span>
                  )}
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-2 text-sm">
                <div className="flex justify-between text-muted-foreground">
                  <span>Priority</span>
                  <span className="tabular-nums font-medium text-foreground">
                    {p.priority}
                  </span>
                </div>
                <div className="flex justify-between text-muted-foreground">
                  <span>Timeout</span>
                  <span className="tabular-nums font-medium text-foreground">
                    {p.timeout_ms} ms
                  </span>
                </div>
                <div className="flex justify-between text-muted-foreground">
                  <span>Max retries</span>
                  <span className="tabular-nums font-medium text-foreground">
                    {p.max_retries}
                  </span>
                </div>
                <div className="flex gap-2 pt-2">
                  <Button
                    variant="outline"
                    size="sm"
                    className="flex-1"
                    onClick={() => testMut.mutate(p.id)}
                    disabled={testMut.isPending && testMut.variables === p.id}
                  >
                    <RefreshCw
                      className={`h-3.5 w-3.5 mr-1 ${
                        testMut.isPending && testMut.variables === p.id
                          ? "animate-spin"
                          : ""
                      }`}
                    />
                    Test
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    className="flex-1"
                    onClick={() => openEdit(p)}
                  >
                    <Pencil className="h-3.5 w-3.5 mr-1" />
                    Edit
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* Edit dialog */}
      <Dialog open={!!editTarget} onOpenChange={(o) => !o && setEditTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>
              Edit {editTarget ? providerLabel(editTarget.id) : ""}
            </DialogTitle>
            <DialogDescription>
              Leave API key blank to keep the existing key.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            <div className="space-y-1.5">
              <Label htmlFor="api_key">API key</Label>
              <Input
                id="api_key"
                type="password"
                placeholder="sk-… (leave blank to keep existing)"
                {...form.register("api_key")}
              />
            </div>
            <div className="grid grid-cols-3 gap-3">
              <div className="space-y-1.5">
                <Label htmlFor="priority">Priority</Label>
                <Input
                  id="priority"
                  type="number"
                  {...form.register("priority")}
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="timeout_ms">Timeout (ms)</Label>
                <Input
                  id="timeout_ms"
                  type="number"
                  {...form.register("timeout_ms")}
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="max_retries">Retries</Label>
                <Input
                  id="max_retries"
                  type="number"
                  {...form.register("max_retries")}
                />
              </div>
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="ghost"
                onClick={() => setEditTarget(null)}
              >
                Cancel
              </Button>
              <Button type="submit" disabled={editMut.isPending}>
                {editMut.isPending ? "Saving…" : "Save"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
