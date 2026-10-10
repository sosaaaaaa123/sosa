//! SosaTune: contatori diagnostici del wrapper VST3 (solo numeri atomici,
//! sicuri anche nel thread audio). L'interfaccia di SosaTune li scrive in un
//! registro per capire, su un computer reale, dove si perde un comando.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub static PROCESS_CALLS: AtomicU64 = AtomicU64::new(0);
pub static IS_PROCESSING: AtomicBool = AtomicBool::new(false);
pub static HOST_PARAM_POINTS: AtomicU64 = AtomicU64::new(0);
pub static LAST_HOST_HASH: AtomicU32 = AtomicU32::new(0);
pub static LAST_HOST_VALUE_BITS: AtomicU32 = AtomicU32::new(0);
pub static GUI_SET_CALLS: AtomicU64 = AtomicU64::new(0);
pub static GUI_APPLIED_DIRECT: AtomicU64 = AtomicU64::new(0);
pub static GUI_QUEUED: AtomicU64 = AtomicU64::new(0);
pub static GUI_APPLIED_FROM_QUEUE: AtomicU64 = AtomicU64::new(0);
pub static HANDLER_MISSING: AtomicU64 = AtomicU64::new(0);
pub static UNKNOWN_PARAM: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn bump(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

/// Una riga leggibile con lo stato di tutti i contatori.
pub fn summary() -> String {
    let r = |c: &AtomicU64| c.load(Ordering::Relaxed);
    format!(
        "audio: chiamate={} attivo={} | host->plugin: modifiche={} ultima=#{} {:.3} | interfaccia: set={} subito={} in_coda={} applicate_da_coda={} | handler_mancante={} param_sconosciuto={}",
        r(&PROCESS_CALLS),
        IS_PROCESSING.load(Ordering::Relaxed),
        r(&HOST_PARAM_POINTS),
        LAST_HOST_HASH.load(Ordering::Relaxed),
        f32::from_bits(LAST_HOST_VALUE_BITS.load(Ordering::Relaxed)),
        r(&GUI_SET_CALLS),
        r(&GUI_APPLIED_DIRECT),
        r(&GUI_QUEUED),
        r(&GUI_APPLIED_FROM_QUEUE),
        r(&HANDLER_MISSING),
        r(&UNKNOWN_PARAM),
    )
}
