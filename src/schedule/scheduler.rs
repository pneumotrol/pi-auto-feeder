//! 一回限りの給餌予定を定期確認し、共通の給餌サービスから実行する。

use super::ScheduleStore;
use crate::feed::{FeedOutcome, FeedService};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const SCHEDULER_INTERVAL: Duration = Duration::from_secs(30);

/// 停止トークンを監視しながら期限到来スケジュールを確認するタスクを起動する。
pub fn start_scheduler(
    store: ScheduleStore,
    feeder: FeedService,
    cancellation: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            run_due_schedule(&store, &feeder).await;
            tokio::select! {
                () = cancellation.cancelled() => break,
                () = tokio::time::sleep(SCHEDULER_INTERVAL) => {}
            }
        }
    })
}

async fn run_due_schedule(store: &ScheduleStore, feeder: &FeedService) {
    match store.claim_due_schedule().await {
        Ok(Some(schedule)) => {
            println!(
                "Running scheduled feed at {}",
                schedule.scheduled_at.as_deref().unwrap_or("unknown")
            );
            // 手動給餌と同じサービスを使うため、同時実行とクールタイムも共通に判定される。
            match feeder.feed().await {
                Ok(FeedOutcome::Fed(_)) => {
                    if let Err(error) = store.complete_scheduled_feed(schedule.id).await {
                        eprintln!("Failed to consume feed schedule: {error}");
                    }
                }
                Ok(FeedOutcome::Cooldown(_)) => {
                    if let Err(error) = store
                        .fail_schedule(schedule.id, "クールタイム中のため給餌されませんでした")
                        .await
                    {
                        eprintln!("Failed to mark schedule as failed: {error}");
                    }
                }
                Err(error) => {
                    eprintln!("Scheduled feed failed: {error}");
                    // 物理操作の成否が不明な場合は再実行せず、利用者の確認が必要な状態にする。
                    if let Err(mark_error) = store
                        .fail_schedule(
                            schedule.id,
                            "給餌結果を確認できませんでした．実機を確認してください",
                        )
                        .await
                    {
                        eprintln!("Failed to mark schedule as failed: {mark_error}");
                    }
                }
            }
        }
        Ok(None) => {}
        Err(error) => eprintln!("Failed to check feed schedules: {error}"),
    }
}
