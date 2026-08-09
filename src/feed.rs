//! サーボ制御と、すべての給餌経路で共有する安全性チェックを提供する。

use crate::schedule::ScheduleStore;
#[cfg(test)]
use color_eyre::eyre::bail;
use color_eyre::eyre::{Result, WrapErr};
use rppal::pwm::{Channel, Polarity, Pwm};
use std::{
    env::{self, VarError},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

const PWM_PERIOD: Duration = Duration::from_millis(20);
const NEUTRAL_PULSE_WIDTH_MICROS: u64 = 1_500;
const MAX_FEED_PULSE_WIDTH_MICROS: u64 = 2_300;

#[derive(Clone)]
/// 実機 PWM または開発用モックを選択してサーボを駆動する低レベルドライバ。
pub struct Feeder {
    backend: FeederBackend,
}

#[derive(Clone, Copy)]
enum FeederBackend {
    Hardware,
    Mock,
    #[cfg(test)]
    Failing,
}

#[derive(Clone)]
/// 排他制御、クールタイム判定、給餌履歴の記録を一つの操作として提供する。
pub struct FeedService {
    feeder: Feeder,
    store: ScheduleStore,
    lock: Arc<Mutex<()>>,
}

/// 給餌要求が物理操作まで進んだか、クールタイムで抑止されたかを表す。
pub enum FeedOutcome {
    /// 給餌と履歴記録が成功した。
    Fed,
    /// 給餌可能になるまでの残り秒数。
    Cooldown(u64),
}

impl FeedService {
    /// 指定したドライバと永続化ストアを共有する給餌サービスを構築する。
    pub fn new(feeder: Feeder, store: ScheduleStore) -> Self {
        Self {
            feeder,
            store,
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// クールタイムを検査して給餌し、成功した物理操作だけを履歴へ記録する。
    pub async fn feed(&self) -> Result<FeedOutcome> {
        // ロック取得後にクールタイムを再確認することで、同時要求による二重給餌を防ぐ。
        let _guard = self.lock.lock().await;
        let remaining = self.store.cooldown_remaining().await?;
        if remaining > 0 {
            return Ok(FeedOutcome::Cooldown(remaining));
        }
        let settings = self.store.settings().await?;
        self.feeder
            .feed(settings.feed_duration_ms, settings.feed_speed_percent)
            .await?;
        // 物理操作後の記録失敗は再試行すると二重給餌になるため、文脈付きエラーとして返す。
        self.store
            .record_feed()
            .await
            .wrap_err("physical feed succeeded but its history could not be recorded")?;
        Ok(FeedOutcome::Fed)
    }
}

impl Feeder {
    /// `FEEDER_MOCK` から実機とモックを選ぶ。未指定時は実機を使用する。
    pub fn from_env() -> Result<Self> {
        let mock = match env::var("FEEDER_MOCK") {
            Ok(value) => value
                .parse::<bool>()
                .wrap_err("FEEDER_MOCK must be true or false")?,
            Err(VarError::NotPresent) => false,
            Err(error) => return Err(error).wrap_err("FEEDER_MOCK is not valid Unicode"),
        };

        Ok(Self {
            backend: if mock {
                FeederBackend::Mock
            } else {
                FeederBackend::Hardware
            },
        })
    }

    /// 起動ログに表示できる現在のドライバ名を返す。
    pub fn mode(&self) -> &'static str {
        match self.backend {
            FeederBackend::Hardware => "hardware",
            FeederBackend::Mock => "mock",
            #[cfg(test)]
            FeederBackend::Failing => "failing test driver",
        }
    }

    /// 選択済みのバックエンドで指定速度・時間だけ給餌方向へ回転する。
    async fn feed(&self, duration_ms: u64, speed_percent: u64) -> Result<()> {
        match self.backend {
            FeederBackend::Mock => {
                println!("Feed requested at {speed_percent}% for {duration_ms} ms (mock)");
                Ok(())
            }
            FeederBackend::Hardware => {
                feed_with_hardware_pwm(
                    Duration::from_millis(duration_ms),
                    feed_pulse_width(speed_percent),
                )
                .await?;
                Ok(())
            }
            #[cfg(test)]
            FeederBackend::Failing => bail!("simulated feeder failure"),
        }
    }
}

async fn feed_with_hardware_pwm(
    feed_duration: Duration,
    feed_pulse_width: Duration,
) -> rppal::pwm::Result<()> {
    // Raspberry Pi 4B maps PWM0 to BCM GPIO12 or GPIO18, and PWM1 to GPIO13 or GPIO19.
    // This application uses PWM0 on BCM GPIO18 (physical pin 12).
    let pwm = Pwm::with_period(
        Channel::Pwm0,
        PWM_PERIOD,
        feed_pulse_width,
        Polarity::Normal,
        true,
    )?;

    // 最初のパルスから設定速度で回転させ、指定時間後に信号を停止する。
    tokio::time::sleep(feed_duration).await;
    pwm.disable()?;

    Ok(())
}

/// 給餌速度 1～100% を、停止位置から最大回転までのパルス幅へ線形変換する。
fn feed_pulse_width(speed_percent: u64) -> Duration {
    let pulse_range = MAX_FEED_PULSE_WIDTH_MICROS - NEUTRAL_PULSE_WIDTH_MICROS;
    Duration::from_micros(NEUTRAL_PULSE_WIDTH_MICROS + pulse_range * speed_percent / 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DATABASE_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn maps_feed_speed_to_pulse_width() {
        assert_eq!(feed_pulse_width(1), Duration::from_micros(1_508));
        assert_eq!(feed_pulse_width(50), Duration::from_micros(1_900));
        assert_eq!(feed_pulse_width(100), Duration::from_micros(2_300));
    }

    async fn service(backend: FeederBackend) -> (FeedService, String) {
        let id = NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
        let path = format!(
            "/tmp/pi-auto-feeder-feed-test-{}-{id}.sqlite3",
            std::process::id()
        );
        let store = ScheduleStore::connect(&format!("sqlite://{path}"))
            .await
            .unwrap();
        (FeedService::new(Feeder { backend }, store), path)
    }

    #[tokio::test]
    async fn successful_feed_is_recorded_and_starts_cooldown() {
        let (service, path) = service(FeederBackend::Mock).await;

        assert!(matches!(service.feed().await.unwrap(), FeedOutcome::Fed));
        assert!(matches!(
            service.feed().await.unwrap(),
            FeedOutcome::Cooldown(1..)
        ));
        assert_eq!(service.store.recent_feed_history().await.unwrap().len(), 1);

        service.store.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn failed_physical_feed_is_not_recorded() {
        let (service, path) = service(FeederBackend::Failing).await;

        assert!(service.feed().await.is_err());
        assert!(
            service
                .store
                .recent_feed_history()
                .await
                .unwrap()
                .is_empty()
        );

        service.store.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_requests_cannot_feed_twice() {
        let (service, path) = service(FeederBackend::Mock).await;

        let (left, right) = tokio::join!(service.feed(), service.feed());
        let outcomes = [left.unwrap(), right.unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, FeedOutcome::Fed))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, FeedOutcome::Cooldown(_)))
                .count(),
            1
        );

        service.store.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }
}
