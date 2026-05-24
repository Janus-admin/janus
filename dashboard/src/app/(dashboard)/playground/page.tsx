"use client";

import { useState, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import { providers, getToken, ApiError } from "@/lib/api";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Send,
  Clock,
  Zap,
  DollarSign,
  Database,
  Hash,
  Plus,
  Trash2,
  ChevronDown,
  ChevronUp,
  RotateCcw,
} from "lucide-react";

const BASE =
  typeof process !== "undefined" ? (process.env.NEXT_PUBLIC_API_URL ?? "") : "";

const COMMON_MODELS = [
  "gpt-4o",
  "gpt-4o-mini",
  "gpt-4-turbo",
  "gpt-3.5-turbo",
  "claude-opus-4-5",
  "claude-sonnet-4-5",
  "claude-haiku-4-5",
  "anthropic.claude-3-5-sonnet-20241022-v2:0",
];

interface Message {
  role: "user" | "assistant" | "system";
  content: string;
}

interface PlaygroundMeta {
  requestId: string;
  provider: string;
  model: string;
  latencyMs: number;
  promptTokens: number;
  completionTokens: number;
  costUsd: string | null;
  cacheHit: string;
}

interface HistoryEntry {
  id: string;
  model: string;
  userMessage: string;
  responseText: string;
  meta: PlaygroundMeta;
  ts: Date;
}

function MetaBadge({ label, value, icon: Icon }: { label: string; value: string; icon: React.ElementType }) {
  return (
    <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
      <Icon className="h-3 w-3" />
      <span className="text-foreground font-medium">{value}</span>
      <span>{label}</span>
    </div>
  );
}

function cacheHitBadge(hit: string) {
  if (hit === "exact") return <Badge variant="outline" className="text-green-600 border-green-600/30 text-xs">exact hit</Badge>;
  if (hit === "semantic") return <Badge variant="outline" className="text-blue-600 border-blue-600/30 text-xs">semantic hit</Badge>;
  return <Badge variant="secondary" className="text-xs">miss</Badge>;
}

