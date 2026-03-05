#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmotionState {
    Calm,
    Stressed,
    Panic,
}

pub fn classify_emotion(score: f32) -> EmotionState {
    if score >= 0.8 {
        EmotionState::Panic
    } else if score >= 0.5 {
        EmotionState::Stressed
    } else {
        EmotionState::Calm
    }
}
