"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { promptsApi, type Prompt, type PromptVersion } from "@/lib/api";
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
import { Textarea } from "@/components/ui/textarea";
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
  SheetDescription,
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
import { Plus, Trash2, ChevronRight } from "lucide-react";

// ── Create prompt form ────────────────────────────────────────────────────────

const createSchema = z.object({
  name: z.string().min(1, "Name is required"),
  description: z.string().optional(),
});
type CreateForm = z.infer<typeof createSchema>;

// ── Add version form ──────────────────────────────────────────────────────────

const versionSchema = z.object({
  content: z.string().min(1, "Content is required"),
  system_prompt: z.string().optional(),
});
type VersionForm = z.infer<typeof versionSchema>;

// ── Detail sheet ──────────────────────────────────────────────────────────────

function PromptDetail({
  promptId,
  open,
  onClose,
}: {
  promptId: string | null;
  open: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [addVersionOpen, setAddVersionOpen] = useState(false);

  const { data, isLoading } = useQuery({
    queryKey: ["prompt", promptId],
    queryFn: () => promptsApi.get(promptId!),
    enabled: !!promptId,
  });

  const createVersionMut = useMutation({
    mutationFn: (body: VersionForm) =>
      promptsApi.createVersion(promptId!, {
        content: body.content,
        system_prompt: body.system_prompt || undefined,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["prompt", promptId] });
      qc.invalidateQueries({ queryKey: ["prompts"] });
      setAddVersionOpen(false);
      versionForm.reset();
    },
  });

  const activateMut = useMutation({
    mutationFn: (version: number) =>
      promptsApi.updateVersion(promptId!, version, { is_active: true }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["prompt", promptId] });
    },
  });

  const weightMut = useMutation({
    mutationFn: ({
      version,
      ab_weight,
    }: {
      version: number;
      ab_weight: number;
    }) =>
      promptsApi.updateVersion(promptId!, version, { ab_weight }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["prompt", promptId] });
    },
  });

  const versionForm = useForm<VersionForm>({
    resolver: zodResolver(versionSchema),
    defaultValues: { content: "", system_prompt: "" },
  });

  const prompt = data?.data.prompt;
  const versions: PromptVersion[] = data?.data.versions ?? [];

  return (
    <>
      <Sheet open={open} onOpenChange={(o) => !o && onClose()}>
        <SheetContent className="w-[560px] sm:w-[640px] overflow-y-auto">
          <SheetHeader>
            <SheetTitle>{prompt?.name ?? "Prompt"}</SheetTitle>
            {prompt?.description && (
              <SheetDescription>{prompt.description}</SheetDescription>
            )}
          </SheetHeader>

          <div className="mt-4 space-y-3">
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-medium">Versions</h3>
              <Button
                size="sm"
                variant="outline"
                onClick={() => setAddVersionOpen(true)}
              >
                <Plus className="h-3.5 w-3.5 mr-1" />
                Add version
              </Button>
            </div>

            {isLoading && (
              <div className="space-y-2">
                {Array.from({ length: 3 }).map((_, i) => (
                  <Skeleton key={i} className="h-24 w-full" />
                ))}
              </div>
            )}

            {!isLoading && versions.length === 0 && (
              <p className="text-sm text-muted-foreground text-center py-8">
                No versions yet. Add one to get started.
              </p>
            )}

            {versions.map((v) => (
              <div
                key={v.id}
                className={`rounded-md border p-3 space-y-2 text-sm ${v.is_active ? "border-primary/40 bg-primary/5" : ""}`}
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <span className="font-medium">v{v.version}</span>
                    {v.is_active && (
                      <Badge
                        variant="outline"
                        className="text-green-600 border-green-600/30 text-xs"
                      >
                        active
                      </Badge>
                    )}
                  </div>
                  <div className="flex items-center gap-2">
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs text-muted-foreground">A/B weight:</span>
                      <Input
                        type="number"
                        min={0}
                        max={100}
                        className="h-6 w-16 text-xs px-1"
                        defaultValue={v.ab_weight}
                        onBlur={(e) => {
                          const val = parseInt(e.target.value);
                          if (!isNaN(val) && val !== v.ab_weight) {
                            weightMut.mutate({
                              version: v.version,
                              ab_weight: val,
                            });
                          }
                        }}
                      />
                    </div>
                    {!v.is_active && (
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-6 text-xs px-2"
                        onClick={() => activateMut.mutate(v.version)}
                        disabled={activateMut.isPending}
                      >
                        Activate
                      </Button>
                    )}
                  </div>
                </div>

                {v.system_prompt && (
                  <div>
                    <p className="text-xs text-muted-foreground mb-1">
                      System prompt
                    </p>
                    <p className="text-xs bg-muted rounded px-2 py-1 font-mono whitespace-pre-wrap">
                      {v.system_prompt}
                    </p>
                  </div>
                )}

                <div>
                  <p className="text-xs text-muted-foreground mb-1">Content</p>
                  <p className="text-xs bg-muted rounded px-2 py-1 font-mono whitespace-pre-wrap max-h-32 overflow-y-auto">
                    {v.content}
                  </p>
                </div>

                <p className="text-xs text-muted-foreground">
                  Created {format(parseISO(v.created_at), "MMM d, yyyy HH:mm")}
                </p>
              </div>
            ))}
          </div>
        </SheetContent>
      </Sheet>

      {/* Add version dialog */}
      <Dialog open={addVersionOpen} onOpenChange={setAddVersionOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Add version</DialogTitle>
            <DialogDescription>
              New versions start inactive. Activate one to make it live.
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={versionForm.handleSubmit((v) =>
              createVersionMut.mutate(v)
            )}
            className="space-y-4"
          >
            <div className="space-y-1.5">
              <Label htmlFor="system-prompt">System prompt (optional)</Label>
              <Textarea
                id="system-prompt"
                rows={3}
                placeholder="You are a helpful assistant…"
                {...versionForm.register("system_prompt")}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="content">Content</Label>
              <Textarea
                id="content"
                rows={6}
                placeholder="Enter the prompt content…"
                {...versionForm.register("content")}
              />
              {versionForm.formState.errors.content && (
                <p className="text-xs text-destructive">
                  {versionForm.formState.errors.content.message}
                </p>
              )}
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="ghost"
                onClick={() => setAddVersionOpen(false)}
              >
                Cancel
              </Button>
              <Button type="submit" disabled={createVersionMut.isPending}>
                {createVersionMut.isPending ? "Adding…" : "Add version"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}

// ── Main page ─────────────────────────────────────────────────────────────────

export default function PromptsPage() {
  const qc = useQueryClient();
  const [createOpen, setCreateOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Prompt | null>(null);
  const [detailTarget, setDetailTarget] = useState<string | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ["prompts"],
    queryFn: () => promptsApi.list(),
  });

  const createMut = useMutation({
    mutationFn: (body: CreateForm) =>
      promptsApi.create({
        name: body.name,
        description: body.description || undefined,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["prompts"] });
      setCreateOpen(false);
      form.reset();
    },
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => promptsApi.delete(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["prompts"] });
      setDeleteTarget(null);
    },
  });

  const form = useForm<CreateForm>({
    resolver: zodResolver(createSchema),
    defaultValues: { name: "", description: "" },
  });

  const prompts: Prompt[] = data?.data ?? [];

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold mb-1">Prompts</h1>
          <p className="text-muted-foreground text-sm">
            Versioned prompt templates with A/B weight support.
          </p>
        </div>
        <Button
          size="sm"
          onClick={() => {
            form.reset({ name: "", description: "" });
            setCreateOpen(true);
          }}
        >
          <Plus className="h-4 w-4 mr-1" />
          New prompt
        </Button>
      </div>

      <Card>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Description</TableHead>
                <TableHead>Last updated</TableHead>
                <TableHead className="w-20" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading
                ? Array.from({ length: 4 }).map((_, i) => (
                    <TableRow key={i}>
                      {Array.from({ length: 4 }).map((_, j) => (
                        <TableCell key={j}>
                          <Skeleton className="h-4 w-full" />
                        </TableCell>
                      ))}
                    </TableRow>
                  ))
                : prompts.map((prompt) => (
                    <TableRow
                      key={prompt.id}
                      className="cursor-pointer hover:bg-muted/50"
                      onClick={() => setDetailTarget(prompt.id)}
                    >
                      <TableCell className="font-medium">
                        <div className="flex items-center gap-1">
                          {prompt.name}
                          <ChevronRight className="h-3.5 w-3.5 text-muted-foreground" />
                        </div>
                      </TableCell>
                      <TableCell className="text-sm text-muted-foreground max-w-xs truncate">
                        {prompt.description ?? "—"}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {format(parseISO(prompt.updated_at), "MMM d, HH:mm")}
                      </TableCell>
                      <TableCell>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7 text-muted-foreground hover:text-destructive"
                          onClick={(e) => {
                            e.stopPropagation();
                            setDeleteTarget(prompt);
                          }}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))}
              {!isLoading && prompts.length === 0 && (
                <TableRow>
                  <TableCell
                    colSpan={4}
                    className="text-center py-12 text-muted-foreground text-sm"
                  >
                    No prompts yet. Create one to get started.
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      {/* Create dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Create prompt</DialogTitle>
            <DialogDescription>
              Add versions after creation to build out the template.
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={form.handleSubmit((v) => createMut.mutate(v))}
            className="space-y-4"
          >
            <div className="space-y-1.5">
              <Label htmlFor="prompt-name">Name</Label>
              <Input
                id="prompt-name"
                placeholder="Customer support greeting"
                {...form.register("name")}
              />
              {form.formState.errors.name && (
                <p className="text-xs text-destructive">
                  {form.formState.errors.name.message}
                </p>
              )}
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="prompt-desc">Description (optional)</Label>
              <Input
                id="prompt-desc"
                placeholder="Used for initial customer contact…"
                {...form.register("description")}
              />
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
              This prompt and all its versions will be permanently removed.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() =>
                deleteTarget && deleteMut.mutate(deleteTarget.id)
              }
              disabled={deleteMut.isPending}
            >
              {deleteMut.isPending ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Detail sheet */}
      <PromptDetail
        promptId={detailTarget}
        open={!!detailTarget}
        onClose={() => setDetailTarget(null)}
      />
    </div>
  );
}
