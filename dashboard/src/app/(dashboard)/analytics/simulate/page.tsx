"use client";

import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { simulate, adminModelsApi, type SimulateResult, type ModelWithPricing } from "@/lib/api";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import {
  BarChart,
  Bar,
  CartesianGrid,
  XAxis,
  YAxis,
  ResponsiveContainer,
} from "recharts";
import { Plus, Trash2, Play, TrendingDown, TrendingUp, ChevronsUpDown } from "lucide-react";

const chartConfig = {
  original: { label: "Original", color: "var(--chart-1)" },
  simulated: { label: "Simulated", color: "var(--chart-2)" },
} satisfies ChartConfig;

interface ModelOverride {
  from: string;
  to: string;
}

// ── Shared model picker helpers ───────────────────────────────────────────────

type QualityTier = "S" | "A" | "B" | "C";

const QUALITY_MAP: Record<string, QualityTier> = {
  "claude-opus-4-7": "S", "claude-opus-4-5": "S", "claude-3-opus-20240229": "S",
  "o3": "S", "gemini-2.5-pro": "S",
  "claude-sonnet-4-6": "A", "claude-sonnet-4-5": "A", "claude-3-5-sonnet-20241022": "A",
  "gpt-4.1": "A", "gpt-4o": "A", "gpt-4-turbo": "A", "o1": "A", "o4-mini": "A",
  "gemini-2.5-flash": "A", "gemini-1.5-pro": "A",
  "deepseek-r1": "A", "deepseek-reasoner": "A",
  "claude-haiku-4-5-20251001": "B", "claude-haiku-4-5": "B",
  "claude-3-5-haiku-20241022": "B", "claude-3-haiku-20240307": "B",
  "gpt-4.1-mini": "B", "gpt-4o-mini": "B", "o3-mini": "B", "o1-mini": "B",
  "gemini-2.0-flash": "B", "gemini-1.5-flash": "B",
  "llama-3.3-70b-versatile": "B", "llama-3.1-70b-versatile": "B",
  "llama3-70b-8192": "B", "llama-3.2-90b-vision-preview": "B",
  "qwen-qwq-32b": "B", "qwen-2.5-coder-32b": "B",
  "mixtral-8x7b-32768": "B", "deepseek-chat": "B",
};

const TIER_STYLE: Record<QualityTier, string> = {
  S: "bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300",
  A: "bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300",
  B: "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300",
  C: "bg-muted text-muted-foreground",
};

function getTier(modelId: string): QualityTier {
  return QUALITY_MAP[modelId] ?? "C";
}

function TierBadge({ tier }: { tier: QualityTier }) {
  return (
    <span className={`inline-flex items-center rounded px-1 py-0 text-[10px] font-bold leading-4 ${TIER_STYLE[tier]}`}>
      {tier}
    </span>
  );
}

const POWER_ORDER: Record<string, string[]> = {
  anthropic: [
    "claude-opus-4-7", "claude-opus-4-5", "claude-sonnet-4-6", "claude-sonnet-4-5",
    "claude-3-5-sonnet-20241022", "claude-3-opus-20240229",
    "claude-haiku-4-5-20251001", "claude-haiku-4-5",
    "claude-3-5-haiku-20241022", "claude-3-haiku-20240307",
  ],
  openai: [
    "o3", "o4-mini", "o1", "gpt-4.1", "gpt-4o", "gpt-4-turbo",
    "o1-mini", "o3-mini", "gpt-4.1-mini", "gpt-3.5-turbo", "gpt-4.1-nano",
  ],
  gemini: [
    "gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash",
    "gemini-1.5-pro", "gemini-2.0-flash-lite", "gemini-1.5-flash",
  ],
  groq: [
    "llama-3.3-70b-versatile", "llama-3.1-70b-versatile",
    "llama-3.2-90b-vision-preview", "qwen-qwq-32b", "qwen-2.5-coder-32b",
    "mixtral-8x7b-32768", "llama3-70b-8192", "llama-3.2-11b-vision-preview",
    "gemma2-9b-it", "llama3-8b-8192", "llama-3.1-8b-instant",
    "llama-3.2-3b-preview", "llama-3.2-1b-preview",
  ],
  deepseek: ["deepseek-r1", "deepseek-reasoner", "deepseek-chat"],
  bedrock: [
    "anthropic.claude-sonnet-4-5", "anthropic.claude-3-5-sonnet-20241022-v2:0",
    "meta.llama3-2-90b-instruct-v1:0", "meta.llama3-1-70b-instruct-v1:0",
    "anthropic.claude-3-haiku-20240307-v1:0", "meta.llama3-70b-instruct-v1:0",
    "amazon.titan-text-express-v1",
  ],
};

