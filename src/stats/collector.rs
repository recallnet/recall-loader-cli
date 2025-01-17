use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::stats::aggregator::Aggregator;
use crate::stats::ops::Operation;

// Collector collects the result of each executed operation
pub struct Collector {
    ops: Arc<Mutex<Vec<Operation>>>,
    aggregator: Arc<Mutex<Aggregator>>,
    sender: Option<mpsc::Sender<Operation>>,
    background_task: Option<JoinHandle<()>>,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector {
    pub fn new() -> Self {
        let (sender, mut receiver) = mpsc::channel::<Operation>(100);
        let ops = Arc::new(Mutex::new(Vec::with_capacity(100)));
        let aggregator = Arc::new(Mutex::new(Aggregator::new()));

        let ops_clone = ops.clone();
        let aggregator_clone = aggregator.clone();
        let background_task = tokio::spawn(async move {
            while let Some(op) = receiver.recv().await {
                let mut ops_guard = ops_clone.lock().unwrap();
                ops_guard.push(op.clone());

                let mut aggregator_guard = aggregator_clone.lock().unwrap();
                aggregator_guard.insert(op);
            }
        });

        Collector {
            ops,
            aggregator,
            sender: Some(sender),
            background_task: Some(background_task),
        }
    }

    /// Adds an operation to the collector
    pub async fn collect(&self, op: Operation) -> Result<(), mpsc::error::SendError<Operation>> {
        if let Some(sender) = &self.sender {
            sender.send(op).await
        } else {
            Err(mpsc::error::SendError(op))
        }
    }

    /// Retrieves the collected operations
    pub fn get_operations(&self) -> Vec<Operation> {
        self.ops.lock().unwrap().clone()
    }

    /// Closes the collector, waits for all messages to be processed, and stops the background task
    pub async fn close(&mut self) {
        // Drop the sender to signal the channel is closed
        self.sender.take();

        // Await the background task to finish
        if let Some(task) = self.background_task.take() {
            task.await.unwrap(); // Wait for the task to complete
        }
    }

    pub fn display_aggregated(&self) {
        self.aggregator.lock().unwrap().display();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::task::JoinSet;

    #[tokio::test]
    async fn test_collect_concurrent() {
        let collector = Arc::new(Collector::new());

        let num_tasks = 10;
        let ops_per_task = 100;

        let mut tasks = JoinSet::new();
        for _ in 0..num_tasks {
            let collector_clone = collector.clone();

            tasks.spawn(async move {
                for i in 0..ops_per_task {
                    let op = Operation {
                        id: format!("{}", i),
                        ..Default::default()
                    };
                    collector_clone.collect(op).await.unwrap();
                }
            });
        }

        tasks.join_all().await;

        let operations = collector.get_operations();
        assert_eq!(operations.len(), num_tasks * ops_per_task);
    }
}
