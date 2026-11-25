#[derive(Clone, Copy, Default)]
pub enum ArenaSeason {
    Winter,
    #[default]
    Summer,
    Desert,
}

/// Encoded vehicle appearances for the arena's season.
#[derive(Clone, Default)]
pub struct BattleOutfits {
    pub winter: Vec<u8>,
    pub summer: Vec<u8>,
    pub desert: Vec<u8>,
}

impl BattleOutfits {
    pub fn for_season(&self, season: ArenaSeason) -> &[u8] {
        match season {
            ArenaSeason::Winter => &self.winter,
            ArenaSeason::Summer => &self.summer,
            ArenaSeason::Desert => &self.desert,
        }
    }
}
