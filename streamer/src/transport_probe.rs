//! Opt-in slow-stage durations only; no payload, token or endpoint logging.
use std::{sync::atomic::{AtomicU32,Ordering},time::Instant};
static ROWS:AtomicU32=AtomicU32::new(0);
pub(crate) fn mark(stage:&'static str,start:Instant){
    let us=start.elapsed().as_micros() as u64;
    if us>=20_000 && ROWS.fetch_add(1,Ordering::Relaxed)<180 {
        log::info!("CG_LOCK k={} us={}",stage,us);
    }
}
