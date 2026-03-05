/// <reference lib="webworker" />
export {};

declare class BarcodeDetector {
  constructor(options?: { formats?: string[] });
  detect(source: ImageBitmapSource): Promise<Array<{ rawValue?: string; format?: string }>>;
  static getSupportedFormats?: () => Promise<string[]>;
}

interface ScanFrameMessage {
  type: 'scan_frame';
  currentFrame?: ImageData;
}

type InboundMessage = ScanFrameMessage;

const MIN_SCAN_INTERVAL_MS = 320;
const MIN_EMIT_INTERVAL_MS = 2500;

let detector: BarcodeDetector | null = null;
let detectorChecked = false;
let lastScanAt = 0;
let lastEmitAt = 0;
let candidateValue = '';
let candidateStableCount = 0;

function postStatus(state: 'ready' | 'fallback' | 'disabled', detail: string): void {
  self.postMessage({
    type: 'DETERMINISTIC_SCAN_STATUS',
    state,
    detail,
  });
}

async function ensureDetector(): Promise<void> {
  if (detectorChecked) {
    return;
  }
  detectorChecked = true;
  if (typeof BarcodeDetector === 'undefined') {
    postStatus('fallback', 'BarcodeDetector is unavailable in this browser.');
    return;
  }
  try {
    const preferredFormats = [
      'qr_code',
      'code_128',
      'ean_13',
      'ean_8',
      'upc_a',
      'upc_e',
      'itf',
      'codabar',
    ];
    detector = new BarcodeDetector({ formats: preferredFormats as unknown as string[] });
    postStatus('ready', 'Deterministic barcode scanner ready.');
  } catch (error) {
    detector = null;
    postStatus('fallback', `Barcode detector init failed: ${String(error)}`);
  }
}

async function processScanFrame(frame: ImageData): Promise<void> {
  await ensureDetector();
  if (!detector) {
    return;
  }

  const now = performance.now();
  if (now - lastScanAt < MIN_SCAN_INTERVAL_MS) {
    return;
  }
  lastScanAt = now;

  let bitmap: ImageBitmap | null = null;
  try {
    bitmap = await createImageBitmap(frame);
    const results = await detector.detect(bitmap);
    if (!Array.isArray(results) || results.length === 0) {
      candidateValue = '';
      candidateStableCount = 0;
      return;
    }
    const first = results[0] || {};
    const rawValue = String(first.rawValue || '').trim();
    const format = String(first.format || '').trim().toUpperCase();
    if (!rawValue) {
      candidateValue = '';
      candidateStableCount = 0;
      return;
    }

    if (candidateValue === rawValue) {
      candidateStableCount += 1;
    } else {
      candidateValue = rawValue;
      candidateStableCount = 1;
    }

    if (candidateStableCount < 2) {
      return;
    }
    if (now - lastEmitAt < MIN_EMIT_INTERVAL_MS) {
      return;
    }
    lastEmitAt = now;

    self.postMessage({
      type: 'DETERMINISTIC_SCAN_RESULT',
      task: 'barcode',
      value: rawValue,
      format,
      confidence: Math.min(0.99, 0.72 + candidateStableCount * 0.09),
      timestamp: new Date().toISOString(),
    });
  } catch (error) {
    postStatus('fallback', `Barcode scan failed: ${String(error)}`);
  } finally {
    bitmap?.close();
  }
}

self.onmessage = (event: MessageEvent<InboundMessage>) => {
  const payload = event.data;
  if (payload.type !== 'scan_frame' || !payload.currentFrame) {
    return;
  }
  void processScanFrame(payload.currentFrame);
};
