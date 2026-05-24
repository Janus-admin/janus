"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { keys, adminModelsApi, type ModelWithPricing, type CreateKeyRequest, type UpdateKeyRequest, type ApiKey } from "@/lib/api";
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
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
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { format, parseISO, isAfter } from "date-fns";
import { Plus, Copy, Check, Trash2, RefreshCw, Clock, Pencil, X, ChevronsUpDown } from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";

const ROUTING_STRATEGIES = [
  { value: "priority", label: "Priority" },
  { value: "cost_optimized", label: "Cost optimized" },
  { value: "latency_optimized", label: "Latency optimized" },
  { value: "round_robin", label: "Round robin" },
];

const keySchema = z.object({
  name: z.string().min(1, "Name is required"),
  budget_limit: z.string().optional(),
  rate_limit_rpm: z.string().optional(),
  rate_limit_tpm: z.string().optional(),
  allowed_models: z.string().optional(),
  expires_at: z.string().optional(),
  routing_strategy: z.string().optional(),
});
type KeyForm = z.infer<typeof keySchema>;

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

function GraceBadge({ expiresAt }: { expiresAt: string | null }) {
  if (!expiresAt) return null;
  const expires = parseISO(expiresAt);
  if (!isAfter(expires, new Date())) return null;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge
          variant="outline"
          className="text-amber-600 border-amber-600/30 text-xs gap-1 cursor-default"
        >
          <Clock className="h-2.5 w-2.5" />
          grace
        </Badge>
      </TooltipTrigger>
      <TooltipContent>
        Old key valid until {format(expires, "MMM d HH:mm")}
      </TooltipContent>
    </Tooltip>
  );
}

function ModelTags({ models }: { models: string[] | null }) {
  if (!models || models.length === 0) return <span className="text-muted-foreground text-xs">all</span>;
  return (
    <div className="flex flex-wrap gap-1">
      {models.map((m) => (
        <Badge key={m} variant="secondary" className="text-xs font-mono px-1.5 py-0">
          {m}
        </Badge>
      ))}
    </div>
  );
}

// Quality tier: S = flagship, A = high, B = mid, C = fast/cheap
type QualityTier = "S" | "A" | "B" | "C";

const QUALITY_MAP: Record<string, QualityTier> = {
  // S — flagship
  "claude-opus-4-7": "S", "claude-opus-4-5": "S", "claude-3-opus-20240229": "S",
  "o3": "S", "gemini-2.5-pro": "S",
  // A — high capability
  "claude-sonnet-4-6": "A", "claude-sonnet-4-5": "A",
  "claude-3-5-sonnet-20241022": "A",
  "gpt-4.1": "A", "gpt-4o": "A", "gpt-4-turbo": "A", "o1": "A", "o4-mini": "A",
  "gemini-2.5-flash": "A", "gemini-1.5-pro": "A",
  "deepseek-r1": "A", "deepseek-reasoner": "A",
  // B — mid
  "claude-haiku-4-5-20251001": "B", "claude-haiku-4-5": "B",
  "claude-3-5-haiku-20241022": "B", "claude-3-haiku-20240307": "B",
  "gpt-4.1-mini": "B", "gpt-4o-mini": "B", "o3-mini": "B", "o1-mini": "B",
  "gemini-2.0-flash": "B", "gemini-1.5-flash": "B",
  "llama-3.3-70b-versatile": "B", "llama-3.1-70b-versatile": "B",
  "llama3-70b-8192": "B", "llama-3.2-90b-vision-preview": "B",
  "qwen-qwq-32b": "B", "qwen-2.5-coder-32b": "B",
  "mixtral-8x7b-32768": "B", "deepseek-chat": "B",
};

function getTier(modelId: string): QualityTier {
  return QUALITY_MAP[modelId] ?? "C";
}

const TIER_STYLE: Record<QualityTier, string> = {
  S: "bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300",
  A: "bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300",
  B: "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300",
  C: "bg-muted text-muted-foreground",
};

function TierBadge({ tier }: { tier: QualityTier }) {
  return (
    <span className={`inline-flex items-center rounded px-1 py-0 text-[10px] font-bold leading-4 ${TIER_STYLE[tier]}`}>
      {tier}
    </span>
  );
}

