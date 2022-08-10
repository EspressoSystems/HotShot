//! The metrics trait is used to collect information from multiple components in the entire system.
//!
//! It is recommended to move this metrics around in an `Arc<dyn Metrics>` so the application is agnostic over what the metric collection system is running.

use std::sync::Arc;

/// The metrics type.
pub trait Metrics {
    /// Add an ever-incrementing counter.
    fn add_counter(&self, label: &str, amount: usize);
    /// Set a gauge to a specific value, overwriting the previous value.
    fn set_gauge(&self, label: &str, amount: usize);
    /// Add a histogram point
    fn add_histogram_point(&self, label: &str, value: f64);
    /// Set a label, overwriting the previous label value
    fn set_label(&self, name: &str, value: String);
    /// Create a subgroup with a specified prefix.
    fn subgroup(&self, subgroup_name: &str) -> Arc<dyn Metrics>;
}

#[cfg(test)]
mod test {
    use super::Metrics;
    use std::collections::hash_map::Entry;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    struct TestMetrics {
        prefix: String,
        values: Arc<Mutex<HashMap<String, MetricValue>>>,
    }

    impl TestMetrics {
        fn label(&self, name: &str) -> String {
            if self.prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{}-{}", self.prefix, name)
            }
        }
    }

    impl Metrics for TestMetrics {
        fn add_counter(&self, label: &str, amount: usize) {
            let label = self.label(label);
            let mut values = self.values.lock().unwrap();
            match values.entry(label) {
                Entry::Occupied(v) => {
                    v.into_mut().add_counter(amount);
                }
                Entry::Vacant(v) => {
                    v.insert(MetricValue::Counter(amount));
                }
            }
        }

        fn set_gauge(&self, label: &str, amount: usize) {
            let label = self.label(label);
            let mut values = self.values.lock().unwrap();
            match values.entry(label) {
                Entry::Occupied(v) => {
                    v.into_mut().set_gauge(amount);
                }
                Entry::Vacant(v) => {
                    v.insert(MetricValue::Gauge(amount));
                }
            }
        }

        fn add_histogram_point(&self, label: &str, value: f64) {
            let label = self.label(label);
            let mut values = self.values.lock().unwrap();
            match values.entry(label) {
                Entry::Occupied(v) => {
                    v.into_mut().add_histogram_point(value);
                }
                Entry::Vacant(v) => {
                    v.insert(MetricValue::Histogram(vec![value]));
                }
            }
        }

        fn set_label(&self, label: &str, value: String) {
            let label = self.label(label);
            let mut values = self.values.lock().unwrap();
            match values.entry(label) {
                Entry::Occupied(v) => {
                    v.into_mut().set_label(value);
                }
                Entry::Vacant(v) => {
                    v.insert(MetricValue::Label(value));
                }
            }
        }

        fn subgroup(&self, subgroup_name: &str) -> Arc<dyn Metrics> {
            Arc::new(Self {
                prefix: self.label(subgroup_name),
                values: Arc::clone(&self.values),
            })
        }
    }

    #[derive(Debug, PartialEq)]
    enum MetricValue {
        Counter(usize),
        Gauge(usize),
        Histogram(Vec<f64>),
        Label(String),
    }

    impl MetricValue {
        fn add_counter(&mut self, value: usize) {
            if let Self::Counter(val) = self {
                *val += value;
            } else {
                eprintln!("Switching from {:?} to Counter", self);
                *self = Self::Counter(value);
            }
        }

        fn set_gauge(&mut self, value: usize) {
            if let Self::Gauge(val) = self {
                *val = value;
            } else {
                eprintln!("Switching from {:?} to Gauge", self);
                *self = Self::Gauge(value);
            }
        }

        fn add_histogram_point(&mut self, value: f64) {
            if let Self::Histogram(val) = self {
                val.push(value);
            } else {
                eprintln!("Switching from {:?} to Histogram", self);
                *self = Self::Histogram(vec![value]);
            }
        }

        fn set_label(&mut self, value: String) {
            if let Self::Label(val) = self {
                *val = value;
            } else {
                eprintln!("Switching from {:?} to Label", self);
                *self = Self::Label(value);
            }
        }
    }

    #[test]
    fn test() {
        let values = Arc::default();
        let metrics: Arc<dyn Metrics> = Arc::new(TestMetrics {
            prefix: String::new(),
            values: Arc::clone(&values),
        });

        metrics.set_gauge("foo", 5);

        for i in 0..5 {
            metrics.add_counter("bar", i);
        }

        for i in 0..10 {
            metrics.add_histogram_point("baz", i as f64);
        }

        let sub = metrics.subgroup("child");

        sub.set_gauge("foo", 10);

        for i in 0..5 {
            sub.add_counter("bar", i * 2);
        }

        for i in 0..10 {
            sub.add_histogram_point("baz", i as f64 * 2.0);
        }

        drop(sub);
        drop(metrics);
        let values = Arc::try_unwrap(values).unwrap().into_inner().unwrap();
        assert_eq!(values["foo"], MetricValue::Gauge(5));
        assert_eq!(values["bar"], MetricValue::Counter(10)); // 0..5
        assert_eq!(
            values["baz"],
            MetricValue::Histogram(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0])
        );
        assert_eq!(values["child-foo"], MetricValue::Gauge(10));
        assert_eq!(values["child-bar"], MetricValue::Counter(20)); // 0..5 *2
        assert_eq!(
            values["child-baz"],
            MetricValue::Histogram(vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0])
        );
    }
}
