import { MutableRefObject } from 'react';

interface CameraViewProps {
  videoRef: MutableRefObject<HTMLVideoElement | null>;
  connectionStatus: string;
  motionState: string;
}

export function CameraView({ videoRef, connectionStatus, motionState }: CameraViewProps): JSX.Element {
  return (
    <section className="camera-view" aria-label="Camera viewfinder">
      <video ref={videoRef} className="camera-feed" autoPlay playsInline muted />
      <div className="camera-hud">
        <span className="chip">{connectionStatus}</span>
        <span className="chip">{motionState}</span>
      </div>
    </section>
  );
}
