"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { keys, type CreateKeyRequest, type ApiKey } from "@/lib/api";
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
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { format, parseISO } from "date-fns";
import { Plus, Copy, Check, Trash2 } from "lucide-react";

const createSchema = z.object({
  name: z.string().min(1, "Name is required"),
  budget_limit: z.string().optional(),
  rate_limit_rpm: z.string().optional(),
  rate_limit_tpm: z.string().optional(),
});
type CreateForm = z.infer<typeof createSchema>;

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      variant="ghost"
      size="icon"
      className="h-6 w-6"
      onClick={async () => {
        await navigator.clipboard.writeText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      }}
    >
      {copied ? (
        <Check className="h-3 w-3 text-green-600" />
      ) : (
        <Copy className="h-3 w-3" />
      )}
    </Button>
  );
}

function BudgetBar({ used, limit }: { used: number; limit: number | null }) {
  if (!limit) return <span className="text-muted-foreground text-xs">unlimited</span>;
  const pct = Math.min(100, (used / limit) * 100);
  return (
    <div className="flex items-center gap-2">
      <div className="h-1.5 w-20 rounded-full bg-muted overflow-hidden">
        <div
          className="h-full rounded-full bg-primary transition-all"
          style={{ width: `${pct}%` }}
        />
      </div>
      <span className="text-xs text-muted-foreground tabular-nums">
        ${used.toFixed(2)} / ${limit.toFixed(2)}
      </span>
    </div>
  );
}

export default function KeysPage() {
  const qc = useQueryClient();
  const [createOpen, setCreateOpen] = useState(false);
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [revokeTarget, setRevokeTarget] = useState<ApiKey | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ["keys"],
    queryFn: () => keys.list(),
  });

  const createMut = useMutation({
    mutationFn: (body: CreateKeyRequest) => keys.create(body),
    onSuccess: (res) => {
      qc.invalidateQueries({ queryKey: ["keys"] });
      setCreatedKey(res.data.key);
      setCreateOpen(false);
    },
  });

  const revokeMut = useMutation({
    mutationFn: (id: string) => keys.revoke(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["keys"] });
      setRevokeTarget(null);
    },
  });

  const form = useForm<CreateForm>({
    resolver: zodResolver(createSchema),
    defaultValues: { name: "" },
  });

  function onSubmit(values: CreateForm) {
    const body: CreateKeyRequest = {
      name: values.name,
      budget_limit: values.budget_limit ? parseFloat(values.budget_limit) : null,
      rate_limit_rpm: values.rate_limit_rpm
        ? parseInt(values.rate_limit_rpm)
        : null,
      rate_limit_tpm: values.rate_limit_tpm
        ? parseInt(values.rate_limit_tpm)
        : null,
    };
    createMut.mutate(body);
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold mb-1">API Keys</h1>
          <p className="text-muted-foreground text-sm">
            Create and manage gateway API keys with budgets and rate limits.
          </p>
        </div>
        <Button
          size="sm"
          onClick={() => {
            form.reset();
            setCreateOpen(true);
          }}
        >
          <Plus className="h-4 w-4 mr-1" />
          New key
        </Button>
      </div>

      <Card>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Prefix</TableHead>
                <TableHead>Budget</TableHead>
                <TableHead>Rate limit</TableHead>
                <TableHead>Last used</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="w-12" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading
                ? Array.from({ length: 4 }).map((_, i) => (
                    <TableRow key={i}>
                      {Array.from({ length: 7 }).map((_, j) => (
                        <TableCell key={j}>
                          <Skeleton className="h-4 w-full" />
                        </TableCell>
                      ))}
                    </TableRow>
                  ))
                : (data?.data ?? []).map((key) => (
                    <TableRow key={key.id} className={key.is_active ? "" : "opacity-50"}>
                      <TableCell className="font-medium">{key.name}</TableCell>
                      <TableCell className="font-mono text-sm text-muted-foreground">
                        {key.key_prefix}…
                      </TableCell>
                      <TableCell>
                        <BudgetBar
                          used={key.budget_used}
                          limit={key.budget_limit}
                        />
                      </TableCell>
                      <TableCell className="text-sm text-muted-foreground">
                        {key.rate_limit_rpm
                          ? `${key.rate_limit_rpm} rpm`
                          : "—"}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {key.last_used_at
                          ? format(parseISO(key.last_used_at), "MMM d HH:mm")
                          : "never"}
                      </TableCell>
                      <TableCell>
                        {key.is_active ? (
                          <Badge
                            variant="outline"
                            className="text-green-600 border-green-600/30"
                          >
                            active
                          </Badge>
                        ) : (
                          <Badge variant="secondary">revoked</Badge>
                        )}
                      </TableCell>
                      <TableCell>
                        {key.is_active && (
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 text-muted-foreground hover:text-destructive"
                            onClick={() => setRevokeTarget(key)}
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                          </Button>
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
              {!isLoading && (data?.data ?? []).length === 0 && (
                <TableRow>
                  <TableCell
                    colSpan={7}
                    className="text-center py-12 text-muted-foreground text-sm"
                  >
                    No API keys yet. Create one to get started.
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      {/* Create key dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Create API key</DialogTitle>
            <DialogDescription>
              The full key is shown only once after creation.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            <div className="space-y-1.5">
              <Label htmlFor="name">Name</Label>
              <Input id="name" placeholder="My app" {...form.register("name")} />
              {form.formState.errors.name && (
                <p className="text-xs text-destructive">
                  {form.formState.errors.name.message}
                </p>
              )}
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label htmlFor="budget">Budget (USD)</Label>
                <Input
                  id="budget"
                  type="number"
                  step="0.01"
                  placeholder="unlimited"
                  {...form.register("budget_limit")}
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="rpm">Rate limit (rpm)</Label>
                <Input
                  id="rpm"
                  type="number"
                  placeholder="unlimited"
                  {...form.register("rate_limit_rpm")}
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

      {/* New key reveal dialog */}
      <Dialog open={!!createdKey} onOpenChange={() => setCreatedKey(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>API key created</DialogTitle>
            <DialogDescription>
              Copy this key now — it will never be shown again.
            </DialogDescription>
          </DialogHeader>
          <div className="flex items-center gap-2 rounded-md border bg-muted px-3 py-2">
            <code className="flex-1 text-sm font-mono break-all text-foreground">
              {createdKey}
            </code>
            {createdKey && <CopyButton text={createdKey} />}
          </div>
          <DialogFooter>
            <Button onClick={() => setCreatedKey(null)}>Done</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Revoke confirmation */}
      <AlertDialog
        open={!!revokeTarget}
        onOpenChange={(o) => !o && setRevokeTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Revoke &ldquo;{revokeTarget?.name}&rdquo;?</AlertDialogTitle>
            <AlertDialogDescription>
              This key will stop working immediately. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() => revokeTarget && revokeMut.mutate(revokeTarget.id)}
              disabled={revokeMut.isPending}
            >
              {revokeMut.isPending ? "Revoking…" : "Revoke"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
