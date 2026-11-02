// "Play with me": hangman. Haiku picks the word and a hint; you guess by clicking letters,
// because her window never takes keyboard focus.

pub const LIVES: usize = 6;

const CATEGORIES: [&str; 14] = [
    "animals", "food", "space", "music", "weather", "ocean life", "sports", "fruit",
    "computers", "fantasy creatures", "jobs", "nature", "household objects", "travel",
];

const PROMPT_EN: &str = "You pick words for a hangman game played with Claudesona, a cute, bubbly anime desktop mascot. \
Choose one common English noun from the given category: 4 to 9 letters, letters only, not one in the avoid list. \
Write a short playful hint of at most 12 words in her cheerful voice that does not contain the word, \
in plain English with no Japanese words (no senpai, nya, desu, kawaii, mou). \
Output exactly two lines and nothing else:\nWORD: <word>\nHINT: <hint>";

// Vietnamese words are typed on the same A-Z grid, so they come without accents or tone marks (MEO for "mèo").
const PROMPT_VI: &str = "You pick words for a hangman game played in Vietnamese with Claudesona, a cute, bubbly anime desktop mascot. \
Choose one common Vietnamese word from the given category: a single syllable, 3 to 7 letters, written in capital letters \
WITHOUT any accents or tone marks (A-Z only, write đ as D), not one in the avoid list. \
Write a short playful hint in Vietnamese of at most 15 words, in her cheerful voice: she calls herself \"tớ\" and the player \"cậu\". \
The hint must not contain the word. No Japanese words. \
Output exactly two lines and nothing else:\nWORD: <word>\nHINT: <hint>";

const FALLBACK_VI: [(&str, &str); 10] = [
    ("MEO", "Bạn nhỏ bốn chân mê cá và hay rừ rừ~"),
    ("SACH", "Cậu đọc nó mỗi tối trước khi ngủ nè!"),
    ("BIEN", "Nước mặn, sóng vỗ rì rào~"),
    ("TRANG", "Tròn vành vạnh trên trời đêm Trung thu!"),
    ("HOA", "Thơm thơm, nở rộ vào mùa xuân~"),
    ("MUA", "Rơi lộp độp từ trên trời xuống!"),
    ("TRA", "Uống nóng hay đá đều ngon, tớ thích lắm~"),
    ("BANH", "Ngọt ngọt, ăn kèm trà là tuyệt nhất!"),
    ("CHO", "Bạn thân bốn chân hay vẫy đuôi~"),
    ("SAO", "Lấp lánh trên trời mỗi đêm!"),
];

// Used when `claude` isn't available or answers oddly, so the game still works.
const FALLBACK_EN: [(&str, &str); 10] = [
    ("PANDA", "A fluffy black-and-white bear that munches bamboo~"),
    ("COMET", "A sparkly space traveler with a glowing tail!"),
    ("GUITAR", "Six strings, lots of feelings~"),
    ("TEAPOT", "It whistles when it's ready for snack time!"),
    ("PENGUIN", "A tuxedo bird that can't fly but swims super fast~"),
    ("RAINBOW", "Seven colors after the rain!"),
    ("KEYBOARD", "Your sidekick taps on this all day~ (・ω・)"),
    ("DRAGON", "A fire-breathing friend from storybooks!"),
    ("MANGO", "A sweet golden fruit, so juicy~"),
    ("LIGHTHOUSE", "It shines for ships at night!"),
];

/// One of `lines` for a random `r` in 0..1, with the word filled in.
pub fn line(lines: &[&str], r: f32, word: &str) -> String {
    lines[(r * lines.len() as f32) as usize % lines.len()].replace("{w}", word)
}

pub struct Game {
    pub word: String, // uppercase A-Z
    pub hint: String,
    pub guessed: Vec<char>,
}

impl Game {
    pub fn new(word: String, hint: String) -> Self {
        Game { word, hint, guessed: vec![] }
    }

    pub fn guess(&mut self, c: char) {
        if !self.over() && !self.guessed.contains(&c) {
            self.guessed.push(c);
        }
    }

    pub fn misses(&self) -> usize {
        self.guessed.iter().filter(|c| !self.word.contains(**c)).count()
    }

    pub fn won(&self) -> bool {
        self.word.chars().all(|c| self.guessed.contains(&c))
    }

