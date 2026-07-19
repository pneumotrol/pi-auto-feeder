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
const MIN_PULSE_WIDTH_MICROS: u64 = 500;
const MAX_PULSE_WIDTH_MICROS: u64 = 2_500;
const MAX_ANGLE_DEGREES: u64 = 180;
const IDLE_ANGLE_DEGREES: u64 = 0;
const FEED_ANGLE_DEGREES: u64 = 90;
const POSITION_SETTLE_TIME: Duration = Duration::from_secs(1);

#[derive(Clone)]
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
pub struct FeedService {
    feeder: Feeder,
    store: ScheduleStore,
    lock: Arc<Mutex<()>>,
}

pub enum FeedOutcome {
    Fed(String),
    Cooldown(u64),
}

impl FeedService {
    pub fn new(feeder: Feeder, store: ScheduleStore) -> Self {
        Self {
            feeder,
            store,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn feed(&self) -> Result<FeedOutcome> {
        let _guard = self.lock.lock().await;
        let remaining = self.store.cooldown_remaining().await?;
        if remaining > 0 {
            return Ok(FeedOutcome::Cooldown(remaining));
        }
        let settings = self.store.settings().await?;
        self.feeder.feed(settings.feed_duration_ms).await?;
        let fed_at = self
            .store
            .record_feed()
            .await
            .wrap_err("physical feed succeeded but its history could not be recorded")?;
        Ok(FeedOutcome::Fed(fed_at))
    }
}

impl Feeder {
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

    pub fn mode(&self) -> &'static str {
        match self.backend {
            FeederBackend::Hardware => "hardware",
            FeederBackend::Mock => "mock",
            #[cfg(test)]
            FeederBackend::Failing => "failing test driver",
        }
    }

    async fn feed(&self, duration_ms: u64) -> Result<()> {
        match self.backend {
            FeederBackend::Mock => {
                println!("Feed requested for {duration_ms} ms (mock)");
                Ok(())
            }
            FeederBackend::Hardware => {
                feed_with_hardware_pwm(Duration::from_millis(duration_ms)).await?;
                Ok(())
            }
            #[cfg(test)]
            FeederBackend::Failing => bail!("simulated feeder failure"),
        }
    }
}

async fn feed_with_hardware_pwm(feed_duration: Duration) -> rppal::pwm::Result<()> {
    // Raspberry Pi 4B maps PWM0 to BCM GPIO12 or GPIO18, and PWM1 to GPIO13 or GPIO19.
    // This application uses PWM0 on BCM GPIO18 (physical pin 12).
    let pwm = Pwm::with_period(
        Channel::Pwm0,
        PWM_PERIOD,
        pulse_width(IDLE_ANGLE_DEGREES),
        Polarity::Normal,
        true,
    )?;

    tokio::time::sleep(POSITION_SETTLE_TIME).await;
    pwm.set_pulse_width(pulse_width(FEED_ANGLE_DEGREES))?;
    tokio::time::sleep(feed_duration).await;
    pwm.set_pulse_width(pulse_width(IDLE_ANGLE_DEGREES))?;
    tokio::time::sleep(POSITION_SETTLE_TIME).await;

    Ok(())
}

fn pulse_width(angle_degrees: u64) -> Duration {
    let pulse_range = MAX_PULSE_WIDTH_MICROS - MIN_PULSE_WIDTH_MICROS;
    let micros = MIN_PULSE_WIDTH_MICROS + pulse_range * angle_degrees / MAX_ANGLE_DEGREES;
    Duration::from_micros(micros)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DATABASE_ID: AtomicU64 = AtomicU64::new(0);

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

        assert!(matches!(service.feed().await.unwrap(), FeedOutcome::Fed(_)));
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
                .filter(|outcome| matches!(outcome, FeedOutcome::Fed(_)))
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
