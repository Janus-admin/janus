"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  alertsApi,
  type Alert,
  type AlertHistoryEntry,
  type CreateAlertRequest,
} from "@/lib/api";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod/v4";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
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
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
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
import { Plus, Trash2, SendHorizontal, History, Pencil } from "lucide-react";

const ALERT_TYPES = [
  { value: "spend", label: "Spend (USD)" },
  { value: "error_rate", label: "Error rate (%)" },
  { value: "latency", label: "Latency (ms)" },
];

const WEBHOOK_FORMATS = [
  { value: "generic", label: "Generic JSON" },
  { value: "slack", label: "Slack" },
  { value: "pagerduty", label: "PagerDuty" },
];

const createSchema = z.object({
  name: z.string().min(1, "Name is required"),
  type: z.string().min(1, "Type is required"),
  threshold: z.string().min(1, "Threshold is required"),
  window_minutes: z.string().optional(),
  webhook_url: z.string().optional(),
  webhook_format: z.string().optional(),
  webhook_secret: z.string().optional(),
});
type CreateForm = z.infer<typeof createSchema>;

const editAlertSchema = z.object({
  name: z.string().min(1, "Name is required"),
  threshold: z.string().min(1, "Threshold is required"),
  window_minutes: z.string().optional(),
  webhook_url: z.string().optional(),
  webhook_format: z.string().optional(),
  webhook_secret: z.string().optional(),
});
type EditAlertForm = z.infer<typeof editAlertSchema>;

function TypeBadge({ type }: { type: string }) {
  const colors: Record<string, string> = {
    spend: "text-amber-600 border-amber-600/30",
    error_rate: "text-red-600 border-red-600/30",
    latency: "text-blue-600 border-blue-600/30",
  };
  return (
    <Badge variant="outline" className={colors[type] ?? ""}>
      {type}
    </Badge>
  );
}

function HistoryDrawer({
  alertId,
  open,
  onClose,
}: {
  alertId: string | null;
  open: boolean;
  onClose: () => void;
}) {
  const { data, isLoading } = useQuery({
    queryKey: ["alert", alertId],
    queryFn: () => alertsApi.get(alertId!),
    enabled: !!alertId,
  });

  const history: AlertHistoryEntry[] = data?.data.history ?? [];

  return (
    <Sheet open={open} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-[480px] sm:w-[540px] overflow-y-auto">
        <SheetHeader>
          <SheetTitle>Alert history</SheetTitle>
        </SheetHeader>
        <div className="mt-4 space-y-2">
          {isLoading && (
            <div className="space-y-2">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-14 w-full" />
              ))}
            </div>
          )}
          {!isLoading && history.length === 0 && (
            <p className="text-sm text-muted-foreground text-center py-8">
              No history yet.
            </p>
          )}
          {history.map((entry) => (
            <div
              key={entry.id}
              className="rounded-md border p-3 text-sm space-y-1"
            >
              <div className="flex items-center justify-between">
                <span className="text-muted-foreground text-xs">
                  {format(parseISO(entry.triggered_at), "MMM d, HH:mm:ss")}
                </span>
                {entry.delivered ? (
                  <Badge
                    variant="outline"
                    className="text-green-600 border-green-600/30 text-xs"
                  >
                    delivered
                  </Badge>
                ) : (
                  <Badge variant="secondary" className="text-xs">
                    failed
                  </Badge>
                )}
              </div>
              {entry.message && (
                <p className="text-foreground">{entry.message}</p>
              )}
              {entry.value != null && (
                <p className="text-muted-foreground text-xs">
                  Value: {entry.value}
                </p>
              )}
              {entry.error && (
                <p className="text-destructive text-xs">{entry.error}</p>
              )}
            </div>
          ))}
        </div>
      </SheetContent>
    </Sheet>
  );
}

