interface HazardCompassProps {
  positionX: number;
  visible: boolean;
}

export function HazardCompass({ positionX, visible }: HazardCompassProps): JSX.Element {
  const clamped = Math.max(-1, Math.min(1, positionX));
  const percentage = ((clamped + 1) / 2) * 100;

  return (
    <div className="hazard-compass" aria-hidden={!visible}>
      <div className="hazard-arc" />
      {visible ? <div className="hazard-dot" style={{ left: `${percentage}%` }} /> : null}
    </div>
  );
}