function PriceLabel({ input, output }: { input: number; output: number }) {
  const fmt = (n: number) => n < 1 ? `$${n.toFixed(3)}` : `$${n.toFixed(2)}`;
  return (
    <span className="text-[10px] text-muted-foreground tabular-nums whitespace-nowrap">
      {fmt(input)} / {fmt(output)}
    </span>
  );
}

// Power rank: lower index = more powerful. Unknown models go to end.
const POWER_ORDER: Record<string, string[]> = {
  anthropic: [
    "claude-opus-4-7", "claude-opus-4-5", "claude-sonnet-4-6", "claude-sonnet-4-5",
    "claude-3-5-sonnet-20241022", "claude-3-opus-20240229",
    "claude-haiku-4-5-20251001", "claude-haiku-4-5",
    "claude-3-5-haiku-20241022", "claude-3-haiku-20240307",
  ],
  openai: [
    "o3", "o4-mini", "o1", "gpt-4.1", "gpt-4o", "gpt-4-turbo",
    "o1-mini", "o3-mini", "gpt-4.1-mini", "gpt-3.5-turbo", "gpt-4.1-nano",
  ],
  gemini: [
    "gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash",
    "gemini-1.5-pro", "gemini-2.0-flash-lite", "gemini-1.5-flash",
  ],
  groq: [
    "llama-3.3-70b-versatile", "llama-3.1-70b-versatile",
    "llama-3.2-90b-vision-preview", "qwen-qwq-32b", "qwen-2.5-coder-32b",
    "mixtral-8x7b-32768", "llama3-70b-8192", "llama-3.2-11b-vision-preview",
    "gemma2-9b-it", "llama3-8b-8192", "llama-3.1-8b-instant",
    "llama-3.2-3b-preview", "llama-3.2-1b-preview",
  ],
  deepseek: ["deepseek-r1", "deepseek-reasoner", "deepseek-chat"],
  bedrock: [
    "anthropic.claude-sonnet-4-5", "anthropic.claude-3-5-sonnet-20241022-v2:0",
    "meta.llama3-2-90b-instruct-v1:0", "meta.llama3-1-70b-instruct-v1:0",
    "anthropic.claude-3-haiku-20240307-v1:0", "meta.llama3-70b-instruct-v1:0",
    "amazon.titan-text-express-v1",
  ],
};

const PROVIDER_ORDER = ["anthropic", "openai", "gemini", "groq", "deepseek", "bedrock"];

function sortByPower(provider: string, models: ModelWithPricing[]): ModelWithPricing[] {
  const order = POWER_ORDER[provider] ?? [];
  return [...models].sort((a, b) => {
    const ia = order.indexOf(a.model_id);
    const ib = order.indexOf(b.model_id);
    if (ia === -1 && ib === -1) return a.model_id.localeCompare(b.model_id);
    if (ia === -1) return 1;
    if (ib === -1) return -1;
    return ia - ib;
  });
}

