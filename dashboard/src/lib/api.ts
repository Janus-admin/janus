/**
 * Velox Admin API client.
 *
 * Base URL resolution:
 *   - In development (Next.js dev server on :3000, Rust on :8080):
 *     set NEXT_PUBLIC_API_URL=http://localhost:8080
 *   - In production (embedded in Rust binary, same origin):
 *     leave unset — all requests are relative to the same origin.
 */

const BASE =
  typeof process !== "undefined"
    ? (process.env.NEXT_PUBLIC_API_URL ?? "")
    : "";

// ── Auth token ────────────────────────────────────────────────────────────────

export function getToken(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem("velox_token");
}

export function setToken(token: string) {
  localStorage.setItem("velox_token", token);
}

export function clearToken() {
  localStorage.removeItem("velox_token");
}

// ── Core fetch wrapper ────────────────────────────────────────────────────────

async function apiFetch<T>(
  path: string,
  options: RequestInit = {}
): Promise<T> {
  const token = getToken();
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(options.headers as Record<string, string>),
  };
  if (token) headers["Authorization"] = `Bearer ${token}`;

  const res = await fetch(`${BASE}${path}`, { ...options, headers });

  if (res.status === 401) {
    clearToken();
    // Let the caller handle the redirect.
    throw new ApiError(401, "Unauthorized");
  }

  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new ApiError(
      res.status,
      body?.error?.message ?? `HTTP ${res.status}`
    );
  }

  return res.json() as Promise<T>;
}

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string
  ) {
    super(message);
  }
}

// ── Shared pagination meta ────────────────────────────────────────────────────

export interface Meta {
  page: number;
  per_page: number;
  total: number;
}

// ── Auth ──────────────────────────────────────────────────────────────────────

export interface LoginRequest {
  email: string;
  password: string;
}

export interface LoginResponse {
  token: string;
  user: {
    id: string;
    email: string;
    name: string;
  };
}

