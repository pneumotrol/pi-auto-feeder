use rppal::pwm::{Channel, Polarity, Pwm};
use std::time::Duration;
use tokio::sync::Mutex;

const PWM_PERIOD: Duration = Duration::from_millis(20);
const MIN_PULSE_WIDTH_MICROS: u64 = 500;
const MAX_PULSE_WIDTH_MICROS: u64 = 2_500;
const MAX_ANGLE_DEGREES: u64 = 180;
const IDLE_ANGLE_DEGREES: u64 = 0;
const FEED_ANGLE_DEGREES: u64 = 90;
const POSITION_HOLD_TIME: Duration = Duration::from_secs(1);

static FEED_LOCK: Mutex<()> = Mutex::const_new(());

pub async fn feed() -> rppal::pwm::Result<()> {
    let _guard = FEED_LOCK.lock().await;

    // Raspberry Pi 4B maps PWM0 to BCM GPIO12 or GPIO18, and PWM1 to GPIO13 or GPIO19.
    // This application uses PWM0 on BCM GPIO18 (physical pin 12).
    let pwm = Pwm::with_period(
        Channel::Pwm0,
        PWM_PERIOD,
        pulse_width(IDLE_ANGLE_DEGREES),
        Polarity::Normal,
        true,
    )?;

    tokio::time::sleep(POSITION_HOLD_TIME).await;
    pwm.set_pulse_width(pulse_width(FEED_ANGLE_DEGREES))?;
    tokio::time::sleep(POSITION_HOLD_TIME).await;
    pwm.set_pulse_width(pulse_width(IDLE_ANGLE_DEGREES))?;
    tokio::time::sleep(POSITION_HOLD_TIME).await;

    Ok(())
}

fn pulse_width(angle_degrees: u64) -> Duration {
    let pulse_range = MAX_PULSE_WIDTH_MICROS - MIN_PULSE_WIDTH_MICROS;
    let micros = MIN_PULSE_WIDTH_MICROS + pulse_range * angle_degrees / MAX_ANGLE_DEGREES;
    Duration::from_micros(micros)
}
