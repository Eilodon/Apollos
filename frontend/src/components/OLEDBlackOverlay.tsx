interface OLEDBlackOverlayProps {
  enabled: boolean;
}

export function OLEDBlackOverlay({ enabled }: OLEDBlackOverlayProps): JSX.Element | null {
  if (!enabled) {
    return null;
  }

  return <div className="oled-black-overlay" aria-hidden="true" />;
}
