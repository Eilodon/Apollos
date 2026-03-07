pub fn requires_human_escalation(risk_score: f32, confidence: f32, repeated_hard_stops: usize, is_panic: bool) -> bool {
    // Cầu cứu khi: Rủi ro cực cao nhưng AI mù mờ (< 0.4) HOẶC kẹt cứng HOẶC hoảng loạn
    (risk_score > 6.0 && confidence < 0.4) || repeated_hard_stops >= 3 || is_panic
}