function AllowedModelsInput({
  value,
  onChange,
}: {
  value: string[];
  onChange: (v: string[]) => void;
}) {
  const [open, setOpen] = useState(false);

  const { data: modelsData } = useQuery({
    queryKey: ["admin-models"],
    queryFn: () => adminModelsApi.list(),
    staleTime: 5 * 60 * 1000,
  });

  const available = modelsData?.data ?? [];

  const grouped = available.reduce<Record<string, ModelWithPricing[]>>((acc, m) => {
    if (!acc[m.provider]) acc[m.provider] = [];
    acc[m.provider].push(m);
    return acc;
  }, {});

  const sortedProviders = [
    ...PROVIDER_ORDER.filter((p) => grouped[p]),
    ...Object.keys(grouped).filter((p) => !PROVIDER_ORDER.includes(p)).sort(),
  ];

  function toggle(modelId: string) {
    if (value.includes(modelId)) {
      onChange(value.filter((m) => m !== modelId));
    } else {
      onChange([...value, modelId]);
    }
  }

  function remove(modelId: string) {
    onChange(value.filter((m) => m !== modelId));
  }

  return (
    <div className="space-y-2">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="outline"
            role="combobox"
            aria-expanded={open}
            className="w-full justify-between font-normal"
          >
            <span className="text-muted-foreground">
              {value.length === 0
                ? "All models allowed"
                : `${value.length} model${value.length > 1 ? "s" : ""} selected`}
            </span>
            <ChevronsUpDown className="h-4 w-4 shrink-0 opacity-50" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-[520px] p-0" align="start">
          <Command className="overflow-visible">
            <CommandInput placeholder="Search models…" />
            <div
              className="h-80 overflow-y-auto py-1"
              onWheel={(e) => e.stopPropagation()}
            >
              <CommandList className="max-h-none overflow-visible">
                <CommandEmpty>No models found.</CommandEmpty>
                {sortedProviders.map((provider) => (
                  <CommandGroup key={provider} heading={provider}>
                    {sortByPower(provider, grouped[provider]).map((m) => {
                      const selected = value.includes(m.model_id);
                      const tier = getTier(m.model_id);
                      return (
                        <CommandItem
                          key={m.model_id}
                          value={`${m.model_id} ${m.model_display_name ?? ""}`}
                          onSelect={() => toggle(m.model_id)}
                          className="flex items-center gap-2 cursor-pointer"
                        >
                          <div
                            className={`h-4 w-4 rounded border flex items-center justify-center shrink-0 ${
                              selected
                                ? "bg-primary border-primary"
                                : "border-muted-foreground/40"
                            }`}
                          >
                            {selected && <Check className="h-2.5 w-2.5 text-primary-foreground" />}
                          </div>
                          <TierBadge tier={tier} />
                          <span className="font-mono text-xs flex-1 truncate">
                            {m.model_display_name ?? m.model_id}
                          </span>
                          <PriceLabel
                            input={m.input_per_1m_tokens}
                            output={m.output_per_1m_tokens}
                          />
                        </CommandItem>
                      );
                    })}
                  </CommandGroup>
                ))}
              </CommandList>
            </div>
            <div className="border-t px-3 py-1.5 flex items-center gap-3 text-[10px] text-muted-foreground">
              <span className="flex items-center gap-1"><TierBadge tier="S" /> Flagship</span>
              <span className="flex items-center gap-1"><TierBadge tier="A" /> High</span>
              <span className="flex items-center gap-1"><TierBadge tier="B" /> Mid</span>
              <span className="flex items-center gap-1"><TierBadge tier="C" /> Fast</span>
              <span className="ml-auto">price per 1M tokens: in / out</span>
            </div>
          </Command>
        </PopoverContent>
      </Popover>

      {value.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {value.map((m) => (
            <Badge key={m} variant="secondary" className="gap-1 font-mono text-xs">
              {m}
              <button
                type="button"
                onClick={() => remove(m)}
                className="ml-0.5 hover:text-destructive"
              >
                <X className="h-2.5 w-2.5" />
              </button>
            </Badge>
          ))}
        </div>
      )}
      <p className="text-xs text-muted-foreground">
        Leave empty to allow all models. Prices shown per 1M tokens (input / output).
      </p>
    </div>
  );
}

