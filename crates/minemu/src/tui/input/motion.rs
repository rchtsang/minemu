use crossterm::event::KeyCode;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeResult {
    Pending,
    Motion(Motion),
    Unhandled,
}

#[derive(Default)]
pub struct MotionDecoder {
    count: Option<usize>,
    pending_g: bool,
}

impl MotionDecoder {
    pub fn push(&mut self, code: KeyCode) -> DecodeResult {
        match code {
            KeyCode::Char(digit @ '1'..='9') => {
                self.count = Some(
                    self.count
                        .unwrap_or(0)
                        .saturating_mul(10)
                        .saturating_add(digit as usize - '0' as usize),
                );
                DecodeResult::Pending
            }
            KeyCode::Char('0') if self.count.is_some() => {
                self.count = Some(self.count.unwrap_or(0).saturating_mul(10));
                DecodeResult::Pending
            }
            KeyCode::Char('g') if self.pending_g => {
                self.reset();
                DecodeResult::Motion(Motion::Top)
            }
            KeyCode::Char('g') => {
                self.pending_g = true;
                DecodeResult::Pending
            }
            KeyCode::Char('G') => {
                self.reset();
                DecodeResult::Motion(Motion::Bottom)
            }
            KeyCode::Char(key) => {
                let count = self.take_count();
                let motion = match key {
                    'h' => Some(Motion::Left(count)),
                    'j' => Some(Motion::Down(count)),
                    'k' => Some(Motion::Up(count)),
                    'l' => Some(Motion::Right(count)),
                    'w' => Some(Motion::NextItem(count)),
                    'e' => Some(Motion::EndItem(count)),
                    _ => None,
                };
                motion.map_or(DecodeResult::Unhandled, DecodeResult::Motion)
            }
            _ => {
                self.reset();
                DecodeResult::Unhandled
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

    use super::{DecodeResult, Motion, MotionDecoder};

    #[test]
    fn parses_counts_and_double_g() {
        let mut decoder = MotionDecoder::default();
        assert_eq!(decoder.push(KeyCode::Char('1')), DecodeResult::Pending);
        assert_eq!(decoder.push(KeyCode::Char('2')), DecodeResult::Pending);
        assert_eq!(
            decoder.push(KeyCode::Char('j')),
            DecodeResult::Motion(Motion::Down(12))
        );
        assert_eq!(decoder.push(KeyCode::Char('g')), DecodeResult::Pending);
        assert_eq!(
            decoder.push(KeyCode::Char('g')),
            DecodeResult::Motion(Motion::Top)
        );
    }
}
