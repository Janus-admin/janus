"use client";

import { useState, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import { providers, keys as keysApi, getToken, ApiError } from "@/lib/api";
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
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
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
  GitCompare,
  AlertCircle,
  CheckSquare,
  Square,
  Key,
} from "lucide-react";

const BASE =
  typeof process !== "undefined" ? (process.env.NEXT_PUBLIC_API_URL ?? "") : "";

const COMMON_MODELS = [
  // ── OpenAI — latest first ─────────────────────────────────────────────
  "gpt-5.5",
  "gpt-5.4",
  "gpt-5.4-mini",
  "gpt-5.4-nano",
  "gpt-5",
  "gpt-5-mini",
  "gpt-5-nano",
  "gpt-5.2",
  "gpt-5.1",
  "o3",
  "gpt-4.1",
  "gpt-4.1-mini",
  "gpt-4.1-nano",
  "gpt-4o",
  "gpt-4o-mini",
  // ── Anthropic — Claude 4 series ───────────────────────────────────────
  "claude-opus-4-8",
  "claude-opus-4-7",
  "claude-sonnet-4-6",
  "claude-opus-4-5",
  "claude-sonnet-4-5",
  "claude-haiku-4-5-20251001",
  "claude-3-5-sonnet-20241022",
  "claude-3-5-haiku-20241022",
  // ── Google Gemini — free tier working models ──────────────────────────
  "gemini-3.5-flash",
  "gemini-3-flash-preview",
  "gemini-3.1-flash-lite",
  "gemini-2.5-flash",
  "gemini-2.5-flash-lite",
  "gemini-flash-latest",
  // ── Groq — latest first ───────────────────────────────────────────────
  "meta-llama/llama-4-scout-17b-16e-instruct",
  "llama-3.3-70b-versatile",
  "llama-3.1-8b-instant",
  "groq/compound",
  "qwen/qwen3-32b",
  // ── DeepSeek ──────────────────────────────────────────────────────────
  "deepseek-v4-pro",
  "deepseek-v4-flash",
  "deepseek-chat",
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

interface MultiModelResult {
  model: string;
  text: string | null;
  error: string | null;
  latency_ms: number;
  prompt_tokens: number;
  completion_tokens: number;
  cost_usd: string | null;
  cache_hit: string;
}

function MetaBadge({
  label,
  value,
  icon: Icon,
}: {
  label: string;
  value: string;
  icon: React.ElementType;
}) {
  return (
    <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
      <Icon className="h-3 w-3" />
      <span className="text-foreground font-medium">{value}</span>
      <span>{label}</span>
    </div>
  );
}

function cacheHitBadge(hit: string) {
  if (hit === "exact")
    return (
      <Badge
        variant="outline"
        className="text-green-600 border-green-600/30 text-xs"
      >
        exact hit
      </Badge>
    );
  if (hit === "semantic")
    return (
      <Badge
        variant="outline"
        className="text-blue-600 border-blue-600/30 text-xs"
      >
        semantic hit
      </Badge>
    );
  return (
    <Badge variant="secondary" className="text-xs">
      miss
    </Badge>
  );
}

// ── Single-model result card ───────────────────────────────────────────────────

function SingleResult({
  result,
}: {
  result: { text: string; meta: PlaygroundMeta };
}) {
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap gap-4 p-3 rounded-md bg-muted/50 text-xs">
        <MetaBadge
          icon={Zap}
          label="provider"
          value={result.meta.provider || "—"}
        />
        <MetaBadge
          icon={Clock}
          label="ms"
          value={result.meta.latencyMs.toLocaleString()}
        />
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
          <MetaBadge icon={DollarSign} label="cost" value="free" />
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
          <MetaBadge
            icon={Hash}
            label="ID"
            value={result.meta.requestId.slice(0, 8) + "…"}
          />
        )}
      </div>
      <Separator />
      <div className="text-sm whitespace-pre-wrap leading-relaxed">
        {result.text}
      </div>
    </div>
  );
}

// ── Multi-model result card ────────────────────────────────────────────────────

