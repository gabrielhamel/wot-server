pub(crate) struct TrainingRoomSettings {
    pub arena_type_id: i32,
    pub duration_seconds: i32,
    pub open: bool,
    pub comment: String,
}

impl TrainingRoomSettings {
    pub fn new(
        arena_type_id: i32,
        duration_seconds: i32,
        open: bool,
        comment: &str,
    ) -> Option<Self> {
        if !(300..=1800).contains(&duration_seconds)
            || duration_seconds % 60 != 0
            || comment.chars().take(401).count() > 400
        {
            return None;
        }
        Some(Self {
            arena_type_id,
            duration_seconds,
            open,
            comment: comment.to_owned(),
        })
    }
}
