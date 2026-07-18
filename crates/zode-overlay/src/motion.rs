//! Ghost-cursor motion: Dubins-path flight with a quartic speed profile and a
//! critically-damped-ish spring overshoot on arrival. Ported from
//! pi-computer-use (MIT) native/macos/agent_cursor_motion.swift; dt-driven.

use std::f64::consts::PI;

const END_OFFSET: f64 = 16.0;
const END_ANGLE: f64 = PI / 4.0;
const TURN_RADIUS: f64 = 80.0;

pub struct Motion {
    pos: (f64, f64),
    heading: f64,
    path: Option<PlannedPath>,
    dist: f64,
    spring: Option<Spring>,
    spring_target: Option<((f64, f64), f64)>,
}

struct Spring {
    ox: f64,
    oy: f64,
    vx: f64,
    vy: f64,
}

impl Default for Motion {
    fn default() -> Self {
        Self::new()
    }
}

impl Motion {
    pub fn new() -> Self {
        Self {
            pos: (-200.0, -200.0),
            heading: PI / 4.0,
            path: None,
            dist: 0.0,
            spring: None,
            spring_target: None,
        }
    }

    pub fn position(&self) -> (f64, f64) {
        self.pos
    }
    pub fn heading(&self) -> f64 {
        self.heading
    }
    pub fn is_idle(&self) -> bool {
        self.path.is_none() && self.spring.is_none()
    }

    pub fn set_position(&mut self, x: f64, y: f64) {
        self.pos = (x, y);
        self.path = None;
        self.spring = None;
        self.spring_target = None;
        self.dist = 0.0;
    }

    pub fn move_to(&mut self, click_x: f64, click_y: f64) {
        let target = (
            click_x + END_ANGLE.cos() * END_OFFSET,
            click_y + END_ANGLE.sin() * END_OFFSET,
        );
        self.path = Some(plan_path(
            self.pos.0,
            self.pos.1,
            self.heading + PI,
            target.0,
            target.1,
            END_ANGLE + PI,
            TURN_RADIUS,
            END_ANGLE,
            target,
        ));
        self.spring = None;
        self.spring_target = None;
        self.dist = 0.0;
    }

    /// Advance by `dt` seconds. Returns true while still animating.
    pub fn tick(&mut self, dt: f64) -> bool {
        let dt = dt.clamp(0.0, 0.05);
        if let Some(path) = &self.path {
            let progress = (self.dist / path.length.max(1.0)).min(1.0);
            let profile = 16.0 * progress * progress * (1.0 - progress) * (1.0 - progress);
            let min_speed = if progress < 0.5 { 300.0 } else { 200.0 };
            let speed = min_speed + (900.0 - min_speed) * profile;
            self.dist += speed * dt;

            if self.dist >= path.length {
                let end = path.sample(path.length);
                self.spring = Some(Spring {
                    ox: 0.0,
                    oy: 0.0,
                    vx: end.heading.cos() * speed * 0.8,
                    vy: end.heading.sin() * speed * 0.8,
                });
                self.spring_target = Some((path.target, path.end_visual_heading));
                self.pos = path.target;
                self.heading = path.end_visual_heading;
                self.path = None;
                self.dist = 0.0;
            } else {
                let st = path.sample(self.dist);
                self.pos = (st.x, st.y);
                self.heading = rotate_toward(self.heading, st.heading + PI, 14.0 * dt);
            }
            true
        } else if let (Some(spring), Some((tp, th))) = (self.spring.as_mut(), self.spring_target) {
            let sub = dt / 4.0;
            for _ in 0..4 {
                spring.vx += (-400.0 * spring.ox - 17.0 * spring.vx) * sub;
                spring.vy += (-400.0 * spring.oy - 17.0 * spring.vy) * sub;
                spring.ox += spring.vx * sub;
                spring.oy += spring.vy * sub;
            }
            self.pos = (tp.0 + spring.ox, tp.1 + spring.oy);
            self.heading = th;
            if spring.ox.hypot(spring.oy) < 0.3 && spring.vx.hypot(spring.vy) < 2.0 {
                self.pos = tp;
                self.spring = None;
                self.spring_target = None;
                false
            } else {
                true
            }
        } else {
            false
        }
    }
}