function MultiResultCard({ result }: { result: MultiModelResult }) {
  return (
    <Card className="flex flex-col">
      <CardHeader className="pb-2 pt-4 px-4">
        <div className="flex items-center justify-between gap-2">
          <Badge variant="secondary" className="font-mono text-xs truncate max-w-[180px]">
            {result.model}
          </Badge>
          <div className="flex items-center gap-2 shrink-0">
            {result.error ? (
              <Badge variant="destructive" className="text-xs">error</Badge>
            ) : (
              cacheHitBadge(result.cache_hit)
            )}
          </div>
        </div>
        {!result.error && (
          <div className="flex flex-wrap gap-3 pt-1">
            <span className="text-xs text-muted-foreground flex items-center gap-1">
              <Clock className="h-3 w-3" />
              {result.latency_ms.toLocaleString()} ms
            </span>
            <span className="text-xs text-muted-foreground flex items-center gap-1">
              <Plus className="h-3 w-3" />
              {result.prompt_tokens + result.completion_tokens} tokens
            </span>
            {result.cost_usd && parseFloat(result.cost_usd) > 0 && (
              <span className="text-xs text-muted-foreground flex items-center gap-1">
                <DollarSign className="h-3 w-3" />
                ${parseFloat(result.cost_usd).toFixed(6)}
              </span>
            )}
          </div>
        )}
      </CardHeader>
      <Separator />
      <CardContent className="pt-3 pb-4 px-4 flex-1">
        {result.error ? (
          <div className="flex items-start gap-2 text-destructive text-sm">
            <AlertCircle className="h-4 w-4 mt-0.5 shrink-0" />
            <span className="break-words">{result.error}</span>
          </div>
        ) : (
          <p className="text-sm whitespace-pre-wrap leading-relaxed">
            {result.text ?? ""}
          </p>
        )}
      </CardContent>
    </Card>
  );
}

// ── Model checkbox selector ────────────────────────────────────────────────────

