//! The tokens on one combat map, and every rule deciding where one sits and what it is
//! called.
//!
//! A token is session state: it becomes no [`Edit`](crate::edit::Edit), sits on no undo
//! stack and is never written to a file, so nothing here is serde.

/// The fewest cells along a side a token may cover.
pub const MIN_TOKEN_SIZE: u8 = 1;

/// The most cells along a side a token may cover.
pub const MAX_TOKEN_SIZE: u8 = 4;

/// The most characters a token's name may have.
pub const MAX_TOKEN_NAME_CHARS: usize = 24;

/// The most tokens one combat map may carry.
pub const MAX_TOKENS: usize = 128;

/// A token that cannot be placed, renamed, moved or removed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenProblem {
    /// A name that is empty or only whitespace.
    #[error("a token needs a name")]
    Unnamed,

    /// A name with a character the map's lettering cannot draw.
    #[error("{name:?} has a character the map cannot letter; use plain ASCII")]
    Undrawable { name: String },

    /// A name longer than [`MAX_TOKEN_NAME_CHARS`].
    #[error("{name:?} is longer than {MAX_TOKEN_NAME_CHARS} characters")]
    TooLong { name: String },

    /// A name another token on the map already has.
    #[error("a token called {name} is already on the map")]
    Taken { name: String },

    /// A name no token on the map has.
    #[error("no token on the map is called {name}")]
    Unknown { name: String },

    /// A size outside [`MIN_TOKEN_SIZE`]`..=`[`MAX_TOKEN_SIZE`].
    #[error("a token is {MIN_TOKEN_SIZE} to {MAX_TOKEN_SIZE} cells a side, not {size}")]
    BadSize { size: u8 },

    /// A size more cells a side than the map has.
    #[error("a token {size} cells a side does not fit on a {width}x{height} map")]
    TooBig { size: u8, width: u32, height: u32 },

    /// A map already carrying [`MAX_TOKENS`].
    #[error("the map already carries {MAX_TOKENS} tokens")]
    Full,

    /// A base whose next number does not fit in a `u32`.
    #[error("there is no number left to give another {base}")]
    OutOfNumbers { base: String },
}

/// One named token, anchored on its top-left cell and covering `size × size` cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    name: String,
    x: u32,
    y: u32,
    size: u8,
}

impl Token {
    /// What it is called; unique on its map.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The column of its top-left cell.
    pub fn x(&self) -> u32 {
        self.x
    }

    /// The row of its top-left cell.
    pub fn y(&self) -> u32 {
        self.y
    }

    /// The cells along each side it covers.
    pub fn size(&self) -> u8 {
        self.size
    }

    /// Whether it covers the cell `(x, y)`.
    pub fn covers(&self, x: i64, y: i64) -> bool {
        let (left, top, side) = (i64::from(self.x), i64::from(self.y), i64::from(self.size));
        (left..left + side).contains(&x) && (top..top + side).contains(&y)
    }

    /// Every cell it covers, row by row.
    pub fn cells(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let side = u32::from(self.size);
        (self.y..self.y + side).flat_map(move |y| (self.x..self.x + side).map(move |x| (x, y)))
    }
}

/// Why `name`, trimmed, may not be a token's name, or `None` if it may.
///
/// Refuses [`TokenProblem::Unnamed`], then [`TokenProblem::Undrawable`] for a character
/// outside ASCII 32–126, then [`TokenProblem::TooLong`].
pub fn name_refusal(name: &str) -> Option<TokenProblem> {
    let name = name.trim();
    if name.is_empty() {
        return Some(TokenProblem::Unnamed);
    }
    if !name.chars().all(|c| (' '..='~').contains(&c)) {
        return Some(TokenProblem::Undrawable { name: name.to_owned() });
    }
    if name.chars().count() > MAX_TOKEN_NAME_CHARS {
        return Some(TokenProblem::TooLong { name: name.to_owned() });
    }
    None
}

/// The name a token placed with `typed` gets, against the names in `taken`.
///
/// `typed` is trimmed and held to [`name_refusal`]. A name ending in an ASCII digit is
/// kept as typed, and refused [`TokenProblem::Taken`] when it is in `taken`. Any other name
/// is a base, and gets one more than the highest number that base carries in `taken` —
/// `orc` against `orc1`, `orc3` is `orc4` — or `1` when it carries none. Matching is
/// case-sensitive, and only the base followed by digits alone counts. Fails
/// [`TokenProblem::OutOfNumbers`] when that number does not fit in a `u32`, and
/// [`TokenProblem::TooLong`] when the numbered name is too long.
pub fn next_name<'a>(typed: &str, taken: impl IntoIterator<Item = &'a str>) -> Result<String, TokenProblem> {
    let typed = typed.trim();
    if let Some(problem) = name_refusal(typed) {
        return Err(problem);
    }
    let mut taken = taken.into_iter();
    if typed.ends_with(|c: char| c.is_ascii_digit()) {
        return match taken.any(|name| name == typed) {
            true => Err(TokenProblem::Taken { name: typed.to_owned() }),
            false => Ok(typed.to_owned()),
        };
    }
    let highest = taken
        .filter_map(|name| name.strip_prefix(typed))
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|digits| digits.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    let Some(next) = highest.checked_add(1) else {
        return Err(TokenProblem::OutOfNumbers { base: typed.to_owned() });
    };
    let name = format!("{typed}{next}");
    match name_refusal(&name) {
        Some(problem) => Err(problem),
        None => Ok(name),
    }
}

