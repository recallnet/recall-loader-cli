use std::fmt;

use chrono::{DateTime, Duration, Utc};

pub struct Throughput(pub f64);

impl fmt::Display for Throughput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = self.0;

        if t < (1 << 10) as f64 {
            return write!(f, "{:.1}B/s", t);
        }
        if t < (1 << 20) as f64 {
            return write!(f, "{:.1}KiB/s", t / (1 << 10) as f64);
        }
        if t < (1 << 30) as f64 {
            return write!(f, "{:.1}MiB/s", t / (1 << 20) as f64);
        }
        if t < (1i64 << 40) as f64 {
            return write!(f, "{:.2}GiB/s", t / (1 << 30) as f64);
        }
        write!(f, "{:.2}TiB/s", t / (1i64 << 40) as f64)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum OperationType {
    #[default]
    Get,
    Put,
    List,
    Delete,
}

impl fmt::Display for OperationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operation = match self {
            OperationType::Get => "Get",
            OperationType::Put => "Put",
            OperationType::List => "List",
            OperationType::Delete => "Delete",
        };
        write!(f, "{}", operation)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Operation {
    pub id: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub op_type: OperationType,
    pub size: i64,
    pub file: String,
    pub error: String,
}

impl Operation {
    pub fn duration(&self) -> Duration {
        self.end.signed_duration_since(self.start)
    }
}
