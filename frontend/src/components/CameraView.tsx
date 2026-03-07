import type { MutableRefObject } from 'react';

interface CameraViewProps {
  videoRef: MutableRefObject<HTMLVideoElement | null>;
  connectionStatus: string;
  motionState: string;
  previewVisible: boolean;
}

export function CameraView({ videoRef, connectionStatus, motionState, previewVisible }: CameraViewProps): JSX.Element {
  return (
    <section className="camera-view" aria-label="Camera viewfinder">
      <video
        ref={videoRef}
        className="camera-feed"
        autoPlay
        playsInline
        muted
        style={{ display: previewVisible ? 'block' : 'none' }}
        aria-hidden={!previewVisible}
      />
      {!previewVisible ? (
        <div className="camera-preview-paused" role="status" aria-live="polite">
          Camera preview hidden to prevent accidental touch and reduce screen power.
        </div>
      ) : null}
      <div className="camera-hud">
        <span className="chip">{connectionStatus}</span>
        <span className="chip">{motionState}</span>
      </div>
    </section>
  );
}
