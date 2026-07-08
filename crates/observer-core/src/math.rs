const WINDOW: usize = 1000;

#[derive(Debug, Clone)]
pub struct Welford {
    n: usize,
    mean: f64,
    m2: f64,
}

impl Default for Welford {
    fn default() -> Self {
        Self {
            n: 0,
            mean: 0.0,
            m2: 0.0,
        }
    }
}

impl Welford {
    pub fn update(&mut self, x: f64) {
        self.n += 1;
        let delta = x - self.mean;
        self.mean += delta / self.n as f64;
        let delta2 = x - self.mean;
        self.m2 += delta * delta2;
    }

    pub fn sigma(&self) -> f64 {
        if self.n < 2 {
            return 0.0;
        }
        (self.m2 / (self.n - 1) as f64).sqrt()
    }

    pub fn z_score(&self, x: f64) -> f64 {
        let s = self.sigma();
        if s <= 1e-12 {
            return 0.0;
        }
        (x - self.mean) / s
    }
}

pub fn ema_update(prev: f64, price: f64, n: usize) -> f64 {
    let k = 2.0 / (n as f64 + 1.0);
    if prev == 0.0 {
        price
    } else {
        (price - prev) * k + prev
    }
}

pub fn velocity(price_now: f64, price_100ms_ago: f64) -> f64 {
    if price_100ms_ago <= 0.0 {
        return 0.0;
    }
    (price_now - price_100ms_ago) / price_100ms_ago / 0.1
}

#[derive(Debug, Clone)]
pub struct SymbolMetrics {
    pub welford: Welford,
    pub prices: [f64; 2048],
    pub head: usize,
    pub len: usize,
    pub ema_50: f64,
    pub ema_200: f64,
    pub ema_500: f64,
    pub atr: f64,
    pub prev_close: f64,
    pub atr_count: u32,
}

impl Default for SymbolMetrics {
    fn default() -> Self {
        Self {
            welford: Welford::default(),
            prices: [0.0; 2048],
            head: 0,
            len: 0,
            ema_50: 0.0,
            ema_200: 0.0,
            ema_500: 0.0,
            atr: 0.0,
            prev_close: 0.0,
            atr_count: 0,
        }
    }
}

impl SymbolMetrics {
    pub fn push_price(&mut self, price: f64) {
        self.prices[self.head] = price;
        self.head = (self.head + 1) % 2048;
        if self.len < 2048 {
            self.len += 1;
        }
        self.welford.update(price);
        self.ema_50 = ema_update(self.ema_50, price, 50);
        self.ema_200 = ema_update(self.ema_200, price, 200);
        self.ema_500 = ema_update(self.ema_500, price, 500);
        if self.prev_close > 0.0 {
            let tr = (price - self.prev_close).abs();
            self.atr_count += 1;
            let n = self.atr_count.min(14) as f64;
            self.atr = if self.atr_count == 1 {
                tr
            } else {
                (self.atr * (n - 1.0) + tr) / n
            };
        }
        self.prev_close = price;
    }

    pub fn price_100ms_ago(&self, samples_per_100ms: usize) -> f64 {
        if self.len <= samples_per_100ms {
            return self.prices[(self.head + 2048 - 1) % 2048];
        }
        let idx = (self.head + 2048 - samples_per_100ms) % 2048;
        self.prices[idx]
    }

    pub fn regime(&self) -> u8 {
        if self.atr <= 0.0 {
            return 0;
        }
        let strength = (self.ema_50 - self.ema_200).abs() / self.atr;
        if strength > 2.0 {
            2
        } else if strength < 1.2 {
            0
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welford_positive_sigma() {
        let mut w = Welford::default();
        for x in [1.0, 2.0, 3.0, 4.0, 5.0] {
            w.update(x);
        }
        assert!(w.sigma() > 0.0);
    }
}
