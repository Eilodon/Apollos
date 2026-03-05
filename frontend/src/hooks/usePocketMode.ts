import { useEffect, useRef, useState } from 'react';

/**
 * Layer 0.5 - Pocket-Safe UI (Ghost Touch Prevention)
 *
 * Dùng AmbientLightSensor API để phát hiện điện thoại đang nằm trong túi
 * (ánh sáng < 5 lux). Khi đó, toàn bộ sự kiện touch sẽ bị chặn để tránh
 * Ghost Touch (cọ xát vải → vuốt mode, tắt mic).
 *
 * Fallback: Sử dụng proximity sensor nếu có (ít phổ biến hơn trên Chrome).
 */
export function usePocketMode(): boolean {
    const [inPocket, setInPocket] = useState(false);
    const inPocketRef = useRef(false);

    useEffect(() => {
        let sensorCleanup: (() => void) | null = null;

        if ('AmbientLightSensor' in window) {
            try {
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                const sensor = new (window as any).AmbientLightSensor({ frequency: 5 });

                const onReading = () => {
                    // < 5 lux = hoàn toàn tối (trong túi/ví)
                    const isPocket = (sensor.illuminance as number) < 5;
                    inPocketRef.current = isPocket;
                    setInPocket(isPocket);
                };

                const onError = () => {
                    // Sensor lỗi không tác động tới app, bỏ qua
                };

                sensor.addEventListener('reading', onReading);
                sensor.addEventListener('error', onError);
                sensor.start();

                sensorCleanup = () => {
                    sensor.removeEventListener('reading', onReading);
                    sensor.removeEventListener('error', onError);
                    try {
                        sensor.stop();
                    } catch {
                        // Ignore
                    }
                };
            } catch {
                // AmbientLightSensor bị từ chối quyền → silently fallback
            }
        }

        // Hard block: chặn tất cả touchstart khi inPocket = true
        const preventTouch = (e: TouchEvent) => {
            if (inPocketRef.current) {
                e.preventDefault();
                e.stopImmediatePropagation();
            }
        };

        document.addEventListener('touchstart', preventTouch, { passive: false });

        return () => {
            sensorCleanup?.();
            document.removeEventListener('touchstart', preventTouch);
        };
    }, []);

    return inPocket;
}
