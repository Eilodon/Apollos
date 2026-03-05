/// <reference lib="webworker" />

declare const tf: any;
declare const tflite: any;

interface DepthInitMessage {
  type: 'init_depth_model';
  modelUrl?: string;
}

interface DepthFrameMessage {
  type: 'depth_frame';
  currentFrame?: ImageData;
  riskScore?: number;
}

type InboundMessage = DepthInitMessage | DepthFrameMessage;

interface DropAheadPayload {
  type: 'DROP_AHEAD_HAZARD';
  hazard_type: 'DROP_AHEAD';
  distance: 'very_close';
  positionX: number;
  confidence: number;
  source: 'tflite' | 'heuristic';
}

const MODEL_CACHE_DB = 'apollos-depth-cache-v1';
const MODEL_CACHE_STORE = 'models';
const MODEL_CACHE_KEY = 'depth-anything-v2-small-fp16';
const DEPTH_INPUT_SIZE = 256;
const DEPTH_OUTPUT_SIZE = 64;
const BASE_INTERVAL_MS = 100;

let depthModel: { predict: (input: unknown) => unknown } | null = null;
let modelLoading = false;
let modelReady = false;
let modelAvailable = false;
let depthLastProcessTime = performance.now();
let runtimeLoaded = false;

function ensureTfRuntimeLoaded(): void {
  if (runtimeLoaded) {
    return;
  }
  self.importScripts(
    '/tflite-wasm/tf-core.min.js',
    '/tflite-wasm/tf-backend-cpu.min.js',
    '/tflite-wasm/tf-tflite.min.js',
  );
  runtimeLoaded = true;
}

function postStatus(state: 'loading' | 'ready' | 'fallback' | 'error', detail: string): void {
  self.postMessage({
    type: 'DEPTH_STATUS',
    state,
    detail,
  });
}

function openModelDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(MODEL_CACHE_DB, 1);
    request.onerror = () => reject(request.error ?? new Error('Failed to open depth model cache DB'));
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(MODEL_CACHE_STORE)) {
        db.createObjectStore(MODEL_CACHE_STORE);
      }
    };
    request.onsuccess = () => resolve(request.result);
  });
}

async function readCachedModel(): Promise<ArrayBuffer | null> {
  const db = await openModelDb();
  return await new Promise((resolve, reject) => {
    const tx = db.transaction(MODEL_CACHE_STORE, 'readonly');
    const store = tx.objectStore(MODEL_CACHE_STORE);
    const request = store.get(MODEL_CACHE_KEY);
    request.onerror = () => reject(request.error ?? new Error('Failed to read cached depth model'));
    request.onsuccess = () => {
      const value = request.result;
      resolve(value instanceof ArrayBuffer ? value : null);
    };
    tx.oncomplete = () => db.close();
  });
}

async function writeCachedModel(bytes: ArrayBuffer): Promise<void> {
  const db = await openModelDb();
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(MODEL_CACHE_STORE, 'readwrite');
    const store = tx.objectStore(MODEL_CACHE_STORE);
    const request = store.put(bytes, MODEL_CACHE_KEY);
    request.onerror = () => reject(request.error ?? new Error('Failed to cache depth model'));
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error ?? new Error('Failed to cache depth model'));
  });
  db.close();
}

async function loadModelBytes(modelUrl: string): Promise<ArrayBuffer> {
  const cached = await readCachedModel();
  if (cached) {
    return cached;
  }

  const response = await fetch(modelUrl, { cache: 'force-cache' });
  if (!response.ok) {
    throw new Error(`Depth model fetch failed: ${response.status}`);
  }
  const bytes = await response.arrayBuffer();
  await writeCachedModel(bytes);
  return bytes;
}

async function ensureDepthModel(modelUrl: string): Promise<void> {
  if (modelReady || modelLoading) {
    return;
  }

  modelLoading = true;
  postStatus('loading', 'Loading depth model...');

  try {
    ensureTfRuntimeLoaded();
    const modelBytes = await loadModelBytes(modelUrl);
    const blobUrl = URL.createObjectURL(new Blob([modelBytes], { type: 'application/octet-stream' }));
    tflite.setWasmPath('/tflite-wasm/');
    await tf.setBackend('cpu');
    await tf.ready();
    depthModel = await tflite.loadTFLiteModel(blobUrl, {
      numThreads: Math.max(1, Math.floor((self.navigator.hardwareConcurrency || 2) / 2)),
    });
    URL.revokeObjectURL(blobUrl);
    modelReady = true;
    modelAvailable = true;
    postStatus('ready', 'Depth model ready.');
  } catch (error) {
    modelReady = false;
    modelAvailable = false;
    postStatus('fallback', `Depth model unavailable, fallback active: ${String(error)}`);
  } finally {
    modelLoading = false;
  }
}

function normalize(values: Float32Array): Float32Array {
  let min = Number.POSITIVE_INFINITY;
  let max = Number.NEGATIVE_INFINITY;
  for (let i = 0; i < values.length; i += 1) {
    const value = values[i] ?? 0;
    if (value < min) {
      min = value;
    }
    if (value > max) {
      max = value;
    }
  }
  const span = Math.max(max - min, 1e-6);
  const normalized = new Float32Array(values.length);
  for (let i = 0; i < values.length; i += 1) {
    normalized[i] = (values[i] - min) / span;
  }
  return normalized;
}

