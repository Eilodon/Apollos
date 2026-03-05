// frontend/src/workers/survivalReflex.worker.ts

// Chạy hoàn toàn độc lập với luồng UI và Gemini WebSocket
// Tủy sống (Layer 0) - Edge WebAssembly/WebGPU giả lập

let previousFrame: ImageData | null = null;

// Giả lập tính toán Optical Expansion siêu nhẹ
function computeOpticalExpansion(prev: ImageData, curr: ImageData): number {
    // Trong thực tế, tính toán sự phóng to đột ngột của các pixel trung tâm
    // Ở đây chúng ta giả lập ngẫu nhiên hoặc heuristic đơn giản nếu không có model thực.
    // Trả về Time To Collision (TTC) tính bằng giây
    // Chúng ta sẽ giả lập một trường hợp có thể trả về TTC < 1.5 ngẫu nhiên để test,
    // hoặc trả về 100 (an toàn) nếu không có sự thay đổi lớn.

    let diff = 0;
    // Lấy mẫu nhanh 1/100 lượng pixel để so sánh (Heuristic)
    const step = 400; // 4 bytes per pixel * 100
    for (let i = 0; i < curr.data.length; i += step) {
        if (prev) {
            const rDiff = Math.abs(curr.data[i] - prev.data[i]);
            diff += rDiff;
        }
    }

    // Giả sử nếu diff tổng vượt mức (Motion đột ngột phóng to) -> TTC thấp
    const avgDiff = diff / (curr.data.length / step);

    if (avgDiff > 50) { // Ngưỡng cực kỳ thô thiển báo hiệu biến động mạnh do vật thể áp sát
        return 1.2; // TTC = 1.2s -> Nguy hiểm (dưới 1.5s)
    }

    return 10.0; // An toàn
}

self.onmessage = (e: MessageEvent) => {
    const { currentFrame } = e.data;

    if (previousFrame) {
        const timeToCollision = computeOpticalExpansion(previousFrame, currentFrame);

        if (timeToCollision < 1.5) {
            // Gửi tín hiệu PANIC về luồng chính ngay lập tức
            self.postMessage({
                type: 'CRITICAL_EDGE_HAZARD',
                urgency: 'high',
                positionX: 0.0, // Giả định phía trước
                hazard_type: 'VA CHẠM (EDGE)', // Cảnh báo local Edge
                distance: 'very_close'
            });
        }
    }
    previousFrame = currentFrame;
};