function KeyFormFields({
  form,
  allowedModels,
  setAllowedModels,
}: {
  form: ReturnType<typeof useForm<KeyForm>>;
  allowedModels: string[];
  setAllowedModels: (v: string[]) => void;
}) {
  return (
    <>
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
          <Label htmlFor="expires_at">Expires at</Label>
          <Input
            id="expires_at"
            type="datetime-local"
            {...form.register("expires_at")}
          />
        </div>
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div className="space-y-1.5">
          <Label htmlFor="rpm">Rate limit (rpm)</Label>
          <Input
            id="rpm"
            type="number"
            placeholder="unlimited"
            {...form.register("rate_limit_rpm")}
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="tpm">Rate limit (tpm)</Label>
          <Input
            id="tpm"
            type="number"
            placeholder="unlimited"
            {...form.register("rate_limit_tpm")}
          />
        </div>
      </div>
      <div className="space-y-1.5">
        <Label>Routing strategy</Label>
        <Select
          defaultValue={form.getValues("routing_strategy") ?? "priority"}
          onValueChange={(v) => form.setValue("routing_strategy", v)}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {ROUTING_STRATEGIES.map((s) => (
              <SelectItem key={s.value} value={s.value}>
                {s.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1.5">
        <Label>Allowed models</Label>
        <AllowedModelsInput value={allowedModels} onChange={setAllowedModels} />
      </div>
    </>
  );
}

export default function KeysPage() {
  const qc = useQueryClient();
  const [createOpen, setCreateOpen] = useState(false);
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [revokeTarget, setRevokeTarget] = useState<ApiKey | null>(null);
  const [rotateTarget, setRotateTarget] = useState<ApiKey | null>(null);
  const [rotatedKey, setRotatedKey] = useState<string | null>(null);
  const [editTarget, setEditTarget] = useState<ApiKey | null>(null);

  const [createModels, setCreateModels] = useState<string[]>([]);
  const [editModels, setEditModels] = useState<string[]>([]);

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
      setCreateModels([]);
    },
  });

  const editMut = useMutation({
    mutationFn: ({ id, body }: { id: string; body: UpdateKeyRequest }) =>
      keys.update(id, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["keys"] });
      setEditTarget(null);
    },
  });

  const revokeMut = useMutation({
    mutationFn: (id: string) => keys.revoke(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["keys"] });
      setRevokeTarget(null);
    },
  });

  const rotateMut = useMutation({
    mutationFn: (id: string) => keys.rotate(id),
    onSuccess: (res) => {
      qc.invalidateQueries({ queryKey: ["keys"] });
      setRotateTarget(null);
      setRotatedKey(res.data.key);
    },
  });

  const createForm = useForm<KeyForm>({
    resolver: zodResolver(keySchema),
    defaultValues: { name: "", routing_strategy: "priority" },
  });

  const editForm = useForm<KeyForm>({
    resolver: zodResolver(keySchema),
  });

  function onCreateSubmit(values: KeyForm) {
    createMut.mutate({
      name: values.name,
      budget_limit: values.budget_limit ? parseFloat(values.budget_limit) : null,
      rate_limit_rpm: values.rate_limit_rpm ? parseInt(values.rate_limit_rpm) : null,
      rate_limit_tpm: values.rate_limit_tpm ? parseInt(values.rate_limit_tpm) : null,
      allowed_models: createModels.length > 0 ? createModels : null,
      expires_at: values.expires_at ? new Date(values.expires_at).toISOString() : null,
      routing_strategy: values.routing_strategy || "priority",
    });
  }

  function openEdit(key: ApiKey) {
    setEditTarget(key);
    setEditModels(key.allowed_models ?? []);
    editForm.reset({
      name: key.name,
      budget_limit: key.budget_limit != null ? String(key.budget_limit) : "",
      rate_limit_rpm: key.rate_limit_rpm != null ? String(key.rate_limit_rpm) : "",
      rate_limit_tpm: key.rate_limit_tpm != null ? String(key.rate_limit_tpm) : "",
      expires_at: key.expires_at
        ? format(parseISO(key.expires_at), "yyyy-MM-dd'T'HH:mm")
        : "",
      routing_strategy: key.routing_strategy ?? "priority",
    });
  }

  function onEditSubmit(values: KeyForm) {
    if (!editTarget) return;
    editMut.mutate({
      id: editTarget.id,
      body: {
        name: values.name,
        budget_limit: values.budget_limit ? parseFloat(values.budget_limit) : null,
        rate_limit_rpm: values.rate_limit_rpm ? parseInt(values.rate_limit_rpm) : null,
        rate_limit_tpm: values.rate_limit_tpm ? parseInt(values.rate_limit_tpm) : null,
        allowed_models: editModels.length > 0 ? editModels : null,
        expires_at: values.expires_at
          ? new Date(values.expires_at).toISOString()
          : null,
      },
    });
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
            createForm.reset({ name: "", routing_strategy: "priority" });
            setCreateModels([]);
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
                <TableHead>Rate limits</TableHead>
                <TableHead>Allowed models</TableHead>
                <TableHead>Routing</TableHead>
                <TableHead>Expires</TableHead>
                <TableHead>Last used</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="w-24" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading
                ? Array.from({ length: 4 }).map((_, i) => (
                    <TableRow key={i}>
                      {Array.from({ length: 10 }).map((_, j) => (
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
                        <div className="space-y-0.5">
                          {key.rate_limit_rpm ? (
                            <div>{key.rate_limit_rpm} rpm</div>
                          ) : null}
                          {key.rate_limit_tpm ? (
                            <div className="text-xs">{key.rate_limit_tpm} tpm</div>
                          ) : null}
                          {!key.rate_limit_rpm && !key.rate_limit_tpm && "—"}
                        </div>
                      </TableCell>
                      <TableCell>
                        <ModelTags models={key.allowed_models} />
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {key.routing_strategy ?? "priority"}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {key.expires_at
                          ? format(parseISO(key.expires_at), "MMM d yyyy")
                          : "—"}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {key.last_used_at
                          ? format(parseISO(key.last_used_at), "MMM d HH:mm")
                          : "never"}
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-1.5">
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
                          <GraceBadge expiresAt={key.rotation_expires_at ?? null} />
                        </div>
                      </TableCell>
                      <TableCell>
                        {key.is_active && (
                          <div className="flex items-center gap-1">
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7 text-muted-foreground"
                                  onClick={() => openEdit(key)}
                                >
                                  <Pencil className="h-3.5 w-3.5" />
                                </Button>
                              </TooltipTrigger>
                              <TooltipContent>Edit key</TooltipContent>
                            </Tooltip>
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7 text-muted-foreground"
                                  onClick={() => setRotateTarget(key)}
                                >
                                  <RefreshCw className="h-3.5 w-3.5" />
                                </Button>
                              </TooltipTrigger>
                              <TooltipContent>Rotate key</TooltipContent>
                            </Tooltip>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7 text-muted-foreground hover:text-destructive"
                              onClick={() => setRevokeTarget(key)}
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
              {!isLoading && (data?.data ?? []).length === 0 && (
                <TableRow>
                  <TableCell
                    colSpan={10}
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
        <DialogContent className="sm:max-w-lg max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Create API key</DialogTitle>
            <DialogDescription>
              The full key is shown only once after creation.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={createForm.handleSubmit(onCreateSubmit)} className="space-y-4">
            <KeyFormFields
              form={createForm}
              allowedModels={createModels}
              setAllowedModels={setCreateModels}
            />
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={() => setCreateOpen(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={createMut.isPending}>
                {createMut.isPending ? "Creating…" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      {/* Edit key dialog */}
      <Dialog open={!!editTarget} onOpenChange={(o) => !o && setEditTarget(null)}>
        <DialogContent className="sm:max-w-lg max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Edit key — {editTarget?.name}</DialogTitle>
            <DialogDescription>
              Changes take effect immediately.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={editForm.handleSubmit(onEditSubmit)} className="space-y-4">
            <KeyFormFields
              form={editForm}
              allowedModels={editModels}
              setAllowedModels={setEditModels}
            />
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={() => setEditTarget(null)}>
                Cancel
              </Button>
              <Button type="submit" disabled={editMut.isPending}>
                {editMut.isPending ? "Saving…" : "Save changes"}
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

      {/* Rotate confirmation */}
      <AlertDialog
        open={!!rotateTarget}
        onOpenChange={(o) => !o && setRotateTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Rotate &ldquo;{rotateTarget?.name}&rdquo;?
            </AlertDialogTitle>
            <AlertDialogDescription>
              A new secret will be generated. The old key remains valid for a
              short grace period so you have time to swap it out.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => rotateTarget && rotateMut.mutate(rotateTarget.id)}
              disabled={rotateMut.isPending}
            >
              {rotateMut.isPending ? "Rotating…" : "Rotate"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Rotated key reveal dialog */}
      <Dialog open={!!rotatedKey} onOpenChange={() => setRotatedKey(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Key rotated</DialogTitle>
            <DialogDescription>
              Copy the new key now — it will never be shown again. The old key
              remains valid during the grace period.
            </DialogDescription>
          </DialogHeader>
          <div className="flex items-center gap-2 rounded-md border bg-muted px-3 py-2">
            <code className="flex-1 text-sm font-mono break-all text-foreground">
              {rotatedKey}
            </code>
            {rotatedKey && <CopyButton text={rotatedKey} />}
          </div>
          <DialogFooter>
            <Button onClick={() => setRotatedKey(null)}>Done</Button>
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
            <AlertDialogTitle>
              Revoke &ldquo;{revokeTarget?.name}&rdquo;?
            </AlertDialogTitle>
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
