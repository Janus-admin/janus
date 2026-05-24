"use client";

import { useQuery } from "@tanstack/react-query";
import { analytics, requests, cache, providers, system } from "@/lib/api";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Badge } from "@/components/ui/badge";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import {
  Area,
  AreaChart,
  CartesianGrid,
  XAxis,
  YAxis,
  ResponsiveContainer,
} from "recharts";
import { format, parseISO } from "date-fns";
import {
  Activity,
  DollarSign,
  Zap,
  AlertCircle,
  Database,
  Clock,
  CheckCircle2,
  XCircle,
  HelpCircle,
  TrendingDown,
  TrendingUp,
  Minus,
  HeartPulse,
} from "lucide-react";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

const costChartConfig = {
  cost: { label: "Cost (USD)", color: "var(--chart-1)" },
} satisfies ChartConfig;

function fmtCost(n: number | null | undefined): string {
  if (n == null) return "—";
  return `$${n.toFixed(4)}`;
}

function fmtMs(n: number | null | undefined): string {
  if (n == null) return "—";
  return `${Math.round(n)} ms`;
}

function statusBadge(status: string) {
  if (status === "success")
    return (
      <Badge variant="outline" className="text-green-600 border-green-600/30">
        success
      </Badge>
    );
  if (status === "error") return <Badge variant="destructive">error</Badge>;
  return <Badge variant="secondary">{status}</Badge>;
}

function StatCard({
  icon: Icon,
  label,
  today,
  week,
  month,
  format: fmtFn,
}: {
  icon: React.ElementType;
  label: string;
  today: number | null | undefined;
  week: number | null | undefined;
  month: number | null | undefined;
  format: (n: number | null | undefined) => string;
}) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-center gap-2 pb-2 space-y-0">
        <Icon className="h-4 w-4 text-muted-foreground" />
        <CardTitle className="text-sm font-medium">{label}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold tabular-nums">{fmtFn(today)}</div>
        <p className="text-xs text-muted-foreground mt-1 space-x-3">
          <span>7d: {fmtFn(week)}</span>
          <span>30d: {fmtFn(month)}</span>
        </p>
      </CardContent>
    </Card>
  );
}

function HealthDot({ status }: { status: string }) {
  if (status === "healthy") return <CheckCircle2 className="h-3.5 w-3.5 text-green-500" />;
  if (status === "unhealthy") return <XCircle className="h-3.5 w-3.5 text-red-500" />;
  return <HelpCircle className="h-3.5 w-3.5 text-muted-foreground" />;
}