function ModelCheckboxGrid({
  selected,
  onChange,
}: {
  selected: string[];
  onChange: (models: string[]) => void;
}) {
  const [customInput, setCustomInput] = useState("");

  function toggle(m: string) {
    onChange(
      selected.includes(m) ? selected.filter((x) => x !== m) : [...selected, m]
    );
  }

  function addCustom() {
    const m = customInput.trim();
    if (!m || selected.includes(m)) return;
    onChange([...selected, m]);
    setCustomInput("");
  }

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-2 gap-2">
        {COMMON_MODELS.map((m) => {
          const checked = selected.includes(m);
          return (
            <button
              key={m}
              type="button"
              onClick={() => toggle(m)}
              className={`flex items-center gap-2 px-3 py-2 rounded-md border text-sm text-left transition-colors ${
                checked
                  ? "bg-primary/10 border-primary/40 text-foreground"
                  : "border-border text-muted-foreground hover:border-muted-foreground"
              }`}
            >
              {checked ? (
                <CheckSquare className="h-3.5 w-3.5 text-primary shrink-0" />
              ) : (
                <Square className="h-3.5 w-3.5 shrink-0" />
              )}
              <span className="truncate font-mono text-xs">{m}</span>
            </button>
          );
        })}
      </div>

      {/* Custom model addition */}
      <div className="flex gap-2">
        <Input
          placeholder="Custom model ID…"
          value={customInput}
          onChange={(e) => setCustomInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && addCustom()}
          className="text-sm h-8"
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={addCustom}
          disabled={!customInput.trim()}
          className="h-8"
        >
          <Plus className="h-3.5 w-3.5" />
        </Button>
      </div>

      {/* Custom models already added (not in COMMON_MODELS) */}
      {selected.filter((m) => !COMMON_MODELS.includes(m)).map((m) => (
        <div
          key={m}
          className="flex items-center justify-between px-3 py-1.5 rounded-md bg-muted text-xs font-mono"
        >
          <span className="truncate">{m}</span>
          <button
            type="button"
            onClick={() => onChange(selected.filter((x) => x !== m))}
            className="ml-2 text-muted-foreground hover:text-destructive"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}

// ── Page ──────────────────────────────────────────────────────────────────────

export default function PlaygroundPage() {
  const [mode, setMode] = useState<"single" | "compare">("single");

  // Single mode state
  const [model, setModel] = useState("gpt-4o-mini");
  const [customModel, setCustomModel] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<{ text: string; meta: PlaygroundMeta } | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [expandedHistory, setExpandedHistory] = useState<string | null>(null);

  // Compare mode state
  const [selectedKeyId, setSelectedKeyId] = useState<string | null>(null);
  const [selectedModels, setSelectedModels] = useState<string[]>([
    "gpt-4o",
    "gpt-4o-mini",
  ]);
  const [multiResults, setMultiResults] = useState<MultiModelResult[] | null>(null);
  const [multiLoading, setMultiLoading] = useState(false);
  const [multiError, setMultiError] = useState<string | null>(null);

  // Shared state
  const [systemPrompt, setSystemPrompt] = useState("");
  const [userMessage, setUserMessage] = useState("");
  const [skipCache, setSkipCache] = useState(false);

  const resultRef = useRef<HTMLDivElement>(null);
  const multiResultRef = useRef<HTMLDivElement>(null);

  const providersQ = useQuery({
    queryKey: ["providers"],
    queryFn: () => providers.list(),
  });

  const keysQ = useQuery({
    queryKey: ["keys"],
    queryFn: () => keysApi.list(1, 100),
  });

  const allKeys = keysQ.data?.data ?? [];
  const selectedKey = allKeys.find((k) => k.id === selectedKeyId) ?? null;

  function handleKeySelect(keyId: string) {
    setSelectedKeyId(keyId);
    const k = allKeys.find((x) => x.id === keyId);
    if (k?.allowed_models && k.allowed_models.length > 0) {
      setSelectedModels(k.allowed_models);
    }
  }

  const enabledProviders = (providersQ.data?.data ?? [])
    .filter((p) => p.is_enabled)
    .map((p) => p.id);

  const effectiveModel = model === "__custom__" ? customModel : model;

  // ── Single submit ────────────────────────────────────────────────────────────

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

    const body = { model: effectiveModel.trim(), messages, skip_cache: skipCache };

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
        throw new ApiError(
          res.status,
          errBody?.error?.message ?? `HTTP ${res.status}`
        );
      }

      const data = await res.json();
      const meta: PlaygroundMeta = {
        requestId: res.headers.get("x-janus-request-id") ?? "",
        provider: res.headers.get("x-janus-provider") ?? "",
        model: res.headers.get("x-janus-model") ?? effectiveModel,
        latencyMs: parseInt(res.headers.get("x-janus-latency-ms") ?? "0", 10),
        promptTokens: parseInt(
          res.headers.get("x-janus-prompt-tokens") ?? "0",
          10
        ),
        completionTokens: parseInt(
          res.headers.get("x-janus-completion-tokens") ?? "0",
          10
        ),
        costUsd: res.headers.get("x-janus-cost-usd"),
        cacheHit: res.headers.get("x-janus-cache-hit") ?? "none",
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

      setTimeout(
        () => resultRef.current?.scrollIntoView({ behavior: "smooth" }),
        100
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : "Unknown error");
    } finally {
      setLoading(false);
    }
  }

  // ── Compare submit ────────────────────────────────────────────────────────────

  async function handleMultiSubmit() {
    if (selectedModels.length === 0 || !userMessage.trim()) return;
    setMultiLoading(true);
    setMultiError(null);
    setMultiResults(null);

    const messages: Message[] = [];
    if (systemPrompt.trim()) {
      messages.push({ role: "system", content: systemPrompt.trim() });
    }
    messages.push({ role: "user", content: userMessage.trim() });

    const body = {
      models: selectedModels,
      messages,
      skip_cache: skipCache,
    };

    try {
      const token = getToken();
      const res = await fetch(`${BASE}/admin/playground/multi`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
        body: JSON.stringify(body),
      });

      if (!res.ok) {
        const errBody = await res.json().catch(() => ({}));
        throw new ApiError(
          res.status,
          errBody?.error?.message ?? `HTTP ${res.status}`
        );
      }

      const data = await res.json();
      setMultiResults(data.results ?? []);
      setTimeout(
        () =>
          multiResultRef.current?.scrollIntoView({ behavior: "smooth" }),
        100
      );
    } catch (e) {
      setMultiError(e instanceof Error ? e.message : "Unknown error");
    } finally {
      setMultiLoading(false);
    }
  }

  function loadFromHistory(entry: HistoryEntry) {
    setModel(COMMON_MODELS.includes(entry.model) ? entry.model : "__custom__");
    if (!COMMON_MODELS.includes(entry.model)) setCustomModel(entry.model);
    setUserMessage(entry.userMessage);
    setResult({ text: entry.responseText, meta: entry.meta });
  }

  // ── Shared prompt inputs ─────────────────────────────────────────────────────

  const promptInputs = (
    <div className="space-y-4">
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
    </div>
  );

  // ── Render ────────────────────────────────────────────────────────────────────

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold mb-1">Playground</h1>
          <p className="text-muted-foreground text-sm">
            Test prompts directly through the gateway. Authenticated as admin —
            no budget or rate limits applied.
          </p>
        </div>
        <Tabs
          value={mode}
          onValueChange={(v) => setMode(v as "single" | "compare")}
        >
          <TabsList>
            <TabsTrigger value="single" className="gap-1.5">
              <Send className="h-3.5 w-3.5" />
              Single
            </TabsTrigger>
            <TabsTrigger value="compare" className="gap-1.5">
              <GitCompare className="h-3.5 w-3.5" />
              Compare
            </TabsTrigger>
          </TabsList>
        </Tabs>
      </div>

      {/* ── SINGLE MODE ─────────────────────────────────────────────────── */}
      {mode === "single" && (
        <div className="grid lg:grid-cols-3 gap-6">
          <div className="lg:col-span-2 space-y-4">
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="text-base">Prompt</CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
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
                          <span className="text-xs text-muted-foreground">
                            None enabled
                          </span>
                        ) : (
                          enabledProviders.map((id) => (
                            <Badge
                              key={id}
                              variant="secondary"
                              className="text-xs"
                            >
                              {id}
                            </Badge>
                          ))
                        )}
                      </div>
                    </div>
                  )}
                </div>

                {promptInputs}

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
                    disabled={
                      loading || !effectiveModel.trim() || !userMessage.trim()
                    }
                    className="gap-2"
                  >
                    <Send className="h-3.5 w-3.5" />
                    {loading ? "Sending…" : "Send"}
                  </Button>
                </div>
              </CardContent>
            </Card>

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
                  {!loading && result && <SingleResult result={result} />}
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
      )}

      {/* ── COMPARE MODE ─────────────────────────────────────────────────── */}
      {mode === "compare" && (
        <div className="space-y-6">
          <div className="grid lg:grid-cols-3 gap-6">
            {/* Config panel */}
            <div className="space-y-4">
              <Card>
                <CardHeader className="pb-3">
                  <CardTitle className="text-base">Select key</CardTitle>
                </CardHeader>
                <CardContent className="space-y-3">
                  <Select
                    value={selectedKeyId ?? ""}
                    onValueChange={handleKeySelect}
                  >
                    <SelectTrigger>
                      <SelectValue placeholder="Pick an API key…" />
                    </SelectTrigger>
                    <SelectContent>
                      {allKeys
                        .filter((k) => k.is_active)
                        .map((k) => (
                          <SelectItem key={k.id} value={k.id}>
                            <span className="flex items-center gap-2">
                              <Key className="h-3 w-3 text-muted-foreground" />
                              {k.name}
                              <span className="font-mono text-xs text-muted-foreground">
                                {k.key_prefix}…
                              </span>
                            </span>
                          </SelectItem>
                        ))}
                    </SelectContent>
                  </Select>

                  {selectedKey && (
                    <div className="rounded-md bg-muted/50 px-3 py-2 text-xs space-y-1">
                      {selectedKey.allowed_models && selectedKey.allowed_models.length > 0 ? (
                        <>
                          <p className="text-muted-foreground">
                            Models loaded from key ({selectedKey.allowed_models.length}):
                          </p>
                          <div className="flex flex-wrap gap-1">
                            {selectedKey.allowed_models.map((m) => (
                              <Badge key={m} variant="secondary" className="font-mono text-xs">
                                {m}
                              </Badge>
                            ))}
                          </div>
                        </>
                      ) : (
                        <p className="text-muted-foreground">
                          This key has no model restriction — select models manually below.
                        </p>
                      )}
                    </div>
                  )}
                </CardContent>
              </Card>

              <Card>
                <CardHeader className="pb-3">
                  <CardTitle className="text-base">Models to compare</CardTitle>
                </CardHeader>
                <CardContent>
                  <ModelCheckboxGrid
                    selected={selectedModels}
                    onChange={setSelectedModels}
                  />
                  {selectedModels.length > 0 && (
                    <p className="text-xs text-muted-foreground mt-3">
                      {selectedModels.length} model
                      {selectedModels.length !== 1 ? "s" : ""} selected
                    </p>
                  )}
                </CardContent>
              </Card>
            </div>

            {/* Prompt panel */}
            <div className="lg:col-span-2 space-y-4">
              <Card>
                <CardHeader className="pb-3">
                  <CardTitle className="text-base">Prompt</CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                  {promptInputs}
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <Switch
                        id="skipCacheMulti"
                        checked={skipCache}
                        onCheckedChange={setSkipCache}
                      />
                      <Label
                        htmlFor="skipCacheMulti"
                        className="text-sm cursor-pointer"
                      >
                        Skip cache
                      </Label>
                    </div>
                    <Button
                      onClick={handleMultiSubmit}
                      disabled={
                        multiLoading ||
                        selectedModels.length === 0 ||
                        !userMessage.trim()
                      }
                      className="gap-2"
                    >
                      <GitCompare className="h-3.5 w-3.5" />
                      {multiLoading
                        ? `Querying ${selectedModels.length} models…`
                        : `Compare ${selectedModels.length} model${selectedModels.length !== 1 ? "s" : ""}`}
                    </Button>
                  </div>
                </CardContent>
              </Card>
            </div>
          </div>

          {/* Results */}
          {(multiLoading || multiError || multiResults) && (
            <div ref={multiResultRef} className="space-y-3">
              <div className="flex items-center justify-between">
                <h2 className="text-base font-medium">
                  Results
                  {multiResults && (
                    <span className="text-sm text-muted-foreground font-normal ml-2">
                      — {multiResults.filter((r) => !r.error).length}/
                      {multiResults.length} succeeded
                    </span>
                  )}
                </h2>
              </div>

              {multiLoading && (
                <div
                  className={`grid gap-4 ${
                    selectedModels.length >= 3
                      ? "md:grid-cols-3"
                      : selectedModels.length === 2
                      ? "md:grid-cols-2"
                      : "grid-cols-1"
                  }`}
                >
                  {selectedModels.map((m) => (
                    <Card key={m}>
                      <CardHeader className="pb-2 pt-4 px-4">
                        <Badge variant="secondary" className="font-mono text-xs w-fit">
                          {m}
                        </Badge>
                      </CardHeader>
                      <CardContent className="px-4 pb-4 space-y-2">
                        <Skeleton className="h-4 w-full" />
                        <Skeleton className="h-4 w-3/4" />
                        <Skeleton className="h-4 w-5/6" />
                      </CardContent>
                    </Card>
                  ))}
                </div>
              )}

              {multiError && (
                <Card className="border-destructive/50">
                  <CardContent className="pt-4 flex items-center gap-2 text-destructive text-sm">
                    <AlertCircle className="h-4 w-4 shrink-0" />
                    {multiError}
                  </CardContent>
                </Card>
              )}

              {!multiLoading && multiResults && (
                <div
                  className={`grid gap-4 ${
                    multiResults.length >= 3
                      ? "md:grid-cols-3"
                      : multiResults.length === 2
                      ? "md:grid-cols-2"
                      : "grid-cols-1"
                  }`}
                >
                  {multiResults.map((r) => (
                    <MultiResultCard key={r.model} result={r} />
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
