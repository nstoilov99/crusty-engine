//! Animation primitives used by widgets.
//!
//! Phase 3 ships one helper: a frame-rate-independent exponential
//! approach. It feels good for the typical "fade towards target" pattern
//! widgets use (hover/press tints, opening/closing panels) and avoids the
//! state machinery a proper spring needs. A critically-damped spring will
//! land in a later polish phase if we want overshoot on bigger animations.

/// Move `current` toward `target` by an amount that converges
/// exponentially with rate `rate` over time `dt`. The result is
/// independent of frame rate: at `dt = 1/rate` we cover ~63 % of the
/// remaining distance, at `dt = 4/rate` ~98 %.
#[inline]
pub fn approach(current: f32, target: f32, dt: f32, rate: f32) -> f32 {
    let alpha = 1.0 - (-(rate * dt).max(0.0)).exp();
    current + (target - current) * alpha
}

/// Convenience: pass a half-life in seconds instead of a rate.
#[inline]
pub fn approach_half_life(current: f32, target: f32, dt: f32, half_life_secs: f32) -> f32 {
    let rate = (2.0_f32.ln()) / half_life_secs.max(1e-6);
    approach(current, target, dt, rate)
}
