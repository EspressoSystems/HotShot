use hotshot_types::traits::metrics::{Counter, Metrics, NoMetrics};

/// CDN-specific metrics
#[derive(Clone)]
pub struct CdnMetricsValue {
    /// The number of failed messages
    pub num_failed_messages: Box<dyn Counter>,
}

impl CdnMetricsValue {
    /// Populate the metrics with the CDN-specific ones
    pub fn new(metrics: &dyn Metrics) -> Self {
        // Create a subgroup for the CDN
        let subgroup = metrics.subgroup("cdn".into());

        // Create the CDN-specific metrics
        Self {
            num_failed_messages: subgroup.create_counter("num_failed_messages".into(), None),
        }
    }
}

impl Default for CdnMetricsValue {
    // The default is empty metrics
    fn default() -> Self {
        Self::new(&*NoMetrics::boxed())
    }
}
