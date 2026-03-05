pub fn requires_human_escalation(confidence: f32, repeated_hard_stops: usize) -> bool {
    confidence >= 0.85 || repeated_hard_stops >= 2
}
