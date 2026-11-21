use std::time::{Duration, Instant};

pub(crate) const LOAD_TIMEOUT: Duration = Duration::from_secs(60);
const COUNTDOWN: Duration = Duration::from_secs(10);
const RESULTS: Duration = Duration::from_secs(7);

#[derive(Clone, Copy)]
pub(crate) enum RoundPhase {
    Loading,
    Countdown,
    Battle,
    Finished,
}

pub(crate) struct Round {
    pub phase: RoundPhase,
    pub deadline: Instant,
    pub phase_length: Duration,
    round_length: Duration,
}

impl Round {
    pub fn new(now: Instant, round_length: Duration) -> Self {
        Self {
            phase: RoundPhase::Loading,
            deadline: now + LOAD_TIMEOUT,
            phase_length: LOAD_TIMEOUT,
            round_length,
        }
    }

    pub fn advance(&mut self, now: Instant, all_ready: bool) -> bool {
        let (phase, length) = match self.phase {
            RoundPhase::Loading if all_ready || now >= self.deadline => {
                (RoundPhase::Countdown, COUNTDOWN)
            }
            RoundPhase::Countdown if now >= self.deadline => {
                (RoundPhase::Battle, self.round_length)
            }
            RoundPhase::Battle if now >= self.deadline => (RoundPhase::Finished, RESULTS),
            _ => return false,
        };
        self.phase = phase;
        self.deadline = now + length;
        self.phase_length = length;
        true
    }

    pub fn finish(&mut self, now: Instant) {
        self.phase = RoundPhase::Finished;
        self.deadline = now + RESULTS;
        self.phase_length = RESULTS;
    }

    pub fn ended(&self, now: Instant) -> bool {
        matches!(self.phase, RoundPhase::Finished) && now >= self.deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_starts_on_readiness_or_timeout_and_ends_after_selected_duration() {
        let now = Instant::now();
        let duration = Duration::from_secs(300);
        for (ready, loaded) in [(true, now), (false, now + LOAD_TIMEOUT)] {
            let mut round = Round::new(now, duration);
            assert!(!round.advance(now, false));
            assert!(round.advance(loaded, ready));
            assert!(matches!(round.phase, RoundPhase::Countdown));
            assert_eq!(round.deadline, loaded + COUNTDOWN);
            let start = round.deadline;
            assert!(round.advance(start, false));
            assert!(matches!(round.phase, RoundPhase::Battle));
            assert_eq!(round.deadline, start + duration);
            assert!(!round.advance(round.deadline - Duration::from_secs(1), true));
            assert!(round.advance(round.deadline, false));
            assert!(matches!(round.phase, RoundPhase::Finished));
            assert!(!round.ended(round.deadline - Duration::from_secs(1)));
            assert!(round.ended(round.deadline));
        }
    }
}