export const auth = {
  login: (body: LoginRequest) =>
    apiFetch<LoginResponse>("/api/v1/auth/login", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  me: () => apiFetch<{ data: LoginResponse["user"] }>("/api/v1/auth/me"),
};

// ── API Keys ──────────────────────────────────────────────────────────────────

export interface ApiKey {
  id: string;
  name: string;
  key_prefix: string;
  workspace_id: string | null;
  budget_limit: number | null;
  budget_used: number;
  rate_limit_rpm: number | null;
  rate_limit_tpm: number | null;
  allowed_models: string[] | null;
  is_active: boolean;
  created_at: string;
  expires_at: string | null;
  last_used_at: string | null;
}

export interface CreateKeyRequest {
  name: string;
  budget_limit?: number | null;
  rate_limit_rpm?: number | null;
  rate_limit_tpm?: number | null;
  allowed_models?: string[] | null;
  expires_at?: string | null;
}

export interface UpdateKeyRequest {
  name?: string;
  budget_limit?: number | null;
  rate_limit_rpm?: number | null;
  rate_limit_tpm?: number | null;
  allowed_models?: string[] | null;
  expires_at?: string | null;
  is_active?: boolean;
}

export const keys = {
  list: (page = 1, per_page = 50) =>
    apiFetch<{ data: ApiKey[]; meta: Meta }>(
      `/admin/keys?page=${page}&per_page=${per_page}`
    ),
  get: (id: string) =>
    apiFetch<{ data: ApiKey }>(`/admin/keys/${id}`),
  create: (body: CreateKeyRequest) =>
    apiFetch<{ data: ApiKey & { key: string } }>("/admin/keys", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  update: (id: string, body: UpdateKeyRequest) =>
    apiFetch<{ data: ApiKey }>(`/admin/keys/${id}`, {
      method: "PATCH",
      body: JSON.stringify(body),
    }),
  revoke: (id: string) =>
    apiFetch<{ data: { revoked: boolean } }>(`/admin/keys/${id}`, {
      method: "DELETE",
    }),
};

// ── Requests ──────────────────────────────────────────────────────────────────

export interface GatewayRequest {
  id: string;
  api_key_id: string | null;
  workspace_id: string | null;
  provider: string;
  model: string;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  total_tokens: number | null;
  cost_usd: number | null;
  latency_ms: number | null;
  ttfb_ms: number | null;
  status: string;
  cache_type: string | null;
  cache_similarity: number | null;
  stream: boolean;
  created_at: string;
}

export interface RequestFilter {
  page?: number;
  per_page?: number;
  provider?: string;
  model?: string;
  status?: string;
  api_key_id?: string;
}

export const requests = {
  list: (filter: RequestFilter = {}) => {
    const p = new URLSearchParams();
    if (filter.page) p.set("page", String(filter.page));
    if (filter.per_page) p.set("per_page", String(filter.per_page));
    if (filter.provider) p.set("provider", filter.provider);
    if (filter.model) p.set("model", filter.model);
    if (filter.status) p.set("status", filter.status);
    if (filter.api_key_id) p.set("api_key_id", filter.api_key_id);
    return apiFetch<{ data: GatewayRequest[]; meta: Meta }>(
      `/admin/requests?${p}`
    );
  },
  get: (id: string) =>
    apiFetch<{ data: GatewayRequest }>(`/admin/requests/${id}`),
};

// ── Analytics ─────────────────────────────────────────────────────────────────

export interface PeriodStats {
  requests: number;
  cost_usd: number | null;
  tokens: number | null;
  cache_hits: number;
  errors: number;
  avg_latency_ms: number | null;
}

export interface OverviewResponse {
  today: PeriodStats;
  last_7d: PeriodStats;
  last_30d: PeriodStats;
}

export interface DailyCostRow {
  day: string;
  cost_usd: number | null;
  requests: number;
  tokens: number | null;
}

export interface GroupCostRow {
  group_key: string;
  cost_usd: number | null;
  requests: number;
}

export interface CostBreakdown {
  by_day: DailyCostRow[];
  by_provider: GroupCostRow[];
  by_model: GroupCostRow[];
}

export interface LatencyRow {
  model: string;
  provider: string;
  p50: number | null;
  p95: number | null;
  p99: number | null;
  avg_ms: number | null;
  sample_count: number;
}

export interface CacheTypeRow {
  cache_type: string;
  hit_count: number;
  tokens_saved: number | null;
  cost_saved: number | null;
}

export interface CacheAnalytics {
  total_requests: number;
  total_hits: number;
  hit_rate: number;
  by_type: CacheTypeRow[];
  avg_semantic_similarity: number | null;
}

export const analytics = {
  overview: () => apiFetch<OverviewResponse>("/admin/analytics/overview"),
  costs: (days = 30) =>
    apiFetch<CostBreakdown>(`/admin/analytics/costs?days=${days}`),
  latency: (hours = 24) =>
    apiFetch<{ data: LatencyRow[] }>(`/admin/analytics/latency?hours=${hours}`),
  cache: (hours = 24) =>
    apiFetch<CacheAnalytics>(`/admin/analytics/cache?hours=${hours}`),
};

// ── Providers ─────────────────────────────────────────────────────────────────

export interface VeloxProvider {
  id: string;
  display_name: string;
  is_enabled: boolean;
  priority: number;
  base_url: string;
  timeout_ms: number;
  max_retries: number;
  health_status: string;
  last_health_check: string | null;
}

export interface UpdateProviderRequest {
  is_enabled?: boolean;
  priority?: number;
  api_key?: string;
  timeout_ms?: number;
  max_retries?: number;
}

export interface TestProviderResult {
  provider: string;
  health_status: string;
  reachable: boolean;
  http_status: number | null;
  error: string | null;
}

export const providers = {
  list: () =>
    apiFetch<{ data: VeloxProvider[] }>("/admin/providers"),
  update: (id: string, body: UpdateProviderRequest) =>
    apiFetch<{ data: VeloxProvider }>(`/admin/providers/${id}`, {
      method: "PATCH",
      body: JSON.stringify(body),
    }),
  test: (id: string) =>
    apiFetch<{ data: TestProviderResult }>(`/admin/providers/${id}/test`, {
      method: "POST",
    }),
};

// ── Cache ─────────────────────────────────────────────────────────────────────

export interface CacheStats {
  exact_entries: number;
  semantic_entries: number;
  total_hits: number;
  exact_hits: number;
  semantic_hits: number;
  tokens_saved: number;
  cost_saved_usd: number;
}

export const cache = {
  stats: () => apiFetch<{ data: CacheStats }>("/admin/cache/stats"),
  flush: () =>
    apiFetch<{ data: { flushed: boolean } }>("/admin/cache", {
      method: "DELETE",
    }),
};

// ── Config ────────────────────────────────────────────────────────────────────

export interface VeloxConfig {
  host: string;
  port: number;
  request_timeout_ms: number;
  db_pool_max_connections: number;
  jwt_expiration_hours: number;
  log_level: string;
  log_request_bodies: boolean;
  log_response_bodies: boolean;
  cache_enabled: boolean;
  cache_ttl_seconds: number;
  cache_max_entries: number;
  semantic_cache_threshold: number;
  embedding_model_path: string;
  embedding_tokenizer_path: string;
  rate_limit_window_secs: number;
  max_retries: number;
  prometheus_enabled: boolean;
  semantic_cache_available: boolean;
}

export const config = {
  get: () => apiFetch<{ data: VeloxConfig }>("/admin/config"),
};
