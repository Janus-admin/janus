"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { analytics, cache, type CostByTagGroup } from "@/lib/api";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  XAxis,
  YAxis,
  ResponsiveContainer,
} from "recharts";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
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
import { format, parseISO } from "date-fns";
import { Trash2 } from "lucide-react";

const COST_DAYS = [7, 14, 30] as const;
type CostDays = (typeof COST_DAYS)[number];

const LATENCY_HOURS = [6, 24, 72] as const;
type LatencyHours = (typeof LATENCY_HOURS)[number];

const CHART_COLORS = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--chart-5)",
];

const costChartConfig = {
  cost: { label: "Cost (USD)", color: "var(--chart-1)" },
} satisfies ChartConfig;

const tagChartConfig = {
  cost: { label: "Cost (USD)", color: "var(--chart-3)" },
} satisfies ChartConfig;

const providerChartConfig = {
  cost: { label: "Cost (USD)", color: "var(--chart-2)" },
} satisfies ChartConfig;

export default function AnalyticsPage() {
  const qc = useQueryClient();
  const [costDays, setCostDays] = useState<CostDays>(30);
  const [latencyHours, setLatencyHours] = useState<LatencyHours>(24);
  const [flushOpen, setFlushOpen] = useState(false);
  const [tagKey, setTagKey] = useState("team");
  const [tagInput, setTagInput] = useState("team");

  const costsQ = useQuery({
    queryKey: ["analytics", "costs", costDays],
    queryFn: () => analytics.costs(costDays),
  });

  const latencyQ = useQuery({
    queryKey: ["analytics", "latency", latencyHours],
    queryFn: () => analytics.latency(latencyHours),
  });

  const cacheQ = useQuery({
    queryKey: ["analytics", "cache", 24],
    queryFn: () => analytics.cache(24),
    refetchInterval: 30_000,
  });

  const cacheStatQ = useQuery({
    queryKey: ["cache", "stats"],
    queryFn: () => cache.stats(),
    refetchInterval: 30_000,
  });

  const tagQ = useQuery({
    queryKey: ["analytics", "cost-by-tag", tagKey, costDays],
    queryFn: () => analytics.costByTag(tagKey, costDays),
    enabled: /^[a-zA-Z0-9_]+$/.test(tagKey),
  });

  const flushMut = useMutation({
    mutationFn: () => cache.flush(),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["cache"] });
      qc.invalidateQueries({ queryKey: ["analytics", "cache"] });
      setFlushOpen(false);
    },
  });

  const dailyData = (costsQ.data?.by_day ?? []).map((r) => ({
    day: format(parseISO(r.day), "MMM d"),
    cost: r.cost_usd ?? 0,
    requests: r.requests,
  }));

  const providerData = (costsQ.data?.by_provider ?? []).map((r) => ({
    name: r.group_key,
    cost: r.cost_usd ?? 0,
    requests: r.requests,
  }));

  const modelData = (costsQ.data?.by_model ?? [])
    .slice(0, 8)
    .map((r) => ({
      name: r.group_key.length > 24 ? r.group_key.slice(0, 24) + "…" : r.group_key,
      cost: r.cost_usd ?? 0,
    }));

  const latencyRows = latencyQ.data?.data ?? [];
  const cacheData = cacheQ.data;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-semibold mb-1">Analytics</h1>
        <p className="text-muted-foreground text-sm">
          Cost breakdowns, latency percentiles, and cache efficiency.
        </p>
      </div>

      {/* Cost section */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-medium">Cost breakdown</h2>
          <Tabs
            value={String(costDays)}
            onValueChange={(v) => setCostDays(Number(v) as CostDays)}
          >
            <TabsList className="h-8">
              {COST_DAYS.map((d) => (
                <TabsTrigger key={d} value={String(d)} className="text-xs px-3">
                  {d}d
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
        </div>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">Daily cost</CardTitle>
          </CardHeader>
          <CardContent>
            {costsQ.isLoading ? (
              <Skeleton className="h-44 w-full" />
            ) : (
              <ChartContainer config={costChartConfig} className="h-44 w-full">
                <ResponsiveContainer width="100%" height="100%">
                  <AreaChart data={dailyData}>
                    <defs>
                      <linearGradient id="aGrad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor="var(--chart-1)" stopOpacity={0.25} />
                        <stop offset="95%" stopColor="var(--chart-1)" stopOpacity={0} />
                      </linearGradient>
                    </defs>
                    <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                    <XAxis
                      dataKey="day"
                      tick={{ fontSize: 11 }}
                      tickLine={false}
                      axisLine={false}
                      interval="preserveStartEnd"
                    />
                    <YAxis
                      tick={{ fontSize: 11 }}
                      tickLine={false}
                      axisLine={false}
                      tickFormatter={(v: number) => `$${v.toFixed(3)}`}
                      width={64}
                    />
                    <ChartTooltip
                      content={
                        <ChartTooltipContent
                          formatter={(value) => [
                            `$${Number(value).toFixed(4)}`,
                            "Cost",
                          ]}
                        />
                      }
                    />
                    <Area
                      type="monotone"
                      dataKey="cost"
                      stroke="var(--chart-1)"
                      fill="url(#aGrad)"
                      strokeWidth={2}
                      dot={false}
                    />
                  </AreaChart>
                </ResponsiveContainer>
              </ChartContainer>
            )}
          </CardContent>
        </Card>

        <div className="grid md:grid-cols-2 gap-4">
          {/* By provider */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium">By provider</CardTitle>
            </CardHeader>
            <CardContent>
              {costsQ.isLoading ? (
                <Skeleton className="h-36 w-full" />
              ) : (
                <ChartContainer
                  config={providerChartConfig}
                  className="h-36 w-full"
                >
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart data={providerData} layout="vertical">
                      <CartesianGrid
                        strokeDasharray="3 3"
                        className="stroke-border"
                        horizontal={false}
                      />
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
                        tick={{ fontSize: 11 }}
                        tickLine={false}
                        axisLine={false}
                        width={80}
                      />
                      <ChartTooltip
                        content={
                          <ChartTooltipContent
                            formatter={(value) => [
                              `$${Number(value).toFixed(4)}`,
                              "Cost",
                            ]}
                          />
                        }
                      />
                      <Bar dataKey="cost" radius={[0, 4, 4, 0]}>
                        {providerData.map((_, i) => (
                          <Cell
                            key={i}
                            fill={CHART_COLORS[i % CHART_COLORS.length]}
                          />
                        ))}
                      </Bar>
                    </BarChart>
                  </ResponsiveContainer>
                </ChartContainer>
              )}
            </CardContent>
          </Card>

          {/* By model */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium">By model (top 8)</CardTitle>
            </CardHeader>
            <CardContent>
              {costsQ.isLoading ? (
                <Skeleton className="h-36 w-full" />
              ) : (
                <ChartContainer
                  config={providerChartConfig}
                  className="h-36 w-full"
                >
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart data={modelData} layout="vertical">
                      <CartesianGrid
                        strokeDasharray="3 3"
                        className="stroke-border"
                        horizontal={false}
                      />
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
                        width={100}
                      />
                      <ChartTooltip
                        content={
                          <ChartTooltipContent
                            formatter={(value) => [
                              `$${Number(value).toFixed(4)}`,
                              "Cost",
                            ]}
                          />
                        }
                      />
                      <Bar dataKey="cost" fill="var(--chart-3)" radius={[0, 4, 4, 0]} />
                    </BarChart>
                  </ResponsiveContainer>
                </ChartContainer>
              )}
            </CardContent>
          </Card>
        </div>
      </div>

      {/* Latency section */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-medium">Latency percentiles</h2>
          <Tabs
            value={String(latencyHours)}
            onValueChange={(v) =>
              setLatencyHours(Number(v) as LatencyHours)
            }
          >
            <TabsList className="h-8">
              {LATENCY_HOURS.map((h) => (
                <TabsTrigger key={h} value={String(h)} className="text-xs px-3">
                  {h}h
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
        </div>

        <Card>
          <CardContent className="p-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Model</TableHead>
                  <TableHead>Provider</TableHead>
                  <TableHead className="text-right">p50</TableHead>
                  <TableHead className="text-right">p95</TableHead>
                  <TableHead className="text-right">p99</TableHead>
                  <TableHead className="text-right">Avg</TableHead>
                  <TableHead className="text-right">Samples</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {latencyQ.isLoading ? (
                  Array.from({ length: 4 }).map((_, i) => (
                    <TableRow key={i}>
                      {Array.from({ length: 7 }).map((_, j) => (
                        <TableCell key={j}>
                          <Skeleton className="h-4 w-full" />
                        </TableCell>
                      ))}
                    </TableRow>
                  ))
                ) : latencyRows.length === 0 ? (
                  <TableRow>
                    <TableCell
                      colSpan={7}
                      className="text-center py-8 text-muted-foreground text-sm"
                    >
                      No data for this window yet.
                    </TableCell>
                  </TableRow>
                ) : (
                  latencyRows.map((r, i) => (
                    <TableRow key={i}>
                      <TableCell className="text-sm font-medium max-w-48 truncate">
                        {r.model}
                      </TableCell>
                      <TableCell className="text-sm text-muted-foreground">
                        {r.provider}
                      </TableCell>
                      <TableCell className="text-right tabular-nums text-sm">
                        {r.p50 != null ? `${Math.round(r.p50)} ms` : "—"}
                      </TableCell>
                      <TableCell className="text-right tabular-nums text-sm">
                        {r.p95 != null ? `${Math.round(r.p95)} ms` : "—"}
                      </TableCell>
                      <TableCell className="text-right tabular-nums text-sm">
                        {r.p99 != null ? `${Math.round(r.p99)} ms` : "—"}
                      </TableCell>
                      <TableCell className="text-right tabular-nums text-sm">
                        {r.avg_ms != null ? `${Math.round(r.avg_ms)} ms` : "—"}
                      </TableCell>
                      <TableCell className="text-right tabular-nums text-sm text-muted-foreground">
                        {r.sample_count.toLocaleString()}
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      </div>

      {/* Cache section */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-medium">Cache analytics (last 24h)</h2>
          {cacheStatQ.data && (
            <div className="flex items-center gap-3 text-sm text-muted-foreground">
              <span>
                {cacheStatQ.data.data.total_tokens_saved.toLocaleString()} tokens saved
              </span>
              <span>·</span>
              <span>
                ${cacheStatQ.data.data.total_cost_saved.toFixed(4)} cost saved
              </span>
              <Button
                variant="outline"
                size="sm"
                onClick={() => setFlushOpen(true)}
                className="text-destructive border-destructive/30 hover:bg-destructive/10"
              >
                <Trash2 className="h-3.5 w-3.5 mr-1" />
                Flush cache
              </Button>
            </div>
          )}
        </div>
        <div className="grid md:grid-cols-3 gap-4">
          <Card>
            <CardContent className="pt-6">
              <p className="text-sm text-muted-foreground">Hit rate</p>
              <p className="text-3xl font-bold tabular-nums mt-1">
                {cacheData
                  ? `${(cacheData.hit_rate * 100).toFixed(1)}%`
                  : "—"}
              </p>
              <p className="text-xs text-muted-foreground mt-1">
                {cacheData?.total_hits.toLocaleString() ?? "—"} hits /{" "}
                {cacheData?.total_requests.toLocaleString() ?? "—"} requests
              </p>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="pt-6">
              <p className="text-sm text-muted-foreground">
                Avg semantic similarity
              </p>
              <p className="text-3xl font-bold tabular-nums mt-1">
                {cacheData?.avg_semantic_similarity != null
                  ? cacheData.avg_semantic_similarity.toFixed(3)
                  : "—"}
              </p>
              <p className="text-xs text-muted-foreground mt-1">
                cosine similarity score
              </p>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="pt-6">
              <p className="text-sm text-muted-foreground mb-3">
                Hits by type
              </p>
              {(cacheData?.by_type ?? []).length === 0 ? (
                <p className="text-sm text-muted-foreground">No cache hits</p>
              ) : (
                <div className="space-y-1.5">
                  {(cacheData?.by_type ?? []).map((row) => (
                    <div
                      key={row.cache_type}
                      className="flex items-center justify-between text-sm"
                    >
                      <span className="text-muted-foreground capitalize">
                        {row.cache_type}
                      </span>
                      <span className="tabular-nums font-medium">
                        {row.hit_count.toLocaleString()}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      </div>

      {/* Cost by tag section */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-lg font-medium">Cost by tag</h2>
            <p className="text-muted-foreground text-xs mt-0.5">
              Breakdown by any tag key sent via <code>metadata</code> field or{" "}
              <code>X-Janus-Tags</code> header.
            </p>
          </div>
          <form
            className="flex items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (/^[a-zA-Z0-9_]+$/.test(tagInput)) setTagKey(tagInput);
            }}
          >
            <Input
              value={tagInput}
              onChange={(e) => setTagInput(e.target.value)}
              placeholder="tag key (e.g. team)"
              className="h-8 w-40 text-sm"
            />
            <Button type="submit" size="sm" variant="outline" className="h-8 text-xs px-3">
              Apply
            </Button>
          </form>
        </div>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">
              Cost by <code className="text-xs bg-muted px-1 py-0.5 rounded">{tagKey}</code> — last {costDays}d
            </CardTitle>
            <CardDescription className="text-xs">
              {tagQ.data?.data.total_cost_usd != null
                ? `Total: $${tagQ.data.data.total_cost_usd.toFixed(4)}`
                : ""}
            </CardDescription>
          </CardHeader>
          <CardContent>
            {tagQ.isLoading ? (
              <Skeleton className="h-44 w-full" />
            ) : !tagQ.data?.data.groups.length ? (
              <p className="text-center text-muted-foreground text-sm py-10">
                No tagged requests in this window. Send requests with{" "}
                <code className="bg-muted px-1 rounded">X-Janus-Tags: {tagKey}=value</code>.
              </p>
            ) : (
              <ChartContainer config={tagChartConfig} className="h-44 w-full">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart
                    data={(tagQ.data?.data.groups ?? []).map((g: CostByTagGroup) => ({
                      name: g.tag_value ?? "(untagged)",
                      cost: g.cost_usd,
                      requests: g.request_count,
                    }))}
                    layout="vertical"
                  >
                    <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                    <XAxis type="number" tickFormatter={(v) => `$${v.toFixed(3)}`} className="text-xs" />
                    <YAxis type="category" dataKey="name" width={90} className="text-xs" />
                    <ChartTooltip
                      content={
                        <ChartTooltipContent
                          formatter={(value) => [`$${Number(value).toFixed(4)}`, "cost"]}
                        />
                      }
                    />
                    <Bar dataKey="cost" fill="var(--chart-3)" radius={[0, 4, 4, 0]} />
                  </BarChart>
                </ResponsiveContainer>
              </ChartContainer>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Flush cache confirmation */}
      <AlertDialog open={flushOpen} onOpenChange={setFlushOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Flush all cache entries?</AlertDialogTitle>
            <AlertDialogDescription>
              This clears both the in-memory hot layer and all persisted cache entries in PostgreSQL.
              Requests will hit providers until the cache warms up again.
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
