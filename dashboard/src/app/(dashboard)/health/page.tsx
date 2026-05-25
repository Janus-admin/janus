"use client";

import { useQuery } from "@tanstack/react-query";
import { system, type ReadinessCheck, type CheckStatus } from "@/lib/api";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { CheckCircle2, XCircle, AlertTriangle, RefreshCw } from "lucide-react";
import { format } from "date-fns";

function StatusIcon({ status }: { status: CheckStatus }) {
  if (status === "pass") return <CheckCircle2 className="h-5 w-5 text-green-500 shrink-0" />;
  if (status === "fail") return <XCircle className="h-5 w-5 text-red-500 shrink-0" />;
  return <AlertTriangle className="h-5 w-5 text-yellow-500 shrink-0" />;
}

function StatusBadge({ status }: { status: CheckStatus }) {
  if (status === "pass")
    return (
      <Badge variant="outline" className="text-green-600 border-green-600/30 text-xs">
        pass
      </Badge>
    );
  if (status === "fail")
    return <Badge variant="destructive" className="text-xs">fail</Badge>;
  return (
    <Badge variant="outline" className="text-yellow-600 border-yellow-600/30 text-xs">
      warn
    </Badge>
  );
}

function CheckRow({ check }: { check: ReadinessCheck }) {
  return (
    <div className="flex items-start gap-3 py-3 border-b last:border-0">
      <StatusIcon status={check.status} />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="font-medium text-sm">{check.name}</span>
          <StatusBadge status={check.status} />
        </div>
        <p className="text-sm text-muted-foreground mt-0.5">{check.message}</p>
      </div>
    </div>
  );
}

export default function HealthPage() {
  const { data, isLoading, dataUpdatedAt, refetch, isFetching } = useQuery({
    queryKey: ["system", "readiness"],
    queryFn: () => system.readiness(),
    refetchInterval: 30_000,
  });

  const report = data?.data;
  const isHealthy = report?.healthy ?? true;

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-semibold mb-1">System Health</h1>
          <p className="text-muted-foreground text-sm">
            Janus readiness checks — auto-refreshes every 30 seconds.
          </p>
        </div>
        <div className="flex items-center gap-2 text-xs text-muted-foreground mt-1">
          {dataUpdatedAt > 0 && (
            <span>Last checked {format(new Date(dataUpdatedAt), "HH:mm:ss")}</span>
          )}
          <RefreshCw
            className={`h-3.5 w-3.5 cursor-pointer hover:text-foreground transition-colors ${isFetching ? "animate-spin" : ""}`}
            onClick={() => refetch()}
          />
        </div>
      </div>

      {!isLoading && report && !isHealthy && (
        <Alert variant="destructive">
          <XCircle className="h-4 w-4" />
          <AlertTitle>System unhealthy</AlertTitle>
          <AlertDescription>
            {report.errors} check{report.errors !== 1 ? "s" : ""} failing.
            Gateway requests may be degraded or unavailable.
          </AlertDescription>
        </Alert>
      )}

      {!isLoading && report && isHealthy && report.warnings > 0 && (
        <Alert className="border-yellow-500/30 bg-yellow-500/5">
          <AlertTriangle className="h-4 w-4 text-yellow-500" />
          <AlertTitle className="text-yellow-700 dark:text-yellow-400">
            {report.warnings} warning{report.warnings !== 1 ? "s" : ""}
          </AlertTitle>
          <AlertDescription>
            All critical checks passed but some optional components may be unavailable.
          </AlertDescription>
        </Alert>
      )}

      <div className="grid md:grid-cols-3 gap-4">
        <Card>
          <CardContent className="pt-6">
            <p className="text-sm text-muted-foreground">Overall status</p>
            {isLoading ? (
              <Skeleton className="h-8 w-24 mt-1" />
            ) : (
              <div className="flex items-center gap-2 mt-1">
                {isHealthy ? (
                  <CheckCircle2 className="h-6 w-6 text-green-500" />
                ) : (
                  <XCircle className="h-6 w-6 text-red-500" />
                )}
                <span className="text-xl font-semibold">
                  {isHealthy ? "Healthy" : "Unhealthy"}
                </span>
              </div>
            )}
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <p className="text-sm text-muted-foreground">Checks passing</p>
            {isLoading ? (
              <Skeleton className="h-8 w-16 mt-1" />
            ) : (
              <p className="text-2xl font-bold tabular-nums mt-1 text-green-600">
                {(report?.checks.filter((c) => c.status === "pass").length ?? 0)}{" "}
                <span className="text-sm font-normal text-muted-foreground">
                  / {report?.checks.length ?? 0}
                </span>
              </p>
            )}
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <p className="text-sm text-muted-foreground">Issues</p>
            {isLoading ? (
              <Skeleton className="h-8 w-16 mt-1" />
            ) : (
              <p className="text-2xl font-bold tabular-nums mt-1">
                <span className={report?.errors ? "text-red-600" : "text-muted-foreground"}>
                  {report?.errors ?? 0} error{report?.errors !== 1 ? "s" : ""}
                </span>
                <span className="text-sm font-normal text-muted-foreground ml-2">
                  {report?.warnings ?? 0} warn{report?.warnings !== 1 ? "s" : ""}
                </span>
              </p>
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle>Readiness checks</CardTitle>
          <CardDescription>
            Each check must pass for Janus to serve traffic correctly.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-4">
              {Array.from({ length: 5 }).map((_, i) => (
                <div key={i} className="flex items-start gap-3 py-3 border-b last:border-0">
                  <Skeleton className="h-5 w-5 rounded-full shrink-0" />
                  <div className="flex-1 space-y-1.5">
                    <Skeleton className="h-4 w-40" />
                    <Skeleton className="h-3.5 w-64" />
                  </div>
                </div>
              ))}
            </div>
          ) : (report?.checks ?? []).length === 0 ? (
            <p className="text-sm text-muted-foreground py-4">No checks returned.</p>
          ) : (
            <div>
              {(report?.checks ?? []).map((check, i) => (
                <CheckRow key={i} check={check} />
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