fn rotate_toward(current: f64, desired: f64, max_step: f64) -> f64 {
    let mut diff = desired - current;
    while diff > PI {
        diff -= 2.0 * PI;
    }
    while diff < -PI {
        diff += 2.0 * PI;
    }
    current + diff.clamp(-max_step, max_step)
}

// ── Path planning ──

pub(crate) struct PlannedPath {
    kind: PathKind,
    pub length: f64,
    end_visual_heading: f64,
    target: (f64, f64),
    x0: f64,
    y0: f64,
    th0: f64,
    r: f64,
    seg: [f64; 3],
    types: [u8; 3], // b'L' | b'S' | b'R'
    x1: f64,
    y1: f64,
    th1: f64,
}

enum PathKind {
    Dubins,
    Linear,
}

pub(crate) struct PathState {
    pub x: f64,
    pub y: f64,
    pub heading: f64,
}

impl PlannedPath {
    pub(crate) fn sample(&self, s: f64) -> PathState {
        match self.kind {
            PathKind::Linear => {
                let u = (s / self.length).clamp(0.0, 1.0);
                let mut diff = self.th1 - self.th0;
                while diff > PI {
                    diff -= 2.0 * PI;
                }
                while diff < -PI {
                    diff += 2.0 * PI;
                }
                PathState {
                    x: self.x0 + (self.x1 - self.x0) * u,
                    y: self.y0 + (self.y1 - self.y0) * u,
                    heading: self.th0 + diff * u,
                }
            }
            PathKind::Dubins => {
                if s <= 0.0 {
                    return PathState {
                        x: self.x0,
                        y: self.y0,
                        heading: self.th0,
                    };
                }
                let l = [
                    self.seg[0] * self.r,
                    self.seg[1] * self.r,
                    self.seg[2] * self.r,
                ];
                let s = s.min(l[0] + l[1] + l[2]);
                let (mut x, mut y, mut th) = (self.x0, self.y0, self.th0);
                let r = self.r;
                let advance = |len: f64, ty: u8, x: &mut f64, y: &mut f64, th: &mut f64| {
                    if ty == b'S' {
                        *x += th.cos() * len;
                        *y += th.sin() * len;
                    } else {
                        let dth = len / r * if ty == b'L' { 1.0 } else { -1.0 };
                        let perp = if ty == b'L' { PI / 2.0 } else { -PI / 2.0 };
                        let cx = *x + (*th + perp).cos() * r;
                        let cy = *y + (*th + perp).sin() * r;
                        let ang = (*y - cy).atan2(*x - cx);
                        *x = cx + (ang + dth).cos() * r;
                        *y = cy + (ang + dth).sin() * r;
                        *th += dth;
                    }
                };
                if s <= l[0] {
                    advance(s, self.types[0], &mut x, &mut y, &mut th);
                    return PathState { x, y, heading: th };
                }
                advance(l[0], self.types[0], &mut x, &mut y, &mut th);
                if s <= l[0] + l[1] {
                    advance(s - l[0], self.types[1], &mut x, &mut y, &mut th);
                    return PathState { x, y, heading: th };
                }
                advance(l[1], self.types[1], &mut x, &mut y, &mut th);
                advance(s - l[0] - l[1], self.types[2], &mut x, &mut y, &mut th);
                PathState { x, y, heading: th }
            }
        }
    }
}

fn mod2pi(x: f64) -> f64 {
    let tau = 2.0 * PI;
    let r = x - tau * (x / tau).floor();
    if r < 0.0 {
        r + tau
    } else {
        r
    }
}

struct DubinsSolution {
    t: f64,
    p: f64,
    q: f64,
    types: [u8; 3],
}
impl DubinsSolution {
    fn length(&self) -> f64 {
        self.t + self.p + self.q
    }
}

