"use client";

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { requests, type RequestFilter, type GatewayRequest } from "@/lib/api";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { format, parseISO } from "date-fns";
import { ChevronLeft, ChevronRight, Download, X } from "lucide-react";

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

function cacheBadge(cacheType: string | null, similarity: number | null) {
  if (!cacheType) return <span className="text-muted-foreground text-xs">—</span>;
  return (
    <div className="flex flex-col gap-0.5">
      <Badge variant="secondary">{cacheType}</Badge>
      {cacheType === "semantic" && similarity != null && (
        <span className="text-xs text-muted-foreground tabular-nums">
          {(similarity * 100).toFixed(1)}%
        </span>
      )}
    </div>
  );
}

const ALL = "all";

function exportCsv(rows: GatewayRequest[]) {
  const header = "id,created_at,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,latency_ms,ttfb_ms,cache_type,cache_similarity,status\n";
  const body = rows.map((r) =>
    [
      r.id, r.created_at, r.provider, `"${r.model}"`,
      r.prompt_tokens ?? "", r.completion_tokens ?? "",
      r.total_tokens ?? "", r.cost_usd ?? "",
      r.latency_ms ?? "", r.ttfb_ms ?? "",
      r.cache_type ?? "", r.cache_similarity ?? "",
      r.status,
    ].join(",")
  ).join("\n");
  const blob = new Blob([header + body], { type: "text/csv" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `velox-requests-${format(new Date(), "yyyy-MM-dd")}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}

function RequestDetailSheet({
  request,
  onClose,
}: {
  request: GatewayRequest | null;
  onClose: () => void;
}) {
  const r = request;
  return (
    <Sheet open={!!r} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-[480px] sm:w-[540px] overflow-y-auto">
        <SheetHeader>
          <SheetTitle>Request detail</SheetTitle>
        </SheetHeader>
        {r && (
          <div className="mt-4 space-y-4 text-sm">
            <div className="grid grid-cols-2 gap-x-4 gap-y-2">
              <DetailRow label="ID" value={<code className="text-xs font-mono break-all">{r.id}</code>} />
              <DetailRow label="Time" value={format(parseISO(r.created_at), "MMM d yyyy HH:mm:ss")} />
              <DetailRow label="Provider" value={r.provider} />
              <DetailRow label="Model" value={<code className="text-xs font-mono">{r.model}</code>} />
              <DetailRow label="Status" value={statusBadge(r.status)} />
              <DetailRow label="Stream" value={r.stream ? "yes" : "no"} />
              <DetailRow label="Prompt tokens" value={r.prompt_tokens?.toLocaleString() ?? "—"} />
              <DetailRow label="Completion tokens" value={r.completion_tokens?.toLocaleString() ?? "—"} />
              <DetailRow label="Total tokens" value={r.total_tokens?.toLocaleString() ?? "—"} />
              <DetailRow
                label="Cost"
                value={
                  r.cache_type && r.cache_type !== "none"
                    ? "free (cache hit)"
                    : r.cost_usd != null
                    ? `$${r.cost_usd.toFixed(6)}`
                    : "—"
                }
              />
              <DetailRow
                label="Latency"
                value={r.latency_ms != null ? `${Math.round(r.latency_ms)} ms` : "—"}
              />
              <DetailRow
                label="TTFB"
                value={r.ttfb_ms != null ? `${r.ttfb_ms} ms` : "—"}
              />
              <DetailRow label="Cache type" value={r.cache_type ?? "—"} />
              {r.cache_similarity != null && (
                <DetailRow
                  label="Cache similarity"
                  value={`${(r.cache_similarity * 100).toFixed(2)}%`}
                />
              )}
              {r.api_key_id && (
                <DetailRow
                  label="API key ID"
                  value={<code className="text-xs font-mono break-all">{r.api_key_id}</code>}
                />
              )}
              {r.workspace_id && (
                <DetailRow
                  label="Workspace ID"
                  value={<code className="text-xs font-mono break-all">{r.workspace_id}</code>}
                />
              )}
            </div>
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}

function DetailRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <>
      <span className="text-muted-foreground">{label}</span>
      <span className="font-medium break-words">{value}</span>
    </>
  );
}

export default function RequestsPage() {
  const [page, setPage] = useState(1);
  const [provider, setProvider] = useState(ALL);
  const [status, setStatus] = useState(ALL);
  const [model, setModel] = useState("");
  const [startTime, setStartTime] = useState("");
  const [endTime, setEndTime] = useState("");
  const [detailRow, setDetailRow] = useState<GatewayRequest | null>(null);

  const hasFilters =
    provider !== ALL || status !== ALL || model !== "" || startTime !== "" || endTime !== "";

  const filter: RequestFilter = {
    page,
    per_page: 50,
    provider: provider === ALL ? undefined : provider,
    status: status === ALL ? undefined : status,
    model: model || undefined,
    start_time: startTime ? new Date(startTime).toISOString() : undefined,
    end_time: endTime ? new Date(endTime).toISOString() : undefined,
  };

  const { data, isLoading, isFetching } = useQuery({
    queryKey: ["requests", filter],
    queryFn: () => requests.list(filter),
    placeholderData: (prev) => prev,
  });

  const total = data?.meta.total ?? 0;
  const perPage = data?.meta.per_page ?? 50;
  const totalPages = Math.max(1, Math.ceil(total / perPage));

  function clearFilters() {
    setProvider(ALL);
    setStatus(ALL);
    setModel("");
    setStartTime("");
    setEndTime("");
    setPage(1);
  }

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-2xl font-semibold mb-1">Requests</h1>
        <p className="text-muted-foreground text-sm">
          Full audit log of every proxied LLM call. Click a row for details.
        </p>
      </div>

      {/* Filters row 1 */}
      <div className="flex items-center gap-3 flex-wrap">
        <Select value={provider} onValueChange={(v) => { setProvider(v); setPage(1); }}>
          <SelectTrigger className="w-40">
            <SelectValue placeholder="Provider" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>All providers</SelectItem>
            <SelectItem value="openai">OpenAI</SelectItem>
            <SelectItem value="anthropic">Anthropic</SelectItem>
            <SelectItem value="bedrock">Bedrock</SelectItem>
            <SelectItem value="gemini">Gemini</SelectItem>
            <SelectItem value="groq">Groq</SelectItem>
            <SelectItem value="deepseek">DeepSeek</SelectItem>
          </SelectContent>
        </Select>

        <Select value={status} onValueChange={(v) => { setStatus(v); setPage(1); }}>
          <SelectTrigger className="w-36">
            <SelectValue placeholder="Status" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>All statuses</SelectItem>
            <SelectItem value="success">Success</SelectItem>
            <SelectItem value="error">Error</SelectItem>
          </SelectContent>
        </Select>

        <Input
          className="w-52"
          placeholder="Filter by model…"
          value={model}
          onChange={(e) => { setModel(e.target.value); setPage(1); }}
        />

        {hasFilters && (
          <Button variant="ghost" size="sm" onClick={clearFilters}>
            <X className="h-3.5 w-3.5 mr-1" />
            Clear filters
          </Button>
        )}

        <span className="ml-auto text-sm text-muted-foreground">
          {total.toLocaleString()} total
        </span>
        <Button
          variant="outline"
          size="sm"
          disabled={!data?.data?.length}
          onClick={() => data?.data && exportCsv(data.data)}
        >
          <Download className="h-3.5 w-3.5 mr-1" />
          Export CSV
        </Button>
      </div>

      {/* Filters row 2 — date range */}
      <div className="flex items-center gap-3 flex-wrap">
        <div className="flex items-center gap-2">
          <Label className="text-sm text-muted-foreground whitespace-nowrap">From</Label>
          <Input
            type="datetime-local"
            className="w-48 text-sm"
            value={startTime}
            onChange={(e) => { setStartTime(e.target.value); setPage(1); }}
          />
        </div>
        <div className="flex items-center gap-2">
          <Label className="text-sm text-muted-foreground whitespace-nowrap">To</Label>
          <Input
            type="datetime-local"
            className="w-48 text-sm"
            value={endTime}
            onChange={(e) => { setEndTime(e.target.value); setPage(1); }}
          />
        </div>
      </div>

      <Card>
        <CardContent className="p-0">
          <div className="relative">
            {isFetching && !isLoading && (
              <div className="absolute inset-x-0 top-0 h-0.5 bg-primary/30 animate-pulse" />
            )}
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-32">Time</TableHead>
                  <TableHead>Provider</TableHead>
                  <TableHead>Model</TableHead>
                  <TableHead className="text-right">Tokens</TableHead>
                  <TableHead className="text-right">Cost</TableHead>
                  <TableHead className="text-right">Latency</TableHead>
                  <TableHead className="text-right">TTFB</TableHead>
                  <TableHead>Cache</TableHead>
                  <TableHead>Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading
                  ? Array.from({ length: 10 }).map((_, i) => (
                      <TableRow key={i}>
                        {Array.from({ length: 9 }).map((_, j) => (
                          <TableCell key={j}>
                            <Skeleton className="h-4 w-full" />
                          </TableCell>
                        ))}
                      </TableRow>
                    ))
                  : (data?.data ?? []).map((r) => (
                      <TableRow
                        key={r.id}
                        className="cursor-pointer hover:bg-muted/50"
                        onClick={() => setDetailRow(r)}
                      >
                        <TableCell className="text-xs text-muted-foreground whitespace-nowrap">
                          {format(parseISO(r.created_at), "MMM d HH:mm:ss")}
                        </TableCell>
                        <TableCell className="font-medium text-sm">
                          {r.provider}
                        </TableCell>
                        <TableCell className="text-sm text-muted-foreground max-w-48 truncate">
                          {r.model}
                        </TableCell>
                        <TableCell className="text-right tabular-nums text-sm">
                          {r.total_tokens?.toLocaleString() ?? "—"}
                        </TableCell>
                        <TableCell className="text-right tabular-nums text-sm">
                          {r.cache_type && r.cache_type !== "none"
                            ? <span className="text-green-600 text-xs font-medium">free</span>
                            : r.cost_usd != null
                            ? `$${r.cost_usd.toFixed(6)}`
                            : "—"}
                        </TableCell>
                        <TableCell className="text-right tabular-nums text-sm">
                          {r.latency_ms != null ? `${Math.round(r.latency_ms)} ms` : "—"}
                        </TableCell>
                        <TableCell className="text-right tabular-nums text-sm">
                          {r.ttfb_ms != null ? `${r.ttfb_ms} ms` : "—"}
                        </TableCell>
                        <TableCell>
                          {cacheBadge(r.cache_type, r.cache_similarity)}
                        </TableCell>
                        <TableCell>{statusBadge(r.status)}</TableCell>
                      </TableRow>
                    ))}
                {!isLoading && (data?.data ?? []).length === 0 && (
                  <TableRow>
                    <TableCell
                      colSpan={9}
                      className="text-center py-12 text-muted-foreground text-sm"
                    >
                      No requests match the current filters.
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
        </CardContent>
      </Card>

      {/* Pagination */}
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          Page {page} of {totalPages}
        </p>
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => setPage((p) => Math.max(1, p - 1))}
            disabled={page === 1}
          >
            <ChevronLeft className="h-4 w-4" />
            Prev
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            disabled={page >= totalPages}
          >
            Next
            <ChevronRight className="h-4 w-4" />
          </Button>
        </div>
      </div>

      <RequestDetailSheet request={detailRow} onClose={() => setDetailRow(null)} />
    </div>
  );
}
