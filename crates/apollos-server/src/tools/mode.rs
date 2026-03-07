use apollos_proto::contracts::NavigationMode;

pub fn next_mode(current: NavigationMode) -> NavigationMode {
    match current {
        NavigationMode::Navigation => NavigationMode::Explore,
        NavigationMode::Explore => NavigationMode::Read,
        NavigationMode::Read => NavigationMode::Navigation,
        NavigationMode::Quiet => NavigationMode::Navigation,
    }
}
