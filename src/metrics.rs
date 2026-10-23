//! Per-stage timers and counters. Collected in memory; written out after the timed region.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct Metrics {
    pub stage_time: BTreeMap<&'static str, Duration>,
    pub hits: usize,
    pub misses: usize,
}

impl Metrics {
    pub fn time<T>(&mut self, stage: &'static str, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let out = f();
        *self.stage_time.entry(stage).or_default() += t.elapsed();
        out
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 { 0.0 } else { self.hits as f64 / total as f64 }
    }

    pub fn report(&self) -> String {
        let mut s = String::new();
        for (stage, d) in &self.stage_time {
            s.push_str(&format!("{stage:<24}{:>10.3} ms\n", d.as_secs_f64() * 1e3));
        }
        s.push_str(&format!("hits={} misses={} hit_rate={:.1}%\n", self.hits, self.misses, self.hit_rate() * 100.0));
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_counts() {
        let mut m = Metrics::default();
        let v = m.time("stage", || 41 + 1);
        assert_eq!(v, 42);
        assert!(m.stage_time.contains_key("stage"));
        m.hits = 3;
        m.misses = 1;
        assert!((m.hit_rate() - 0.75).abs() < 1e-9);
    }
}
