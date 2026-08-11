import type { BaseQueryFn } from "@reduxjs/toolkit/query";
import type { ApiErrorEnvelope } from "./generated";
import type { RootState } from "../store";

export const SESSION_EXPIRED_EVENT = "dim:session-expired";
export const DEFAULT_TIMEOUT_MS = 15_000;

export type ClientErrorKind =
  | "server"
  | "network"
  | "offline"
  | "timeout"
  | "cancelled"
  | "parse";

export interface ClientError {
  status: number | "FETCH_ERROR" | "TIMEOUT_ERROR" | "PARSING_ERROR";
  kind: ClientErrorKind;
  code: string;
  message: string;
  requestId?: string;
  details?: Record<string, unknown>;
}

export interface RequestOptions extends Omit<RequestInit, "body"> {
  body?: unknown;
  timeoutMs?: number;
  token?: string | null;
}

const apiUrl = (path: string) =>
  path.startsWith("/") ? path : `/api/v1/${path}`;

const parseBody = async (response: Response): Promise<unknown> => {
  if (response.status === 204) return undefined;
  const text = await response.text();
  if (!text) return undefined;
  const contentType = response.headers.get("content-type") ?? "";
  if (!contentType.includes("json")) return text;
  try {
    return JSON.parse(text);
  } catch {
    throw <ClientError>{
      status: "PARSING_ERROR",
      kind: "parse",
      code: "invalid_response",
      message: "Dim returned an invalid response.",
      requestId: response.headers.get("x-request-id") ?? undefined,
    };
  }
};

export async function apiRequest<T>(
  path: string,
  options: RequestOptions = {}
): Promise<T> {
  const {
    body: requestBody,
    timeoutMs = DEFAULT_TIMEOUT_MS,
    token,
    ...requestInit
  } = options;
  const controller = new AbortController();
  const timeout = window.setTimeout(
    () => controller.abort("timeout"),
    timeoutMs
  );
  const signal = requestInit.signal
    ? AbortSignal.any([requestInit.signal, controller.signal])
    : controller.signal;
  const headers = new Headers(requestInit.headers);
  if (token) headers.set("Authorization", token);
  let body = requestBody as BodyInit | null | undefined;
  if (body != null && !(body instanceof FormData) && typeof body !== "string") {
    headers.set("Content-Type", "application/json");
    body = JSON.stringify(body);
  }

  try {
    const response = await fetch(apiUrl(path), {
      ...requestInit,
      body,
      credentials: "same-origin",
      headers,
      signal,
    });
    const data = await parseBody(response);
    if (!response.ok) {
      const envelope = data as Partial<ApiErrorEnvelope> | undefined;
      const error: ClientError = {
        status: response.status,
        kind: "server",
        code: envelope?.error?.code ?? `http_${response.status}`,
        message:
          envelope?.error?.message ??
          (response.status >= 500
            ? "Dim is temporarily unavailable."
            : "The request could not be completed."),
        requestId:
          envelope?.request_id ??
          response.headers.get("x-request-id") ??
          undefined,
        details: envelope?.error?.details,
      };
      if (response.status === 401 && token) {
        window.dispatchEvent(new CustomEvent(SESSION_EXPIRED_EVENT));
      }
      throw error;
    }
    return data as T;
  } catch (cause) {
    if ((cause as ClientError)?.kind) throw cause;
    if (signal.aborted) {
      const timedOut =
        controller.signal.aborted && !requestInit.signal?.aborted;
      throw <ClientError>{
        status: timedOut ? "TIMEOUT_ERROR" : "FETCH_ERROR",
        kind: timedOut ? "timeout" : "cancelled",
        code: timedOut ? "request_timeout" : "request_cancelled",
        message: timedOut
          ? "Dim took too long to respond."
          : "The request was cancelled.",
      };
    }
    const offline = typeof navigator !== "undefined" && !navigator.onLine;
    throw <ClientError>{
      status: "FETCH_ERROR",
      kind: offline ? "offline" : "network",
      code: offline ? "offline" : "server_unavailable",
      message: offline
        ? "You are offline. Reconnect to continue."
        : "Dim is unavailable. Check that the server is running.",
    };
  } finally {
    window.clearTimeout(timeout);
  }
}

type QueryArgs =
  | string
  | ({ url: string; params?: Record<string, unknown> } & RequestOptions);

export const baseQuery: BaseQueryFn<QueryArgs, unknown, ClientError> = async (
  args,
  api
) => {
  const request = typeof args === "string" ? { url: args } : args;
  const { url, params, ...options } = request;
  const token = (api.getState() as RootState).auth.token as string | null;
  const query = params
    ? `?${new URLSearchParams(
        Object.entries(params).reduce<Record<string, string>>(
          (values, [key, value]) => ({ ...values, [key]: String(value) }),
          {}
        )
      )}`
    : "";
  try {
    return {
      data: await apiRequest(url + query, {
        ...options,
        signal: api.signal,
        token,
      }),
    };
  } catch (error) {
    return { error: error as ClientError };
  }
};
