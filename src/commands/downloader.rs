use crate::stats::collector::Collector;
use crate::stats::ops::{Operation, OperationType};
use crate::targets::Target;
use chrono::Utc;
use hoku_provider::fvm_shared::address::Address;
use hoku_sdk::machine::bucket::{Bucket, GetOptions};
use hoku_sdk::machine::Machine;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::{error, info};

pub struct Downloader {
    sender: Option<mpsc::Sender<String>>,
    background_tasks: Vec<Option<JoinHandle<()>>>,
}

impl Downloader {
    pub fn new(
        target: Arc<dyn Target>,
        collector: Arc<Collector>,
        thread_id: String,
        bucket_address: Address,
        concurrency: i32,
        blob_size: i64,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<String>(100);
        let rx = Arc::new(Mutex::new(receiver));
        let mut background_tasks = Vec::new();
        // let bucket = Arc::new(bucket.clone());
        for i in 0..concurrency {
            let thread_id = format!("{}-{}", thread_id, i);
            let target_clone = target.clone();
            let collector_clone = collector.clone();
            let rx_clone = rx.clone();
            //let bucket_clone = bucket.clone();
            let background_task = tokio::spawn(async move {
                loop {
                    let mut rx_guard = rx_clone.lock().await;
                    if let Some(key) = rx_guard.recv().await {
                        drop(rx_guard);
                        if let Err(err) = download_blob(
                            target_clone.clone(),
                            collector_clone.clone(),
                            thread_id.clone(),
                            bucket_address,
                            &key,
                            blob_size,
                        )
                        .await
                        {
                            error!("downloading blob failed: {}", err)
                        }
                        continue;
                    }
                    break;
                }
            });
            background_tasks.push(Some(background_task));
        }

        Self {
            sender: Some(sender),
            background_tasks,
        }
    }

    pub async fn download(&self, keys: &Vec<String>) -> Result<(), mpsc::error::SendError<String>> {
        for key in keys {
            if let Some(sender) = &self.sender {
                sender.send(key.clone()).await?;
                continue;
            }

            return Err(mpsc::error::SendError(key.clone()));
        }

        Ok(())
    }

    pub async fn close(&mut self) {
        // Drop the sender to signal the channel is closed
        self.sender.take();

        for task in &mut self.background_tasks {
            // Await the background task to finish
            if let Some(task) = task.take() {
                task.await.unwrap(); // Wait for the task to complete
            }
        }
    }
}

async fn download_blob(
    target: Arc<dyn Target>,
    collector: Arc<Collector>,
    thread_id: String,
    bucket_address: Address,
    key: &str,
    size: i64,
) -> anyhow::Result<()> {
    let opts = GetOptions {
        range: None,
        height: Default::default(),
        show_progress: false,
    };

    let start = Utc::now();
    let mut operation = Operation {
        id: thread_id.clone(),
        start,
        op_type: OperationType::Get,
        size,
        file: key.to_string(),
        ..Default::default()
    };

    let obj_file = async_tempfile::TempFile::new().await.unwrap();
    let bucket = Bucket::attach(bucket_address).await.unwrap();
    let result = target
        .get_object(&bucket, key, Box::new(obj_file), opts.range)
        .await;

    match result {
        Ok(_) => {
            let end = Utc::now();
            operation.end = end;
            collector.collect(operation).await?;
            info!(
                "successfully downloaded object {} (took {:?}).",
                key,
                end.signed_duration_since(start).num_milliseconds()
            );
            Ok(())
        }
        Err(e) => {
            operation.end = Utc::now();
            operation.error = e.to_string();
            collector.collect(operation).await?;
            error!(error=?e, "failed to download data");
            Err(e)
        }
    }
}