export default function PlaygroundPage() {
  const [model, setModel] = useState("gpt-4o-mini");
  const [customModel, setCustomModel] = useState("");
  const [systemPrompt, setSystemPrompt] = useState("");
  const [userMessage, setUserMessage] = useState("");
  const [skipCache, setSkipCache] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<{ text: string; meta: PlaygroundMeta } | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [expandedHistory, setExpandedHistory] = useState<string | null>(null);
  const resultRef = useRef<HTMLDivElement>(null);

  const providersQ = useQuery({
    queryKey: ["providers"],
    queryFn: () => providers.list(),
  });

  const enabledProviders = (providersQ.data?.data ?? [])
    .filter((p) => p.is_enabled)
    .map((p) => p.id);

  const effectiveModel = model === "__custom__" ? customModel : model;

  async function handleSubmit() {
    if (!effectiveModel.trim() || !userMessage.trim()) return;
    setLoading(true);
    setError(null);
    setResult(null);

    const messages: Message[] = [];
    if (systemPrompt.trim()) {
      messages.push({ role: "system", content: systemPrompt.trim() });
    }
    messages.push({ role: "user", content: userMessage.trim() });

    const body = {
      model: effectiveModel.trim(),
      messages,
      skip_cache: skipCache,
    };

    try {
      const token = getToken();
      const res = await fetch(`${BASE}/admin/playground`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
        body: JSON.stringify(body),
      });

      if (!res.ok) {
        const errBody = await res.json().catch(() => ({}));
        throw new ApiError(res.status, errBody?.error?.message ?? `HTTP ${res.status}`);
      }

      const data = await res.json();
      const meta: PlaygroundMeta = {
        requestId: res.headers.get("x-velox-request-id") ?? "",
        provider: res.headers.get("x-velox-provider") ?? "",
        model: res.headers.get("x-velox-model") ?? effectiveModel,
        latencyMs: parseInt(res.headers.get("x-velox-latency-ms") ?? "0", 10),
        promptTokens: parseInt(res.headers.get("x-velox-prompt-tokens") ?? "0", 10),
        completionTokens: parseInt(res.headers.get("x-velox-completion-tokens") ?? "0", 10),
        costUsd: res.headers.get("x-velox-cost-usd"),
        cacheHit: res.headers.get("x-velox-cache-hit") ?? "none",
      };

      const text =
        data?.choices?.[0]?.message?.content ??
        data?.content?.[0]?.text ??
        JSON.stringify(data, null, 2);

      setResult({ text, meta });

      const entry: HistoryEntry = {
        id: meta.requestId || String(Date.now()),
        model: meta.model,
        userMessage: userMessage.trim(),
        responseText: text,
        meta,
        ts: new Date(),
      };
      setHistory((prev) => [entry, ...prev].slice(0, 10));

      setTimeout(() => resultRef.current?.scrollIntoView({ behavior: "smooth" }), 100);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Unknown error");
    } finally {
      setLoading(false);
    }
  }

  function loadFromHistory(entry: HistoryEntry) {
    setModel(
      COMMON_MODELS.includes(entry.model) ? entry.model : "__custom__"
    );
    if (!COMMON_MODELS.includes(entry.model)) setCustomModel(entry.model);
    setUserMessage(entry.userMessage);
    setResult({ text: entry.responseText, meta: entry.meta });
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-semibold mb-1">Playground</h1>
        <p className="text-muted-foreground text-sm">
          Test prompts directly through the gateway. Authenticated as admin — no budget or rate limits applied.
        </p>
      </div>

      <div className="grid lg:grid-cols-3 gap-6">
        {/* Composer */}
        <div className="lg:col-span-2 space-y-4">
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-base">Prompt</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              {/* Model selector */}
              <div className="grid sm:grid-cols-2 gap-3">
                <div className="space-y-1.5">
                  <Label htmlFor="model">Model</Label>
                  <Select value={model} onValueChange={setModel}>
                    <SelectTrigger id="model">
                      <SelectValue placeholder="Select model" />
                    </SelectTrigger>
                    <SelectContent>
                      {COMMON_MODELS.map((m) => (
                        <SelectItem key={m} value={m}>
                          {m}
                        </SelectItem>
                      ))}
                      <SelectItem value="__custom__">Custom…</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                {model === "__custom__" && (
                  <div className="space-y-1.5">
                    <Label htmlFor="customModel">Custom model ID</Label>
                    <Input
                      id="customModel"
                      placeholder="e.g. gpt-4o-2024-08-06"
                      value={customModel}
                      onChange={(e) => setCustomModel(e.target.value)}
                    />
                  </div>
                )}
                {model !== "__custom__" && (
                  <div className="space-y-1.5">
                    <Label>Active providers</Label>
                    <div className="flex flex-wrap gap-1 pt-1">
                      {providersQ.isLoading ? (
                        <Skeleton className="h-5 w-20" />
                      ) : enabledProviders.length === 0 ? (
                        <span className="text-xs text-muted-foreground">None enabled</span>
                      ) : (
                        enabledProviders.map((id) => (
                          <Badge key={id} variant="secondary" className="text-xs">
                            {id}
                          </Badge>
                        ))
                      )}
                    </div>
                  </div>
                )}
              </div>

              {/* System prompt */}
              <div className="space-y-1.5">
                <Label htmlFor="systemPrompt">System prompt (optional)</Label>
                <Textarea
                  id="systemPrompt"
                  placeholder="You are a helpful assistant…"
                  rows={2}
                  value={systemPrompt}
                  onChange={(e) => setSystemPrompt(e.target.value)}
                  className="resize-none text-sm"
                />
              </div>

              {/* User message */}
              <div className="space-y-1.5">
                <Label htmlFor="userMessage">User message</Label>
                <Textarea
                  id="userMessage"
                  placeholder="Write your prompt here…"
                  rows={5}
                  value={userMessage}
                  onChange={(e) => setUserMessage(e.target.value)}
                  className="resize-none text-sm"
                />
              </div>

              {/* Options row */}
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Switch
                    id="skipCache"
                    checked={skipCache}
                    onCheckedChange={setSkipCache}
                  />
                  <Label htmlFor="skipCache" className="text-sm cursor-pointer">
                    Skip cache
                  </Label>
                </div>
                <Button
                  onClick={handleSubmit}
                  disabled={loading || !effectiveModel.trim() || !userMessage.trim()}
                  className="gap-2"
                >
                  <Send className="h-3.5 w-3.5" />
                  {loading ? "Sending…" : "Send"}
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Result */}
          {(loading || error || result) && (
            <Card ref={resultRef}>
              <CardHeader className="pb-2">
                <CardTitle className="text-base">Response</CardTitle>
              </CardHeader>
              <CardContent>
                {loading && (
                  <div className="space-y-2">
                    <Skeleton className="h-4 w-full" />
                    <Skeleton className="h-4 w-3/4" />
                    <Skeleton className="h-4 w-5/6" />
                  </div>
                )}
                {error && (
                  <p className="text-sm text-destructive">{error}</p>
                )}
                {!loading && result && (
                  <div className="space-y-4">
                    {/* Metadata strip */}
                    <div className="flex flex-wrap gap-4 p-3 rounded-md bg-muted/50 text-xs">
                      <MetaBadge icon={Zap} label="provider" value={result.meta.provider || "—"} />
                      <MetaBadge icon={Clock} label="ms" value={result.meta.latencyMs.toLocaleString()} />
                      <MetaBadge
                        icon={Plus}
                        label="prompt tokens"
                        value={result.meta.promptTokens.toLocaleString()}
                      />
                      <MetaBadge
                        icon={RotateCcw}
                        label="completion tokens"
                        value={result.meta.completionTokens.toLocaleString()}
                      />
                      {result.meta.cacheHit !== "none" ? (
                        <MetaBadge
                          icon={DollarSign}
                          label="cost"
                          value="free"
                        />
                      ) : result.meta.costUsd ? (
                        <MetaBadge
                          icon={DollarSign}
                          label="cost"
                          value={`$${parseFloat(result.meta.costUsd).toFixed(6)}`}
                        />
                      ) : null}
                      <div className="flex items-center gap-1.5">
                        <Database className="h-3 w-3 text-muted-foreground" />
                        {cacheHitBadge(result.meta.cacheHit)}
                      </div>
                      {result.meta.requestId && (
                        <MetaBadge icon={Hash} label="ID" value={result.meta.requestId.slice(0, 8) + "…"} />
                      )}
                    </div>
                    <Separator />
                    {/* Response text */}
                    <div className="text-sm whitespace-pre-wrap leading-relaxed">
                      {result.text}
                    </div>
                  </div>
                )}
              </CardContent>
            </Card>
          )}
        </div>

        {/* History sidebar */}
        <div>
          <Card className="sticky top-20">
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium">
                Recent ({history.length}/10)
              </CardTitle>
            </CardHeader>
            <CardContent className="p-0">
              {history.length === 0 ? (
                <p className="text-xs text-muted-foreground px-4 py-3">
                  No requests yet this session.
                </p>
              ) : (
                <ScrollArea className="max-h-[560px]">
                  <div className="divide-y">
                    {history.map((entry) => (
                      <div key={entry.id} className="px-4 py-3 space-y-1.5">
                        <div
                          className="flex items-start justify-between gap-2 cursor-pointer"
                          onClick={() =>
                            setExpandedHistory(
                              expandedHistory === entry.id ? null : entry.id
                            )
                          }
                        >
                          <div className="min-w-0">
                            <p className="text-xs font-medium text-muted-foreground truncate">
                              {entry.model}
                            </p>
                            <p className="text-xs truncate mt-0.5">
                              {entry.userMessage.slice(0, 60)}
                              {entry.userMessage.length > 60 ? "…" : ""}
                            </p>
                          </div>
                          <div className="shrink-0">
                            {expandedHistory === entry.id ? (
                              <ChevronUp className="h-3.5 w-3.5 text-muted-foreground" />
                            ) : (
                              <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
                            )}
                          </div>
                        </div>
                        <div className="flex items-center gap-2 text-xs text-muted-foreground">
                          <Clock className="h-3 w-3" />
                          <span>{entry.meta.latencyMs} ms</span>
                          <span>·</span>
                          {cacheHitBadge(entry.meta.cacheHit)}
                        </div>
                        {expandedHistory === entry.id && (
                          <div className="space-y-2 pt-1">
                            <p className="text-xs text-muted-foreground line-clamp-4">
                              {entry.responseText.slice(0, 200)}
                              {entry.responseText.length > 200 ? "…" : ""}
                            </p>
                            <Button
                              variant="outline"
                              size="sm"
                              className="w-full h-7 text-xs"
                              onClick={() => loadFromHistory(entry)}
                            >
                              <RotateCcw className="h-3 w-3 mr-1" />
                              Reload prompt
                            </Button>
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                </ScrollArea>
              )}
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
