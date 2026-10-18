//! Animated textures: a `.png.mcmeta` beside a texture makes it a strip of
//! frames (top to bottom) played in order, each for `frametime` ticks,
//! optionally blending into the next (`interpolate`), as in modern
//! Minecraft's `AnimationMetadataSection`.

use anyhow::{Context, Result};
use image::{RgbaImage, imageops};

/// What a `.png.mcmeta`'s `animation` section says.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    /// Frame size in pixels; `None` means square, as wide as the image.
    pub frame_width: Option<u32>,
    pub frame_height: Option<u32>,
    /// Frames to show in order, as (frame index, ticks); empty means every
    /// frame in the strip, each for `frametime`.
    pub frames: Vec<(u32, u32)>,
    pub frametime: u32,
    pub interpolate: bool,
}

impl Animation {
    /// Reads a `.png.mcmeta`. `None` if it has no `animation` section.
    pub fn parse(text: &str) -> Result<Option<Self>> {
        let json = super::parse_json(text).context("bad .png.mcmeta")?;
        let Some(anim) = json.get("animation") else {
            return Ok(None);
        };
        let number = |key: &str| anim.get(key).and_then(|v| v.as_u64()).map(|v| v as u32);
        let frametime = number("frametime").unwrap_or(1).max(1);
        let frames = anim
            .get("frames")
            .and_then(|f| f.as_array())
            .map(|frames| {
                frames
                    .iter()
                    .filter_map(|frame| match frame {
                        serde_json::Value::Number(n) => Some((n.as_u64()? as u32, frametime)),
                        serde_json::Value::Object(o) => Some((
                            o.get("index")?.as_u64()? as u32,
                            o.get("time")
                                .and_then(|t| t.as_u64())
                                .map_or(frametime, |t| t.max(1) as u32),
                        )),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Some(Self {
            frame_width: number("width"),
            frame_height: number("height"),
            frames,
            frametime,
            interpolate: anim
                .get("interpolate")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }))
    }

    /// The frame size in `image`.
    fn frame_size(&self, image: &RgbaImage) -> (u32, u32) {
        let (width, height) = image.dimensions();
        match (self.frame_width, self.frame_height) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, height),
            (None, Some(h)) => (width, h),
            (None, None) => {
                let side = width.min(height);
                (side, side)
            }
        }
    }

    /// Cuts `image` into its frames, read left to right, then top to bottom.
    pub fn split(&self, image: &RgbaImage) -> Vec<RgbaImage> {
        let (fw, fh) = self.frame_size(image);
        if fw == 0 || fh == 0 {
            return Vec::new();
        }
        let (columns, rows) = (image.width() / fw, image.height() / fh);
        (0..rows)
            .flat_map(|row| (0..columns).map(move |col| (col, row)))
            .map(|(col, row)| imageops::crop_imm(image, col * fw, row * fh, fw, fh).to_image())
            .collect()
    }

    /// The play order for a strip of `count` frames, dropping frames
    /// that don't exist.
    pub fn schedule(&self, count: u32) -> Vec<(u32, u32)> {
        if self.frames.is_empty() {
            return (0..count).map(|i| (i, self.frametime)).collect();
        }
        self.frames
            .iter()
            .copied()
            .filter(|&(index, _)| index < count)
            .collect()
    }
}

/// An animation ready to play: its frames and their order.
#[derive(Debug, Clone)]
pub struct Player {
    pub frames: Vec<RgbaImage>,
    /// (frame, ticks) in play order.
    pub schedule: Vec<(u32, u32)>,
    pub interpolate: bool,
    /// Ticks in one loop.
    period: u64,
}

/// What a texture shows at some tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shown {
    Frame(u32),
    /// `from` blending into `to` by `amount` (0–1).
    Blend {
        from: u32,
        to: u32,
        amount: f32,
    },
}

impl Player {
    /// `None` unless there's more than one step to play.
    pub fn new(
        frames: Vec<RgbaImage>,
        schedule: Vec<(u32, u32)>,
        interpolate: bool,
    ) -> Option<Self> {
        let period = schedule.iter().map(|&(_, t)| t as u64).sum();
        (schedule.len() > 1 && period > 0).then_some(Self {
            frames,
            schedule,
            interpolate,
            period,
        })
    }

