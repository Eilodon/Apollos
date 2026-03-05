type ExpansionPattern = 'radial' | 'uniform' | 'directional' | 'none';

interface EdgeHazardPayload {
  type: 'CRITICAL_EDGE_HAZARD';
  urgency: 'high';
  positionX: number;
  hazard_type: string;
  subtype?: 'floor_drop';
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
const FLOOR_DROP_BOTTOM_THRESHOLD = 60;
const FLOOR_DROP_TOP_MAX = 20;
const FLICKER_SUPPRESS_LUMA_DELTA = 28;

const diffHistory: number[] = [];
let previousFrame: ImageData | null = null;
let dynamicInterval = 33;
let reflexLastProcessTime = performance.now();

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
): { centerDiff: number; avgDiff: number; pattern: ExpansionPattern; lateralBias: number } {
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
  const leftEnergy = q1 + q3;
  const rightEnergy = q2 + q4;
  const lateralBias = clamp((rightEnergy - leftEnergy) / Math.max(leftEnergy + rightEnergy, 1e-3), -1, 1);

  return {
    centerDiff,
    avgDiff,
    pattern,
    lateralBias,
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

function buildHazardPayload(centerDiff: number, avgDiff: number, pattern: ExpansionPattern, lateralBias: number): EdgeHazardPayload {
  return {
    type: 'CRITICAL_EDGE_HAZARD',
    urgency: 'high',
    positionX: lateralBias,
    hazard_type: 'EDGE_APPROACHING_OBJECT',
    distance: 'very_close',
    diagnostics: {
      centerDiff,
      avgDiff,
      pattern,
    },
  };
}

function averageLuma(gray: Float32Array): number {
  if (gray.length === 0) {
    return 0;
  }
  let sum = 0;
  for (let i = 0; i < gray.length; i += 1) {
    sum += gray[i] ?? 0;
  }
  return sum / gray.length;
}

function detectFloorDrop(prev: ImageData, curr: ImageData): EdgeHazardPayload | null {
  const width = curr.width;
  const height = curr.height;
  const prevGray = toGrayLuma(prev);
  const currGray = toGrayLuma(curr);

  const topDiff = absDiffAverage(prevGray, currGray, width, 0, 0, width, Math.floor(height * 0.25));
  const bottomDiff = absDiffAverage(
    prevGray,
    currGray,
    width,
    0,
    Math.floor(height * 0.75),
    width,
    height,
  );
  const lumaDelta = Math.abs(averageLuma(currGray) - averageLuma(prevGray));

  // Regression guard: neon sign flicker often changes whole-frame luminance.
  if (lumaDelta > FLICKER_SUPPRESS_LUMA_DELTA && topDiff > 18) {
    return null;
  }
  const isDrop = (
    bottomDiff >= FLOOR_DROP_BOTTOM_THRESHOLD
    && topDiff <= FLOOR_DROP_TOP_MAX
    && bottomDiff > topDiff * 2.4
  );
  if (!isDrop) {
    return null;
  }

  return {
    type: 'CRITICAL_EDGE_HAZARD',
    urgency: 'high',
    positionX: 0,
    hazard_type: 'EDGE_DROP_HAZARD',
    subtype: 'floor_drop',
    distance: 'very_close',
    diagnostics: {
      centerDiff: bottomDiff,
      avgDiff: (topDiff + bottomDiff) / 2,
      pattern: 'directional',
    },
  };
}

self.onmessage = (event: MessageEvent<{ currentFrame?: ImageData; riskScore?: number }>) => {
  const currentFrame = event.data?.currentFrame;
  const riskScore = Number.isFinite(event.data?.riskScore) ? Number(event.data.riskScore) : 1;
  if (!currentFrame) {
    return;
  }

  // Thermal-kinematic governor:
  // risk=1 => ~150ms (6fps), risk=4 => ~16ms (60fps)
  dynamicInterval = Math.max(16, 150 - riskScore * 33);
  const now = performance.now();
  if (now - reflexLastProcessTime < dynamicInterval) {
    return;
  }
  reflexLastProcessTime = now;

  if (!previousFrame) {
    previousFrame = currentFrame;
    return;
  }

  const { centerDiff, avgDiff, pattern, lateralBias } = computeOpticalExpansion(previousFrame, currentFrame);
  const floorDrop = detectFloorDrop(previousFrame, currentFrame);
  if (floorDrop) {
    self.postMessage(floorDrop);
    previousFrame = currentFrame;
    return;
  }
  pushDiff(avgDiff);

  const immediateThreat = avgDiff > BASELINE_THRESHOLD && pattern === 'radial';
  const sustainedThreat = isSustainedThreat(pattern);

  if (immediateThreat || sustainedThreat) {
    self.postMessage(buildHazardPayload(centerDiff, avgDiff, pattern, lateralBias));
  }

  previousFrame = currentFrame;
};
