/**
 * HTTP client adapter for Kubb-generated SDK.
 *
 * The generated hooks import this as default and expect:
 * - default export: async function accepting RequestConfig, returning { data: T }
 * - named exports: RequestConfig and ResponseErrorConfig types
 */

const API_BASE = '/api';

export type RequestConfig = {
  method: string;
  url: string;
  params?: Record<string, unknown>;
  data?: unknown;
  signal?: AbortSignal;
  headers?: Record<string, string>;
};

export type ResponseErrorConfig<E> = {
  data: E;
  status: number;
};

export type Client = typeof client;

export class ApiError extends Error {
  code: string;
  status: number;

  constructor(code: string, message: string, status: number) {
    super(message);
    this.code = code;
    this.status = status;
  }
}

async function client<TData, _TError = unknown, _TBody = unknown>(
  config: RequestConfig
): Promise<{ data: TData }> {
  const { method, url, params, signal, headers } = config;

  // Build URL with query params
  const fullUrl = new URL(`${API_BASE}${url}`, window.location.origin);
  if (params) {
    for (const [key, value] of Object.entries(params)) {
      if (value != null) {
        fullUrl.searchParams.set(key, String(value));
      }
    }
  }

  const response = await fetch(fullUrl.toString(), {
    method,
    signal,
    headers: {
      'Accept': 'application/json',
      ...headers,
    },
  });

  if (!response.ok) {
    // Try to parse structured error response
    try {
      const body = await response.json();
      if (body?.error?.code) {
        throw new ApiError(body.error.code, body.error.message, response.status);
      }
    } catch (e) {
      if (e instanceof ApiError) throw e;
    }
    throw new ApiError('UNKNOWN', `API error: ${response.status}`, response.status);
  }

  const data = await response.json();
  return { data };
}

export default client;