fn dubins_lsl(d: f64, a: f64, b: f64) -> Option<DubinsSolution> {
    let tmp0 = d + a.sin() - b.sin();
    let p2 = 2.0 + d * d - 2.0 * (a - b).cos() + 2.0 * d * (a.sin() - b.sin());
    if p2 < 0.0 {
        return None;
    }
    let tmp1 = (b.cos() - a.cos()).atan2(tmp0);
    Some(DubinsSolution {
        t: mod2pi(-a + tmp1),
        p: p2.sqrt(),
        q: mod2pi(b - tmp1),
        types: *b"LSL",
    })
}
fn dubins_rsr(d: f64, a: f64, b: f64) -> Option<DubinsSolution> {
    let tmp0 = d - a.sin() + b.sin();
    let p2 = 2.0 + d * d - 2.0 * (a - b).cos() + 2.0 * d * (b.sin() - a.sin());
    if p2 < 0.0 {
        return None;
    }
    let tmp1 = (a.cos() - b.cos()).atan2(tmp0);
    Some(DubinsSolution {
        t: mod2pi(a - tmp1),
        p: p2.sqrt(),
        q: mod2pi(-b + tmp1),
        types: *b"RSR",
    })
}
fn dubins_lsr(d: f64, a: f64, b: f64) -> Option<DubinsSolution> {
    let p2 = -2.0 + d * d + 2.0 * (a - b).cos() + 2.0 * d * (a.sin() + b.sin());
    if p2 < 0.0 {
        return None;
    }
    let p = p2.sqrt();
    let tmp1 = (-a.cos() - b.cos()).atan2(d + a.sin() + b.sin()) - (-2.0f64).atan2(p);
    Some(DubinsSolution {
        t: mod2pi(-a + tmp1),
        p,
        q: mod2pi(-mod2pi(b) + tmp1),
        types: *b"LSR",
    })
}
fn dubins_rsl(d: f64, a: f64, b: f64) -> Option<DubinsSolution> {
    let p2 = d * d - 2.0 + 2.0 * (a - b).cos() - 2.0 * d * (a.sin() + b.sin());
    if p2 < 0.0 {
        return None;
    }
    let p = p2.sqrt();
    let tmp1 = (a.cos() + b.cos()).atan2(d - a.sin() - b.sin()) - 2.0f64.atan2(p);
    Some(DubinsSolution {
        t: mod2pi(a - tmp1),
        p,
        q: mod2pi(b - tmp1),
        types: *b"RSL",
    })
}
fn dubins_rlr(d: f64, a: f64, b: f64) -> Option<DubinsSolution> {
    let tmp = (6.0 - d * d + 2.0 * (a - b).cos() + 2.0 * d * (a.sin() - b.sin())) / 8.0;
    if tmp.abs() > 1.0 {
        return None;
    }
    let p = mod2pi(2.0 * PI - tmp.acos());
    let t = mod2pi(a - (a.cos() - b.cos()).atan2(d - a.sin() + b.sin()) + p / 2.0);
    Some(DubinsSolution {
        t,
        p,
        q: mod2pi(a - b - t + p),
        types: *b"RLR",
    })
}
fn dubins_lrl(d: f64, a: f64, b: f64) -> Option<DubinsSolution> {
    let tmp = (6.0 - d * d + 2.0 * (a - b).cos() + 2.0 * d * (b.sin() - a.sin())) / 8.0;
    if tmp.abs() > 1.0 {
        return None;
    }
    let p = mod2pi(2.0 * PI - tmp.acos());
    let t = mod2pi(-a + (-a.cos() + b.cos()).atan2(d + a.sin() - b.sin()) + p / 2.0);
    Some(DubinsSolution {
        t,
        p,
        q: mod2pi(mod2pi(b) - a - t + p),
        types: *b"LRL",
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_path(
    x0: f64,
    y0: f64,
    th0: f64,
    x1: f64,
    y1: f64,
    th1: f64,
    r: f64,
    end_visual_heading: f64,
    target: (f64, f64),
) -> PlannedPath {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let dist = dx.hypot(dy);
    if dist > 0.5 {
        let d = dist / r;
        let theta = mod2pi(dy.atan2(dx));
        let a = mod2pi(th0 - theta);
        let b = mod2pi(th1 - theta);
        let solvers = [
            dubins_lsl, dubins_rsr, dubins_lsr, dubins_rsl, dubins_rlr, dubins_lrl,
        ];
        let best = solvers
            .iter()
            .filter_map(|s| s(d, a, b))
            .filter(|s| s.length().is_finite() && s.length() >= 0.0)
            .min_by(|p, q| p.length().partial_cmp(&q.length()).unwrap());
        if let Some(bst) = best {
            return PlannedPath {
                kind: PathKind::Dubins,
                length: bst.length() * r,
                end_visual_heading,
                target,
                x0,
                y0,
                th0,
                r,
                seg: [bst.t, bst.p, bst.q],
                types: bst.types,
                x1,
                y1,
                th1,
            };
        }
    }
    PlannedPath {
        kind: PathKind::Linear,
        length: dist.max(1.0),
        end_visual_heading,
        target,
        x0,
        y0,
        th0,
        r,
        seg: [0.0; 3],
        types: *b"SSS",
        x1,
        y1,
        th1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(m: &mut Motion, max_secs: f64) -> usize {
        let mut ticks = 0;
        while m.tick(1.0 / 60.0) {
            ticks += 1;
            assert!(
                ticks as f64 / 60.0 < max_secs,
                "did not settle in {max_secs}s"
            );
        }
        ticks
    }

    #[test]
    fn move_to_settles_at_offset_target() {
        let mut m = Motion::new();
        m.set_position(100.0, 100.0);
        m.move_to(500.0, 400.0);
        settle(&mut m, 10.0);
        // Target is the click point + 16px along the fixed 45° end heading.
        let expect = (
            500.0 + (std::f64::consts::FRAC_PI_4).cos() * 16.0,
            400.0 + (std::f64::consts::FRAC_PI_4).sin() * 16.0,
        );
        let (x, y) = m.position();
        assert!(
            (x - expect.0).abs() < 0.5 && (y - expect.1).abs() < 0.5,
            "got ({x},{y})"
        );
        assert!(m.is_idle());
    }

    #[test]
    fn motion_is_continuous_and_finite() {
        let mut m = Motion::new();
        m.set_position(50.0, 900.0);
        m.move_to(1200.0, 80.0);
        let mut prev = m.position();
        for _ in 0..1200 {
            if !m.tick(1.0 / 60.0) {
                break;
            }
            let cur = m.position();
            assert!(cur.0.is_finite() && cur.1.is_finite() && m.heading().is_finite());
            let step = ((cur.0 - prev.0).powi(2) + (cur.1 - prev.1).powi(2)).sqrt();
            // Path speed caps at 900 px/s (15 px/frame); spring adds bounded
            // overshoot velocity. 30 px/frame is a generous continuity bound.
            assert!(step < 30.0, "jump of {step}px in one frame");
            prev = cur;
        }
    }

    #[test]
    fn short_hop_and_same_point_do_not_nan() {
        let mut m = Motion::new();
        m.set_position(300.0, 300.0);
        m.move_to(300.0, 300.0); // degenerate: distance ~16px offset only
        settle(&mut m, 10.0);
        let (x, y) = m.position();
        assert!(x.is_finite() && y.is_finite());
    }

    #[test]
    fn set_position_resets_to_idle() {
        let mut m = Motion::new();
        m.move_to(500.0, 500.0);
        m.set_position(10.0, 10.0);
        assert!(m.is_idle());
        assert_eq!(m.position(), (10.0, 10.0));
        assert!(!m.tick(1.0 / 60.0));
    }

    #[test]
    fn dubins_path_at_least_straight_distance() {
        let p = plan_path(
            0.0,
            0.0,
            0.0,
            400.0,
            300.0,
            std::f64::consts::FRAC_PI_4,
            80.0,
            std::f64::consts::FRAC_PI_4,
            (400.0, 300.0),
        );
        assert!(p.length >= (400.0f64.powi(2) + 300.0f64.powi(2)).sqrt() - 1.0);
        // Samples stay finite along the whole path.
        let mut s = 0.0;
        while s <= p.length {
            let st = p.sample(s);
            assert!(st.x.is_finite() && st.y.is_finite() && st.heading.is_finite());
            s += p.length / 50.0;
        }
    }
}
