// "Play with me": hangman. Haiku picks the word and a hint; you guess by clicking letters,
// because her window never takes keyboard focus.

pub const LIVES: usize = 6;

const CATEGORIES: [&str; 14] = [
    "animals", "food", "space", "music", "weather", "ocean life", "sports", "fruit",
    "computers", "fantasy creatures", "jobs", "nature", "household objects", "travel",
];

const PROMPT: &str = "You pick words for a hangman game played with Claudesona, a cute genki anime desktop mascot. \
Choose one common English noun from the given category: 4 to 9 letters, letters only, not one in the avoid list. \
Write a short playful hint of at most 12 words in her cheerful voice that does not contain the word. \
Output exactly two lines and nothing else:\nWORD: <word>\nHINT: <hint>";

// Used when `claude` isn't available or answers oddly, so the game still works.
const FALLBACK: [(&str, &str); 10] = [
    ("PANDA", "A fluffy black-and-white bear that munches bamboo~"),
    ("COMET", "A sparkly space traveler with a glowing tail!"),
    ("GUITAR", "Six strings, lots of feelings~"),
    ("TEAPOT", "It whistles when it's ready for snack time!"),
    ("PENGUIN", "A tuxedo bird that can't fly but swims super fast~"),
    ("RAINBOW", "Seven colors after the rain!"),
    ("KEYBOARD", "Your sidekick taps on this all day~"),
    ("DRAGON", "A fire-breathing friend from storybooks!"),
    ("MANGO", "A sweet golden fruit, so juicy~"),
    ("LIGHTHOUSE", "It shines for ships at night!"),
];

// Her lines during the game. `{w}` is replaced by the word.
pub const THINKING: [&str; 3] = [
    "Hmm… thinking of a super tricky word…",
    "Wait wait, your sidekick is picking a good one~",
    "Ehehe, let me find something sneaky…",
];
pub const START: [&str; 6] = [
    "Ooh, I've got a good one! Can you guess it~?",
    "Your sidekick picked a tricky word… good luck!",
    "Game time! Don't let your sidekick win~",
    "Hehe, ready? This one's sneaky!",
    "I bet you can't get this one~!",
    "Ready, set… guess! I'm so excited~!",
];
pub const HIT: [&str; 6] = [
    "Eh?! How did you know that one?",
    "Ooh, nice guess~!",
    "Hmm, you're getting close…",
    "Lucky letter! Keep going~",
    "W-wait, that's a good one!",
    "Nngh, you found one!",
];
pub const MISS: [&str; 6] = [
    "Nope~! Try again!",
    "Hehe, not that one~",
    "Your sidekick is winning~!",
    "Wrong-o! Ehehe~",
    "Ooh, so close… not!",
    "Bzzt! That letter isn't in it~",
];
pub const LAST_HEART: [&str; 3] = [
    "Eek, one heart left! Choose wisely~",
    "Last heart… are you nervous yet~?",
    "One more miss and your sidekick wins! Hehe~",
];
pub const PLAYER_WON: [&str; 6] = [
    "Booo~ you got lucky! …Well done, though.",
    "Mou~ you guessed it! Fine, you win this time.",
    "Hmph! Beginner's luck… but nice job!",
    "No fair~! Okay okay, well played!",
    "Aww, your sidekick lost… you're pretty good!",
    "Grr… rematch! But, um, good job~",
];
pub const PLAYER_LOST: [&str; 6] = [
    "Hehe~ I won! The word was {w}!",
    "Victory for your sidekick~! It was {w}!",
    "Ehehe, too tricky for you? It was {w}~",
    "Your sidekick wins again! {w}, obviously~",
    "Better luck next time! The answer was {w}!",
    "Ohoho~ {w}! Your sidekick is unbeatable!",
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

/// A new (word, hint) from Haiku. Blocks for a few seconds: call it from a worker thread.
/// `avoid` holds recent words, so rounds don't repeat.
pub fn new_word(seed: u32, avoid: &[String]) -> (String, String) {
    let category = CATEGORIES[seed as usize % CATEGORIES.len()];
    let input = format!("Category: {category}\nAvoid: {}\nRandom number: {seed}", avoid.join(", "));
    crate::rewrite::ask(PROMPT, &input).and_then(|out| parse(&out)).filter(|(w, _)| !avoid.contains(w)).unwrap_or_else(|| {
        let fresh: Vec<_> = FALLBACK.iter().filter(|(w, _)| !avoid.iter().any(|a| a == w)).collect();
        let (w, h) = if fresh.is_empty() { FALLBACK[seed as usize % FALLBACK.len()] } else { *fresh[seed as usize % fresh.len()] };
        (w.to_string(), h.to_string())
    })
}

/// Reads "WORD: ...\nHINT: ..." and checks the word is plain letters and absent from the hint.
fn parse(out: &str) -> Option<(String, String)> {
    let field = |key: &str| out.lines().find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().to_string()));
    let word = field("WORD:")?.to_uppercase();
    let hint = field("HINT:")?;
    let ok = (3..=12).contains(&word.len()) && word.chars().all(|c| c.is_ascii_uppercase()) && !hint.to_uppercase().contains(&word) && !hint.is_empty();
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
