type ExpansionPattern = 'radial' | 'uniform' | 'directional' | 'none';

interface EdgeHazardPayload {
  type: 'CRITICAL_EDGE_HAZARD';
  urgency: 'high';
  positionX: number;
  hazard_type: string;
  distance: 'very_close';
  diagnostics: {
    centerDiff: number;
    avgDiff: number;
    pattern: ExpansionPattern;
  };
}

const RING_SIZE = 3;
const BASELINE_THRESHOLD = 50;
const SUSTAINED_THRESHOLD = 40;

const diffHistory: number[] = [];
let previousFrame: ImageData | null = null;

function toGrayLuma(frame: ImageData): Float32Array {
  const gray = new Float32Array(frame.width * frame.height);
  let g = 0;
  for (let i = 0; i < frame.data.length; i += 4) {
    const r = frame.data[i] ?? 0;
    const gg = frame.data[i + 1] ?? 0;
    const b = frame.data[i + 2] ?? 0;
    gray[g] = 0.299 * r + 0.587 * gg + 0.114 * b;
    g += 1;
  }
  return gray;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function absDiffAverage(
  prevGray: Float32Array,
  currGray: Float32Array,
  width: number,
  xStart: number,
  yStart: number,
  xEnd: number,
  yEnd: number,
): number {
  let total = 0;
  let count = 0;

  for (let y = yStart; y < yEnd; y += 1) {
    const row = y * width;
    for (let x = xStart; x < xEnd; x += 1) {
      const index = row + x;
      total += Math.abs(currGray[index] - prevGray[index]);
      count += 1;
    }
  }

  return count > 0 ? total / count : 0;
}

function classifyPattern(quadrants: number[]): ExpansionPattern {
  const minQ = Math.min(...quadrants);
  const maxQ = Math.max(...quadrants);
  const spread = maxQ - minQ;
  const avg = quadrants.reduce((acc, value) => acc + value, 0) / quadrants.length;

  if (avg < 1) {
    return 'none';
  }

  const activeCount = quadrants.filter((value) => value > SUSTAINED_THRESHOLD).length;
  if (activeCount >= 3 && spread <= 14) {
    return 'radial';
  }

  if (spread <= 8 && avg > 20) {
    return 'uniform';
  }

  if (activeCount >= 2 && spread > 14) {
    return 'directional';
  }

  return 'none';
}

function computeOpticalExpansion(
  prev: ImageData,
  curr: ImageData,
): { centerDiff: number; avgDiff: number; pattern: ExpansionPattern } {
  const width = curr.width;
  const height = curr.height;
  const prevGray = toGrayLuma(prev);
  const currGray = toGrayLuma(curr);

  const halfW = Math.floor(width / 2);
  const halfH = Math.floor(height / 2);

  const q1 = absDiffAverage(prevGray, currGray, width, 0, 0, halfW, halfH);
  const q2 = absDiffAverage(prevGray, currGray, width, halfW, 0, width, halfH);
  const q3 = absDiffAverage(prevGray, currGray, width, 0, halfH, halfW, height);
  const q4 = absDiffAverage(prevGray, currGray, width, halfW, halfH, width, height);
  const quadrants = [q1, q2, q3, q4];

  const centerStartX = Math.floor(width * 0.25);
  const centerEndX = Math.floor(width * 0.75);
  const centerStartY = Math.floor(height * 0.25);
  const centerEndY = Math.floor(height * 0.75);
  const centerDiff = absDiffAverage(prevGray, currGray, width, centerStartX, centerStartY, centerEndX, centerEndY);

  const avgDiff = (q1 + q2 + q3 + q4) / 4;
  const pattern = classifyPattern(quadrants);

  return {
    centerDiff,
    avgDiff,
    pattern,
  };
}

function pushDiff(diff: number): void {
  diffHistory.push(diff);
  if (diffHistory.length > RING_SIZE) {
    diffHistory.shift();
  }
}

function isSustainedThreat(pattern: ExpansionPattern): boolean {
  if (diffHistory.length < RING_SIZE) {
    return false;
  }

  const sustained = diffHistory.every((value) => value > SUSTAINED_THRESHOLD);
  return sustained && pattern === 'radial';
}

function buildHazardPayload(centerDiff: number, avgDiff: number, pattern: ExpansionPattern): EdgeHazardPayload {
  const normalized = clamp((centerDiff - SUSTAINED_THRESHOLD) / 35, -1, 1);
  return {
    type: 'CRITICAL_EDGE_HAZARD',
    urgency: 'high',
    positionX: normalized,
    hazard_type: 'EDGE_APPROACHING_OBJECT',
    distance: 'very_close',
    diagnostics: {
      centerDiff,
      avgDiff,
      pattern,
    },
  };
}

self.onmessage = (event: MessageEvent<{ currentFrame?: ImageData }>) => {
  const currentFrame = event.data?.currentFrame;
  if (!currentFrame) {
    return;
  }

  if (!previousFrame) {
    previousFrame = currentFrame;
    return;
  }

  const { centerDiff, avgDiff, pattern } = computeOpticalExpansion(previousFrame, currentFrame);
  pushDiff(avgDiff);

  const immediateThreat = avgDiff > BASELINE_THRESHOLD && pattern === 'radial';
  const sustainedThreat = isSustainedThreat(pattern);

  if (immediateThreat || sustainedThreat) {
    self.postMessage(buildHazardPayload(centerDiff, avgDiff, pattern));
  }

  previousFrame = currentFrame;
};