    /// What shows `tick` ticks after the animation started.
    pub fn shown(&self, tick: u64) -> Shown {
        let mut t = tick % self.period;
        for (i, &(frame, time)) in self.schedule.iter().enumerate() {
            if t < time as u64 {
                if !self.interpolate {
                    return Shown::Frame(frame);
                }
                let next = self.schedule[(i + 1) % self.schedule.len()].0;
                return Shown::Blend {
                    from: frame,
                    to: next,
                    amount: t as f32 / time as f32,
                };
            }
            t -= time as u64;
        }
        Shown::Frame(self.schedule[0].0)
    }

    /// The picture for `shown`.
    pub fn image(&self, shown: Shown) -> RgbaImage {
        match shown {
            Shown::Frame(i) => self.frames[i as usize].clone(),
            Shown::Blend { from, to, amount } => {
                let (a, b) = (&self.frames[from as usize], &self.frames[to as usize]);
                let mut out = a.clone();
                for (o, q) in out.pixels_mut().zip(b.pixels()) {
                    for c in 0..4 {
                        let mixed = o.0[c] as f32 * (1.0 - amount) + q.0[c] as f32 * amount;
                        o.0[c] = mixed.round() as u8;
                    }
                }
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(frames: u32) -> RgbaImage {
        RgbaImage::from_fn(2, 2 * frames, |_, y| {
            image::Rgba([(y / 2) as u8 * 10, 0, 0, 255])
        })
    }

    #[test]
    fn reads_mcmeta_like_minecraft() {
        assert_eq!(Animation::parse("{}").unwrap(), None);
        let plain = Animation::parse(r#"{"animation": {}}"#).unwrap().unwrap();
        assert_eq!(plain.schedule(3), [(0, 1), (1, 1), (2, 1)]);
        let custom = Animation::parse(
            r#"{"animation": {"frametime": 3, "interpolate": true,
                "frames": [2, {"index": 0, "time": 7}, 9]}}"#,
        )
        .unwrap()
        .unwrap();
        assert!(custom.interpolate);
        // Frame 9 doesn't exist in a 3-frame strip.
        assert_eq!(custom.schedule(3), [(2, 3), (0, 7)]);
        assert!(Animation::parse("not json").is_err());
    }

    #[test]
    fn splits_square_and_sized_frames() {
        let anim = Animation::parse(r#"{"animation": {}}"#).unwrap().unwrap();
        let frames = anim.split(&strip(4));
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[3].get_pixel(0, 0).0[0], 30);
        let wide = Animation::parse(r#"{"animation": {"width": 1, "height": 2}}"#)
            .unwrap()
            .unwrap();
        // Two columns of frames, read across first.
        let frames = wide.split(&strip(2));
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[1].get_pixel(0, 0).0[0], 0);
        assert_eq!(frames[2].get_pixel(0, 0).0[0], 10);
    }

    #[test]
    fn plays_frames_in_order_and_loops() {
        let player = Player::new(Vec::new(), vec![(0, 2), (3, 1)], false).unwrap();
        let shown: Vec<_> = (0..7).map(|t| player.shown(t)).collect();
        use Shown::Frame;
        assert_eq!(
            shown,
            [
                Frame(0),
                Frame(0),
                Frame(3),
                Frame(0),
                Frame(0),
                Frame(3),
                Frame(0)
            ]
        );
        assert!(Player::new(Vec::new(), vec![(0, 5)], false).is_none());
    }

    #[test]
    fn interpolation_blends_toward_the_next_frame() {
        let frames = Animation::parse(r#"{"animation": {}}"#)
            .unwrap()
            .unwrap()
            .split(&strip(2));
        let player = Player::new(frames, vec![(0, 4), (1, 4)], true).unwrap();
        assert_eq!(
            player.shown(1),
            Shown::Blend {
                from: 0,
                to: 1,
                amount: 0.25
            }
        );
        // Halfway from 0 to 10.
        assert_eq!(player.image(player.shown(2)).get_pixel(0, 0).0[0], 5);
        // The last frame blends back into the first.
        assert_eq!(
            player.shown(6),
            Shown::Blend {
                from: 1,
                to: 0,
                amount: 0.5
            }
        );
    }
}