const PROVIDER_ORDER = ["anthropic", "openai", "gemini", "groq", "deepseek", "bedrock"];

function sortByPower(provider: string, models: ModelWithPricing[]): ModelWithPricing[] {
  const order = POWER_ORDER[provider] ?? [];
  return [...models].sort((a, b) => {
    const ia = order.indexOf(a.model_id);
    const ib = order.indexOf(b.model_id);
    if (ia === -1 && ib === -1) return a.model_id.localeCompare(b.model_id);
    if (ia === -1) return 1;
    if (ib === -1) return -1;
    return ia - ib;
  });
}

function ModelCombobox({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
}) {
  const [open, setOpen] = useState(false);

  const { data } = useQuery({
    queryKey: ["admin-models"],
    queryFn: () => adminModelsApi.list(),
    staleTime: 5 * 60 * 1000,
  });

  const available = data?.data ?? [];
  const grouped = available.reduce<Record<string, ModelWithPricing[]>>((acc, m) => {
    if (!acc[m.provider]) acc[m.provider] = [];
    acc[m.provider].push(m);
    return acc;
  }, {});
  const sortedProviders = [
    ...PROVIDER_ORDER.filter((p) => grouped[p]),
    ...Object.keys(grouped).filter((p) => !PROVIDER_ORDER.includes(p)).sort(),
  ];

  const selected = available.find((m) => m.model_id === value);
  const tier = value ? getTier(value) : null;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          role="combobox"
          aria-expanded={open}
          className="h-8 w-full justify-between font-normal text-sm"
        >
          {value ? (
            <span className="flex items-center gap-1.5 truncate">
              {tier && <TierBadge tier={tier} />}
              <span className="font-mono text-xs truncate">
                {selected?.model_display_name ?? value}
              </span>
            </span>
          ) : (
            <span className="text-muted-foreground truncate">{placeholder}</span>
          )}
          <ChevronsUpDown className="h-3.5 w-3.5 shrink-0 opacity-50 ml-1" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[380px] p-0" align="start">
        <Command>
          <CommandInput placeholder="Search models…" />
          <div
            className="h-72 overflow-y-auto py-1"
            onWheel={(e) => e.stopPropagation()}
          >
            <CommandList className="max-h-none overflow-visible">
              <CommandEmpty>No models found.</CommandEmpty>
              {sortedProviders.map((provider) => (
                <CommandGroup key={provider} heading={provider}>
                  {sortByPower(provider, grouped[provider]).map((m) => {
                    const t = getTier(m.model_id);
                    const fmt = (n: number) => n < 1 ? `$${n.toFixed(3)}` : `$${n.toFixed(2)}`;
                    return (
                      <CommandItem
                        key={m.model_id}
                        value={`${m.model_id} ${m.model_display_name ?? ""}`}
                        onSelect={() => { onChange(m.model_id); setOpen(false); }}
                        className="flex items-center gap-2 cursor-pointer"
                      >
                        <TierBadge tier={t} />
                        <span className="font-mono text-xs flex-1 truncate">
                          {m.model_display_name ?? m.model_id}
                        </span>
                        <span className="text-[10px] text-muted-foreground tabular-nums whitespace-nowrap">
                          {fmt(m.input_per_1m_tokens)} / {fmt(m.output_per_1m_tokens)}
                        </span>
                      </CommandItem>
                    );
                  })}
                </CommandGroup>
              ))}
            </CommandList>
          </div>
          <div className="border-t px-3 py-1.5 text-[10px] text-muted-foreground flex gap-3">
            <span className="flex items-center gap-1"><TierBadge tier="S" /> Flagship</span>
            <span className="flex items-center gap-1"><TierBadge tier="A" /> High</span>
            <span className="flex items-center gap-1"><TierBadge tier="B" /> Mid</span>
            <span className="flex items-center gap-1"><TierBadge tier="C" /> Fast</span>
            <span className="ml-auto">in / out per 1M</span>
          </div>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

// ── Delta badge ───────────────────────────────────────────────────────────────

function deltaBadge(original: number, simulated: number) {
  const delta = simulated - original;
  const pct = original > 0 ? (Math.abs(delta) / original) * 100 : 0;
  if (Math.abs(delta) < 0.000001) return <Badge variant="secondary" className="text-xs">no change</Badge>;
  if (delta < 0)
    return (
      <Badge variant="outline" className="text-green-600 border-green-600/30 text-xs gap-1">
        <TrendingDown className="h-3 w-3" />
        -{pct.toFixed(1)}%
      </Badge>
    );
  return (
    <Badge variant="outline" className="text-red-600 border-red-600/30 text-xs gap-1">
      <TrendingUp className="h-3 w-3" />
      +{pct.toFixed(1)}%
    </Badge>
  );
}

// ── Page ──────────────────────────────────────────────────────────────────────

export default function SimulatePage() {
  const [strategy, setStrategy] = useState("cost_optimized");
  const [period, setPeriod] = useState("30d");
  const [overrides, setOverrides] = useState<ModelOverride[]>([]);
  const [result, setResult] = useState<SimulateResult | null>(null);

  const mut = useMutation({
    mutationFn: () => {
      const overrideMap: Record<string, string> = {};
      overrides.forEach(({ from, to }) => {
        if (from.trim() && to.trim()) overrideMap[from.trim()] = to.trim();
      });
      return simulate.run(strategy, period, overrideMap);
    },
    onSuccess: (data) => setResult(data.data),
  });

  const chartData = result
    ? result.by_model.slice(0, 12).map((row) => ({
        name: row.model.length > 20 ? row.model.slice(0, 20) + "…" : row.model,
        original: row.original_cost_usd,
        simulated: row.simulated_cost_usd,
      }))
    : [];

  function addOverride() {
    setOverrides((prev) => [...prev, { from: "", to: "" }]);
  }

  function removeOverride(i: number) {
    setOverrides((prev) => prev.filter((_, j) => j !== i));
  }

  function updateOverride(i: number, field: "from" | "to", value: string) {
    setOverrides((prev) =>
      prev.map((o, j) => (j === i ? { ...o, [field]: value } : o))
    );
  }

  const savings = result ? result.savings_usd : 0;
  const savingsPct = result ? result.savings_percent : 0;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-semibold mb-1">Cost Simulator</h1>
        <p className="text-muted-foreground text-sm">
          Replay past requests under a different routing strategy to estimate savings.
        </p>
      </div>

      {/* Config card */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Simulation parameters</CardTitle>
          <CardDescription>
            Choose a strategy and period, then optionally substitute specific models.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-5">
          <div className="grid sm:grid-cols-2 gap-4">
            <div className="space-y-1.5">
              <Label>Routing strategy</Label>
              <Select value={strategy} onValueChange={setStrategy}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="cost_optimized">Cost optimized</SelectItem>
                  <SelectItem value="priority">Priority (current)</SelectItem>
                  <SelectItem value="round_robin">Round robin</SelectItem>
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {strategy === "cost_optimized" && "Re-routes each request to the cheapest provider for that model."}
                {strategy === "priority" && "Uses the same provider priority as today — baseline."}
                {strategy === "round_robin" && "Distributes evenly across all providers; uses average pricing."}
              </p>
            </div>
            <div className="space-y-1.5">
              <Label>Period</Label>
              <Select value={period} onValueChange={setPeriod}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="7d">Last 7 days</SelectItem>
                  <SelectItem value="30d">Last 30 days</SelectItem>
                  <SelectItem value="90d">Last 90 days</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Model overrides */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label>Model substitutions (optional)</Label>
              <Button variant="outline" size="sm" onClick={addOverride} className="h-7 text-xs gap-1">
                <Plus className="h-3 w-3" />
                Add
              </Button>
            </div>
            {overrides.length === 0 && (
              <p className="text-xs text-muted-foreground">
                No substitutions — uses actual models from the selected period.
              </p>
            )}
            {overrides.map((o, i) => (
              <div key={i} className="flex items-center gap-2">
                <div className="flex-1">
                  <ModelCombobox
                    value={o.from}
                    onChange={(v) => updateOverride(i, "from", v)}
                    placeholder="From model…"
                  />
                </div>
                <span className="text-muted-foreground text-sm shrink-0">→</span>
                <div className="flex-1">
                  <ModelCombobox
                    value={o.to}
                    onChange={(v) => updateOverride(i, "to", v)}
                    placeholder="To model…"
                  />
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 text-muted-foreground hover:text-destructive shrink-0"
                  onClick={() => removeOverride(i)}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
          </div>

          <Button onClick={() => mut.mutate()} disabled={mut.isPending} className="gap-2">
            <Play className="h-3.5 w-3.5" />
            {mut.isPending ? "Running…" : "Run simulation"}
          </Button>

          {mut.isError && (
            <p className="text-sm text-destructive">
              {(mut.error as Error)?.message ?? "Simulation failed"}
            </p>
          )}
        </CardContent>
      </Card>

      {/* Results */}
      {(mut.isPending || result) && (
        <>
          {/* Summary cards */}
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
            {[
              {
                label: "Original cost",
                value: mut.isPending ? null : `$${result!.original_cost_usd.toFixed(4)}`,
              },
              {
                label: "Simulated cost",
                value: mut.isPending ? null : `$${result!.simulated_cost_usd.toFixed(4)}`,
              },
              {
                label: "Savings",
                value: mut.isPending
                  ? null
                  : savings >= 0
                  ? `$${savings.toFixed(4)}`
                  : `-$${Math.abs(savings).toFixed(4)}`,
                accent: mut.isPending ? false : savings > 0,
                negative: mut.isPending ? false : savings < 0,
              },
              {
                label: "Requests analysed",
                value: mut.isPending ? null : result!.request_count.toLocaleString(),
              },
            ].map(({ label, value, accent, negative }) => (
              <Card key={label}>
                <CardContent className="pt-4">
                  <p className="text-xs text-muted-foreground">{label}</p>
                  {value === null ? (
                    <Skeleton className="h-7 w-20 mt-1" />
                  ) : (
                    <p
                      className={`text-xl font-semibold tabular-nums mt-1 ${
                        accent ? "text-green-600" : negative ? "text-red-600" : ""
                      }`}
                    >
                      {value}
                    </p>
                  )}
                </CardContent>
              </Card>
            ))}
          </div>

          {/* Savings badge */}
          {!mut.isPending && result && Math.abs(savingsPct) >= 0.01 && (
            <div className="flex items-center gap-2">
              {savings > 0 ? (
                <Badge className="bg-green-100 text-green-700 border-green-200 hover:bg-green-100 gap-1">
                  <TrendingDown className="h-3.5 w-3.5" />
                  {savingsPct.toFixed(1)}% cheaper with &ldquo;{result.strategy}&rdquo; over {result.period}
                </Badge>
              ) : (
                <Badge className="bg-red-100 text-red-700 border-red-200 hover:bg-red-100 gap-1">
                  <TrendingUp className="h-3.5 w-3.5" />
                  {Math.abs(savingsPct).toFixed(1)}% more expensive with &ldquo;{result.strategy}&rdquo; over {result.period}
                </Badge>
              )}
            </div>
          )}

          {/* Bar chart */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium">Original vs Simulated — by model</CardTitle>
            </CardHeader>
            <CardContent>
              {mut.isPending ? (
                <Skeleton className="h-52 w-full" />
              ) : chartData.length === 0 ? (
                <p className="text-sm text-muted-foreground py-4">No request data for this period.</p>
              ) : (
                <ChartContainer config={chartConfig} className="h-52 w-full">
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart data={chartData} layout="vertical">
                      <CartesianGrid strokeDasharray="3 3" className="stroke-border" horizontal={false} />
                      <XAxis
                        type="number"
                        tick={{ fontSize: 11 }}
                        tickLine={false}
                        axisLine={false}
                        tickFormatter={(v: number) => `$${v.toFixed(3)}`}
                      />
                      <YAxis
                        dataKey="name"
                        type="category"
                        tick={{ fontSize: 10 }}
                        tickLine={false}
                        axisLine={false}
                        width={120}
                      />
                      <ChartTooltip
                        content={
                          <ChartTooltipContent
                            formatter={(value) => [`$${Number(value).toFixed(6)}`, ""]}
                          />
                        }
                      />
                      <Bar dataKey="original" fill="var(--chart-1)" radius={[0, 3, 3, 0]} name="Original" />
                      <Bar dataKey="simulated" fill="var(--chart-2)" radius={[0, 3, 3, 0]} name="Simulated" />
                    </BarChart>
                  </ResponsiveContainer>
                </ChartContainer>
              )}
            </CardContent>
          </Card>

          {/* Per-model breakdown table */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium">Per-model breakdown</CardTitle>
            </CardHeader>
            <CardContent className="p-0">
              {mut.isPending ? (
                <div className="p-4 space-y-2">
                  {Array.from({ length: 5 }).map((_, i) => (
                    <Skeleton key={i} className="h-8 w-full" />
                  ))}
                </div>
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Model</TableHead>
                      <TableHead className="text-right">Requests</TableHead>
                      <TableHead className="text-right">Original</TableHead>
                      <TableHead className="text-right">Simulated</TableHead>
                      <TableHead className="text-right">Delta</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {(result?.by_model ?? []).length === 0 ? (
                      <TableRow>
                        <TableCell colSpan={5} className="text-center py-8 text-muted-foreground text-sm">
                          No data for this period.
                        </TableCell>
                      </TableRow>
                    ) : (
                      (result?.by_model ?? []).map((row) => (
                        <TableRow key={row.model}>
                          <TableCell className="font-medium text-sm max-w-48 truncate">
                            {row.model}
                          </TableCell>
                          <TableCell className="text-right tabular-nums text-sm text-muted-foreground">
                            {row.request_count.toLocaleString()}
                          </TableCell>
                          <TableCell className="text-right tabular-nums text-sm">
                            ${row.original_cost_usd.toFixed(6)}
                          </TableCell>
                          <TableCell className="text-right tabular-nums text-sm">
                            ${row.simulated_cost_usd.toFixed(6)}
                          </TableCell>
                          <TableCell className="text-right">
                            {deltaBadge(row.original_cost_usd, row.simulated_cost_usd)}
                          </TableCell>
                        </TableRow>
                      ))
                    )}
                  </TableBody>
                </Table>
              )}
            </CardContent>
          </Card>

          {/* Apply strategy shortcut hint */}
          {!mut.isPending && result && savings > 0 && (
            <Card className="border-green-500/30 bg-green-500/5">
              <CardContent className="pt-4 pb-3">
                <p className="text-sm font-medium text-green-700 dark:text-green-400">
                  Apply this strategy to new API keys
                </p>
                <p className="text-xs text-muted-foreground mt-1">
                  Go to <strong>API Keys → Create key</strong> and set routing strategy to{" "}
                  <strong>{result.strategy.replace("_", " ")}</strong> to start using the cheaper routing.
                </p>
              </CardContent>
            </Card>
          )}
        </>
      )}
    </div>
  );
}