export default function AlertsPage() {
  const qc = useQueryClient();
  const [createOpen, setCreateOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Alert | null>(null);
  const [editTarget, setEditTarget] = useState<Alert | null>(null);
  const [historyTarget, setHistoryTarget] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ["alerts"],
    queryFn: () => alertsApi.list(),
  });

  const createMut = useMutation({
    mutationFn: (body: CreateAlertRequest) => alertsApi.create(body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["alerts"] });
      setCreateOpen(false);
      form.reset();
    },
  });

  const toggleMut = useMutation({
    mutationFn: ({ id, is_active }: { id: string; is_active: boolean }) =>
      alertsApi.update(id, { is_active }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["alerts"] }),
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => alertsApi.delete(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["alerts"] });
      setDeleteTarget(null);
    },
  });

  const testMut = useMutation({
    mutationFn: (id: string) => alertsApi.test(id),
    onSettled: () => setTestingId(null),
  });

  const editMut = useMutation({
    mutationFn: ({ id, body }: { id: string; body: Parameters<typeof alertsApi.update>[1] }) =>
      alertsApi.update(id, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["alerts"] });
      setEditTarget(null);
    },
  });

  const form = useForm<CreateForm>({
    resolver: zodResolver(createSchema),
    defaultValues: {
      name: "",
      type: "spend",
      threshold: "",
      window_minutes: "60",
      webhook_format: "generic",
    },
  });

  const editForm = useForm<EditAlertForm>({
    resolver: zodResolver(editAlertSchema),
  });

  function openEdit(alert: Alert) {
    setEditTarget(alert);
    editForm.reset({
      name: alert.name,
      threshold: String(alert.threshold),
      window_minutes: String(alert.window_minutes),
      webhook_url: alert.webhook_url ?? "",
      webhook_format: alert.webhook_format ?? "generic",
      webhook_secret: "",
    });
  }

  function onEditSubmit(values: EditAlertForm) {
    if (!editTarget) return;
    editMut.mutate({
      id: editTarget.id,
      body: {
        name: values.name,
        threshold: parseFloat(values.threshold),
        window_minutes: values.window_minutes ? parseInt(values.window_minutes) : undefined,
        webhook_url: values.webhook_url || null,
        webhook_format: values.webhook_format || "generic",
        webhook_secret: values.webhook_secret || null,
      },
    });
  }

  function onSubmit(values: CreateForm) {
    createMut.mutate({
      name: values.name,
      type: values.type,
      threshold: parseFloat(values.threshold),
      window_minutes: values.window_minutes
        ? parseInt(values.window_minutes)
        : 60,
      webhook_url: values.webhook_url || null,
      webhook_format: values.webhook_format || "generic",
      webhook_secret: values.webhook_secret || null,
    });
  }

  const alerts: Alert[] = data?.data ?? [];

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold mb-1">Alerts</h1>
          <p className="text-muted-foreground text-sm">
            Threshold-based alerts with webhook delivery.
          </p>
        </div>
        <Button
          size="sm"
          onClick={() => {
            form.reset({
              name: "",
              type: "spend",
              threshold: "",
              window_minutes: "60",
              webhook_format: "generic",
            });
            setCreateOpen(true);
          }}
        >
          <Plus className="h-4 w-4 mr-1" />
          New alert
        </Button>
      </div>

      <Card>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Type</TableHead>
                <TableHead>Threshold</TableHead>
                <TableHead>Window</TableHead>
                <TableHead>Webhook</TableHead>
                <TableHead>Last triggered</TableHead>
                <TableHead>Active</TableHead>
                <TableHead className="w-28" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading
                ? Array.from({ length: 3 }).map((_, i) => (
                    <TableRow key={i}>
                      {Array.from({ length: 8 }).map((_, j) => (
                        <TableCell key={j}>
                          <Skeleton className="h-4 w-full" />
                        </TableCell>
                      ))}
                    </TableRow>
                  ))
                : alerts.map((alert) => (
                    <TableRow
                      key={alert.id}
                      className={alert.is_active ? "" : "opacity-50"}
                    >
                      <TableCell className="font-medium">{alert.name}</TableCell>
                      <TableCell>
                        <TypeBadge type={alert.alert_type} />
                      </TableCell>
                      <TableCell className="tabular-nums text-sm">
                        {alert.threshold}
                      </TableCell>
                      <TableCell className="text-sm text-muted-foreground">
                        {alert.window_minutes} min
                      </TableCell>
                      <TableCell className="text-sm">
                        {alert.webhook_url ? (
                          <Badge
                            variant="outline"
                            className="text-green-600 border-green-600/30 text-xs"
                          >
                            configured
                          </Badge>
                        ) : (
                          <span className="text-muted-foreground">—</span>
                        )}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {alert.last_triggered
                          ? format(
                              parseISO(alert.last_triggered),
                              "MMM d HH:mm"
                            )
                          : "never"}
                      </TableCell>
                      <TableCell>
                        <Switch
                          checked={alert.is_active}
                          onCheckedChange={(v) =>
                            toggleMut.mutate({
                              id: alert.id,
                              is_active: v,
                            })
                          }
                        />
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-1">
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 text-muted-foreground"
                            title="Edit alert"
                            onClick={() => openEdit(alert)}
                          >
                            <Pencil className="h-3.5 w-3.5" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 text-muted-foreground"
                            title="View history"
                            onClick={() => setHistoryTarget(alert.id)}
                          >
                            <History className="h-3.5 w-3.5" />
                          </Button>
                          {alert.webhook_url && (
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7 text-muted-foreground"
                              title="Send test webhook"
                              disabled={testingId === alert.id}
                              onClick={() => {
                                setTestingId(alert.id);
                                testMut.mutate(alert.id);
                              }}
                            >
                              <SendHorizontal className="h-3.5 w-3.5" />
                            </Button>
                          )}
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 text-muted-foreground hover:text-destructive"
                            title="Delete"
                            onClick={() => setDeleteTarget(alert)}
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
              {!isLoading && alerts.length === 0 && (
                <TableRow>
                  <TableCell
                    colSpan={8}
                    className="text-center py-12 text-muted-foreground text-sm"
                  >
                    No alerts yet. Create one to get started.
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      {/* Create dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Create alert</DialogTitle>
            <DialogDescription>
              Fires a webhook when the threshold is exceeded within the window.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            <div className="space-y-1.5">
              <Label htmlFor="alert-name">Name</Label>
              <Input
                id="alert-name"
                placeholder="High spend alert"
                {...form.register("name")}
              />
              {form.formState.errors.name && (
                <p className="text-xs text-destructive">
                  {form.formState.errors.name.message}
                </p>
              )}
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>Type</Label>
                <Select
                  defaultValue="spend"
                  onValueChange={(v) => form.setValue("type", v)}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {ALERT_TYPES.map((t) => (
                      <SelectItem key={t.value} value={t.value}>
                        {t.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="threshold">Threshold</Label>
                <Input
                  id="threshold"
                  type="number"
                  step="any"
                  placeholder="10.00"
                  {...form.register("threshold")}
                />
                {form.formState.errors.threshold && (
                  <p className="text-xs text-destructive">
                    {form.formState.errors.threshold.message}
                  </p>
                )}
              </div>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="window">Window (minutes)</Label>
              <Input
                id="window"
                type="number"
                placeholder="60"
                {...form.register("window_minutes")}
              />
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="webhook-url">Webhook URL</Label>
              <Input
                id="webhook-url"
                type="url"
                placeholder="https://hooks.example.com/..."
                {...form.register("webhook_url")}
              />
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>Webhook format</Label>
                <Select
                  defaultValue="generic"
                  onValueChange={(v) => form.setValue("webhook_format", v)}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {WEBHOOK_FORMATS.map((f) => (
                      <SelectItem key={f.value} value={f.value}>
                        {f.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="webhook-secret">Webhook secret</Label>
                <Input
                  id="webhook-secret"
                  type="password"
                  placeholder="optional"
                  {...form.register("webhook_secret")}
                />
              </div>
            </div>

            <DialogFooter>
              <Button
                type="button"
                variant="ghost"
                onClick={() => setCreateOpen(false)}
              >
                Cancel
              </Button>
              <Button type="submit" disabled={createMut.isPending}>
                {createMut.isPending ? "Creating…" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      {/* Delete confirmation */}
      <AlertDialog
        open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Delete &ldquo;{deleteTarget?.name}&rdquo;?
            </AlertDialogTitle>
            <AlertDialogDescription>
              This alert and all its history will be permanently removed.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() => deleteTarget && deleteMut.mutate(deleteTarget.id)}
              disabled={deleteMut.isPending}
            >
              {deleteMut.isPending ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Edit dialog */}
      <Dialog open={!!editTarget} onOpenChange={(o) => !o && setEditTarget(null)}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Edit alert — {editTarget?.name}</DialogTitle>
            <DialogDescription>
              Changes take effect on the next evaluation window.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={editForm.handleSubmit(onEditSubmit)} className="space-y-4">
            <div className="space-y-1.5">
              <Label htmlFor="edit-name">Name</Label>
              <Input id="edit-name" {...editForm.register("name")} />
              {editForm.formState.errors.name && (
                <p className="text-xs text-destructive">{editForm.formState.errors.name.message}</p>
              )}
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label htmlFor="edit-threshold">Threshold</Label>
                <Input id="edit-threshold" type="number" step="any" {...editForm.register("threshold")} />
                {editForm.formState.errors.threshold && (
                  <p className="text-xs text-destructive">{editForm.formState.errors.threshold.message}</p>
                )}
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="edit-window">Window (minutes)</Label>
                <Input id="edit-window" type="number" {...editForm.register("window_minutes")} />
              </div>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="edit-webhook-url">Webhook URL</Label>
              <Input id="edit-webhook-url" type="url" placeholder="https://…" {...editForm.register("webhook_url")} />
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>Webhook format</Label>
                <Select
                  defaultValue={editTarget?.webhook_format ?? "generic"}
                  onValueChange={(v) => editForm.setValue("webhook_format", v)}
                >
                  <SelectTrigger><SelectValue /></SelectTrigger>
                  <SelectContent>
                    {WEBHOOK_FORMATS.map((f) => (
                      <SelectItem key={f.value} value={f.value}>{f.label}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="edit-secret">New webhook secret</Label>
                <Input id="edit-secret" type="password" placeholder="leave blank to keep" {...editForm.register("webhook_secret")} />
              </div>
            </div>
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={() => setEditTarget(null)}>Cancel</Button>
              <Button type="submit" disabled={editMut.isPending}>
                {editMut.isPending ? "Saving…" : "Save changes"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      {/* History drawer */}
      <HistoryDrawer
        alertId={historyTarget}
        open={!!historyTarget}
        onClose={() => setHistoryTarget(null)}
      />
    </div>
  );
}
