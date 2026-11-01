//! One value easing toward a target at a steady rate: the opacity of the screen cover, the textbox, a HUD
//! element or a particle layer, and the progress of a tint.

/// `alpha` moves to `target` by `rate` per second (0: it is there at once, on the next tick).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fade {
    pub alpha: f32,
    pub target: f32,
    pub rate: f32,
}

impl Fade {
    /// Already at `value`, staying.
    pub fn at(value: f32) -> Self {
        Self { alpha: value, target: value, rate: 0.0 }
    }

    /// The rate of a fade lasting `seconds` (a fade of no time: 0).
    pub fn rate_over(seconds: f32) -> f32 {
        if seconds > 0.0 { 1.0 / seconds } else { 0.0 }
    }

    pub fn tick(&mut self, dt: f32) {
        self.alpha = if self.rate > 0.0 {
            let step = self.rate * dt;
            if self.alpha < self.target { (self.alpha + step).min(self.target) } else { (self.alpha - step).max(self.target) }
        } else {
            self.target
        };
    }

    /// Whether it has arrived (nothing is moving).
    pub fn done(&self) -> bool {
        self.alpha == self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_to_the_target_from_either_side_and_stops_there() {
        let mut up = Fade { alpha: 0.0, target: 1.0, rate: Fade::rate_over(2.0) };
        up.tick(0.5);
        assert_eq!((up.alpha, up.done()), (0.25, false));
        up.tick(5.0);
        assert_eq!((up.alpha, up.done()), (1.0, true));
        let mut down = Fade { alpha: 1.0, target: 0.0, rate: Fade::rate_over(4.0) };
        down.tick(1.0);
        assert_eq!(down.alpha, 0.75);
        down.tick(9.0);
        assert_eq!(down.alpha, 0.0);
    }

    #[test]
    fn no_rate_means_at_once() {
        assert_eq!(Fade::rate_over(0.0), 0.0);
        let mut f = Fade { alpha: 0.2, target: 1.0, rate: 0.0 };
        f.tick(0.0);
        assert_eq!(f, Fade::at(1.0));
    }
}
