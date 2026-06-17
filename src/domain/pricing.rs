use crate::error::{AppError, AppResult};

pub fn lmsr_probabilities(shares: &[f64], liquidity_b: f64) -> AppResult<Vec<f64>> {
    validate_inputs(shares, liquidity_b)?;
    let max_scaled = shares
        .iter()
        .map(|value| value / liquidity_b)
        .fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = shares
        .iter()
        .map(|value| ((value / liquidity_b) - max_scaled).exp())
        .collect();
    let sum = exps.iter().sum::<f64>();
    Ok(exps.iter().map(|value| value / sum).collect())
}

pub fn shares_for_budget(
    shares: &[f64],
    option_index: usize,
    budget: i64,
    liquidity_b: f64,
) -> AppResult<f64> {
    validate_inputs(shares, liquidity_b)?;
    if budget <= 0 {
        return Err(AppError::Validation(
            "buy amount must be positive".to_string(),
        ));
    }

    let budget = budget as f64;
    let base_cost = lmsr_cost(shares, liquidity_b)?;
    let mut low = 0.0_f64;
    let mut high = budget.max(1.0) * 32.0;

    while incremental_cost(shares, option_index, high, liquidity_b)? < budget {
        high *= 2.0;
        if high > 1_000_000.0 {
            break;
        }
    }

    for _ in 0..80 {
        let mid = (low + high) / 2.0;
        let mut updated = shares.to_vec();
        updated[option_index] += mid;
        let cost = lmsr_cost(&updated, liquidity_b)? - base_cost;
        if cost <= budget {
            low = mid;
        } else {
            high = mid;
        }
    }

    Ok(low)
}

pub fn sale_value_for_shares(
    shares: &[f64],
    option_index: usize,
    shares_to_sell: f64,
    liquidity_b: f64,
) -> AppResult<f64> {
    validate_inputs(shares, liquidity_b)?;
    if shares_to_sell <= 0.0 {
        return Err(AppError::Validation(
            "sell shares must be positive".to_string(),
        ));
    }
    if shares[option_index] + 1e-9 < shares_to_sell {
        return Err(AppError::Validation(
            "cannot sell more shares than exist in the market".to_string(),
        ));
    }

    let before = lmsr_cost(shares, liquidity_b)?;
    let mut updated = shares.to_vec();
    updated[option_index] -= shares_to_sell;
    let after = lmsr_cost(&updated, liquidity_b)?;
    Ok((before - after).max(0.0))
}

pub fn lmsr_cost(shares: &[f64], liquidity_b: f64) -> AppResult<f64> {
    validate_inputs(shares, liquidity_b)?;
    let max_scaled = shares
        .iter()
        .map(|value| value / liquidity_b)
        .fold(f64::NEG_INFINITY, f64::max);
    let sum = shares
        .iter()
        .map(|value| ((value / liquidity_b) - max_scaled).exp())
        .sum::<f64>();
    Ok(liquidity_b * (max_scaled + sum.ln()))
}

fn incremental_cost(
    shares: &[f64],
    option_index: usize,
    delta: f64,
    liquidity_b: f64,
) -> AppResult<f64> {
    let before = lmsr_cost(shares, liquidity_b)?;
    let mut updated = shares.to_vec();
    updated[option_index] += delta;
    let after = lmsr_cost(&updated, liquidity_b)?;
    Ok(after - before)
}

fn validate_inputs(shares: &[f64], liquidity_b: f64) -> AppResult<()> {
    if shares.len() < 2 {
        return Err(AppError::Validation(
            "markets need at least two options".to_string(),
        ));
    }
    if !liquidity_b.is_finite() || liquidity_b <= 0.0 {
        return Err(AppError::Validation(
            "liquidity must be positive".to_string(),
        ));
    }
    if shares
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(AppError::Validation(
            "share state must be finite and non-negative".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{lmsr_probabilities, sale_value_for_shares, shares_for_budget};

    #[test]
    fn lmsr_probabilities_are_monotonic() {
        let start = vec![0.0, 0.0];
        let initial = lmsr_probabilities(&start, 100.0).expect("probabilities");
        let shares = shares_for_budget(&start, 0, 100, 100.0).expect("shares");
        let after = lmsr_probabilities(&[shares, 0.0], 100.0).expect("probabilities");
        assert!(after[0] > initial[0]);
        assert!(after[1] < initial[1]);
    }

    #[test]
    fn selling_returns_positive_value() {
        let state = vec![50.0, 0.0];
        let sale = sale_value_for_shares(&state, 0, 5.0, 100.0).expect("sale value");
        assert!(sale > 0.0);
    }
}