function toLumaGrid(frame: ImageData): Float32Array {
  const luma = new Float32Array(frame.width * frame.height);
  let p = 0;
  for (let i = 0; i < frame.data.length; i += 4) {
    const r = frame.data[i] ?? 0;
    const g = frame.data[i + 1] ?? 0;
    const b = frame.data[i + 2] ?? 0;
    luma[p] = 0.299 * r + 0.587 * g + 0.114 * b;
    p += 1;
  }
  return luma;
}

function detectDropAhead(depth: Float32Array, width: number, height: number): DropAheadPayload | null {
  const xStart = Math.floor(width * 0.15);
  const xEnd = Math.floor(width * 0.85);
  const yStart = Math.floor(height * 0.52);
  const yEnd = height - 1;

  let discontinuityCount = 0;
  let confidenceSum = 0;
  let weightedX = 0;
  let weightedWeight = 0;

  for (let y = yStart + 1; y < yEnd; y += 1) {
    const row = y * width;
    const prevRow = (y - 1) * width;
    for (let x = xStart; x < xEnd; x += 1) {
      const current = depth[row + x] ?? 0;
      const prev = depth[prevRow + x] ?? 0;
      const delta = current - prev;

      // Positive jump means farther depth in lower rows -> likely drop.
      if (delta > 0.22) {
        discontinuityCount += 1;
        confidenceSum += delta;
        weightedX += x * delta;
        weightedWeight += delta;
      }
    }
  }

  const sampleCount = Math.max((xEnd - xStart) * (yEnd - yStart), 1);
  const discontinuityRatio = discontinuityCount / sampleCount;
  const confidence = Math.min(1, discontinuityRatio * 2.2 + confidenceSum / sampleCount);

  if (confidence < 0.58 || discontinuityRatio < 0.12) {
    return null;
  }

  const avgX = weightedWeight > 0 ? weightedX / weightedWeight : width / 2;
  const positionX = Math.max(-1, Math.min(1, (avgX / width) * 2 - 1));

  return {
    type: 'DROP_AHEAD_HAZARD',
    hazard_type: 'DROP_AHEAD',
    distance: 'very_close',
    positionX,
    confidence,
    source: modelAvailable ? 'tflite' : 'heuristic',
  };
}

async function inferDepthMap(frame: ImageData): Promise<Float32Array | null> {
  if (!depthModel) {
    return null;
  }

  const channels = 3;
  const rgb = new Float32Array(frame.width * frame.height * channels);
  for (let i = 0, p = 0; i < frame.data.length; i += 4, p += 3) {
    rgb[p] = (frame.data[i] ?? 0) / 255;
    rgb[p + 1] = (frame.data[i + 1] ?? 0) / 255;
    rgb[p + 2] = (frame.data[i + 2] ?? 0) / 255;
  }

  const input = tf.tensor3d(rgb, [frame.height, frame.width, channels], 'float32');
  const resized = tf.image.resizeBilinear(input, [DEPTH_INPUT_SIZE, DEPTH_INPUT_SIZE]);
  const batched = tf.expandDims(resized, 0);

  const output = depthModel.predict(batched) as any;
  const outputTensor = Array.isArray(output) ? output[0] : output;
  const squeezed = tf.squeeze(outputTensor);
  const needsExpand = squeezed.rank === 2;
  const output3d = needsExpand ? tf.expandDims(squeezed, -1) : squeezed;
  const resizedDepth = tf.image.resizeBilinear(output3d, [DEPTH_OUTPUT_SIZE, DEPTH_OUTPUT_SIZE]);
  const mapped = tf.squeeze(resizedDepth);
  const data = await mapped.data();

  mapped.dispose();
  resizedDepth.dispose();
  if (needsExpand) {
    output3d.dispose();
  }
  squeezed.dispose();
  outputTensor.dispose();
  if (Array.isArray(output)) {
    output.forEach((tensor: any, index: number) => {
      if (index > 0) {
        tensor.dispose();
      }
    });
  }
  batched.dispose();
  resized.dispose();
  input.dispose();

  return normalize(Float32Array.from(data));
}

function inferHeuristicDepth(frame: ImageData): Float32Array {
  const luma = toLumaGrid(frame);
  const depth = new Float32Array(luma.length);
  for (let i = 0; i < luma.length; i += 1) {
    depth[i] = 1 - luma[i] / 255;
  }
  return depth;
}

async function processDepthFrame(frame: ImageData, riskScore: number): Promise<void> {
  const dynamicInterval = Math.max(50, BASE_INTERVAL_MS - (riskScore - 1) * 15);
  const now = performance.now();
  if (now - depthLastProcessTime < dynamicInterval) {
    return;
  }
  depthLastProcessTime = now;

  let depthMap = await inferDepthMap(frame);
  if (!depthMap) {
    depthMap = inferHeuristicDepth(frame);
  }

  const hazard = detectDropAhead(depthMap, frame.width, frame.height);
  if (hazard) {
    self.postMessage(hazard);
  }
}

self.onmessage = (event: MessageEvent<InboundMessage>) => {
  const payload = event.data;

  if (payload.type === 'init_depth_model') {
    const modelUrl = payload.modelUrl || '/models/depth_anything_v2_small_fp16.tflite';
    void ensureDepthModel(modelUrl);
    return;
  }

  if (payload.type === 'depth_frame' && payload.currentFrame) {
    const riskScore = Number.isFinite(payload.riskScore) ? Number(payload.riskScore) : 1;
    void processDepthFrame(payload.currentFrame, riskScore);
  }
};