    pub fn lost(&self) -> bool {
        self.misses() >= LIVES
    }

    pub fn over(&self) -> bool {
        self.won() || self.lost()
    }

    /// "P _ N _ A"; the whole word once the game is over.
    pub fn masked(&self) -> String {
        let show = self.over();
        self.word.chars().map(|c| if show || self.guessed.contains(&c) { c } else { '_' }).map(String::from).collect::<Vec<_>>().join(" ")
    }
}

/// A new (word, hint) from Haiku in the current language. Blocks for a few seconds: call it from a worker thread.
/// `avoid` holds recent words, so rounds don't repeat.
pub fn new_word(seed: u32, avoid: &[String]) -> (String, String) {
    let (prompt, fallback) = match crate::i18n::lang() {
        crate::i18n::Lang::En => (PROMPT_EN, &FALLBACK_EN),
        crate::i18n::Lang::Vi => (PROMPT_VI, &FALLBACK_VI),
    };
    let category = CATEGORIES[seed as usize % CATEGORIES.len()];
    let input = format!("Category: {category}\nAvoid: {}\nRandom number: {seed}", avoid.join(", "));
    crate::rewrite::ask(prompt, &input).and_then(|out| parse(&out)).filter(|(w, _)| !avoid.contains(w)).unwrap_or_else(|| {
        let fresh: Vec<_> = fallback.iter().filter(|(w, _)| !avoid.iter().any(|a| a == w)).collect();
        let (w, h) = if fresh.is_empty() { fallback[seed as usize % fallback.len()] } else { *fresh[seed as usize % fresh.len()] };
        (w.to_string(), h.to_string())
    })
}

/// Reads "WORD: ...\nHINT: ..." and checks the word is plain letters and absent from the hint.
fn parse(out: &str) -> Option<(String, String)> {
    let field = |key: &str| out.lines().find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().to_string()));
    let word = crate::i18n::fold(&field("WORD:")?).to_uppercase(); // in case Haiku leaves a Vietnamese mark on
    let hint = field("HINT:")?;
    // Compared without accents too, so "mèo" in a hint still counts as giving away MEO.
    let gives_away = crate::i18n::fold(&hint).to_uppercase().contains(&word);
    let ok = (3..=12).contains(&word.len()) && word.chars().all(|c| c.is_ascii_uppercase()) && !gives_away && !hint.is_empty();
    ok.then_some((word, hint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_haiku_answers_and_rejects_bad_ones() {
        assert_eq!(parse("WORD: Otter\nHINT: Holds hands while napping on the water~"), Some(("OTTER".into(), "Holds hands while napping on the water~".into())));
        assert_eq!(parse("Sure!\n  WORD: comet  \n HINT: A sparkly space traveler!\n"), Some(("COMET".into(), "A sparkly space traveler!".into())));
        assert_eq!(parse("WORD: ice cream\nHINT: Cold and sweet"), None); // not letters only
        assert_eq!(parse("WORD: CAT\nHINT: A cat says meow"), None); // gives the word away
        assert_eq!(parse("just chatting"), None);
        assert_eq!(parse("WORD: MÈO\nHINT: Bạn nhỏ mê cá~"), Some(("MEO".into(), "Bạn nhỏ mê cá~".into()))); // marks folded
        assert_eq!(parse("WORD: MEO\nHINT: Con mèo kêu meo meo"), None); // gives it away, accents or not
    }

    #[test]
    fn game_tracks_hits_misses_and_the_end() {
        let mut g = Game::new("PANDA".into(), "bear".into());
        g.guess('A');
        g.guess('A'); // repeats don't count twice
        g.guess('Z');
        assert_eq!((g.masked().as_str(), g.misses()), ("_ A _ _ A", 1));
        for c in ['P', 'N', 'D'] {
            g.guess(c);
        }
        assert!(g.won() && !g.lost());
        let mut g = Game::new("CAT".into(), "meow".into());
        for c in ['Q', 'W', 'E', 'R', 'Y', 'U'] {
            g.guess(c);
        }
        assert!(g.lost() && g.masked() == "C A T");
        g.guess('C'); // no guesses after the end
        assert_eq!(g.guessed.len(), LIVES);
    }
}
