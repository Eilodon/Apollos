interface HazardCompassProps {
  positionX: number;
  visible: boolean;
  distance: 'very_close' | 'mid' | 'far';
}

function directionLabel(positionX: number): string {
  if (positionX <= -0.7) {
    return '10 o\'clock, far left';
  }
  if (positionX <= -0.25) {
    return '9 o\'clock, left';
  }
  if (positionX < 0.25) {
    return '12 o\'clock, ahead';
  }
  if (positionX < 0.7) {
    return '3 o\'clock, right';
  }
  return '2 o\'clock, far right';
}

function distanceLabel(distance: HazardCompassProps['distance']): string {
  if (distance === 'very_close') {
    return 'very close';
  }
  if (distance === 'far') {
    return 'far';
  }
  return 'mid range';
}

export function HazardCompass({ positionX, visible, distance }: HazardCompassProps): JSX.Element {
  const clamped = Math.max(-1, Math.min(1, positionX));
  const percentage = ((clamped + 1) / 2) * 100;
  const valueNow = Math.round(percentage);
  const valuetext = visible
    ? `Obstacle ${distanceLabel(distance)} at ${directionLabel(clamped)}`
    : 'No active obstacle in compass view';

  return (
    <section className="hazard-compass" aria-label="Hazard compass">
      <div className="hazard-compass-header">
        <span className="mode-label">Hazard Direction</span>
        <span className="hazard-compass-readout" aria-hidden={!visible}>
          {visible ? `${distanceLabel(distance)} · ${directionLabel(clamped)}` : 'Clear'}
        </span>
      </div>
      <div className="hazard-arc" />
      <div
        className="hazard-meter"
        role="meter"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={valueNow}
        aria-valuetext={valuetext}
      >
        {visible ? <div className="hazard-dot" style={{ left: `${percentage}%` }} /> : null}
      </div>
      <span className="sr-only" aria-live="assertive" aria-atomic="true">
        {visible ? `Warning: ${valuetext}` : ''}
      </span>
    </section>
  );
}
