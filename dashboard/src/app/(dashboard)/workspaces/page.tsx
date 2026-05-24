"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  workspacesApi,
  membersApi,
  type Workspace,
  type WorkspaceMember,
} from "@/lib/api";
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
import { ChevronDown, ChevronRight, UserPlus, Pencil, Trash2 } from "lucide-react";

const ROLES = ["admin", "api_manager", "billing_viewer", "read_only"] as const;
type Role = (typeof ROLES)[number];

function roleBadge(role: Role) {
  const variants: Record<Role, string> = {
    admin: "bg-red-500/10 text-red-600 border-red-500/20",
    api_manager: "bg-blue-500/10 text-blue-600 border-blue-500/20",
    billing_viewer: "bg-yellow-500/10 text-yellow-600 border-yellow-500/20",
    read_only: "bg-gray-500/10 text-gray-600 border-gray-500/20",
  };
  const labels: Record<Role, string> = {
    admin: "Admin",
    api_manager: "API Manager",
    billing_viewer: "Billing Viewer",
    read_only: "Read Only",
  };
  return (
    <Badge
      variant="outline"
      className={`text-xs ${variants[role] ?? ""}`}
    >
      {labels[role] ?? role}
    </Badge>
  );
}

// ── Members panel ─────────────────────────────────────────────────────────────

