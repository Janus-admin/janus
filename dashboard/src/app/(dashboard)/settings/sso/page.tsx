"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { idpApi, type CreateIdpRequest, type IdentityProvider } from "@/lib/api";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
import { Plus, Trash2, ShieldCheck, ExternalLink } from "lucide-react";

// ── Add IdP dialog ─────────────────────────────────────────────────────────────

function AddIdpDialog({
  open,
  onClose,
  onSubmit,
  isPending,
}: {
  open: boolean;
  onClose: () => void;
  onSubmit: (data: CreateIdpRequest) => void;
  isPending: boolean;
}) {
  const [form, setForm] = useState<CreateIdpRequest>({
    name: "",
    discovery_url: "",
    client_id: "",
    client_secret: "",
    group_role_map: {},
  });

  const set = (field: keyof CreateIdpRequest) => (
    e: React.ChangeEvent<HTMLInputElement>
  ) => setForm((f) => ({ ...f, [field]: e.target.value }));

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit(form);
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Add OIDC Identity Provider</DialogTitle>
          <DialogDescription>
            Connect an OIDC-compatible IdP (Google, GitHub, Auth0, Okta, etc.).
            Users can log in with their existing accounts.
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4 pt-2">
          <div className="space-y-1.5">
            <Label htmlFor="name">Display name</Label>
            <Input
              id="name"
              placeholder="Google Workspace"
              value={form.name}
              onChange={set("name")}
              required
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="discovery_url">Issuer / Discovery URL</Label>
            <Input
              id="discovery_url"
              placeholder="https://accounts.google.com"
              value={form.discovery_url}
              onChange={set("discovery_url")}
              required
            />
            <p className="text-xs text-muted-foreground">
              Velox appends{" "}
              <code className="font-mono text-xs">
                /.well-known/openid-configuration
              </code>{" "}
              automatically if missing.
            </p>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="client_id">Client ID</Label>
            <Input
              id="client_id"
              placeholder="your-app.apps.googleusercontent.com"
              value={form.client_id}
              onChange={set("client_id")}
              required
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="client_secret">Client secret</Label>
            <Input
              id="client_secret"
              type="password"
              placeholder="Stored encrypted at rest"
              value={form.client_secret}
              onChange={set("client_secret")}
              required
            />
          </div>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={isPending}>
              {isPending ? "Adding…" : "Add provider"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

// ── Main page ──────────────────────────────────────────────────────────────────

export default function SsoPage() {
  const qc = useQueryClient();
  const [addOpen, setAddOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<IdentityProvider | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ["idps"],
    queryFn: () => idpApi.list().then((r) => r.data),
  });

  const addMutation = useMutation({
    mutationFn: idpApi.create,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["idps"] });
      setAddOpen(false);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => idpApi.delete(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["idps"] });
      setDeleteTarget(null);
    },
  });

  const idps = data ?? [];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">
            Single Sign-On
          </h1>
          <p className="text-sm text-muted-foreground mt-1">
            Allow your team to log in with Google, GitHub, Auth0, Okta, or any
            OIDC-compatible provider.
          </p>
        </div>
        <Button onClick={() => setAddOpen(true)}>
          <Plus className="mr-2 h-4 w-4" />
          Add provider
        </Button>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <ShieldCheck className="h-5 w-5 text-muted-foreground" />
            Configured providers
          </CardTitle>
          <CardDescription>
            Each provider gets a dedicated login URL:{" "}
            <code className="text-xs font-mono">
              /auth/oidc/&lt;id&gt;/start
            </code>
          </CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-2">
              <Skeleton className="h-10 w-full" />
              <Skeleton className="h-10 w-full" />
            </div>
          ) : idps.length === 0 ? (
            <p className="text-sm text-muted-foreground py-6 text-center">
              No identity providers configured yet.
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Issuer</TableHead>
                  <TableHead>Client ID</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Added</TableHead>
                  <TableHead className="w-16" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {idps.map((idp) => (
                  <TableRow key={idp.id}>
                    <TableCell className="font-medium">{idp.name}</TableCell>
                    <TableCell className="max-w-xs truncate text-sm text-muted-foreground">
                      <a
                        href={idp.discovery_url}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center gap-1 hover:underline"
                      >
                        {new URL(idp.discovery_url).hostname}
                        <ExternalLink className="h-3 w-3" />
                      </a>
                    </TableCell>
                    <TableCell className="max-w-[180px] truncate font-mono text-xs">
                      {idp.client_id}
                    </TableCell>
                    <TableCell>
                      <Badge variant={idp.enabled ? "default" : "secondary"}>
                        {idp.enabled ? "enabled" : "disabled"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-sm text-muted-foreground">
                      {format(parseISO(idp.created_at), "MMM d, yyyy")}
                    </TableCell>
                    <TableCell>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="text-destructive hover:text-destructive"
                        onClick={() => setDeleteTarget(idp)}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>How it works</CardTitle>
        </CardHeader>
        <CardContent className="prose prose-sm dark:prose-invert max-w-none text-muted-foreground">
          <ol className="list-decimal list-inside space-y-2 text-sm">
            <li>
              Register a Velox OAuth app in your IdP and set the redirect URI to{" "}
              <code className="font-mono text-xs">
                https://your-velox-host/auth/oidc/&lt;idp_id&gt;/callback
              </code>
              .
            </li>
            <li>
              Paste the issuer URL, client ID, and client secret above. Velox
              auto-discovers the authorization and token endpoints.
            </li>
            <li>
              Send users to{" "}
              <code className="font-mono text-xs">
                /auth/oidc/&lt;idp_id&gt;/start
              </code>{" "}
              — Velox handles the PKCE flow and mints a JWT on callback.
            </li>
            <li>
              First-time users are automatically provisioned with the{" "}
              <strong>ReadOnly</strong> role. Map IdP groups to Velox roles via
              the API (<code className="font-mono text-xs">group_role_map</code>
              ).
            </li>
          </ol>
        </CardContent>
      </Card>

      {/* Add dialog */}
      <AddIdpDialog
        open={addOpen}
        onClose={() => setAddOpen(false)}
        onSubmit={(data) => addMutation.mutate(data)}
        isPending={addMutation.isPending}
      />

      {/* Delete confirmation */}
      <AlertDialog
        open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove identity provider?</AlertDialogTitle>
            <AlertDialogDescription>
              Removing <strong>{deleteTarget?.name}</strong> will prevent users
              from logging in via this provider. Existing user accounts are
              unaffected.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() =>
                deleteTarget && deleteMutation.mutate(deleteTarget.id)
              }
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
