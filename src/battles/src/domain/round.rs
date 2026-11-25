use std::time::{Duration, Instant};

use super::parameters::BattleParameters;

#[derive(Clone, Copy, PartialEq, Eq)]
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
    pub load_timeout: Duration,
    countdown: Duration,
    results: Duration,
}

impl Round {
    pub fn new(now: Instant, round_length: Duration, parameters: &BattleParameters) -> Self {
        let load_timeout = Duration::from_secs(parameters.load_timeout_seconds);
        Self {
            phase: RoundPhase::Loading,
            deadline: now + load_timeout,
            phase_length: load_timeout,
            load_timeout,
            countdown: Duration::from_secs(parameters.countdown_seconds),
            results: Duration::from_secs(parameters.results_seconds),
            round_length,
        }
    }

    pub fn advance(&mut self, now: Instant, all_ready: bool) -> bool {
        let (phase, length) = match self.phase {
            RoundPhase::Loading if all_ready || now >= self.deadline => {
                (RoundPhase::Countdown, self.countdown)
            }
            RoundPhase::Countdown if now >= self.deadline => {
                (RoundPhase::Battle, self.round_length)
            }
            RoundPhase::Battle if now >= self.deadline => (RoundPhase::Finished, self.results),
            _ => return false,
        };
        self.phase = phase;
        self.deadline = now + length;
        self.phase_length = length;
        true
    }

    pub fn finish(&mut self, now: Instant) {
        self.phase = RoundPhase::Finished;
        self.deadline = now + self.results;
        self.phase_length = self.results;
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
        let parameters = BattleParameters::default();
        let load_timeout = Duration::from_secs(parameters.load_timeout_seconds);
        for (ready, loaded) in [(true, now), (false, now + load_timeout)] {
            let mut round = Round::new(now, duration, &parameters);
            assert!(!round.advance(now, false));
            assert!(round.advance(loaded, ready));
            assert!(matches!(round.phase, RoundPhase::Countdown));
            assert_eq!(
                round.deadline,
                loaded + Duration::from_secs(parameters.countdown_seconds)
            );
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
