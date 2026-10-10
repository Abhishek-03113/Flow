//! The deskflow-style screen arrangement: every screen is a rectangle in
//! one shared virtual coordinate space, and two screens are neighbours
//! where their edges touch.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::device::{DeviceId, ScreenSize, LOCAL_DEVICE_ID};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub device_id: DeviceId,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Placement {
    fn right(&self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    fn bottom(&self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    fn overlaps(&self, other: &Placement) -> bool {
        i64::from(self.x) < other.right()
            && i64::from(other.x) < self.right()
            && i64::from(self.y) < other.bottom()
            && i64::from(other.y) < self.bottom()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenLayout {
    pub screens: Vec<Placement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LayoutError {
    #[error("the layout must include this computer ({LOCAL_DEVICE_ID})")]
    MissingLocal,
    #[error("screen {0:?} appears twice")]
    Duplicate(DeviceId),
    #[error("screen {0:?} has no size")]
    EmptyScreen(DeviceId),
    #[error("screens {0:?} and {1:?} overlap")]
    Overlap(DeviceId, DeviceId),
}

/// Where the pointer lands on the neighbour it crossed into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crossing {
    pub to: DeviceId,
    /// In the neighbour's own coordinates.
    pub x: i32,
    pub y: i32,
}

impl ScreenLayout {
    /// Just this computer, at the origin.
    pub fn local_only(local: ScreenSize) -> Self {
        Self {
            screens: vec![Placement {
                device_id: DeviceId(LOCAL_DEVICE_ID.to_string()),
                x: 0,
                y: 0,
                width: local.width,
                height: local.height,
            }],
        }
    }

    pub fn validate(&self) -> Result<(), LayoutError> {
        if !self
            .screens
            .iter()
            .any(|s| s.device_id.0 == LOCAL_DEVICE_ID)
        {
            return Err(LayoutError::MissingLocal);
        }
        for (i, screen) in self.screens.iter().enumerate() {
            if screen.width == 0 || screen.height == 0 {
                return Err(LayoutError::EmptyScreen(screen.device_id.clone()));
            }
            for other in &self.screens[i + 1..] {
                if other.device_id == screen.device_id {
                    return Err(LayoutError::Duplicate(screen.device_id.clone()));
                }
                if screen.overlaps(other) {
                    return Err(LayoutError::Overlap(
                        screen.device_id.clone(),
                        other.device_id.clone(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn placement(&self, id: &DeviceId) -> Option<&Placement> {
        self.screens.iter().find(|s| &s.device_id == id)
    }

    /// Adds `id` to the right of everything already placed, if missing.
    pub fn add_to_the_right(&mut self, id: &DeviceId, size: ScreenSize) {
        if self.placement(id).is_some() {
            return;
        }
        let x = self.screens.iter().map(Placement::right).max().unwrap_or(0);
        self.screens.push(Placement {
            device_id: id.clone(),
            x: x as i32,
            y: 0,
            width: size.width,
            height: size.height,
        });
    }

    /// Keeps a screen's position when its resolution changes.
    pub fn resize(&mut self, id: &DeviceId, size: ScreenSize) {
        if let Some(screen) = self.screens.iter_mut().find(|s| &s.device_id == id) {
            screen.width = size.width;
            screen.height = size.height;
        }
    }

    pub fn remove(&mut self, id: &DeviceId) {
        self.screens.retain(|s| &s.device_id != id);
    }

    /// The screen `from` borders on `edge` at `along` (pixels from the
    /// top or left of `from`, along that edge), and where the pointer
    /// lands on it: same virtual position, clamped to the neighbour.
    pub fn neighbour(&self, from: &DeviceId, edge: Edge, along: i32) -> Option<Crossing> {
        let me = self.placement(from)?;
        let (vx, vy) = match edge {
            Edge::Left | Edge::Right => (0, i64::from(me.y) + i64::from(along)),
            Edge::Top | Edge::Bottom => (i64::from(me.x) + i64::from(along), 0),
        };
        let target = self.screens.iter().find(|s| {
            if &s.device_id == from {
                return false;
            }
            match edge {
                Edge::Right => {
                    i64::from(s.x) == me.right() && (i64::from(s.y)..s.bottom()).contains(&vy)
                }
                Edge::Left => {
                    s.right() == i64::from(me.x) && (i64::from(s.y)..s.bottom()).contains(&vy)
                }
                Edge::Bottom => {
                    i64::from(s.y) == me.bottom() && (i64::from(s.x)..s.right()).contains(&vx)
                }
                Edge::Top => {
                    s.bottom() == i64::from(me.y) && (i64::from(s.x)..s.right()).contains(&vx)
                }
            }
        })?;
        let clamp = |value: i64, size: u32| value.clamp(0, i64::from(size) - 1) as i32;
        // Land one pixel inside the neighbour's edge, not on it: on the edge
        // pixel the pointer would already be pressing against the border it
        // came through and bounce straight back.
        let inset = |size: u32, far: bool| {
            let last = size.saturating_sub(1) as i32;
            let step = i32::from(size > 2);
            if far {
                last - step
            } else {
                step
            }
        };
        let (x, y) = match edge {
            Edge::Right => (
                inset(target.width, false),
                clamp(vy - i64::from(target.y), target.height),
            ),
            Edge::Left => (
                inset(target.width, true),
                clamp(vy - i64::from(target.y), target.height),
            ),
            Edge::Bottom => (
                clamp(vx - i64::from(target.x), target.width),
                inset(target.height, false),
            ),
            Edge::Top => (
                clamp(vx - i64::from(target.x), target.width),
                inset(target.height, true),
            ),
        };
        Some(Crossing {
            to: target.device_id.clone(),
            x,
            y,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> DeviceId {
        DeviceId(s.to_string())
    }

    fn place(id_: &str, x: i32, y: i32, width: u32, height: u32) -> Placement {
        Placement {
            device_id: id(id_),
            x,
            y,
            width,
            height,
        }
    }

    fn row() -> ScreenLayout {
        ScreenLayout {
            screens: vec![
                place("d1", 0, 0, 1920, 1080),
                place("p", 1920, 200, 1000, 600),
            ],
        }
    }

    #[test]
    fn crossing_right_lands_at_the_same_virtual_height() {
        let crossing = row().neighbour(&id("d1"), Edge::Right, 500).unwrap();
        assert_eq!(
            crossing,
            Crossing {
                to: id("p"),
                x: 1,
                y: 300
            }
        );
    }

    #[test]
    fn crossing_right_where_the_neighbour_is_not_there_finds_nothing() {
        // Local y=100 is above the peer, which starts at virtual y=200.
        assert_eq!(row().neighbour(&id("d1"), Edge::Right, 100), None);
        assert_eq!(row().neighbour(&id("d1"), Edge::Left, 500), None);
    }

    #[test]
    fn crossing_back_left_mirrors_into_the_local_screen() {
        let crossing = row().neighbour(&id("p"), Edge::Left, 300).unwrap();
        assert_eq!(
            crossing,
            Crossing {
                to: id("d1"),
                x: 1918,
                y: 500
            }
        );
    }

    #[test]
    fn vertical_neighbours_work_on_the_other_axis() {
        let layout = ScreenLayout {
            screens: vec![place("d1", 0, 0, 1000, 800), place("p", 0, 800, 1000, 800)],
        };
        let down = layout.neighbour(&id("d1"), Edge::Bottom, 250).unwrap();
        assert_eq!(
            down,
            Crossing {
                to: id("p"),
                x: 250,
                y: 1
            }
        );
        let up = layout.neighbour(&id("p"), Edge::Top, 250).unwrap();
        assert_eq!(
            up,
            Crossing {
                to: id("d1"),
                x: 250,
                y: 798
            }
        );
    }

    #[test]
    fn a_crossing_never_lands_on_the_border_it_came_through() {
        // Landing on the edge pixel would put the pointer straight back
        // against the neighbour it just left.
        let layout = ScreenLayout {
            screens: vec![place("d1", 0, 0, 1000, 800), place("p", 1000, 0, 1000, 800)],
        };
        let there = layout.neighbour(&id("d1"), Edge::Right, 400).unwrap();
        assert!(there.x > 0, "{there:?}");
        let back = layout.neighbour(&id("p"), Edge::Left, 400).unwrap();
        assert!(back.x < 999, "{back:?}");
    }

    #[test]
    fn the_landing_point_is_clamped_to_a_smaller_neighbour() {
        let layout = ScreenLayout {
            screens: vec![place("d1", 0, 0, 1000, 1000), place("p", 1000, 0, 500, 300)],
        };
        // Virtual y=900 is past the neighbour's bottom: not adjacent there.
        assert_eq!(layout.neighbour(&id("d1"), Edge::Right, 900), None);
        let crossing = layout.neighbour(&id("d1"), Edge::Right, 299).unwrap();
        assert_eq!(crossing.y, 299);
    }

    #[test]
    fn the_default_layout_puts_new_screens_to_the_right_in_order() {
        let mut layout = ScreenLayout::local_only(ScreenSize {
            width: 1920,
            height: 1080,
        });
        layout.add_to_the_right(
            &id("a"),
            ScreenSize {
                width: 1000,
                height: 600,
            },
        );
        layout.add_to_the_right(
            &id("b"),
            ScreenSize {
                width: 800,
                height: 600,
            },
        );
        layout.add_to_the_right(
            &id("a"),
            ScreenSize {
                width: 1,
                height: 1,
            },
        );
        assert_eq!(layout.placement(&id("a")).unwrap().x, 1920);
        assert_eq!(layout.placement(&id("b")).unwrap().x, 2920);
        assert_eq!(layout.screens.len(), 3);
        assert_eq!(layout.validate(), Ok(()));
    }

    #[test]
    fn validation_rejects_overlaps_duplicates_empties_and_a_missing_local() {
        let mut overlapping = row();
        overlapping.screens[1].x = 1000;
        assert!(matches!(
            overlapping.validate(),
            Err(LayoutError::Overlap(..))
        ));

        let mut duplicate = row();
        duplicate.screens.push(place("p", 5000, 0, 10, 10));
        assert!(matches!(
            duplicate.validate(),
            Err(LayoutError::Duplicate(_))
        ));

        let mut empty = row();
        empty.screens[1].width = 0;
        assert!(matches!(empty.validate(), Err(LayoutError::EmptyScreen(_))));

        let no_local = ScreenLayout {
            screens: vec![place("p", 0, 0, 10, 10)],
        };
        assert_eq!(no_local.validate(), Err(LayoutError::MissingLocal));
    }

    #[test]
    fn a_resolution_change_keeps_the_position() {
        let mut layout = row();
        layout.resize(
            &id("p"),
            ScreenSize {
                width: 1280,
                height: 720,
            },
        );
        let p = layout.placement(&id("p")).unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (1920, 200, 1280, 720));
    }
}