function MembersPanel({ workspace }: { workspace: Workspace }) {
  const qc = useQueryClient();
  const [addOpen, setAddOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<WorkspaceMember | null>(null);
  const [addEmail, setAddEmail] = useState("");
  const [addRole, setAddRole] = useState<string>("api_manager");
  const [editRole, setEditRole] = useState<string>("");

  const { data, isLoading } = useQuery({
    queryKey: ["members", workspace.id],
    queryFn: () => membersApi.list(workspace.id),
  });

  const addMut = useMutation({
    mutationFn: () =>
      membersApi.add(workspace.id, { email: addEmail, role: addRole }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["members", workspace.id] });
      qc.invalidateQueries({ queryKey: ["workspaces"] });
      setAddOpen(false);
      setAddEmail("");
    },
  });

  const updateMut = useMutation({
    mutationFn: ({ userId, role }: { userId: string; role: string }) =>
      membersApi.update(workspace.id, userId, role),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["members", workspace.id] });
      setEditTarget(null);
    },
  });

  const removeMut = useMutation({
    mutationFn: (userId: string) => membersApi.remove(workspace.id, userId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["members", workspace.id] });
      qc.invalidateQueries({ queryKey: ["workspaces"] });
    },
  });

  const members = data?.data ?? [];

  return (
    <div className="mt-3 space-y-3">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          {members.length} member{members.length !== 1 ? "s" : ""}
        </p>
        <Button
          size="sm"
          variant="outline"
          onClick={() => setAddOpen(true)}
          className="gap-1.5"
        >
          <UserPlus className="h-3.5 w-3.5" />
          Add member
        </Button>
      </div>

      {isLoading ? (
        <div className="space-y-2">
          {Array.from({ length: 2 }).map((_, i) => (
            <Skeleton key={i} className="h-10 w-full" />
          ))}
        </div>
      ) : members.length === 0 ? (
        <p className="text-sm text-muted-foreground italic">No members yet.</p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Email</TableHead>
              <TableHead>Role</TableHead>
              <TableHead>Joined</TableHead>
              <TableHead className="w-20" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {members.map((m) => (
              <TableRow key={m.id}>
                <TableCell className="font-medium">{m.name}</TableCell>
                <TableCell className="text-muted-foreground text-sm">
                  {m.email}
                </TableCell>
                <TableCell>{roleBadge(m.role as Role)}</TableCell>
                <TableCell className="text-muted-foreground text-sm tabular-nums">
                  {format(parseISO(m.created_at), "MMM d, yyyy")}
                </TableCell>
                <TableCell>
                  <div className="flex gap-1">
                    <Button
                      size="icon"
                      variant="ghost"
                      className="h-7 w-7"
                      onClick={() => {
                        setEditTarget(m);
                        setEditRole(m.role);
                      }}
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      className="h-7 w-7 text-destructive hover:text-destructive"
                      onClick={() => removeMut.mutate(m.user_id)}
                      disabled={removeMut.isPending}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      {/* Add member dialog */}
      <Dialog open={addOpen} onOpenChange={setAddOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Add member to {workspace.name}</DialogTitle>
            <DialogDescription>
              The user must already have an account. Enter their email and choose a role.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-1.5">
              <Label htmlFor="add-email">Email</Label>
              <Input
                id="add-email"
                type="email"
                placeholder="user@example.com"
                value={addEmail}
                onChange={(e) => setAddEmail(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="add-role">Role</Label>
              <Select value={addRole} onValueChange={setAddRole}>
                <SelectTrigger id="add-role">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="admin">Admin</SelectItem>
                  <SelectItem value="api_manager">API Manager</SelectItem>
                  <SelectItem value="billing_viewer">Billing Viewer</SelectItem>
                  <SelectItem value="read_only">Read Only</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setAddOpen(false)}>
              Cancel
            </Button>
            <Button
              onClick={() => addMut.mutate()}
              disabled={!addEmail || addMut.isPending}
            >
              {addMut.isPending ? "Adding…" : "Add member"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit role dialog */}
      <Dialog open={!!editTarget} onOpenChange={(o) => !o && setEditTarget(null)}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>Change role</DialogTitle>
            <DialogDescription>
              Update {editTarget?.name}&apos;s role in {workspace.name}.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-1.5">
            <Label>Role</Label>
            <Select value={editRole} onValueChange={setEditRole}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="admin">Admin</SelectItem>
                <SelectItem value="api_manager">API Manager</SelectItem>
                <SelectItem value="billing_viewer">Billing Viewer</SelectItem>
                <SelectItem value="read_only">Read Only</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setEditTarget(null)}>
              Cancel
            </Button>
            <Button
              onClick={() =>
                editTarget &&
                updateMut.mutate({ userId: editTarget.user_id, role: editRole })
              }
              disabled={updateMut.isPending}
            >
              {updateMut.isPending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

// ── Workspace card ────────────────────────────────────────────────────────────

function WorkspaceCard({ workspace }: { workspace: Workspace }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <div>
            <CardTitle className="text-base">{workspace.name}</CardTitle>
            <CardDescription className="text-xs mt-0.5">
              /{workspace.slug} · Created{" "}
              {format(parseISO(workspace.created_at), "MMM d, yyyy")}
            </CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <Badge variant="secondary" className="text-xs">
              {workspace.member_count} member{workspace.member_count !== 1 ? "s" : ""}
            </Badge>
            <Button
              size="sm"
              variant="ghost"
              className="h-8 w-8 p-0"
              onClick={() => setExpanded((e) => !e)}
              aria-label={expanded ? "Collapse" : "Expand"}
            >
              {expanded ? (
                <ChevronDown className="h-4 w-4" />
              ) : (
                <ChevronRight className="h-4 w-4" />
              )}
            </Button>
          </div>
        </div>
      </CardHeader>
      {expanded && (
        <CardContent className="pt-0">
          <MembersPanel workspace={workspace} />
        </CardContent>
      )}
    </Card>
  );
}

// ── Page ──────────────────────────────────────────────────────────────────────

export default function WorkspacesPage() {
  const { data, isLoading } = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => workspacesApi.list(),
  });

  const workspaces = data?.data ?? [];

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-2xl font-semibold mb-1">Workspaces</h1>
        <p className="text-muted-foreground text-sm">
          Manage team members and their roles across workspaces.
          Role hierarchy: Admin &gt; API Manager &gt; Billing Viewer &gt; Read Only.
        </p>
      </div>

      {isLoading ? (
        <div className="space-y-3">
          {Array.from({ length: 2 }).map((_, i) => (
            <Card key={i}>
              <CardHeader>
                <Skeleton className="h-5 w-40" />
                <Skeleton className="h-4 w-24" />
              </CardHeader>
            </Card>
          ))}
        </div>
      ) : workspaces.length === 0 ? (
        <Card>
          <CardContent className="py-8 text-center text-muted-foreground text-sm">
            No workspaces found. Workspaces are created via the API or database seeding.
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3">
          {workspaces.map((ws) => (
            <WorkspaceCard key={ws.id} workspace={ws} />
          ))}
        </div>
      )}
    </div>
  );
}
