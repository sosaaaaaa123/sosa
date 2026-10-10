//! Disattiva i numeri "denormali" (valori vicinissimi a zero) durante
//! l'elaborazione audio. Con code di riverbero o silenzi possono rendere i
//! calcoli anche 100 volte piu' lenti su alcuni processori, causando scatti.
//! All'uscita dal blocco l'impostazione precedente viene ripristinata, cosi'
//! il DAW non se ne accorge.

pub struct FlushToZero {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    previous: u64,
}

impl FlushToZero {
    #[cfg(target_arch = "x86_64")]
    #[allow(deprecated)]
    pub fn new() -> Self {
        use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        // Bit 15 = flush-to-zero, bit 6 = denormals-are-zero.
        unsafe {
            let previous = _mm_getcsr();
            _mm_setcsr(previous | (1 << 15) | (1 << 6));
            Self { previous: previous as u64 }
        }
    }

    #[cfg(target_arch = "aarch64")]
    pub fn new() -> Self {
        // Bit 24 del registro FPCR = flush-to-zero.
        let previous: u64;
        unsafe {
            std::arch::asm!("mrs {}, fpcr", out(reg) previous);
            std::arch::asm!("msr fpcr, {}", in(reg) previous | (1 << 24));
        }
        Self { previous }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn new() -> Self {
        Self {}
    }
}

impl Drop for FlushToZero {
    fn drop(&mut self) {
        #[cfg(target_arch = "x86_64")]
        #[allow(deprecated)]
        unsafe {
            std::arch::x86_64::_mm_setcsr(self.previous as u32);
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            std::arch::asm!("msr fpcr, {}", in(reg) self.previous);
        }
    }
}
