use panzo_core::time::{QpcClock, TimeError};
use thiserror::Error;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

pub struct QpcSource {
    frequency: i64,
}

impl QpcSource {
    pub fn new() -> Result<Self, QpcSourceError> {
        let mut frequency = 0_i64;
        unsafe { QueryPerformanceFrequency(&raw mut frequency) }
            .map_err(QpcSourceError::Windows)?;
        if frequency <= 0 {
            return Err(QpcSourceError::InvalidFrequency(frequency));
        }
        Ok(Self { frequency })
    }

    pub const fn frequency(&self) -> i64 {
        self.frequency
    }

    pub fn now(&self) -> Result<i64, QpcSourceError> {
        let mut counter = 0_i64;
        unsafe { QueryPerformanceCounter(&raw mut counter) }.map_err(QpcSourceError::Windows)?;
        Ok(counter)
    }

    pub fn clock_from_system_relative_tick(
        &self,
        system_relative_tick: i64,
    ) -> Result<QpcClock, QpcSourceError> {
        let start_qpc =
            QpcClock::qpc_from_system_relative_tick(self.frequency, system_relative_tick)?;
        QpcClock::new(self.frequency, start_qpc).map_err(Into::into)
    }
}

#[derive(Debug, Error)]
pub enum QpcSourceError {
    #[error("Windows QPC call failed: {0}")]
    Windows(windows::core::Error),
    #[error("Windows returned invalid QPC frequency {0}")]
    InvalidFrequency(i64),
    #[error(transparent)]
    Time(#[from] TimeError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qpc_is_monotonic_and_has_positive_frequency() {
        let qpc = QpcSource::new().unwrap();
        let first = qpc.now().unwrap();
        let second = qpc.now().unwrap();
        assert!(qpc.frequency() > 0);
        assert!(second >= first);
    }
}
