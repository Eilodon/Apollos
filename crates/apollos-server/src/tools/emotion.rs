#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmotionState {
    Calm,
    Stressed,
    Panic,
}

// TODO: KRONOS-CRITICAL: Áp dụng Schmitt Trigger (Hysteresis).
// Một khi đã lên `Panic` (>=0.8), điểm số phải giảm xuống dưới `0.6` thì mới được hạ cấp về `Stressed`
// để tránh hiện tượng "nhấp nháy trạng thái" (Emotion Flapping).
pub fn classify_emotion(score: f32) -> EmotionState {
    if score >= 0.8 {
        EmotionState::Panic
    } else if score >= 0.5 {
        EmotionState::Stressed
    } else {
        EmotionState::Calm
    }
}
