import type { BrowserTrackInfo } from "../types/job";

/** Must match package.json @xenova/transformers (used for CDN wasm / ort paths). */
const TRANSFORMERS_NPM_VERSION = "2.17.2";

const CATALOG_TO_XENOVA: Record<string, string> = {
  tiny: "Xenova/whisper-tiny",
  base: "Xenova/whisper-base",
  small: "Xenova/whisper-small",
  medium: "Xenova/whisper-medium",
  "large-v3-turbo": "Xenova/whisper-large-v3-turbo",
};

function mapCatalogToXenova(catalogId: string | undefined | null): string {
  const k = (catalogId ?? "base").trim().toLowerCase();
  return CATALOG_TO_XENOVA[k] ?? "Xenova/whisper-tiny";
}

function extractAsrText(result: unknown): string {
  if (result == null) return "";
  if (typeof result === "string") return result;
  if (typeof result === "object" && "text" in result) {
    const t = (result as { text: unknown }).text;
    return typeof t === "string" ? t : "";
  }
  return String(result);
}

function configureTransformersForTauriWebview(
  env: Awaited<typeof import("@xenova/transformers")>["env"],
): void {
  // Vite может подмешать полифиллы fs/path → transformers.js считает среду Node и
  // выставляет локальный wasmPaths (неверный в webview). Явно грузим ort-wasm с CDN.
  env.useFS = false;
  env.useFSCache = false;
  env.allowLocalModels = false;
  env.allowRemoteModels = true;

  const wasm = env.backends?.onnx?.wasm;
  if (wasm) {
    wasm.wasmPaths = `https://cdn.jsdelivr.net/npm/@xenova/transformers@${TRANSFORMERS_NPM_VERSION}/dist/`;
    // WebView2: проще без SIMD / потоков (меньше сюрпризов при первом запуске).
    if ("simd" in wasm) {
      (wasm as { simd: boolean }).simd = false;
    }
    if ("numThreads" in wasm) {
      (wasm as { numThreads: number }).numThreads = 1;
    }
  }
}

export async function transcribeBrowserTracks(args: {
  whisperModelId: string;
  tracks: BrowserTrackInfo[];
  language: string | null;
  shouldStop: () => boolean;
  onProgress?: (message: string) => void;
}): Promise<string[]> {
  let mod: typeof import("@xenova/transformers");
  try {
    mod = await import("@xenova/transformers");
  } catch (e) {
    const inner = e instanceof Error ? e.message : String(e);
    throw new Error(`Transformers.js не загрузился: ${inner}`);
  }

  const { pipeline, env } = mod;
  configureTransformersForTauriWebview(env);

  const modelId = mapCatalogToXenova(args.whisperModelId);
  args.onProgress?.(`Loading in-app model ${modelId}…`);

  let transcriber: Awaited<ReturnType<typeof pipeline>>;
  try {
    transcriber = await pipeline("automatic-speech-recognition", modelId);
  } catch (e) {
    const inner = e instanceof Error ? e.message : String(e);
    throw new Error(`Не удалось открыть модель ${modelId}: ${inner}`);
  }

  const { convertFileSrc } = await import("@tauri-apps/api/core");
  const texts: string[] = [];

  for (let i = 0; i < args.tracks.length; i++) {
    if (args.shouldStop()) {
      throw new Error("Job cancelled");
    }
    const t = args.tracks[i]!;
    if (t.skipTranscribe) {
      texts.push("");
      continue;
    }
    args.onProgress?.(`Transcribing track ${i + 1}/${args.tracks.length} (WASM)…`);
    const url = convertFileSrc(t.wavPath);
    try {
      const raw = await transcriber(url, {
        chunk_length_s: 30,
        language: args.language ?? undefined,
        task: "transcribe",
      });
      texts.push(extractAsrText(raw).trim());
    } catch (e) {
      const inner = e instanceof Error ? e.message : String(e);
      throw new Error(
        `Транскрипция трека ${i + 1} не удалась (${t.wavPath}): ${inner}`,
      );
    }
  }

  return texts;
}
