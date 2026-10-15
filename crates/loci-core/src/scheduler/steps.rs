//! Anki learning/relearning steps (rslib/src/scheduler/states/steps.rs, fsrs-spec §8.2).
//! Steps are configured in minutes (fractional allowed)
//! and used as whole seconds. `remaining` is the card's `left % 1000` (steps left until
//! graduation).

pub const DAY_SECS: i64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearningSteps {
    /// Step delays, seconds (floor(minutes·60)).
    pub secs: Vec<i64>,
}

impl LearningSteps {
    /// Builds the step list from minutes; non-finite and non-positive entries are dropped.
    pub fn new(minutes: &[f64]) -> Self {
        LearningSteps { secs: minutes.iter().filter(|m| m.is_finite() && **m > 0.0).map(|m| (m * 60.0).floor() as i64).collect() }
    }
    pub fn len(&self) -> usize {
        self.secs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.secs.is_empty()
    }

    /// Current step index for `remaining` steps left: clamp(len − remaining, 0, len − 1).
    pub fn index(&self, remaining: i64) -> usize {
        let len = self.secs.len() as i64;
        let r = remaining % 1000;
        (len - r).clamp(0, (len - 1).max(0)) as usize
    }
    /// Delay for Again: the first step, seconds; `None` when there are no steps.
    pub fn again_delay(&self) -> Option<i64> {
        self.secs.first().copied()
    }
    /// `remaining` after Again (back to the first step).
    pub fn remaining_for_failed(&self) -> i64 {
        self.secs.len() as i64
    }

    /// Delay for Hard: on the first step the average of steps 1 and 2 (or min(1.5×, +1 day)
    /// with a single step), rounded to whole days when over a day; on later steps the current
    /// step repeats. `None` when there are no steps.
    pub fn hard_delay(&self, remaining: i64) -> Option<i64> {
        let idx = self.index(remaining);
        let current = *self.secs.get(idx)?;
        if idx != 0 {
            return Some(current);
        }
        let secs = match self.secs.get(1) {
            Some(&second) => (current + second) / 2,
            None => ((current as f64 * 1.5).floor() as i64).min(current + DAY_SECS),
        };
        Some(if secs > DAY_SECS { (secs as f64 / DAY_SECS as f64).round() as i64 * DAY_SECS } else { secs })
    }
    /// Delay for Good: the next step, or `None` when Good graduates.
    pub fn good_delay(&self, remaining: i64) -> Option<i64> {
        self.secs.get(self.index(remaining) + 1).copied()
    }
    /// `remaining` after Good moves to the next step.
    pub fn remaining_for_good(&self, remaining: i64) -> i64 {
        (self.secs.len() as i64 - (self.index(remaining) as i64 + 1)).max(0)
    }
    /// Delay of the current step (used for revlog `last_ivl`).
    pub fn current_delay(&self, remaining: i64) -> Option<i64> {
        self.secs.get(self.index(remaining)).copied()
    }
    /// Number of steps whose cumulative length stays under one day (optimizer clipping input).
    pub fn steps_within_one_day(&self) -> usize {
        let mut total = 0;
        let mut n = 0;
        for &s in &self.secs {
            total += s;
            if total < DAY_SECS {
                n += 1;
            } else {
                break;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_and_ten_minutes_again_60_hard_330_good_600() {
        let s = LearningSteps::new(&[1.0, 10.0]);
        assert_eq!(s.again_delay(), Some(60));
        assert_eq!(s.hard_delay(2), Some(330));
        assert_eq!(s.good_delay(2), Some(600));
        assert_eq!(s.remaining_for_good(2), 1);
        assert_eq!(s.good_delay(1), None);
        assert_eq!(s.current_delay(1), Some(600));
    }

    #[test]
    fn hard_on_a_later_step_repeats_it_single_3_day_step_hard_is_4_days() {
        let s = LearningSteps::new(&[1.0, 10.0]);
        assert_eq!(s.hard_delay(1), Some(600));
        assert_eq!(LearningSteps::new(&[3.0 * 1440.0]).hard_delay(1), Some(4 * 86400));
        assert_eq!(LearningSteps::new(&[10.0]).hard_delay(1), Some(900));
        assert_eq!(LearningSteps::new(&[]).hard_delay(0), None);
    }

    #[test]
    fn steps_within_one_day_and_filtering() {
        assert_eq!(LearningSteps::new(&[1.0, 10.0, 1440.0]).steps_within_one_day(), 2);
        assert_eq!(LearningSteps::new(&[0.0, -1.0, f64::NAN, 2.5]).secs, vec![150]);
        assert_eq!(LearningSteps::new(&[1.0, 10.0]).index(0), 1);
        assert_eq!(LearningSteps::new(&[1.0, 10.0]).index(1002), 0);
    }
}
