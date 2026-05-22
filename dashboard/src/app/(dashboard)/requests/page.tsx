"use client";

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { requests, type RequestFilter } from "@/lib/api";
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { format, parseISO } from "date-fns";
import { ChevronLeft, ChevronRight, Download } from "lucide-react";

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

const ALL = "all";

function exportCsv(rows: ReturnType<typeof Array.prototype.map>) {
  const header = "id,created_at,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,latency_ms,ttfb_ms,cache_type,status\n";
  const body = (rows as Parameters<typeof exportCsvRow>[0][]).map(exportCsvRow).join("\n");
  const blob = new Blob([header + body], { type: "text/csv" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `velox-requests-${format(new Date(), "yyyy-MM-dd")}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}

function exportCsvRow(r: {
  id: string; created_at: string; provider: string; model: string;
  prompt_tokens: number | null; completion_tokens: number | null;
  total_tokens: number | null; cost_usd: number | null;
  latency_ms: number | null; ttfb_ms: number | null;
  cache_type: string | null; status: string;
}) {
  return [
    r.id, r.created_at, r.provider, `"${r.model}"`,
    r.prompt_tokens ?? "", r.completion_tokens ?? "",
    r.total_tokens ?? "", r.cost_usd ?? "",
    r.latency_ms ?? "", r.ttfb_ms ?? "",
    r.cache_type ?? "", r.status,
  ].join(",");
}

export default function RequestsPage() {
  const [page, setPage] = useState(1);
  const [provider, setProvider] = useState(ALL);
  const [status, setStatus] = useState(ALL);

  const filter: RequestFilter = {
    page,
    per_page: 50,
    provider: provider === ALL ? undefined : provider,
    status: status === ALL ? undefined : status,
  };

  const { data, isLoading, isFetching } = useQuery({
    queryKey: ["requests", filter],
    queryFn: () => requests.list(filter),
    placeholderData: (prev) => prev,
  });

  const total = data?.meta.total ?? 0;
  const perPage = data?.meta.per_page ?? 50;
  const totalPages = Math.max(1, Math.ceil(total / perPage));

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-2xl font-semibold mb-1">Requests</h1>
        <p className="text-muted-foreground text-sm">
          Full audit log of every proxied LLM call.
        </p>
      </div>

      {/* Filters */}
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

        {(provider !== ALL || status !== ALL) && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => { setProvider(ALL); setStatus(ALL); setPage(1); }}
          >
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
                      <TableRow key={r.id}>
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
                          {r.cost_usd != null
                            ? `$${r.cost_usd.toFixed(4)}`
                            : "—"}
                        </TableCell>
                        <TableCell className="text-right tabular-nums text-sm">
                          {r.latency_ms != null
                            ? `${Math.round(r.latency_ms)} ms`
                            : "—"}
                        </TableCell>
                        <TableCell className="text-right tabular-nums text-sm">
                          {r.ttfb_ms != null ? `${r.ttfb_ms} ms` : "—"}
                        </TableCell>
                        <TableCell>
                          {r.cache_type ? (
                            <Badge variant="secondary">{r.cache_type}</Badge>
                          ) : (
                            <span className="text-muted-foreground text-xs">—</span>
                          )}
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
    </div>
  );
}
