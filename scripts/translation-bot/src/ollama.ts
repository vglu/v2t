/**
 * Ollama HTTP client — call /api/generate with retry/backoff,
 * /api/ps to verify model is loaded, /api/tags to list.
 *
 * No external deps — uses Node 22 built-in fetch.
 */
import {
  CALL_TIMEOUT_MS,
  INFERENCE_OPTIONS,
  OLLAMA_MODEL,
  OLLAMA_URL,
  RETRY_DELAYS_MS,
} from './config.js';

export interface GenerateResult {
  response: string;
  elapsedMs: number;
  attempts: number;
}

export class OllamaError extends Error {
  constructor(public readonly code: string, message: string) {
    super(message);
    this.name = 'OllamaError';
  }
}

/**
 * Single /api/generate call with timeout. Returns raw response text.
 * Throws OllamaError on transport errors or non-200 status.
 */
async function generateOnce(prompt: string): Promise<string> {
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), CALL_TIMEOUT_MS);
  try {
    const res = await fetch(`${OLLAMA_URL}/api/generate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        model: OLLAMA_MODEL,
        prompt,
        stream: false,
        options: INFERENCE_OPTIONS,
      }),
      signal: ac.signal,
    });
    if (!res.ok) {
      const body = await res.text().catch(() => '');
      throw new OllamaError(
        `http-${res.status}`,
        `Ollama returned ${res.status}: ${body.slice(0, 200)}`,
      );
    }
    const data = (await res.json()) as { response?: string };
    return (data.response ?? '').trim();
  } catch (err) {
    if (err instanceof OllamaError) throw err;
    if (err instanceof Error && err.name === 'AbortError') {
      throw new OllamaError('timeout', `Ollama call exceeded ${CALL_TIMEOUT_MS}ms`);
    }
    throw new OllamaError('transport', `Ollama unreachable: ${String(err)}`);
  } finally {
    clearTimeout(timer);
  }
}

/**
 * /api/generate with retry/backoff. Returns first successful result
 * with elapsed timing + attempt count.
 */
export async function generate(prompt: string): Promise<GenerateResult> {
  const t0 = Date.now();
  let lastErr: unknown = undefined;
  for (let attempt = 1; attempt <= RETRY_DELAYS_MS.length + 1; attempt++) {
    try {
      const response = await generateOnce(prompt);
      return { response, elapsedMs: Date.now() - t0, attempts: attempt };
    } catch (err) {
      lastErr = err;
      if (attempt > RETRY_DELAYS_MS.length) break;
      const delay = RETRY_DELAYS_MS[attempt - 1] ?? 5_000;
      // eslint-disable-next-line no-console
      console.warn(
        `[ollama] attempt ${attempt} failed (${
          err instanceof OllamaError ? err.code : 'unknown'
        }), retrying in ${delay}ms…`,
      );
      await sleep(delay);
    }
  }
  throw lastErr instanceof Error
    ? lastErr
    : new OllamaError('unknown', 'all retries exhausted');
}

/** List loaded models (`/api/ps`). Used to verify warm-up. */
export async function listLoadedModels(): Promise<string[]> {
  const res = await fetch(`${OLLAMA_URL}/api/ps`);
  if (!res.ok) return [];
  const data = (await res.json()) as { models?: Array<{ name: string }> };
  return (data.models ?? []).map((m) => m.name);
}

/** List available models (`/api/tags`). */
export async function listAvailableModels(): Promise<string[]> {
  const res = await fetch(`${OLLAMA_URL}/api/tags`);
  if (!res.ok) return [];
  const data = (await res.json()) as { models?: Array<{ name: string }> };
  return (data.models ?? []).map((m) => m.name);
}

/**
 * Pre-flight: check model is available + warm up VRAM.
 * Throws if model not pulled, returns warm-up call timing on success.
 */
export async function preflight(): Promise<{
  warmupMs: number;
  loadedModels: string[];
}> {
  const available = await listAvailableModels().catch(() => {
    throw new OllamaError(
      'unreachable',
      `Ollama not reachable at ${OLLAMA_URL}. Is the bare-metal Ollama server running?`,
    );
  });

  const target = OLLAMA_MODEL.toLowerCase();
  const has = available.some((m) => m.toLowerCase() === target);
  if (!has) {
    throw new OllamaError(
      'model-missing',
      `Model "${OLLAMA_MODEL}" not pulled. Run: ollama pull ${OLLAMA_MODEL}\n` +
        `Available: ${available.join(', ')}`,
    );
  }

  // Warm up — first call loads model into VRAM (slow), subsequent are fast.
  const t0 = Date.now();
  await generateOnce('Reply with just the word: ready');
  const warmupMs = Date.now() - t0;

  const loadedModels = await listLoadedModels();
  return { warmupMs, loadedModels };
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