/// The top-left cell of a token `size` cells a side pressed at the cell `at`, on a map
/// `width` by `height` cells.
///
/// Clamped on each axis so the token lies wholly on the map. The caller has checked that
/// `size` is no more than either extent; if it is not, the token is anchored at `0`.
pub fn anchor(at: (i64, i64), size: u8, width: u32, height: u32) -> (u32, u32) {
    let clamp = |at: i64, extent: u32| {
        let last = i64::from(extent.saturating_sub(u32::from(size)));
        u32::try_from(at.clamp(0, last)).unwrap_or(0)
    };
    (clamp(at.0, width), clamp(at.1, height))
}

/// The tokens on one combat map, in the order they were placed; a later one is drawn over
/// an earlier.
///
/// Every name on it is unique, every token lies wholly on the map it was placed against,
/// and it never holds more than [`MAX_TOKENS`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tokens {
    tokens: Vec<Token>,
}

impl Tokens {
    /// Place a token named from `typed` by [`next_name`], `size` cells a side, pressed at
    /// the cell `at` on a map of `extent` `(width, height)` cells, and hand it back.
    ///
    /// The anchor is clamped onto the map by [`anchor`]. Refuses, in this order,
    /// [`TokenProblem::BadSize`], [`TokenProblem::TooBig`], [`TokenProblem::Full`], then
    /// whatever [`next_name`] refuses. A refusal places nothing.
    pub fn place(
        &mut self,
        typed: &str,
        size: u8,
        at: (i64, i64),
        extent: (u32, u32),
    ) -> Result<&Token, TokenProblem> {
        let (width, height) = extent;
        if !(MIN_TOKEN_SIZE..=MAX_TOKEN_SIZE).contains(&size) {
            return Err(TokenProblem::BadSize { size });
        }
        if u32::from(size) > width || u32::from(size) > height {
            return Err(TokenProblem::TooBig { size, width, height });
        }
        if self.tokens.len() >= MAX_TOKENS {
            return Err(TokenProblem::Full);
        }
        let name = next_name(typed, self.tokens.iter().map(Token::name))?;
        let (x, y) = anchor(at, size, width, height);
        self.tokens.push(Token { name, x, y, size });
        Ok(self.tokens.last().expect("a token was just pushed"))
    }

    /// Where the token called `name` would be anchored if the cell `grabbed` were dragged
    /// to the cell `now`, on a map of `extent` cells: its anchor moved by the difference,
    /// clamped onto the map. `None` when no token has that name. Changes nothing.
    pub fn dragged(&self, name: &str, grabbed: (i64, i64), now: (i64, i64), extent: (u32, u32)) -> Option<(u32, u32)> {
        let token = self.get(name)?;
        let at = (
            i64::from(token.x) + now.0 - grabbed.0,
            i64::from(token.y) + now.1 - grabbed.1,
        );
        Some(anchor(at, token.size, extent.0, extent.1))
    }

    /// Move the token called `name` to where [`Tokens::dragged`] says, and say whether its
    /// anchor changed.
    ///
    /// Refuses [`TokenProblem::Unknown`], having changed nothing.
    pub fn move_by(
        &mut self,
        name: &str,
        grabbed: (i64, i64),
        now: (i64, i64),
        extent: (u32, u32),
    ) -> Result<bool, TokenProblem> {
        let Some((x, y)) = self.dragged(name, grabbed, now, extent) else {
            return Err(TokenProblem::Unknown { name: name.to_owned() });
        };
        let token = self.find_mut(name).expect("dragged found the token");
        let moved = (token.x, token.y) != (x, y);
        (token.x, token.y) = (x, y);
        Ok(moved)
    }

    /// Call the token named `from` by `to`, trimmed and taken as typed — never numbered —
    /// and hand back the new name.
    ///
    /// Refuses [`TokenProblem::Unknown`] for `from`, then what [`name_refusal`] refuses for
    /// `to`, then [`TokenProblem::Taken`] when another token is called `to`. Renaming a token
    /// to its own name succeeds. A refusal changes nothing.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<String, TokenProblem> {
        if self.get(from).is_none() {
            return Err(TokenProblem::Unknown { name: from.to_owned() });
        }
        let to = to.trim();
        if let Some(problem) = name_refusal(to) {
            return Err(problem);
        }
        if to != from && self.get(to).is_some() {
            return Err(TokenProblem::Taken { name: to.to_owned() });
        }
        let token = self.find_mut(from).expect("the token was just found");
        to.clone_into(&mut token.name);
        Ok(token.name.clone())
    }

    /// Take the token called `name` off the map and hand it back.
    ///
    /// Refuses [`TokenProblem::Unknown`].
    pub fn remove(&mut self, name: &str) -> Result<Token, TokenProblem> {
        match self.tokens.iter().position(|token| token.name == name) {
            Some(index) => Ok(self.tokens.remove(index)),
            None => Err(TokenProblem::Unknown { name: name.to_owned() }),
        }
    }

    /// Take every token off the map.
    pub fn clear(&mut self) {
        self.tokens.clear();
    }

    /// The token called `name`.
    pub fn get(&self, name: &str) -> Option<&Token> {
        self.tokens.iter().find(|token| token.name == name)
    }

    /// Every token, in the order it was placed.
    pub fn iter(&self) -> impl Iterator<Item = &Token> {
        self.tokens.iter()
    }

    /// How many tokens are on the map.
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Whether the map carries no token.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// The token drawn topmost over the cell `(x, y)`: the last placed that covers it.
    pub fn topmost_at(&self, x: i64, y: i64) -> Option<&Token> {
        self.tokens.iter().rev().find(|token| token.covers(x, y))
    }

    fn find_mut(&mut self, name: &str) -> Option<&mut Token> {
        self.tokens.iter_mut().find(|token| token.name == name)
    }
}