export default function OverviewPage() {
  const overviewQ = useQuery({
    queryKey: ["analytics", "overview"],
    queryFn: () => analytics.overview(),
    refetchInterval: 30_000,
  });

  const costsQ = useQuery({
    queryKey: ["analytics", "costs", 30],
    queryFn: () => analytics.costs(30),
    refetchInterval: 60_000,
  });

  const providersQ = useQuery({
    queryKey: ["providers"],
    queryFn: () => providers.list(),
    refetchInterval: 60_000,
  });

  const cacheQ = useQuery({
    queryKey: ["cache", "stats"],
    queryFn: () => cache.stats(),
    refetchInterval: 30_000,
  });

  const recentQ = useQuery({
    queryKey: ["requests", "recent"],
    queryFn: () => requests.list({ per_page: 10, page: 1 }),
    refetchInterval: 15_000,
  });

  const costs7Q = useQuery({
    queryKey: ["analytics", "costs", 7],
    queryFn: () => analytics.costs(7),
    refetchInterval: 60_000,
  });

  const readinessQ = useQuery({
    queryKey: ["system", "readiness"],
    queryFn: () => system.readiness(),
    refetchInterval: 30_000,
  });

  const ov = overviewQ.data;

  const chartData = (costsQ.data?.by_day ?? []).map((row) => ({
    day: format(parseISO(row.day), "MMM d"),
    cost: row.cost_usd ?? 0,
  }));

  // Derive yesterday's cost from the 7-day cost breakdown (second-to-last entry).
  const days7 = costs7Q.data?.by_day ?? [];
  const todayCost = ov?.today.cost_usd ?? null;
  const yesterdayCost =
    days7.length >= 2
      ? (days7[days7.length - 2]?.cost_usd ?? null)
      : null;
  const spendDelta =
    todayCost != null && yesterdayCost != null && yesterdayCost > 0
      ? ((todayCost - yesterdayCost) / yesterdayCost) * 100
      : null;

  const readinessReport = readinessQ.data?.data;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-semibold mb-1">Overview</h1>
        <p className="text-muted-foreground text-sm">
          Gateway activity at a glance.
        </p>
      </div>

      {overviewQ.isLoading ? (
        <div className="grid grid-cols-2 lg:grid-cols-3 gap-4">
          {Array.from({ length: 6 }).map((_, i) => (
            <Card key={i}>
              <CardHeader className="pb-2">
                <Skeleton className="h-4 w-24" />
              </CardHeader>
              <CardContent>
                <Skeleton className="h-8 w-20 mb-2" />
                <Skeleton className="h-3 w-32" />
              </CardContent>
            </Card>
          ))}
        </div>
      ) : (
        <div className="grid grid-cols-2 lg:grid-cols-3 gap-4">
          <StatCard
            icon={Activity}
            label="Requests"
            today={ov?.today.requests}
            week={ov?.last_7d.requests}
            month={ov?.last_30d.requests}
            format={(n) => (n == null ? "—" : n.toLocaleString())}
          />
          <StatCard
            icon={DollarSign}
            label="Cost"
            today={ov?.today.cost_usd}
            week={ov?.last_7d.cost_usd}
            month={ov?.last_30d.cost_usd}
            format={fmtCost}
          />
          <StatCard
            icon={Zap}
            label="Tokens"
            today={ov?.today.tokens}
            week={ov?.last_7d.tokens}
            month={ov?.last_30d.tokens}
            format={(n) =>
              n == null ? "—" : n > 1000 ? (n / 1000).toFixed(1) + "k" : String(n)
            }
          />
          <StatCard
            icon={Database}
            label="Cache hits"
            today={ov?.today.cache_hits}
            week={ov?.last_7d.cache_hits}
            month={ov?.last_30d.cache_hits}
            format={(n) => (n == null ? "—" : n.toLocaleString())}
          />
          <StatCard
            icon={AlertCircle}
            label="Errors"
            today={ov?.today.errors}
            week={ov?.last_7d.errors}
            month={ov?.last_30d.errors}
            format={(n) => (n == null ? "—" : n.toLocaleString())}
          />
          <StatCard
            icon={Clock}
            label="Avg latency"
            today={ov?.today.avg_latency_ms}
            week={ov?.last_7d.avg_latency_ms}
            month={ov?.last_30d.avg_latency_ms}
            format={fmtMs}
          />
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>Cost — last 30 days</CardTitle>
          <CardDescription>Daily USD spend across all providers</CardDescription>
        </CardHeader>
        <CardContent>
          {costsQ.isLoading ? (
            <Skeleton className="h-48 w-full" />
          ) : (
            <ChartContainer config={costChartConfig} className="h-48 w-full">
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={chartData}>
                  <defs>
                    <linearGradient id="costGrad" x1="0" y1="0" x2="0" y2="1">
                      <stop
                        offset="5%"
                        stopColor="var(--chart-1)"
                        stopOpacity={0.3}
                      />
                      <stop
                        offset="95%"
                        stopColor="var(--chart-1)"
                        stopOpacity={0}
                      />
                    </linearGradient>
                  </defs>
                  <CartesianGrid
                    strokeDasharray="3 3"
                    className="stroke-border"
                  />
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
                    fill="url(#costGrad)"
                    strokeWidth={2}
                    dot={false}
                  />
                </AreaChart>
              </ResponsiveContainer>
            </ChartContainer>
          )}
        </CardContent>
      </Card>

      {/* Operational panels */}
      <div className="grid md:grid-cols-3 gap-4">
        {/* Today vs yesterday spend */}
        <Card>
          <CardHeader className="flex flex-row items-center gap-2 pb-2 space-y-0">
            <DollarSign className="h-4 w-4 text-muted-foreground" />
            <CardTitle className="text-sm font-medium">Today vs yesterday</CardTitle>
          </CardHeader>
          <CardContent>
            {overviewQ.isLoading || costs7Q.isLoading ? (
              <Skeleton className="h-8 w-24" />
            ) : (
              <>
                <div className="text-2xl font-bold tabular-nums">{fmtCost(todayCost)}</div>
                <div className="flex items-center gap-1.5 mt-1 text-xs text-muted-foreground">
                  {spendDelta === null ? (
                    <span>Yesterday: {fmtCost(yesterdayCost)}</span>
                  ) : spendDelta === 0 ? (
                    <>
                      <Minus className="h-3 w-3" />
                      <span>Same as yesterday</span>
                    </>
                  ) : spendDelta < 0 ? (
                    <>
                      <TrendingDown className="h-3 w-3 text-green-500" />
                      <span className="text-green-600">{Math.abs(spendDelta).toFixed(1)}% vs yesterday</span>
                    </>
                  ) : (
                    <>
                      <TrendingUp className="h-3 w-3 text-red-500" />
                      <span className="text-red-600">+{spendDelta.toFixed(1)}% vs yesterday</span>
                    </>
                  )}
                </div>
              </>
            )}
          </CardContent>
        </Card>

        {/* Cache savings */}
        <Card>
          <CardHeader className="flex flex-row items-center gap-2 pb-2 space-y-0">
            <Database className="h-4 w-4 text-muted-foreground" />
            <CardTitle className="text-sm font-medium">Cache savings (all-time)</CardTitle>
          </CardHeader>
          <CardContent>
            {cacheQ.isLoading ? (
              <Skeleton className="h-8 w-24" />
            ) : (
              <>
                <div className="text-2xl font-bold tabular-nums text-green-600">
                  {cacheQ.data ? fmtCost(cacheQ.data.data.total_cost_saved) : "—"}
                </div>
                <p className="text-xs text-muted-foreground mt-1">
                  {cacheQ.data
                    ? `${cacheQ.data.data.total_tokens_saved.toLocaleString()} tokens saved`
                    : "—"}
                </p>
              </>
            )}
          </CardContent>
        </Card>

        {/* System health summary */}
        <Card>
          <CardHeader className="flex flex-row items-center gap-2 pb-2 space-y-0">
            <HeartPulse className="h-4 w-4 text-muted-foreground" />
            <CardTitle className="text-sm font-medium">System health</CardTitle>
          </CardHeader>
          <CardContent>
            {readinessQ.isLoading ? (
              <Skeleton className="h-8 w-24" />
            ) : (
              <>
                <div className="flex items-center gap-2">
                  {readinessReport?.healthy ? (
                    <CheckCircle2 className="h-6 w-6 text-green-500" />
                  ) : (
                    <XCircle className="h-6 w-6 text-red-500" />
                  )}
                  <span className="text-xl font-semibold">
                    {readinessReport?.healthy ? "Healthy" : "Degraded"}
                  </span>
                </div>
                <p className="text-xs text-muted-foreground mt-1">
                  {readinessReport
                    ? `${readinessReport.checks.filter((c) => c.status === "pass").length}/${readinessReport.checks.length} checks passing`
                    : "—"}
                  {readinessReport && readinessReport.warnings > 0
                    ? ` · ${readinessReport.warnings} warning${readinessReport.warnings !== 1 ? "s" : ""}`
                    : ""}
                </p>
              </>
            )}
          </CardContent>
        </Card>
      </div>

      {cacheQ.data && (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          {[
            {
              label: "Exact entries",
              value: cacheQ.data.data.exact_entries.toLocaleString(),
            },
            {
              label: "Semantic entries",
              value: cacheQ.data.data.semantic_entries.toLocaleString(),
            },
            {
              label: "Tokens saved",
              value: cacheQ.data.data.total_tokens_saved.toLocaleString(),
            },
            {
              label: "Cost saved",
              value: fmtCost(cacheQ.data.data.total_cost_saved),
            },
          ].map(({ label, value }) => (
            <Card key={label}>
              <CardContent className="pt-4">
                <p className="text-xs text-muted-foreground">{label}</p>
                <p className="text-xl font-semibold tabular-nums mt-0.5">
                  {value}
                </p>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* Provider health + top-5 models */}
      <div className="grid md:grid-cols-2 gap-4">
        {/* Provider health */}
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-sm font-medium">Provider health</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            {providersQ.isLoading ? (
              Array.from({ length: 3 }).map((_, i) => (
                <Skeleton key={i} className="h-8 w-full" />
              ))
            ) : (
              (providersQ.data?.data ?? []).map((p) => (
                <div key={p.id} className="flex items-center justify-between">
                  <div className="flex items-center gap-2 text-sm">
                    <HealthDot status={p.health_status} />
                    <span className={p.is_enabled ? "font-medium" : "text-muted-foreground"}>
                      {p.id.charAt(0).toUpperCase() + p.id.slice(1)}
                    </span>
                    {!p.is_enabled && (
                      <span className="text-xs text-muted-foreground">(disabled)</span>
                    )}
                  </div>
                  <span className="text-xs text-muted-foreground capitalize">
                    {p.health_status}
                  </span>
                </div>
              ))
            )}
          </CardContent>
        </Card>

        {/* Top-5 most expensive models today */}
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-sm font-medium">Top models by cost (30d)</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            {costsQ.isLoading ? (
              Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-6 w-full" />
              ))
            ) : (
              (costsQ.data?.by_model ?? []).slice(0, 5).map((m, i) => {
                const maxCost = costsQ.data?.by_model[0]?.cost_usd ?? 1;
                const pct = Math.round(((m.cost_usd ?? 0) / (maxCost || 1)) * 100);
                return (
                  <div key={m.group_key} className="space-y-0.5">
                    <div className="flex items-center justify-between text-sm">
                      <span className="truncate max-w-44 text-muted-foreground" title={m.group_key}>
                        {i + 1}. {m.group_key.length > 30 ? m.group_key.slice(0, 30) + "…" : m.group_key}
                      </span>
                      <span className="tabular-nums font-medium shrink-0 ml-2">
                        {fmtCost(m.cost_usd)}
                      </span>
                    </div>
                    <div className="h-1 w-full rounded-full bg-muted overflow-hidden">
                      <div
                        className="h-full rounded-full bg-chart-1"
                        style={{ width: `${pct}%`, backgroundColor: "var(--chart-1)" }}
                      />
                    </div>
                  </div>
                );
              })
            )}
            {!costsQ.isLoading && (costsQ.data?.by_model ?? []).length === 0 && (
              <p className="text-sm text-muted-foreground">No cost data yet.</p>
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Recent requests</CardTitle>
          <CardDescription>Last 10 gateway calls</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          {recentQ.isLoading ? (
            <div className="p-4 space-y-2">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-8 w-full" />
              ))}
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Time</TableHead>
                  <TableHead>Provider / Model</TableHead>
                  <TableHead className="text-right">Tokens</TableHead>
                  <TableHead className="text-right">Cost</TableHead>
                  <TableHead className="text-right">Latency</TableHead>
                  <TableHead>Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {(recentQ.data?.data ?? []).map((r) => (
                  <TableRow key={r.id}>
                    <TableCell className="text-xs text-muted-foreground whitespace-nowrap">
                      {format(parseISO(r.created_at), "HH:mm:ss")}
                    </TableCell>
                    <TableCell>
                      <span className="font-medium">{r.provider}</span>
                      <span className="text-muted-foreground text-xs ml-1">
                        / {r.model}
                      </span>
                    </TableCell>
                    <TableCell className="text-right tabular-nums text-sm">
                      {r.total_tokens?.toLocaleString() ?? "—"}
                    </TableCell>
                    <TableCell className="text-right tabular-nums text-sm">
                      {fmtCost(r.cost_usd)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums text-sm">
                      {fmtMs(r.latency_ms)}
                    </TableCell>
                    <TableCell>
                      {statusBadge(r.status)}
                      {r.cache_type && (
                        <Badge variant="secondary" className="ml-1">
                          {r.cache_type}
                        </Badge>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
                {(recentQ.data?.data ?? []).length === 0 && (
                  <TableRow>
                    <TableCell
                      colSpan={6}
                      className="text-center py-8 text-muted-foreground text-sm"
                    >
                      No requests yet
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
