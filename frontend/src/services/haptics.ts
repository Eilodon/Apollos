export function vibrate(pattern: number | number[]): void {
  if ('vibrate' in navigator) {
    navigator.vibrate(pattern);
  }
}

export function vibrateHardStop(): void {
  vibrate([120, 80, 120, 80, 120]);
}

export function vibrateReconnect(): void {
  vibrate([60, 40, 60]);
}

export function vibrateSoftConfirm(): void {
  vibrate(30);
}
