//! Fixture: functions, methods, closures, nested units, comments, small bodies.

pub struct Counter {
    total: i64,
}

impl Counter {
    /// Adds a batch of values, skipping negatives.
    pub fn add_batch(&mut self, values: &[i64]) -> usize {
        // count how many were accepted
        let mut accepted = 0;
        for value in values {
            if *value >= 0 {
                self.total += value;
                accepted += 1;
            }
        }
        accepted
    }
}

pub fn summarize(values: Vec<i64>) -> (i64, i64) {
    let positives: Vec<i64> = values
        .iter()
        .filter(|v| **v > 0) // small closure, filtered out
        .cloned()
        .collect();
    let doubled: Vec<i64> = positives
        .iter()
        .map(|value| {
            let bumped = value + 1;
            let scaled = bumped * 2;
            scaled - 1
        })
        .collect();
    (positives.iter().sum(), doubled.iter().sum())
}

fn tiny() -> i32 {
    1
}
