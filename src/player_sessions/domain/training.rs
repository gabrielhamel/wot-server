#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceChatMode {
    General,
    Team,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TrainingRoomSettings {
    pub arena_type_id: i32,
    pub duration_seconds: i32,
    pub open: bool,
    pub comment: String,
    pub voice_chat: VoiceChatMode,
}

impl TrainingRoomSettings {
    pub fn accepts_duration(seconds: i32) -> bool {
        (300..=1800).contains(&seconds) && seconds % 60 == 0
    }

    pub fn accepts_comment(comment: &str) -> bool {
        comment.chars().take(401).count() <= 400
    }

    pub fn new(
        arena_type_id: i32,
        duration_seconds: i32,
        open: bool,
        comment: &str,
    ) -> Option<Self> {
        if !Self::accepts_duration(duration_seconds) || !Self::accepts_comment(comment) {
            return None;
        }
        Some(Self {
            arena_type_id,
            duration_seconds,
            open,
            comment: comment.to_owned(),
            voice_chat: VoiceChatMode::General,
        })
    }
}
