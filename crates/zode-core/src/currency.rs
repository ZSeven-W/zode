//! Display currencies for cost. Per-provider prices are entered in USD per
//! million tokens; the accumulated USD cost is converted to the chosen currency
//! for display. Conversion rates are approximate built-in defaults (cost is an
//! estimate) — USD is exact (rate 1.0). The `/settings` currency picker and the
//! config `currency` field select one.

/// A display currency: ISO code, symbol, and approximate units per 1 USD.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Currency {
    pub code: &'static str,
    pub symbol: &'static str,
    /// Units of this currency per 1 USD (USD = 1.0). Approximate.
    pub per_usd: f64,
}

/// Supported display currencies. USD first (exact). Rates are rough and only
/// affect the displayed estimate; for an exact figure, set the provider's
/// prices in USD and read in USD.
pub const CURRENCIES: &[Currency] = &[
    Currency { code: "USD", symbol: "$", per_usd: 1.0 },
    Currency { code: "CNY", symbol: "¥", per_usd: 7.2 },
    Currency { code: "EUR", symbol: "€", per_usd: 0.92 },
    Currency { code: "GBP", symbol: "£", per_usd: 0.79 },
    Currency { code: "JPY", symbol: "¥", per_usd: 155.0 },
    Currency { code: "KRW", symbol: "₩", per_usd: 1350.0 },
    Currency { code: "INR", symbol: "₹", per_usd: 83.0 },
    Currency { code: "TWD", symbol: "NT$", per_usd: 32.0 },
    Currency { code: "HKD", symbol: "HK$", per_usd: 7.8 },
];

impl Currency {
    /// Look up by ISO code (case-insensitive); falls back to USD.
    pub fn from_code(code: &str) -> Currency {
        let c = code.trim().to_ascii_uppercase();
        CURRENCIES
            .iter()
            .copied()
            .find(|x| x.code == c)
            .unwrap_or(CURRENCIES[0])
    }

    /// Format a USD amount in this currency (convert, then symbol + value).
    /// Sub-cent amounts get 4 decimals, matching the USD formatter.
    pub fn format(&self, usd: f64) -> String {
        let amount = usd * self.per_usd;
        if amount != 0.0 && amount.abs() < 0.01 {
            format!("{}{:.4}", self.symbol, amount)
        } else {
            format!("{}{:.2}", self.symbol, amount)
        }
    }
}

/// All currency codes, for the settings picker.
pub fn codes() -> Vec<String> {
    CURRENCIES.iter().map(|c| c.code.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_code_is_case_insensitive_and_falls_back() {
        assert_eq!(Currency::from_code("cny").code, "CNY");
        assert_eq!(Currency::from_code(" Usd ").code, "USD");
        assert_eq!(Currency::from_code("???").code, "USD"); // fallback
    }

    #[test]
    fn format_converts_and_symbols() {
        assert_eq!(Currency::from_code("USD").format(1.5), "$1.50");
        // 1.5 USD * 7.2 = 10.80 CNY.
        assert_eq!(Currency::from_code("CNY").format(1.5), "¥10.80");
        // Sub-cent gets 4 decimals.
        assert_eq!(Currency::from_code("USD").format(0.0008), "$0.0008");
        assert_eq!(Currency::from_code("USD").format(0.0), "$0.00");
    }
}
