#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    pub lat: f64,
    pub lng: f64,
    pub accuracy_m: f32,
}

pub fn location_quality(location: Location) -> &'static str {
    if location.accuracy_m <= 10.0 {
        "high"
    } else if location.accuracy_m <= 30.0 {
        "medium"
    } else {
        "low"
    }
}
