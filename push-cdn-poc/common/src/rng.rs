use rand::{CryptoRng, RngCore};

pub struct DeterministicRng(pub u64);

impl CryptoRng for DeterministicRng {}

impl RngCore for DeterministicRng {
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for item in dest {
            *item = self.0 as u8;
            self.0 >>= 8;
        }
    }

    fn next_u32(&mut self) -> u32 {
        self.0 as u32
    }

    fn next_u64(&mut self) -> u64 {
        self.0
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}
