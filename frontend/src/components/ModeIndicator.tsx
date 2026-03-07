import type { NavigationMode } from '../types/contracts';

interface ModeIndicatorProps {
  mode: NavigationMode;
}

export function ModeIndicator({ mode }: ModeIndicatorProps): JSX.Element {
  return (
    <div className="mode-indicator" role="status" aria-live="polite">
      <span className="mode-label">Mode</span>
      <strong>{mode}</strong>
    </div>
  );
}
