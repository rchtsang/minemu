use crossterm::event::KeyCode;

/// Motions shared by all non-console panes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Motion {
    Left(usize),
    Down(usize),
    Up(usize),
    Right(usize),
    NextItem(usize),
    EndItem(usize),
    Top,
    Bottom,
}

/// Count-aware parser for the supported Vim motion subset.
#[derive(Default)]
pub struct MotionDecoder {
    count: Option<usize>,
    pending_g: bool,
}

impl MotionDecoder {
    pub fn push(&mut self, code: KeyCode) -> Option<Motion> {
        match code {
            KeyCode::Char(digit @ '1'..='9') => {
                self.count = Some(
                    self.count
                        .unwrap_or(0)
                        .saturating_mul(10)
                        .saturating_add(digit as usize - '0' as usize),
                );
                None
            }
            KeyCode::Char('0') if self.count.is_some() => {
                self.count = Some(self.count.unwrap_or(0).saturating_mul(10));
                None
            }
            KeyCode::Char('g') if self.pending_g => {
                self.reset();
                Some(Motion::Top)
            }
            KeyCode::Char('g') => {
                self.pending_g = true;
                None
            }
            KeyCode::Char('G') => {
                self.reset();
                Some(Motion::Bottom)
            }
            KeyCode::Char(key) => {
                let count = self.take_count();
                match key {
                    'h' => Some(Motion::Left(count)),
                    'j' => Some(Motion::Down(count)),
                    'k' => Some(Motion::Up(count)),
                    'l' => Some(Motion::Right(count)),
                    'w' => Some(Motion::NextItem(count)),
                    'e' => Some(Motion::EndItem(count)),
                    _ => None,
                }
            }
            _ => {
                self.reset();
                None
            }
        }
    }

    pub fn reset(&mut self) {
        self.count = None;
        self.pending_g = false;
    }

    fn take_count(&mut self) -> usize {
        let count = self.count.take().unwrap_or(1);
        self.pending_g = false;
        count
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::{Motion, MotionDecoder};

    #[test]
    fn parses_counts_and_double_g() {
        let mut decoder = MotionDecoder::default();
        assert_eq!(decoder.push(KeyCode::Char('1')), None);
        assert_eq!(decoder.push(KeyCode::Char('2')), None);
        assert_eq!(decoder.push(KeyCode::Char('j')), Some(Motion::Down(12)));
        assert_eq!(decoder.push(KeyCode::Char('g')), None);
        assert_eq!(decoder.push(KeyCode::Char('g')), Some(Motion::Top));
        assert_eq!(decoder.push(KeyCode::Char('G')), Some(Motion::Bottom));
    }
}
