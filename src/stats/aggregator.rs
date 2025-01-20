use std::collections::{HashMap, HashSet};
use std::{cmp, fmt};

use crate::stats::ops::{Operation, OperationType, Throughput};
use chrono::{DateTime, Duration, Utc};
use tracing::info;

pub struct Aggregator {
    operations: HashMap<OperationType, AggregatedOperation>,
}

impl Aggregator {
    pub fn new() -> Self {
        Aggregator {
            operations: HashMap::new(),
        }
    }
    pub fn insert(&mut self, operation: Operation) {
        let op_type = operation.op_type.clone();
        self.operations
            .entry(op_type)
            .or_insert(AggregatedOperation {
                start_time: DateTime::<Utc>::MAX_UTC,
                end_time: DateTime::<Utc>::MIN_UTC,
                min_duration: Duration::MAX,
                max_duration: Duration::MIN,
                ..Default::default()
            })
            .insert(operation)
    }

    pub fn display(&self) {
        for (op_type, operation) in &self.operations {
            info!(
                operation = %op_type,
                concurrency = operation.concurrency(),
                duration = %HumanDuration(operation.duration()),
                total = operation.n,
                errors = operation.errors,
                throughput = %operation.avg_throughput(),
                objects_per_sec = operation.objects_per_sec(),
                min_duration = %HumanDuration(operation.min_duration),
                avg_duration = %HumanDuration(operation.avg_duration()),
                max_duration = %HumanDuration(operation.max_duration),
                "Test results"
            );

            println!("----------------------------------------------------");
            println!(
                "Operation: {}. Concurrency: {}. Duration: {}",
                op_type,
                operation.concurrency(),
                HumanDuration(operation.duration())
            );
            println!("Total: {}", operation.n);
            println!("Errors: {}", operation.errors);
            println!();
            println!("Averages");
            println!("* Throughput: {}", operation.avg_throughput());
            println!("* Objects/s: {:.1}", operation.objects_per_sec());
            println!();
            println!("Duration Per Operation ");
            println!("* Min: {}", HumanDuration(operation.min_duration));
            println!("* Avg: {}", HumanDuration(operation.avg_duration()));
            println!("* Max: {}", HumanDuration(operation.max_duration));
            println!();
        }
    }
}

#[derive(Default, Debug)]
struct AggregatedOperation {
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    min_duration: Duration,
    max_duration: Duration,
    errors: i32,
    n: i32,
    total_duration: Duration,
    total_bytes: i64,
    threads: HashSet<String>,
}

impl AggregatedOperation {
    pub fn insert(&mut self, operation: Operation) {
        self.n += 1;
        if !operation.error.is_empty() {
            self.errors += 1;
            // early return because we don't want operations that failed to count on stats
            return;
        }
        self.total_bytes += operation.size;
        self.total_duration += operation.duration();
        self.threads.insert(operation.id.clone());
        self.start_time = cmp::min(self.start_time, operation.start);
        self.end_time = cmp::max(self.end_time, operation.end);
        self.min_duration = cmp::min(self.min_duration, operation.duration());
        self.max_duration = cmp::max(self.max_duration, operation.duration());
    }

    pub fn duration(&self) -> Duration {
        self.end_time.signed_duration_since(self.start_time)
    }

    pub fn avg_throughput(&self) -> Throughput {
        if self.total_bytes == 0 {
            return Throughput(0.0);
        }

        let d = self.duration().num_nanoseconds().unwrap() as f64;
        let one_second = Duration::seconds(1).num_nanoseconds().unwrap() as f64;

        Throughput(self.total_bytes as f64 * one_second / d)
    }

    pub fn objects_per_sec(&self) -> f64 {
        let d = self.duration().num_nanoseconds().unwrap() as f64;
        let one_second = Duration::seconds(1).num_nanoseconds().unwrap() as f64;

        self.n as f64 * one_second / d
    }

    pub fn concurrency(&self) -> i32 {
        self.threads.len() as i32
    }

    pub fn avg_duration(&self) -> Duration {
        let d = self.total_duration.num_nanoseconds().unwrap() as f64;
        let one_second = Duration::seconds(1).num_nanoseconds().unwrap() as f64;
        let avg_in_secs = d / (self.n as f64 * one_second);
        Duration::milliseconds((avg_in_secs * 1000.0).round() as i64)
    }
}

pub struct HumanDuration(Duration);

impl fmt::Display for HumanDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total_secs = self.0.num_seconds();
        let millis = self.0.num_nanoseconds().unwrap_or(0) % 1_000_000_000 / 1_000_000;

        if total_secs == 0 {
            return write!(f, "{}ms", millis);
        }

        if total_secs < 60 {
            if millis > 0 {
                return write!(f, "{}.{:03}s", total_secs, millis);
            }
            return write!(f, "{}s", total_secs);
        }

        let minutes = total_secs / 60;
        let seconds = total_secs % 60;
        if minutes < 60 {
            return write!(f, "{}m {}.{:03}s", minutes, seconds, millis);
        }

        let hours = minutes / 60;
        let minutes = minutes % 60;
        if hours < 24 {
            return write!(f, "{}h {}m {}.{:03}s", hours, minutes, seconds, millis);
        }

        let days = hours / 24;
        let hours = hours % 24;
        write!(
            f,
            "{}d {}h {}m {}.{:03}s",
            days, hours, minutes, seconds, millis
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::stats::aggregator::{Aggregator, HumanDuration};
    use crate::stats::ops::{Operation, OperationType};
    use chrono::DateTime;

    #[test]
    fn test_insert_new_operation_to_aggregator() {
        let mut aggregator = Aggregator::new();
        let operation1 = Operation {
            id: "1".to_string(),
            start: DateTime::from_timestamp_millis(1736886531819).unwrap(), // Tue Jan 14 2025 20:28:51.819
            end: DateTime::from_timestamp_millis(1736886532619).unwrap(), // Tue Jan 14 2025 20:28:52.619
            op_type: OperationType::Get,
            size: 10,
            file: "bar/1.txt".to_string(),
            error: "".to_string(),
        };

        let operation2 = Operation {
            id: "2".to_string(),
            start: DateTime::from_timestamp_millis(1736886531989).unwrap(), // Tue Jan 14 2025 20:28:51.989
            end: DateTime::from_timestamp_millis(1736886533619).unwrap(), // Tue Jan 14 2025 20:28:53.619
            op_type: OperationType::Get,
            size: 30,
            file: "bar/2.txt".to_string(),
            error: "".to_string(),
        };

        aggregator.insert(operation1);
        aggregator.insert(operation2);

        let aggregated_operation = aggregator.operations.get(&OperationType::Get).unwrap();
        assert_eq!(2, aggregated_operation.concurrency());
        assert_eq!(
            "1.800s",
            HumanDuration(aggregated_operation.duration()).to_string()
        );
        assert_eq!(
            (10 + 30) as f64 / 1.8,
            aggregated_operation.avg_throughput().0
        );
        assert_eq!(2f64 / 1.8, aggregated_operation.objects_per_sec());
        assert_eq!(
            "0.800s",
            HumanDuration(aggregated_operation.min_duration).to_string()
        );
        assert_eq!(
            "1.215s",
            HumanDuration(aggregated_operation.avg_duration()).to_string()
        );
        assert_eq!(
            "1.630s",
            HumanDuration(aggregated_operation.max_duration).to_string()
        );
    }
}
